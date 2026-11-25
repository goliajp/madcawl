//! The display list's paint order and clips, and the raster read back pixel by pixel.

use madcawl_css::style::{Device, Styles, style_document};
use madcawl_dom::Dom;
use madcawl_html::{DocumentParser, Options, Source, Step};
use madcawl_layout::Layout;
use madcawl_paint::list::{MAX_ENTRIES, Rgba};
use madcawl_paint::{DisplayList, Item, Raster, RectF, paint};

fn parse(html: &str) -> Dom {
    let mut p = DocumentParser::new(Dom::new, Options::default(), Source::default());
    p.feed(html.as_bytes());
    p.end();
    assert_eq!(p.run(), Step::Done);
    p.into_sink()
}

fn style(dom: &Dom) -> Styles {
    style_document(dom, Device::default(), &mut |_| None)
}

const VIEW: RectF = RectF {
    x: 0.0,
    y: 0.0,
    w: 200.0,
    h: 100.0,
};

fn list_of(html: &str) -> DisplayList {
    let dom = parse(html);
    let styles = style(&dom);
    let mut l = Layout::new(&dom, &styles, VIEW.w, VIEW.h);
    paint(&mut l, &dom, &styles, None)
}

// the red channel of every solid rectangle, in paint order, identifying the boxes of a test page
fn rect_reds(list: &DisplayList) -> Vec<u8> {
    list.entries
        .iter()
        .filter_map(|e| match e.item {
            Item::Rect { color, .. } => Some(color.0[0]),
            _ => None,
        })
        .collect()
}

#[test]
fn paint_order_follows_stacking_layers() {
    // in tree order: z 1, auto positioned, float, block, z -1; each background's red channel names it
    let list = list_of(
        "<body style='margin:0'>\
         <div style='position:absolute;z-index:1;width:5px;height:5px;background:rgb(5,0,0)'></div>\
         <div style='position:relative;width:5px;height:5px;background:rgb(4,0,0)'></div>\
         <div style='float:left;width:5px;height:5px;background:rgb(3,0,0)'></div>\
         <div style='height:5px;background:rgb(2,0,0)'>text</div>\
         <div style='position:absolute;z-index:-1;width:5px;height:5px;background:rgb(1,0,0)'></div>",
    );
    assert_eq!(rect_reds(&list), [1, 2, 3, 4, 5]);
    // the text of the block comes after the float and before the positioned boxes
    let glyphs = list
        .entries
        .iter()
        .position(|e| matches!(e.item, Item::Glyphs { .. }))
        .expect("the text is painted");
    let reds: Vec<usize> = (0..list.entries.len())
        .filter(|&k| matches!(list.entries[k].item, Item::Rect { .. }))
        .collect();
    assert!(reds[2] < glyphs && glyphs < reds[3], "{reds:?} {glyphs}");
}

#[test]
fn negative_z_paints_below_positive_by_value() {
    let list = list_of(
        "<body style='margin:0'>\
         <div style='position:absolute;z-index:3;width:5px;height:5px;background:rgb(3,0,0)'></div>\
         <div style='position:absolute;z-index:-1;width:5px;height:5px;background:rgb(2,0,0)'></div>\
         <div style='position:absolute;z-index:-5;width:5px;height:5px;background:rgb(1,0,0)'></div>\
         <div style='position:absolute;z-index:2;width:5px;height:5px;background:rgb(4,0,0)'></div>",
    );
    assert_eq!(rect_reds(&list), [1, 2, 4, 3]);
}

#[test]
fn overflow_clips_contents_to_the_padding_box() {
    let list = list_of(
        "<body style='margin:0'><div style='overflow:hidden;width:50px;height:20px;border:2px solid black'>\
         <div style='width:300px;height:300px;background:rgb(9,0,0)'></div></div>",
    );
    let inner = list
        .entries
        .iter()
        .find(|e| matches!(e.item, Item::Rect { color, .. } if color.0[0] == 9))
        .expect("the inner box is painted");
    assert_ne!(inner.clip, 0);
    let r = list.clips[inner.clip as usize].rect;
    assert_eq!([r.x, r.y, r.w, r.h], [2.0, 2.0, 50.0, 20.0]);
    // the border itself is not clipped
    let border = list
        .entries
        .iter()
        .find(|e| matches!(e.item, Item::Border { .. }))
        .expect("the border is painted");
    assert_eq!(border.clip, 0);
}

#[test]
fn area_culls_what_lies_outside() {
    let dom = parse(
        "<body style='margin:0'><div style='height:50px;background:red'></div>\
         <div style='margin-top:500px;height:50px;background:blue'></div>",
    );
    let styles = style(&dom);
    let mut l = Layout::new(&dom, &styles, VIEW.w, VIEW.h);
    let list = paint(&mut l, &dom, &styles, Some(VIEW));
    assert_eq!(rect_reds(&list), [255]);
    assert_eq!(list.culled, 1);
}

fn raster(html: &str, scale: f32) -> madcawl_paint::Bitmap {
    let dom = parse(html);
    let styles = style(&dom);
    let mut l = Layout::new(&dom, &styles, VIEW.w, VIEW.h);
    let list = paint(&mut l, &dom, &styles, Some(VIEW));
    Raster { view: VIEW, scale }.draw(&list, &mut l.shaper().fonts)
}

#[test]
fn raster_reads_back_boxes_at_scale() {
    let bm = raster(
        "<body style='margin:0;background:rgb(0,0,255)'>\
         <div style='margin:10px;width:20px;height:10px;background:rgb(255,0,0);border-bottom:2px solid rgb(0,128,0)'></div>",
        2.0,
    );
    assert_eq!((bm.width, bm.height), (400, 200));
    // bgra, premultiplied and opaque
    assert_eq!(
        bm.pixel(1, 1),
        [255, 0, 0, 255],
        "the canvas takes the body's color"
    );
    assert_eq!(bm.pixel(30, 30), [0, 0, 255, 255], "inside the box");
    assert_eq!(bm.pixel(30, 41), [0, 128, 0, 255], "the bottom border");
    assert_eq!(bm.pixel(70, 30), [255, 0, 0, 255], "right of the box");
    let one = raster(
        "<body style='margin:0'><div style='width:20px;height:10px;background:red'>",
        1.0,
    );
    assert_eq!((one.width, one.height), (200, 100));
    assert_eq!(one.pixel(19, 9), [0, 0, 255, 255]);
    assert_eq!(one.pixel(20, 9), [255, 255, 255, 255]);
}

#[test]
fn raster_draws_text_upright_above_its_baseline() {
    let bm = raster(
        "<body style='margin:0;font:40px/50px Helvetica'><div>HHHH</div>",
        1.0,
    );
    // ink rows: an upright H has its ink above the baseline and none far below it
    let rows: Vec<usize> = (0..bm.height)
        .filter(|&y| (0..bm.width).any(|x| bm.pixel(x, y)[0] < 128))
        .collect();
    let (top, bottom) = (rows[0], rows[rows.len() - 1]);
    assert!(top >= 5 && bottom <= 45, "ink from {top} to {bottom}");
    assert!(
        bottom - top >= 25,
        "an H of 40px is about 29px tall: {top} to {bottom}"
    );
    // the glyphs start at the left of the line
    let left = (0..bm.width).find(|&x| (0..bm.height).any(|y| bm.pixel(x, y)[0] < 128));
    assert!(left.is_some_and(|x| x < 8), "{left:?}");
}

#[test]
fn clip_holds_in_the_raster() {
    let bm = raster(
        "<body style='margin:0'><div style='overflow:hidden;width:10px;height:10px'>\
         <div style='width:100px;height:100px;background:rgb(255,0,0)'></div></div>",
        1.0,
    );
    assert_eq!(bm.pixel(5, 5), [0, 0, 255, 255]);
    assert_eq!(bm.pixel(15, 5), [255, 255, 255, 255]);
    assert_eq!(bm.pixel(5, 15), [255, 255, 255, 255]);
}

#[test]
fn decorations_and_images_raster_from_a_list() {
    let mut list = DisplayList::default();
    let r = RectF {
        x: 2.0,
        y: 4.0,
        w: 20.0,
        h: 1.0,
    };
    let color = Rgba([0, 0, 0, 255]);
    list.push(Item::Decoration { rect: r, color }, r, 0);
    let img = RectF {
        x: 30.0,
        y: 10.0,
        w: 20.0,
        h: 20.0,
    };
    list.push(Item::Image { rect: img }, img, 0);
    let mut fonts = madcawl_text::Fonts::new();
    let bm = Raster {
        view: VIEW,
        scale: 1.0,
    }
    .draw(&list, &mut fonts);
    assert_eq!(bm.pixel(10, 4), [0, 0, 0, 255]);
    assert_eq!(bm.pixel(10, 5), [255, 255, 255, 255]);
    assert_ne!(
        bm.pixel(30, 20),
        [255, 255, 255, 255],
        "the placeholder's outline"
    );
    assert_eq!(bm.pixel(40, 20), [255, 255, 255, 255], "its inside");
}

#[test]
fn entries_stop_at_the_cap() {
    let mut list = DisplayList::default();
    let r = RectF {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };
    for _ in 0..MAX_ENTRIES + 10 {
        list.push(Item::Image { rect: r }, r, 0);
    }
    assert_eq!(list.entries.len(), MAX_ENTRIES);
    assert_eq!(list.dropped, 10);
}

#[test]
fn bitmap_size_stays_under_the_side_and_pixel_caps() {
    use madcawl_paint::raster::{MAX_PIXELS, MAX_SIDE};
    let at = |w: f32, h: f32, scale: f32| {
        Raster {
            view: RectF {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
            scale,
        }
        .device_size()
    };
    assert_eq!(at(1280.0, 800.0, 2.0), (2560, 1600));
    assert_eq!(at(1280.0, 1e6, 1.0), (1280, MAX_SIDE));
    let (w, h) = at(16384.0, 16384.0, 1.0);
    assert_eq!((w, h), (MAX_SIDE, MAX_PIXELS / MAX_SIDE));
}
