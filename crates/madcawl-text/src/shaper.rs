//! Shaping a word: the cache first, then font fallback by grapheme cluster and CoreText.

use crate::cache::{ShapeCache, Shaped};
use crate::font::{FaceId, Fonts};
use crate::shape::{self, Glyph, Run, Scratch};
use madcawl_unicode::{Script, Source, Utf16, grapheme_breaks, props};
use std::sync::Arc;

/// The text style shaping depends on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub face: FaceId,
    /// Font size in pixels.
    pub size: f32,
    /// Feature bits, such as [`crate::NO_LIGATURES`].
    pub features: u32,
}

/// Default bounds of the shaping cache: words, and glyphs plus code units over all words.
pub const CACHE_ENTRIES: usize = 16 * 1024;
pub const CACHE_WEIGHT: usize = 1 << 20;

/// Fonts and the shaping cache, with the buffers shaping reuses.
pub struct Shaper {
    pub fonts: Fonts,
    pub cache: ShapeCache,
    scratch: Scratch,
    glyphs: Vec<Glyph>,
    cover: Vec<u16>,
    // (code unit start, face) of each fallback run of the word being shaped
    runs: Vec<(usize, FaceId)>,
}

impl Default for Shaper {
    fn default() -> Self {
        Self::new(CACHE_ENTRIES, CACHE_WEIGHT)
    }
}

fn script_of(text: &[u16]) -> Script {
    let src = Utf16(text);
    let mut i = 0;
    while i < src.len() {
        let (cp, next) = src.next(i);
        let s = props(cp).script;
        if !matches!(s, Script::Common | Script::Inherited | Script::Unknown) {
            return s;
        }
        i = next;
    }
    Script::Common
}

impl Shaper {
    pub fn new(cache_entries: usize, cache_weight: usize) -> Self {
        Shaper {
            fonts: Fonts::new(),
            cache: ShapeCache::new(cache_entries, cache_weight),
            scratch: Scratch::default(),
            glyphs: Vec::new(),
            cover: Vec::new(),
            runs: Vec::new(),
        }
    }

    /// The glyphs of one word (text with no line break opportunity inside and one bidi level) in `style`.
    pub fn shape_word(&mut self, text: &[u16], style: &Style, rtl: bool) -> Arc<Shaped> {
        if let Some(hit) = self
            .cache
            .get(text, style.face, style.size, style.features, rtl)
        {
            return hit;
        }
        self.split_by_coverage(text, style.face);
        self.glyphs.clear();
        let mut advance = 0.0;
        let n = self.runs.len();
        for k in 0..n {
            // right to left, the last run is leftmost
            let k = if rtl { n - 1 - k } else { k };
            let (start, face) = self.runs[k];
            let end = self.runs.get(k + 1).map_or(text.len(), |r| r.0);
            let font = self.fonts.instance(face, style.size);
            let run = Run {
                face,
                font: &font,
                rtl,
                features: style.features,
                base: start as u32,
            };
            advance += shape::shape_run(
                &mut self.fonts,
                &run,
                &text[start..end],
                &mut self.scratch,
                &mut self.glyphs,
            );
        }
        let shaped = Arc::new(Shaped {
            glyphs: self.glyphs.as_slice().into(),
            advance,
            rtl,
        });
        self.cache
            .insert(text, style.face, style.size, style.features, shaped.clone());
        shaped
    }

    // fills `self.runs` with the faces covering each grapheme cluster: `base` where it has the glyphs,
    // a fallback for the cluster's script where it does not
    fn split_by_coverage(&mut self, text: &[u16], base: FaceId) {
        self.runs.clear();
        if self.fonts.covers(base, text, &mut self.cover) {
            self.runs.push((0, base));
            return;
        }
        let mut start = 0;
        for end in grapheme_breaks(&Utf16(text)) {
            let cluster = &text[start..end];
            let face = if self.fonts.covers(base, cluster, &mut self.cover) {
                base
            } else {
                self.fonts
                    .fallback(base, script_of(cluster), cluster, &mut self.cover)
            };
            if self.runs.last().is_none_or(|r| r.1 != face) {
                self.runs.push((start, face));
            }
            start = end;
        }
    }
}
