//! Paint order of CSS 2 appendix E beyond plain blocks: positioned and translucent inline boxes, boxes that
//! paint as if they made a stacking context, and the clips out-of-flow boxes are drawn under.

mod common;

use common::{glyph_runs, list_of, raster, rect_at, rect_reds};
use madcawl_paint::Item;

#[test]
fn positioned_inline_box_paints_its_background_under_its_text() {
    let page = "<body style='margin:0;font:40px/50px Helvetica'>\
                <span style='position:relative;background:rgb(255,255,0)'>HHHH</span>";
    let list = list_of(page);
    let runs = glyph_runs(&list);
    assert_eq!(runs.len(), 1);
    assert!(rect_at(&list, 255) < runs[0], "{:?}", list.entries);
    // dark ink shows inside the yellow background
    let bm = raster(page);
    let ink = (0..50).any(|y| (0..150).any(|x| bm.pixel(x, y)[2] < 100));
    assert!(ink, "the text is visible over its background");
}

#[test]
fn translucent_inline_box_fades_its_text() {
    let list = list_of(
        "<body style='margin:0'><span style='opacity:0.5;background:rgb(9,0,0)'>text</span>",
    );
    let runs = glyph_runs(&list);
    assert!(rect_at(&list, 9) < runs[0]);
    let Item::Glyphs { color, .. } = &list.entries[runs[0]].item else {
        unreachable!()
    };
    assert_eq!(color.0[3], 128);
}

#[test]
fn text_inside_a_nested_inline_box_paints_with_the_positioned_one() {
    let list = list_of(
        "<body style='margin:0'><p style='background:rgb(1,0,0)'>a \
         <span style='position:relative;background:rgb(2,0,0)'>b <b style='background:rgb(3,0,0)'>c</b></span> d</p>",
    );
    // the paragraph, then its own text a and d, then the positioned span, its nested box and their text
    assert_eq!(rect_reds(&list), [1, 2, 3]);
    let runs = glyph_runs(&list);
    assert_eq!(runs.len(), 4, "{:?}", list.entries);
    assert!(runs[1] < rect_at(&list, 2) && rect_at(&list, 3) < runs[2]);
}

#[test]
fn negative_child_of_an_auto_positioned_box_joins_the_root_context() {
    // appendix E: z-index -1 paints in the root context's step 2, under its in-flow blocks (1) and under its
    // parent (3), whose z-index auto makes no stacking context; the canvas (body) is under all of them
    let list = list_of(
        "<body style='margin:0;background:rgb(0,0,9)'>\
         <div style='height:20px;background:rgb(1,0,0)'></div>\
         <div style='position:relative;height:20px;background:rgb(3,0,0)'>\
         <div style='position:absolute;z-index:-1;width:5px;height:5px;background:rgb(2,0,0)'></div></div>",
    );
    assert_eq!(rect_reds(&list), [2, 1, 3]);
}

#[test]
fn negative_child_of_a_stacking_context_stays_in_it() {
    let list = list_of(
        "<body style='margin:0'><div style='height:20px;background:rgb(1,0,0)'></div>\
         <div style='position:relative;z-index:0;height:20px;background:rgb(3,0,0)'>\
         <div style='position:absolute;z-index:-1;width:5px;height:5px;background:rgb(2,0,0)'></div></div>",
    );
    assert_eq!(rect_reds(&list), [1, 3, 2]);
}

#[test]
fn positioned_child_of_a_float_paints_after_the_inline_content() {
    let list = list_of(
        "<body style='margin:0'><div style='float:left;width:20px;height:20px;background:rgb(1,0,0)'>\
         <div style='position:relative;width:5px;height:5px;background:rgb(2,0,0)'></div></div>\
         <div>text</div>",
    );
    let runs = glyph_runs(&list);
    assert!(rect_at(&list, 1) < runs[0] && runs[0] < rect_at(&list, 2));
}

#[test]
fn positioned_child_of_an_inline_block_paints_after_the_line() {
    let list = list_of(
        "<body style='margin:0'><span style='display:inline-block;background:rgb(1,0,0)'>\
         <span style='position:relative;background:rgb(2,0,0)'>x</span></span> after",
    );
    // the line's text "after", then the positioned span and its "x" over it
    let runs = glyph_runs(&list);
    let span = rect_at(&list, 2);
    assert!(
        rect_at(&list, 1) < span && runs[0] < span && span < runs[1],
        "{:?}",
        list.entries
    );
}

fn clip_of(list: &madcawl_paint::DisplayList, red: u8) -> u32 {
    list.entries[rect_at(list, red)].clip
}

#[test]
fn absolute_box_escapes_the_clip_of_a_box_below_its_containing_block() {
    let list = list_of(
        "<body style='margin:0'><div style='position:relative'>\
         <div style='overflow:hidden;width:10px;height:10px'>\
         <div style='position:absolute;width:50px;height:50px;background:rgb(1,0,0)'></div>\
         <div style='width:50px;height:50px;background:rgb(2,0,0)'></div></div></div>",
    );
    assert_eq!(clip_of(&list, 1), 0);
    assert_ne!(clip_of(&list, 2), 0);
}

#[test]
fn absolute_box_keeps_the_clip_of_its_containing_block() {
    let list = list_of(
        "<body style='margin:0'><div style='position:relative;overflow:hidden;width:10px;height:10px'>\
         <div><div style='position:absolute;width:50px;height:50px;background:rgb(1,0,0)'></div></div></div>",
    );
    let c = clip_of(&list, 1);
    assert_ne!(c, 0);
    let r = list.clips[c as usize].rect;
    assert_eq!([r.w, r.h], [10.0, 10.0]);
}

#[test]
fn fixed_box_is_not_clipped_by_its_ancestors() {
    let list = list_of(
        "<body style='margin:0'><div style='position:relative;overflow:hidden;width:10px;height:10px'>\
         <div style='position:fixed;width:50px;height:50px;background:rgb(1,0,0)'></div></div>",
    );
    assert_eq!(clip_of(&list, 1), 0);
}
