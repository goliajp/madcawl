//! Glyph runs placed where layout put their text, and inline boxes split across lines.

mod common;

use common::{glyph_runs, list_and_runs, list_of};
use madcawl_paint::{DisplayList, Item};

// runs here start with a glyph, not a space, so the first glyph is at the run's left edge
// the x of the first glyph of every glyph run, in list order
fn run_xs(list: &DisplayList) -> Vec<f32> {
    glyph_runs(list)
        .into_iter()
        .map(|k| match &list.entries[k].item {
            Item::Glyphs { glyphs, .. } => list.glyphs[glyphs.start as usize].x,
            _ => unreachable!(),
        })
        .collect()
}

fn assert_glyphs_at_runs(html: &str) {
    let (list, xs) = list_and_runs(html);
    let got = run_xs(&list);
    assert_eq!(
        got.len(),
        xs.len(),
        "a glyph run per text run: {got:?} {xs:?}"
    );
    for (g, x) in got.iter().zip(&xs) {
        assert!((g - x).abs() < 0.05, "glyphs at {got:?}, runs at {xs:?}");
    }
}

#[test]
fn glyphs_start_at_their_run_across_lines() {
    assert_glyphs_at_runs(
        "<body style='margin:0;font:20px Helvetica'><div style='width:70px'>aaa bbb ccc ddd eee</div>",
    );
}

#[test]
fn glyphs_start_at_their_run_after_padded_inline_boxes() {
    assert_glyphs_at_runs(
        "<body style='margin:0;font:20px/25px Helvetica'><div style='width:120px'>aaa \
         <span style='padding-left:30px;border-left:2px solid red'>bbb ccc ddd </span>eee fff ggg</div>",
    );
}

#[test]
fn glyphs_start_at_their_run_over_several_text_nodes() {
    assert_glyphs_at_runs(
        "<body style='margin:0;font:16px Helvetica'><div style='width:90px'>one <i>two three </i>\
         four<b>five six </b>seven <span style='padding:0 7px'>eight</span>nine</div>",
    );
}

#[test]
fn zero_width_segments_stay_on_their_line() {
    // the long word does not fit after "aaa ", so the zero width space and the combining mark before it
    // start the second line with it: the first line has the glyphs of "aaa" only
    let list = list_of(
        "<body style='margin:0;font:20px/30px Helvetica'><div style='width:50px'>\
         aaa &#x200B;&#x301;bbbbbbbbb</div>",
    );
    let first_line = list.glyphs.iter().filter(|g| g.y < 30.0);
    let xs: Vec<f32> = first_line.map(|g| g.x).collect();
    assert_eq!(xs.len(), 3, "{:?}", list.glyphs);
    assert!(xs.iter().all(|&x| x < 34.0), "{xs:?}");
}

#[test]
fn right_to_left_words_run_right_to_left() {
    let list = list_of(
        "<body style='margin:0;font:20px Helvetica'><p style='direction:rtl;margin:0'>\
         &#x5D0;&#x5D1;&#x5D2; &#x5D3;&#x5D4;&#x5D5;</p>",
    );
    let g = &list.glyphs;
    assert!(g.len() >= 6, "{g:?}");
    // the first word in logical order is the rightmost
    let first = g[..3].iter().map(|p| p.x).fold(f32::MIN, f32::max);
    let second = g[g.len() - 3..]
        .iter()
        .map(|p| p.x)
        .fold(f32::MIN, f32::max);
    assert!(first > second, "{g:?}");
}

// the border widths of each piece of the bordered box, in paint order
fn border_pieces(list: &DisplayList) -> Vec<[f32; 4]> {
    list.entries
        .iter()
        .filter_map(|e| match e.item {
            Item::Border { widths, .. } => Some(widths),
            _ => None,
        })
        .collect()
}

#[test]
fn split_inline_box_slices_its_borders() {
    let list = list_of(
        "<body style='margin:0;font:20px Helvetica'><div style='width:60px'>\
         aaa <span style='border:2px solid red'>bbb ccc ddd</span> eee</div>",
    );
    let p = border_pieces(&list);
    assert_eq!(p.len(), 3, "{p:?}");
    // top, right, bottom, left: the start edge only on the first piece, the end edge only on the last
    assert_eq!(p[0], [2.0, 0.0, 2.0, 2.0]);
    assert_eq!(p[1], [2.0, 0.0, 2.0, 0.0]);
    assert_eq!(p[2], [2.0, 2.0, 2.0, 0.0]);
}

#[test]
fn unsplit_inline_box_keeps_all_borders() {
    let list = list_of(
        "<body style='margin:0;font:20px Helvetica'>a <span style='border:2px solid red'>b</span> c",
    );
    assert_eq!(border_pieces(&list), [[2.0; 4]]);
}
