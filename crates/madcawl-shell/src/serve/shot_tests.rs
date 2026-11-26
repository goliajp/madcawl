//! Screenshots of a page: marks are in the pixels, a reference handed
//! out before the page changed is refused, and a tall page at a small
//! scale is cut where the raster stops.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use madcawl_paint::raster::MAX_SIDE;

use super::*;

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("mcs-shot-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn load(&self, p: &mut Page, name: &str, html: &str) {
        let path = self.0.join(name);
        std::fs::write(&path, html).unwrap();
        let url = format!("file://{}", path.display());
        p.load_file(&path, url, None, std::slice::from_ref(&self.0))
            .unwrap();
    }

    fn page(&self, html: &str) -> Page {
        let mut p = Page::blank(1, (1280.0, 800.0), None, 1);
        self.load(&mut p, "p.html", html);
        p.sync();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const LINK: &str = "<body style='margin:0;background:white'><a href=x style='display:block;margin:100px;width:200px;height:40px'>Go</a>";

fn shot(area: Area, scale: f32, marks: bool) -> ShotRequest {
    ShotRequest {
        area,
        scale,
        marks,
        file: None,
    }
}

#[test]
fn marks_are_drawn_into_the_pixels_over_the_element() {
    let d = Dir::new();
    let mut p = d.page(LINK);
    let area = RectF {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 800.0,
    };
    let marks = p.marked(area).unwrap();
    assert_eq!(marks.len(), 1, "the link");
    let r = marks[0].rect;
    let (plain, _) = p.draw(area, 1.0, &[]);
    let (marked, _) = p.draw(area, 1.0, &marks);
    // the outline's first color, red, along the element's top edge, as BGRA
    let (x, y) = ((r.x + r.w / 2.0) as usize, r.y as usize);
    assert_eq!(plain.pixel(x, y), [255, 255, 255, 255]);
    assert_eq!(marked.pixel(x, y), [40, 39, 214, 255]);
    // inside the box the page shows through
    let mid = (r.y + r.h / 2.0) as usize;
    assert_eq!(marked.pixel(x, mid), plain.pixel(x, mid));
    let t = p.screenshot(&shot(Area::Viewport, 1.0, true)).unwrap();
    assert_eq!(t.marks.len(), 1);
    assert!(
        describe(&shot(Area::Viewport, 1.0, true), &t)
            .contains(&format!("[{}] link \"Go\"", t.marks[0].reference))
    );
}

#[test]
fn a_reference_from_before_the_page_changed_is_refused() {
    let d = Dir::new();
    let mut p = d.page(LINK);
    let area = RectF {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 800.0,
    };
    let r = p.marked(area).unwrap()[0].reference.clone();
    assert!(
        p.screenshot(&shot(Area::Element(r.clone()), 1.0, false))
            .is_ok()
    );
    d.load(&mut p, "q.html", "<body><a href=y>Other</a>");
    let e = p
        .screenshot(&shot(Area::Element(r), 1.0, false))
        .err()
        .expect("a stale reference is refused");
    assert!(e.contains("stale"), "{e}");
}

#[test]
fn a_tall_page_at_a_small_scale_comes_back_cut_at_the_side_cap() {
    let d = Dir::new();
    let mut p =
        d.page("<body style='margin:0'><div style='height:1000000px;background:green'></div>");
    let t = p.screenshot(&shot(Area::Page, 0.1, false)).unwrap();
    assert!(t.cut);
    assert_eq!(t.height, MAX_SIDE);
    assert_eq!(
        t.height,
        (t.area.h * 0.1).round() as usize,
        "the area named is the area drawn"
    );
    assert_eq!(t.width, 128);
}
