//! Reference marks drawn over a raster, and PNG bytes made in memory.

mod common;

use common::{VIEW, raster};
use madcawl_paint::raster::{MAX_MARKS, encode_png};
use madcawl_paint::{Bitmap, Mark, Raster, RectF};

const PAGE: &str = "<body style='margin:0;background:white'><div style='height:100px'></div>";

fn white(px: [u8; 4]) -> bool {
    px == [255, 255, 255, 255]
}

#[test]
fn marks_outline_the_element_and_label_it_with_white_text() {
    let mut bm = raster(PAGE);
    assert!(white(bm.pixel(60, 60)));
    let mark = Mark {
        rect: RectF {
            x: 40.0,
            y: 40.0,
            w: 100.0,
            h: 40.0,
        },
        label: "e3k".into(),
    };
    let r = Raster {
        view: VIEW,
        scale: 1.0,
    };
    assert_eq!(r.draw_marks(&mut bm, &[mark]), 1);
    // the outline runs along the rectangle's edge, its inside stays as it was
    assert!(!white(bm.pixel(90, 41)), "outline at the top edge");
    assert!(!white(bm.pixel(41, 60)), "outline at the left edge");
    assert!(white(bm.pixel(90, 60)), "inside untouched");
    // the label sits above the top left corner: a colored box with white glyph pixels in it
    let label: Vec<[u8; 4]> = (24..40)
        .flat_map(|y| (40..70).map(move |x| (x, y)))
        .map(|(x, y)| bm.pixel(x, y))
        .collect();
    let colored = label.iter().filter(|p| !white(**p)).count();
    let lit = label.iter().filter(|p| white(**p)).count();
    assert!(
        colored > label.len() / 2,
        "a filled label box: {colored} of {}",
        label.len()
    );
    assert!(lit > 4, "white text in the label: {lit}");
}

#[test]
fn marks_keep_their_device_size_on_a_shrunken_picture() {
    // a black bitmap of the view at a quarter scale: 50x25 device px
    let mut bm = Bitmap {
        width: 50,
        height: 25,
        data: [0, 0, 0, 255].repeat(50 * 25),
    };
    let r = Raster {
        view: VIEW,
        scale: 0.25,
    };
    let mark = Mark {
        rect: RectF {
            x: 40.0,
            y: 40.0,
            w: 100.0,
            h: 40.0,
        },
        label: "e3k".into(),
    };
    assert_eq!(r.draw_marks(&mut bm, &[mark]), 1);
    let black = |p: [u8; 4]| p[..3] == [0, 0, 0];
    let lit = |p: [u8; 4]| p[..3].iter().all(|&c| c > 200);
    // the outline is two device px wide at the element's right edge, device x 34
    assert!(
        !black(bm.pixel(34, 15)) && !black(bm.pixel(33, 15)),
        "outline"
    );
    // the label is a box about 17 device px tall, not shrunk to a quarter of it
    let rows = (0..25).filter(|&y| !black(bm.pixel(11, y))).count();
    assert!(rows >= 13, "label box rows at device x 11: {rows}");
    let text = (0..25)
        .flat_map(|y| (10..40).map(move |x| (x, y)))
        .filter(|&(x, y)| lit(bm.pixel(x, y)))
        .count();
    assert!(text > 8, "white glyph pixels in the label: {text}");
}

#[test]
fn marks_past_the_cap_are_left_out_and_none_draws_nothing() {
    let mut bm = raster(PAGE);
    let before = bm.data.clone();
    let r = Raster {
        view: VIEW,
        scale: 1.0,
    };
    assert_eq!(r.draw_marks(&mut bm, &[]), 0);
    assert_eq!(bm.data, before);
    let many = vec![
        Mark {
            rect: RectF {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0
            },
            label: "e1".into()
        };
        MAX_MARKS + 5
    ];
    assert_eq!(r.draw_marks(&mut bm, &many), MAX_MARKS);
}

#[test]
fn png_bytes_carry_the_bitmap_size() {
    let bm = raster(PAGE);
    let png = encode_png(&bm).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[12..16], b"IHDR");
    let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!((w as usize, h as usize), (bm.width, bm.height));
}
