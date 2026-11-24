//! Floats, clearance, formatting contexts beside floats, and absolute positions; the expected rectangles are
//! what WebKit gives the same markup.

mod common;

use common::*;
use madcawl_layout::Layout;

const PAGE: &str = "<!doctype html><body style='margin:0;font:16px/20px Menlo'>\
<div id=c1 style='width:400px'>\
<div id=f1 style='float:left;width:100px;height:50px'></div>\
<div id=f2 style='float:right;width:50px;height:80px'></div>\
<p id=p1 style='margin:0'>aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn oooo pppp</p>\
<div id=bf style='overflow:hidden;height:10px'></div>\
<div id=cl style='clear:both;height:5px'></div>\
<span id=sp>x<span id=fi style='float:left;width:30px;height:30px'></span>y</span>\
</div>\
<div id=c2 style='overflow:hidden'><div id=f3 style='float:left;width:10px;height:100px'></div></div>\
<div id=c3><div id=f4 style='float:left;width:10px;height:100px'></div></div>\
<div id=after style='height:5px'></div>\
<div id=ab1 style='position:relative;height:40px;margin-top:30px'><span id=sab>x<b id=ab2 \
style='position:absolute;top:0;left:100%;width:5px;height:5px'></b></span></div>";

#[test]
fn floats_shorten_lines_and_are_held_by_their_formatting_context() {
    let dom = parse(PAGE);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let want: [(&str, [f32; 4]); 12] = [
        ("c1", [0.0, 0.0, 400.0, 115.0]),
        ("f1", [0.0, 0.0, 100.0, 50.0]),
        ("f2", [350.0, 0.0, 50.0, 80.0]),
        // four lines of 20px beside and then below the floats
        ("p1", [0.0, 0.0, 400.0, 80.0]),
        // a new formatting context goes below the floats it does not fit beside
        ("bf", [0.0, 80.0, 400.0, 10.0]),
        ("cl", [0.0, 90.0, 400.0, 5.0]),
        // a float among a line's text goes at the top of that line
        ("fi", [0.0, 95.0, 30.0, 30.0]),
        // overflow: hidden makes a formatting context, which holds its float; a plain block does not
        ("c2", [30.0, 115.0, 1250.0, 100.0]),
        ("f3", [30.0, 115.0, 10.0, 100.0]),
        ("c3", [0.0, 215.0, 1280.0, 0.0]),
        ("f4", [0.0, 215.0, 10.0, 100.0]),
        ("after", [0.0, 215.0, 1280.0, 5.0]),
    ];
    for (id, r) in want {
        assert_eq!(rect(&mut l, &dom, id), r, "#{id}");
    }
    let sp = rect(&mut l, &dom, "sp");
    assert!((sp[0] - 30.0).abs() < 0.02, "{sp:?}");
}

#[test]
fn absolute_box_in_a_relative_inline_uses_the_block_containing_it() {
    let dom = parse(PAGE);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert_eq!(rect(&mut l, &dom, "ab1"), [0.0, 250.0, 1280.0, 40.0]);
    assert_eq!(rect(&mut l, &dom, "ab2"), [1280.0, 250.0, 5.0, 5.0]);
}

#[test]
fn a_box_reused_after_a_change_still_puts_its_floats_back() {
    let dom = parse(
        "<!doctype html><body style='margin:0'><div id=a><span style='float:left;width:100px;\
         height:300px'></span>x</div><div id=b><span id=t>text</span></div><div id=c>c</div>",
    );
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 800.0, 600.0);
    let before = rect(&mut l, &dom, "t");
    assert_eq!(before[0], 100.0);
    l.invalidate(by_id(&dom, "c"));
    assert_eq!(rect(&mut l, &dom, "t"), before);
}
