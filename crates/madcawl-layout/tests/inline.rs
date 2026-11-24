mod common;

use common::*;
use madcawl_layout::Layout;

fn layout_of(html: &str) -> (madcawl_dom::Dom, madcawl_css::style::Styles) {
    let dom = parse(html);
    let st = style(&dom);
    (dom, st)
}

// a monospace face makes advances easy to reason about: every character of Menlo 10px is 6.0205px wide
const MONO: &str = "font:10px/20px Menlo;margin:0";

#[test]
fn lines_break_at_the_width_and_hang_spaces() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div id=d style='width:50px'>aaaa bbbb cccc</div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    // `aaaa bbbb` is 9 characters, more than 62px: three lines of 20px
    assert_eq!(rect(&mut l, &dom, "d")[3], 60.0);
}

#[test]
fn white_space_collapses_and_pre_keeps_it() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div id=a>  x   <span id=s>y</span></div>\
         <pre id=p style='margin:0;{MONO}'>a\nb\n\nc</pre><div id=n style='white-space:nowrap;width:10px'>a b c</div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let s = rect(&mut l, &dom, "s");
    // leading white space is removed and the run of spaces is one: `x y`
    assert!((s[0] - 2.0 * 6.0205).abs() < 0.05, "{s:?}");
    assert_eq!(rect(&mut l, &dom, "p")[3], 80.0);
    assert_eq!(rect(&mut l, &dom, "n")[3], 20.0);
}

#[test]
fn inline_box_pieces_and_their_edges() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div style='width:100px'>ab <span id=s \
         style='padding:0 5px;border-left:3px solid;margin-left:2px'>cd</span> <span id=w>ef gh ij kl mn</span></div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let s = rect(&mut l, &dom, "s");
    // the span starts after `ab ` and its margin; its border box holds its border, padding and text
    let ch = 6.0205;
    assert!((s[0] - (3.0 * ch + 2.0)).abs() < 0.05, "{s:?}");
    assert!((s[2] - (2.0 * ch + 13.0)).abs() < 0.05, "{s:?}");
    // a span that wraps: its pieces' union spans both lines
    let w = rect(&mut l, &dom, "w");
    assert!(w[3] > 30.0 && w[0] == 0.0, "{w:?}");
}

#[test]
fn br_and_empty_lines() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div id=a>a<br>b</div><div id=e>   </div><div id=c><br></div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert_eq!(rect(&mut l, &dom, "a")[3], 40.0);
    // only white space: no line box
    assert_eq!(rect(&mut l, &dom, "e")[3], 0.0);
    assert_eq!(rect(&mut l, &dom, "c")[3], 20.0);
}

#[test]
fn line_height_and_baseline_alignment() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div id=a>x<span id=big style='font-size:20px;line-height:40px'>y</span></div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    // the strut's 20px and the span's 40px share a baseline: the line is taller than either half-leading box
    let a = rect(&mut l, &dom, "a");
    assert!(a[3] >= 40.0, "{a:?}");
    let big = rect(&mut l, &dom, "big");
    assert!(big[1] > 0.0 && big[1] + big[3] <= a[3], "{big:?} in {a:?}");
}

#[test]
fn inline_block_shrinks_to_fit_and_sits_on_the_baseline() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div id=a>x<span id=ib style='display:inline-block'>abc</span>\
         <span id=w style='display:inline-block;width:50px;height:30px'></span></div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let ib = rect(&mut l, &dom, "ib");
    assert!((ib[2] - 3.0 * 6.0205).abs() < 0.05, "{ib:?}");
    let w = rect(&mut l, &dom, "w");
    assert_eq!(w[2..], [50.0, 30.0]);
    // an empty inline-block's baseline is its bottom margin edge: it sits on the line's baseline
    let a = rect(&mut l, &dom, "a");
    assert!(a[3] >= 30.0, "{a:?}");
}

#[test]
fn anonymous_blocks_wrap_inline_runs_among_blocks() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div id=p>text<div id=b style='height:5px'></div>more</div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert_eq!(rect(&mut l, &dom, "b"), [0.0, 20.0, 1280.0, 5.0]);
    assert_eq!(rect(&mut l, &dom, "p")[3], 45.0);
}

#[test]
fn a_block_inside_an_inline_splits_it() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div id=p><span id=s>a<div id=b style='height:5px'></div>b</span></div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert_eq!(rect(&mut l, &dom, "b"), [0.0, 20.0, 1280.0, 5.0]);
    // the span has a piece above the block and one below it
    let s = rect(&mut l, &dom, "s");
    assert!(s[1] < 20.0 && s[1] + s[3] > 25.0, "{s:?}");
    assert_eq!(rect(&mut l, &dom, "p")[3], 45.0);
}

#[test]
fn text_align_center_and_right() {
    let (dom, st) = layout_of(&format!(
        "<!doctype html><body style='{MONO}'><div style='width:100px;text-align:center'><span id=c>ab</span></div>\
         <div style='width:100px;text-align:right'><span id=r>ab</span></div>"
    ));
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let w = 2.0 * 6.0205;
    assert!((rect(&mut l, &dom, "c")[0] - (100.0 - w) / 2.0).abs() < 0.05);
    assert!((rect(&mut l, &dom, "r")[0] - (100.0 - w)).abs() < 0.05);
}

// what WebKit gives the same markup
#[test]
fn atomic_inlines_aligned_with_the_top_or_bottom_of_their_line() {
    let (dom, st) = layout_of(
        "<!doctype html><body style='margin:0;font:16px/20px Menlo'><div id=l1>x<span id=a \
         style='display:inline-block;width:10px;height:50px;vertical-align:top'></span>y</div><div id=l2>x\
         <span id=b style='display:inline-block;width:10px;height:50px;vertical-align:bottom'></span>y</div>\
         <div id=l3>x<span id=c style='display:inline-block;width:10px;height:5px;vertical-align:top'></span>\
         <span id=d style='display:inline-block;width:10px;height:5px;vertical-align:bottom'></span><span \
         id=e style='display:inline-block;width:10px;height:40px'></span></div>",
    );
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let want: [(&str, [f32; 4]); 8] = [
        ("l1", [0.0, 0.0, 1280.0, 50.0]),
        ("a", [9.625, 0.0, 10.0, 50.0]),
        ("l2", [0.0, 50.0, 1280.0, 50.0]),
        ("b", [9.625, 50.0, 10.0, 50.0]),
        ("l3", [0.0, 100.0, 1280.0, 44.453125]),
        ("c", [9.625, 100.0, 10.0, 5.0]),
        ("d", [19.625, 139.45312, 10.0, 5.0]),
        ("e", [29.625, 100.0, 10.0, 40.0]),
    ];
    for (id, r) in want {
        let got = rect(&mut l, &dom, id);
        let close = got.iter().zip(r).all(|(a, b)| (a - b).abs() <= 0.02);
        assert!(close, "#{id}: {got:?}, webkit {r:?}");
    }
}
