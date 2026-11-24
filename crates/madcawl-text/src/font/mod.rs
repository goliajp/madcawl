//! Font faces through CoreText: matching a family list, weight and style to an installed face, metrics, sized
//! instances, and per-script fallback.

use crate::ffi::{self, Cf};
use crate::lru::{FxBuild, Lru};
use madcawl_unicode::Script;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

mod select;

/// An installed face, by its index in [`Fonts`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FaceId(pub u32);

/// Vertical metrics in em units (multiply by the font size for pixels).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Metrics {
    pub ascent: f32,
    pub descent: f32,
    pub leading: f32,
    pub x_height: f32,
    pub cap_height: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
    pub units_per_em: u32,
}

struct Face {
    // the face at REF_SIZE; sized instances are copies of it
    font: Cf,
    postscript: String,
    family: String,
    metrics: Metrics,
}

// a request to `select`, stored next to the face it matched
struct MatchKey {
    families: Box<[Box<str>]>,
    weight: u32,
    italic: bool,
}

const REF_SIZE: f64 = 16.0;
const INSTANCES: usize = 256;
const MATCHES: usize = 512;
const FALLBACKS: usize = 1024;

/// The faces met so far and the caches over them. Faces are never dropped: there is at most one entry per
/// installed (or registered) face, so the table is bounded by what the system has.
pub struct Fonts {
    faces: Vec<Face>,
    by_name: HashMap<String, FaceId, FxBuild>,
    instances: Lru<(FaceId, u32), Arc<Cf>>,
    matches: Lru<u64, (MatchKey, FaceId)>,
    fallbacks: Lru<(FaceId, Script), FaceId>,
    system: Option<FaceId>,
}

fn metrics(font: &Cf) -> Metrics {
    let f = font.get();
    // SAFETY: `f` is a live CTFont
    unsafe {
        let size = ffi::CTFontGetSize(f);
        let em = |v: f64| (v / size) as f32;
        Metrics {
            ascent: em(ffi::CTFontGetAscent(f)),
            descent: em(ffi::CTFontGetDescent(f)),
            leading: em(ffi::CTFontGetLeading(f)),
            x_height: em(ffi::CTFontGetXHeight(f)),
            cap_height: em(ffi::CTFontGetCapHeight(f)),
            underline_position: em(ffi::CTFontGetUnderlinePosition(f)),
            underline_thickness: em(ffi::CTFontGetUnderlineThickness(f)),
            units_per_em: ffi::CTFontGetUnitsPerEm(f),
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    pub fn new() -> Self {
        Fonts {
            faces: Vec::new(),
            by_name: HashMap::default(),
            instances: Lru::new(INSTANCES, usize::MAX),
            matches: Lru::new(MATCHES, usize::MAX),
            fallbacks: Lru::new(FALLBACKS, usize::MAX),
            system: None,
        }
    }

    /// Registers a CTFont (at any size) and returns its face, the existing one for a PostScript name met before.
    pub(crate) fn intern(&mut self, font: Cf) -> FaceId {
        // SAFETY: `font` is a live CTFont
        let name = ffi::Cf::new(unsafe { ffi::CTFontCopyPostScriptName(font.get()) })
            .map(|s| ffi::to_string(&s));
        let name = name.unwrap_or_default();
        // a face without a PostScript name is not looked up by it
        if !name.is_empty()
            && let Some(&id) = self.by_name.get(&name)
        {
            return id;
        }
        // SAFETY: as above; a null result leaves the family empty
        let family = ffi::Cf::new(unsafe { ffi::CTFontCopyFamilyName(font.get()) })
            .map(|s| ffi::to_string(&s));
        // SAFETY: copying a live font at another size
        let font = Cf::new(unsafe {
            ffi::CTFontCreateCopyWithAttributes(
                font.get(),
                REF_SIZE,
                std::ptr::null(),
                std::ptr::null(),
            )
        })
        .unwrap_or(font);
        let id = FaceId(self.faces.len() as u32);
        let metrics = metrics(&font);
        self.faces.push(Face {
            font,
            postscript: name.clone(),
            family: family.unwrap_or_default(),
            metrics,
        });
        if !name.is_empty() {
            self.by_name.insert(name, id);
        }
        id
    }

    /// The system UI font.
    pub fn system(&mut self) -> FaceId {
        if let Some(id) = self.system {
            return id;
        }
        // SAFETY: the null language selects the user's
        let font = Cf::new(unsafe {
            ffi::CTFontCreateUIFontForLanguage(ffi::kCTFontUIFontSystem, REF_SIZE, std::ptr::null())
        });
        let id = match font {
            Some(f) => self.intern(f),
            None => self.named("Helvetica").unwrap_or(FaceId(0)),
        };
        self.system = Some(id);
        id
    }

    fn named(&mut self, name: &str) -> Option<FaceId> {
        let s = ffi::string(name)?;
        // SAFETY: `s` is a live CFString
        let f = Cf::new(unsafe { ffi::CTFontCreateWithName(s.get(), REF_SIZE, std::ptr::null()) })?;
        Some(self.intern(f))
    }

    pub fn metrics(&self, face: FaceId) -> Metrics {
        self.faces[face.0 as usize].metrics
    }

    pub fn postscript_name(&self, face: FaceId) -> &str {
        &self.faces[face.0 as usize].postscript
    }

    pub fn family_name(&self, face: FaceId) -> &str {
        &self.faces[face.0 as usize].family
    }

    /// The face as a CTFont at `size` pixels.
    pub(crate) fn instance(&mut self, face: FaceId, size: f32) -> Arc<Cf> {
        let key = (face, size.to_bits());
        if let Some(f) = self.instances.get(&key) {
            return f.clone();
        }
        let base = &self.faces[face.0 as usize].font;
        // SAFETY: copying a live font at another size
        let made = Cf::new(unsafe {
            ffi::CTFontCreateCopyWithAttributes(
                base.get(),
                size as f64,
                std::ptr::null(),
                std::ptr::null(),
            )
        });
        let f = Arc::new(match made {
            Some(m) => m,
            // SAFETY: retaining a live font; the new reference is owned by the result
            None => {
                Cf::new(unsafe { ffi::CFRetain(base.get()) }).expect("CFRetain of a live object")
            }
        });
        self.instances.insert(key, f.clone(), 1);
        f
    }

    /// Whether `face` has a glyph for every character of `text`.
    pub(crate) fn covers(&self, face: FaceId, text: &[u16], glyphs: &mut Vec<u16>) -> bool {
        glyphs.clear();
        glyphs.resize(text.len(), 0);
        let f = self.faces[face.0 as usize].font.get();
        // SAFETY: both buffers have text.len() units
        unsafe {
            ffi::CTFontGetGlyphsForCharacters(
                f,
                text.as_ptr(),
                glyphs.as_mut_ptr(),
                text.len() as ffi::CFIndex,
            )
        }
    }

    /// A face for `text` (one grapheme cluster) when `base` lacks it: the cached fallback of `base` for the
    /// script if that covers it, otherwise the one CoreText picks, which then replaces the cached one.
    pub(crate) fn fallback(
        &mut self,
        base: FaceId,
        script: Script,
        text: &[u16],
        glyphs: &mut Vec<u16>,
    ) -> FaceId {
        if let Some(&f) = self.fallbacks.get(&(base, script))
            && self.covers(f, text, glyphs)
        {
            return f;
        }
        let Some(s) = ffi::utf16(text) else {
            return base;
        };
        let range = ffi::CFRange {
            location: 0,
            length: text.len() as ffi::CFIndex,
        };
        let b = self.faces[base.0 as usize].font.get();
        // SAFETY: `b` and `s` are live; the range lies within `s`
        let Some(f) = Cf::new(unsafe { ffi::CTFontCreateForString(b, s.get(), range) }) else {
            return base;
        };
        let id = self.intern(f);
        self.fallbacks.insert((base, script), id, 1);
        id
    }

    /// Entries in the fallback cache, and how many it has evicted.
    pub fn fallback_stats(&self) -> (usize, u64) {
        (self.fallbacks.len(), self.fallbacks.evictions)
    }
}
