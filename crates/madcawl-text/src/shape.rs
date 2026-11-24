//! Shaping one run of text in one face and direction through CoreText (`CTLine` and `CTRun`).

use crate::ffi::{self, Cf};
use crate::font::{FaceId, Fonts};

/// A positioned glyph. Glyphs of a shaped word are in visual order, left to right.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Glyph {
    pub id: u16,
    pub face: FaceId,
    pub advance: f32,
    // offset from the pen position
    pub dx: f32,
    pub dy: f32,
    /// The code unit offset, within the shaped text, of the first character of the glyph's cluster.
    pub cluster: u32,
}

/// The ligature feature off (`font-variant-ligatures: none`).
pub const NO_LIGATURES: u32 = 1;

#[derive(Default)]
pub(crate) struct Scratch {
    glyphs: Vec<u16>,
    advances: Vec<ffi::CGSize>,
    positions: Vec<ffi::CGPoint>,
    indices: Vec<ffi::CFIndex>,
}

impl Scratch {
    fn reset(&mut self, n: usize) {
        self.glyphs.clear();
        self.glyphs.resize(n, 0);
        self.advances.clear();
        self.advances.resize(n, ffi::CGSize::default());
        self.positions.clear();
        self.positions.resize(n, ffi::CGPoint::default());
        self.indices.clear();
        self.indices.resize(n, 0);
    }
}

fn attributes(font: &Cf, rtl: bool, features: u32) -> Option<Cf> {
    let dir = ffi::number_i32(
        ffi::kCTWritingDirectionOverride
            | if rtl {
                ffi::kCTWritingDirectionRightToLeft
            } else {
                0
            },
    )?;
    let dirs = ffi::array(&[dir.get()])?;
    let lig = ffi::number_i32(if features & NO_LIGATURES != 0 { 0 } else { 1 })?;
    // SAFETY: the keys are CoreText's attribute names and the values live CF objects
    unsafe {
        ffi::dictionary(
            &[
                ffi::kCTFontAttributeName,
                ffi::kCTWritingDirectionAttributeName,
                ffi::kCTLigatureAttributeName,
            ],
            &[font.get(), dirs.get(), lig.get()],
        )
    }
}

/// What one CoreText call shapes: a face, its instance at the size wanted, the direction, the feature bits,
/// and the offset of the text within the word, added to every cluster.
pub(crate) struct Run<'a> {
    pub face: FaceId,
    pub font: &'a Cf,
    pub rtl: bool,
    pub features: u32,
    pub base: u32,
}

/// Appends the glyphs of `text` shaped as `run` says to `out` and returns their total advance. A run CoreText
/// shapes in another face (its own fallback) records that face.
pub(crate) fn shape_run(
    fonts: &mut Fonts,
    run: &Run,
    text: &[u16],
    scratch: &mut Scratch,
    out: &mut Vec<Glyph>,
) -> f32 {
    let (Some(s), Some(attrs)) = (
        ffi::utf16(text),
        attributes(run.font, run.rtl, run.features),
    ) else {
        return 0.0;
    };
    // SAFETY: `s` and `attrs` are live; the line owns its runs, which are read only while it lives
    let Some(astr) =
        Cf::new(unsafe { ffi::CFAttributedStringCreate(std::ptr::null(), s.get(), attrs.get()) })
    else {
        return 0.0;
    };
    // SAFETY: `astr` is live
    let Some(line) = Cf::new(unsafe { ffi::CTLineCreateWithAttributedString(astr.get()) }) else {
        return 0.0;
    };
    let mut pen = 0.0f64;
    // SAFETY: the runs array belongs to `line`, which outlives this loop
    unsafe {
        let runs = ffi::CTLineGetGlyphRuns(line.get());
        for r in 0..ffi::CFArrayGetCount(runs) {
            let ct = ffi::CFArrayGetValueAtIndex(runs, r);
            pen = read_run(fonts, run, ct, pen, scratch, out);
        }
    }
    pen as f32
}

// SAFETY: `run` must be a live CTRun
unsafe fn read_run(
    fonts: &mut Fonts,
    spec: &Run,
    run: ffi::CFTypeRef,
    mut pen: f64,
    sc: &mut Scratch,
    out: &mut Vec<Glyph>,
) -> f64 {
    let (face, font, base) = (spec.face, spec.font, spec.base);
    // SAFETY: the caller passes a live run; every buffer is sized to its glyph count
    unsafe {
        let n = ffi::CTRunGetGlyphCount(run);
        let all = ffi::CFRange {
            location: 0,
            length: n,
        };
        let n = n as usize;
        sc.reset(n);
        ffi::CTRunGetGlyphs(run, all, sc.glyphs.as_mut_ptr());
        ffi::CTRunGetAdvances(run, all, sc.advances.as_mut_ptr());
        ffi::CTRunGetPositions(run, all, sc.positions.as_mut_ptr());
        ffi::CTRunGetStringIndices(run, all, sc.indices.as_mut_ptr());
        let run_font =
            ffi::CFDictionaryGetValue(ffi::CTRunGetAttributes(run), ffi::kCTFontAttributeName);
        let run_face = if run_font.is_null() || run_font == font.get() {
            face
        } else {
            match Cf::new(ffi::CFRetain(run_font)) {
                Some(f) => fonts.intern(f),
                None => face,
            }
        };
        for k in 0..n {
            let (a, p) = (sc.advances[k], sc.positions[k]);
            out.push(Glyph {
                id: sc.glyphs[k],
                face: run_face,
                advance: a.width as f32,
                dx: (p.x - pen) as f32,
                dy: p.y as f32,
                cluster: base + sc.indices[k] as u32,
            });
            pen += a.width;
        }
        pen
    }
}
