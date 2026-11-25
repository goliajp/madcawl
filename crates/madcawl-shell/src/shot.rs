//! A styled page laid out in the viewport and painted offscreen to a PNG: the first screen, or the whole
//! page as tall as its fragments reach.

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use madcawl_css::style::Styles;
use madcawl_dom::Dom;
use madcawl_layout::Layout;
use madcawl_paint::raster::{self, MAX_PIXELS, MAX_SIDE};
use madcawl_paint::{Bitmap, Raster, RectF};

pub const VIEWPORT: (f32, f32) = (1280.0, 800.0);

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Device pixels per CSS px
    pub scale: f32,
    /// The whole page instead of the first screen
    pub full: bool,
}

/// What a shot drew and how long each stage took.
pub struct Shot {
    pub bitmap: Bitmap,
    pub entries: usize,
    pub glyphs: usize,
    pub dropped: u64,
    pub culled: u64,
    /// How far down the page's fragments reach, in CSS px
    pub page_height: f32,
    pub times: [(&'static str, Duration); 3],
}

// the bottom of the lowest fragment on the page
fn page_height(layout: &mut Layout) -> f32 {
    let Some(root) = layout.fragments() else {
        return 0.0;
    };
    let mut bottom = root.offset.y + root.frag.size.h;
    root.frag
        .walk(|at, f| bottom = bottom.max(root.offset.y + at.y + f.size.h));
    bottom.px()
}

/// Lays out, paints and rasters `dom` under `styles`.
pub fn shot(dom: &Dom, styles: &Styles, o: Options) -> Shot {
    let t = Instant::now();
    let mut layout = Layout::new(dom, styles, VIEWPORT.0, VIEWPORT.1);
    let tall = page_height(&mut layout);
    let t_layout = t.elapsed();
    let h = if o.full {
        let cap = (MAX_SIDE as f32).min(MAX_PIXELS as f32 / (VIEWPORT.0 * o.scale));
        tall.max(VIEWPORT.1).min(cap / o.scale)
    } else {
        VIEWPORT.1
    };
    let view = RectF {
        x: 0.0,
        y: 0.0,
        w: VIEWPORT.0,
        h,
    };
    let t = Instant::now();
    let list = madcawl_paint::paint(&mut layout, dom, styles, Some(view));
    let t_paint = t.elapsed();
    let t = Instant::now();
    let r = Raster {
        view,
        scale: o.scale,
    };
    let bitmap = r.draw(&list, &mut layout.shaper().fonts);
    let t_raster = t.elapsed();
    Shot {
        bitmap,
        entries: list.entries.len(),
        glyphs: list.glyphs.len(),
        dropped: list.dropped,
        culled: list.culled,
        page_height: tall,
        times: [
            ("layout", t_layout),
            ("paint", t_paint),
            ("raster", t_raster),
        ],
    }
}

pub fn write(s: &Shot, out: &Path) -> io::Result<()> {
    raster::write_png(&s.bitmap, out)
}
