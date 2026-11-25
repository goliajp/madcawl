//! Matching a family list, weight and style to an installed face.

use super::{FaceId, Fonts, MatchKey, REF_SIZE};
use crate::ffi::{self, Cf};
use crate::lru::FxBuild;
use std::hash::{BuildHasher, Hash, Hasher};

// CSS font-weight to the CoreText weight trait, at the named weights
const WEIGHTS: [(f32, f64); 9] = [
    (100.0, -0.8),
    (200.0, -0.6),
    (300.0, -0.4),
    (400.0, 0.0),
    (500.0, 0.23),
    (600.0, 0.3),
    (700.0, 0.4),
    (800.0, 0.56),
    (900.0, 0.62),
];

fn weight_trait(w: f32) -> f64 {
    let w = w.clamp(100.0, 900.0);
    let k = WEIGHTS
        .iter()
        .position(|&(c, _)| c >= w)
        .unwrap_or(8)
        .max(1);
    let ((a, ta), (b, tb)) = (WEIGHTS[k - 1], WEIGHTS[k]);
    ta + (tb - ta) * ((w - a) / (b - a)) as f64
}

fn generic(family: &str) -> Option<&'static str> {
    Some(match family {
        "serif" => "Times",
        "sans-serif" => "Helvetica",
        "monospace" => "Menlo",
        "cursive" => "Apple Chancery",
        "fantasy" => "Papyrus",
        _ => return None,
    })
}

impl Fonts {
    /// The first family of `families` that is installed, at the closest weight (CSS 1 to 1000) and style; the
    /// system font when none is. Generic family names map to the system's default faces.
    pub fn select(&mut self, families: &[&str], weight: f32, italic: bool) -> FaceId {
        let mut h = FxBuild::default().build_hasher();
        families.hash(&mut h);
        weight.to_bits().hash(&mut h);
        italic.hash(&mut h);
        let hash = h.finish();
        // the hash alone does not identify the request: family lists come from pages, so compare the key
        if let Some((key, id)) = self.matches.get(&hash)
            && key.weight == weight.to_bits()
            && key.italic == italic
            && key.families.len() == families.len()
            && key.families.iter().zip(families).all(|(a, b)| **a == **b)
        {
            return *id;
        }
        let id = families
            .iter()
            .find_map(|f| self.match_family(f, weight, italic))
            .unwrap_or_else(|| self.system());
        let key = MatchKey {
            families: families.iter().map(|&f| f.into()).collect(),
            weight: weight.to_bits(),
            italic,
        };
        self.matches.insert(hash, (key, id), 1);
        id
    }

    fn match_family(&mut self, family: &str, weight: f32, italic: bool) -> Option<FaceId> {
        if family.eq_ignore_ascii_case("system-ui") {
            return self.ui_face(None, weight, italic);
        }
        if family.eq_ignore_ascii_case("ui-monospace") {
            return self.ui_face(Some(".AppleSystemUIFontMonospaced-Regular"), weight, italic);
        }
        let lower = family.to_ascii_lowercase();
        let name = generic(&lower).unwrap_or(family);
        let fam = ffi::string(name)?;
        let w = ffi::number_f64(weight_trait(weight))?;
        let slant = ffi::number_f64(if italic { 0.07 } else { 0.0 })?;
        // SAFETY: the attribute keys are CoreText's constants and the values live CF objects
        let font = unsafe {
            let traits = ffi::dictionary(
                &[ffi::kCTFontWeightTrait, ffi::kCTFontSlantTrait],
                &[w.get(), slant.get()],
            )?;
            let attrs = ffi::dictionary(
                &[ffi::kCTFontFamilyNameAttribute, ffi::kCTFontTraitsAttribute],
                &[fam.get(), traits.get()],
            )?;
            let desc = Cf::new(ffi::CTFontDescriptorCreateWithAttributes(attrs.get()))?;
            let matched = Cf::new(ffi::CTFontDescriptorCreateMatchingFontDescriptor(
                desc.get(),
                std::ptr::null(),
            ))?;
            Cf::new(ffi::CTFontCreateWithFontDescriptor(
                matched.get(),
                REF_SIZE,
                std::ptr::null(),
            ))?
        };
        // SAFETY: `font` is a live CTFont
        let got = Cf::new(unsafe { ffi::CTFontCopyFamilyName(font.get()) })
            .map(|s| ffi::to_string(&s))?;
        got.eq_ignore_ascii_case(name).then(|| self.intern(font))
    }

    // the system font, or its monospaced design, at a weight and slant: the system faces are found by
    // their traits, not by family name
    fn ui_face(&mut self, name: Option<&str>, weight: f32, italic: bool) -> Option<FaceId> {
        let base = match name {
            None if weight == 400.0 && !italic => return Some(self.system()),
            None => {
                // SAFETY: the null language selects the user's
                Cf::new(unsafe {
                    ffi::CTFontCreateUIFontForLanguage(
                        ffi::kCTFontUIFontSystem,
                        REF_SIZE,
                        std::ptr::null(),
                    )
                })?
            }
            Some(n) => {
                let s = ffi::string(n)?;
                // SAFETY: `s` is a live CFString
                Cf::new(unsafe { ffi::CTFontCreateWithName(s.get(), REF_SIZE, std::ptr::null()) })?
            }
        };
        if weight == 400.0 && !italic {
            return Some(self.intern(base));
        }
        let w = ffi::number_f64(weight_trait(weight))?;
        let slant = ffi::number_f64(if italic { 0.07 } else { 0.0 })?;
        // SAFETY: the attribute keys are CoreText's constants and the values live CF objects
        let font = unsafe {
            let traits = ffi::dictionary(
                &[ffi::kCTFontWeightTrait, ffi::kCTFontSlantTrait],
                &[w.get(), slant.get()],
            )?;
            let attrs = ffi::dictionary(&[ffi::kCTFontTraitsAttribute], &[traits.get()])?;
            let desc = Cf::new(ffi::CTFontDescriptorCreateWithAttributes(attrs.get()))?;
            Cf::new(ffi::CTFontCreateCopyWithAttributes(
                base.get(),
                REF_SIZE,
                std::ptr::null(),
                desc.get(),
            ))?
        };
        Some(self.intern(font))
    }
}
