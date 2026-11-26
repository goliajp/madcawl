//! A screenshot of a windowless page: its viewport where it is scrolled
//! to, the whole page, or one element by reference, laid out, painted
//! and rastered offscreen only when asked and dropped once it is
//! encoded. Marks with each interactive element's reference are drawn
//! from the semantic tree over the raster, after the page, so nothing
//! the page paints can cover or move one. A page can still paint boxes
//! that look like marks: a reference is to be trusted only from the
//! list of marked elements in the reply's text, never read off the
//! picture alone.

use std::time::{Duration, Instant};

use madcawl_agent::{Frame, Marked, Rect, tag};
use madcawl_layout::Layout;
use madcawl_paint::raster::{MAX_MARKS, MAX_SIDE, encode_png};
use madcawl_paint::{Bitmap, Mark, Raster, RectF};

use super::page::Page;

/// The most device pixels a screenshot holds (64 MiB of BGRA); a taller
/// area is cut where it reaches them, as a wider or taller one is at
/// [`MAX_SIDE`] device pixels a side.
pub const MAX_SHOT_PIXELS: f32 = (16 << 20) as f32;
/// The largest PNG a reply carries.
pub const MAX_SHOT_BYTES: usize = 8 << 20;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Area {
    Viewport,
    Page,
    Element(String),
}

#[derive(Clone, Debug)]
pub(crate) struct ShotRequest {
    pub area: Area,
    /// Device pixels per CSS px
    pub scale: f32,
    pub marks: bool,
    /// A file name to save the picture under instead of sending it
    pub file: Option<String>,
}

impl ShotRequest {
    /// The request a frame carries; Err with why it cannot be taken.
    pub fn from_frame(f: &Frame) -> Result<ShotRequest, String> {
        let area = match (f.get_num(tag::SCOPE).unwrap_or(1), f.get_str(tag::REF)) {
            (2, Some(r)) if !r.trim().is_empty() => Area::Element(r.trim().to_owned()),
            (2, _) => return Err("an element screenshot needs a ref".into()),
            (0, _) => Area::Page,
            (1, _) => Area::Viewport,
            (n, _) => return Err(format!("unknown screenshot scope {n}")),
        };
        let hundredths = f.get_num(tag::SCALE).unwrap_or(100);
        if !(10..=200).contains(&hundredths) {
            return Err("scale must be between 0.1 and 2".into());
        }
        let marks = f.get_num(tag::MARKS).unwrap_or(0) != 0;
        let file = f
            .get_str(tag::FILE)
            .map(str::trim)
            .filter(|n| !n.is_empty());
        Ok(ShotRequest {
            area,
            scale: hundredths as f32 / 100.0,
            marks,
            file: file.map(str::to_owned),
        })
    }
}

/// What a screenshot drew.
pub(crate) struct Taken {
    pub png: Vec<u8>,
    pub width: usize,
    pub height: usize,
    /// The part of the page drawn, in CSS px
    pub area: RectF,
    /// The area was cut at the pixel cap
    pub cut: bool,
    pub marks: Vec<Marked>,
    pub times: [(&'static str, Duration); 4],
}

fn rect(r: Rect) -> RectF {
    RectF {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
    }
}

/// `area` cut to the canvas, to at most [`MAX_SIDE`] device pixels a
/// side and [`MAX_SHOT_PIXELS`] in all at `scale`, so the raster draws
/// all of what is returned; whether it was cut short.
pub(crate) fn bounded(area: RectF, scale: f32) -> (RectF, bool) {
    let x = area.x.max(0.0);
    let y = area.y.max(0.0);
    let side = MAX_SIDE as f32 / scale;
    let w = (area.right() - x).max(1.0);
    let h = (area.bottom() - y).max(1.0);
    let widest = w.min(side);
    let tallest = (MAX_SHOT_PIXELS / (widest * scale * scale))
        .min(side)
        .max(1.0);
    (
        RectF {
            x,
            y,
            w: widest,
            h: h.min(tallest),
        },
        w > side || h > tallest,
    )
}

impl Page {
    /// The area a request draws, in CSS px.
    fn shot_area(&mut self, a: &Area) -> Result<RectF, String> {
        let (w, h) = (self.width, self.height);
        Ok(match a {
            Area::Viewport => rect(self.geometry().viewport),
            Area::Page => RectF {
                x: 0.0,
                y: 0.0,
                w,
                h: self.geometry().bottom().max(h),
            },
            Area::Element(r) => {
                let n = self.resolve(r).map_err(|e| e.message().to_owned())?;
                match self.geometry().place(n) {
                    Some(p) if p.shown => rect(p.rect),
                    _ => return Err("the element is not shown: it has no box to draw".into()),
                }
            }
        })
    }

    /// Lays out, paints and rasters what `req` asks for, marks drawn over it.
    pub(crate) fn screenshot(&mut self, req: &ShotRequest) -> Result<Taken, String> {
        self.last_used = Instant::now();
        self.sync();
        let (area, cut) = bounded(self.shot_area(&req.area)?, req.scale);
        let marks = match req.marks {
            false => Vec::new(),
            true => self.marked(area)?,
        };
        let (bm, [layout, paint, raster]) = self.draw(area, req.scale, &marks);
        let t = Instant::now();
        let png = encode_png(&bm).map_err(|e| e.to_string())?;
        Ok(Taken {
            png,
            width: bm.width,
            height: bm.height,
            area,
            cut,
            marks,
            times: [
                ("layout", layout),
                ("paint", paint),
                ("raster", raster),
                ("png", t.elapsed()),
            ],
        })
    }

    /// The interactive elements in `area` the agent tree hands out refs
    /// for, the page laid out first: without geometry there are none.
    fn marked(&mut self, area: RectF) -> Result<Vec<Marked>, String> {
        self.geometry();
        let a = Rect {
            x: area.x,
            y: area.y,
            w: area.w,
            h: area.h,
        };
        let (dom, inputs) = self.doc.parts();
        Ok(self
            .agents
            .marks(self.session, dom, inputs, a, MAX_MARKS)
            .map_err(|e| e.message().to_owned())?
            .1)
    }

    /// `area` drawn at `scale` with `marks` over it, and how long the
    /// layout, the paint and the raster took.
    fn draw(&self, area: RectF, scale: f32, marks: &[Marked]) -> (Bitmap, [Duration; 3]) {
        let t = Instant::now();
        let mut layout = Layout::new(&self.doc.dom, &self.doc.styles, self.width, self.height);
        layout.fragments();
        let t_layout = t.elapsed();
        let t = Instant::now();
        let list = madcawl_paint::paint(&mut layout, &self.doc.dom, &self.doc.styles, Some(area));
        let t_paint = t.elapsed();
        let t = Instant::now();
        let r = Raster { view: area, scale };
        let mut bm = r.draw(&list, &mut layout.shaper().fonts);
        let drawn: Vec<Mark> = marks
            .iter()
            .map(|m| Mark {
                rect: rect(m.rect),
                label: m.reference.clone(),
            })
            .collect();
        r.draw_marks(&mut bm, &drawn);
        (bm, [t_layout, t_paint, t.elapsed()])
    }
}

/// The reply's text: what was drawn, how long each stage took, and each
/// marked element a line.
pub(crate) fn describe(req: &ShotRequest, t: &Taken) -> String {
    let what = match &req.area {
        Area::Viewport => "the viewport".to_owned(),
        Area::Page => "the page".to_owned(),
        Area::Element(r) => format!("element {r}"),
    };
    let a = t.area;
    let mut s = format!(
        "screenshot of {what} ({:.0},{:.0} {:.0}x{:.0} CSS px) at scale {}: {}x{} PNG, {} bytes\n",
        a.x,
        a.y,
        a.w,
        a.h,
        req.scale,
        t.width,
        t.height,
        t.png.len()
    );
    if t.cut {
        s.push_str("cut short at the pixel cap; pass a smaller scale to see more\n");
    }
    let times: Vec<String> = t
        .times
        .iter()
        .map(|(k, d)| format!("{k} {:.1} ms", d.as_secs_f64() * 1e3))
        .collect();
    s.push_str(&times.join(", "));
    s.push('\n');
    if req.marks {
        s.push_str(&format!("marks: {} interactive elements\n", t.marks.len()));
    }
    for m in &t.marks {
        let r = m.rect;
        s.push_str(&format!("[{}] {} ", m.reference, m.role.name()));
        madcawl_agent::push_quoted(&mut s, &m.name);
        s.push_str(&format!(
            " at {:.0},{:.0} {:.0}x{:.0}\n",
            r.x, r.y, r.w, r.h
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_read_and_bounded() {
        use madcawl_agent::kind;
        let f = Frame::new(kind::SCREENSHOT);
        let r = ShotRequest::from_frame(&f).unwrap();
        assert_eq!((r.area, r.scale, r.marks), (Area::Viewport, 1.0, false));
        let f = Frame::new(kind::SCREENSHOT)
            .num(tag::SCOPE, 2)
            .str(tag::REF, "e3")
            .num(tag::SCALE, 50)
            .num(tag::MARKS, 1);
        let r = ShotRequest::from_frame(&f).unwrap();
        assert_eq!(
            (r.area, r.scale, r.marks),
            (Area::Element("e3".into()), 0.5, true)
        );
        assert!(ShotRequest::from_frame(&Frame::new(kind::SCREENSHOT).num(tag::SCOPE, 2)).is_err());
        assert!(
            ShotRequest::from_frame(&Frame::new(kind::SCREENSHOT).num(tag::SCALE, 500)).is_err()
        );
        assert!(ShotRequest::from_frame(&Frame::new(kind::SCREENSHOT).num(tag::SCOPE, 3)).is_err());
        let f = Frame::new(kind::SCREENSHOT).str(tag::FILE, " a.png ");
        assert_eq!(
            ShotRequest::from_frame(&f).unwrap().file.as_deref(),
            Some("a.png")
        );
        assert_eq!(
            ShotRequest::from_frame(&Frame::new(kind::SCREENSHOT).num(tag::SCOPE, 0))
                .unwrap()
                .area,
            Area::Page
        );
    }

    #[test]
    fn a_tall_area_is_cut_at_the_pixel_cap() {
        let (a, cut) = bounded(
            RectF {
                x: -5.0,
                y: 0.0,
                w: 1280.0,
                h: 100_000.0,
            },
            2.0,
        );
        assert!(cut);
        assert_eq!(a.x, 0.0);
        assert!(a.w * a.h * 4.0 <= MAX_SHOT_PIXELS + 1.0);
        let (b, cut) = bounded(
            RectF {
                x: 0.0,
                y: 0.0,
                w: 1280.0,
                h: 800.0,
            },
            2.0,
        );
        assert!(!cut);
        assert_eq!(b.h, 800.0);
    }

    #[test]
    fn a_tall_area_at_a_small_scale_is_cut_at_the_side_cap() {
        let page = RectF {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 1_000_000.0,
        };
        let (a, cut) = bounded(page, 0.1);
        assert!(cut);
        let (w, h) = Raster {
            view: a,
            scale: 0.1,
        }
        .device_size();
        assert_eq!(h, MAX_SIDE);
        assert_eq!(
            h,
            (a.h * 0.1).round() as usize,
            "the raster draws all of the area"
        );
        assert_eq!(w, 80);
        let (wide, cut) = bounded(
            RectF {
                w: 1e6,
                h: 10.0,
                ..page
            },
            1.0,
        );
        assert!(cut);
        assert_eq!(wide.w, MAX_SIDE as f32);
    }
}

#[cfg(test)]
#[path = "shot_tests.rs"]
mod page_tests;
