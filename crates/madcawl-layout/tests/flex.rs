//! Flex layout; the expected rectangles are what WebKit gives the same markup (to 1/64 px).

mod common;

use common::*;
use madcawl_layout::Layout;

fn check(html: &str, want: &[(&str, [f32; 4])]) {
    same_without_reuse(html, 1280.0, 800.0);
    let dom = parse(html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    for (id, r) in want {
        let got = rect(&mut l, &dom, id);
        let close = got.iter().zip(r).all(|(a, b)| (a - b).abs() <= 0.02);
        assert!(close, "#{id}: {got:?}, webkit {r:?}");
    }
}

#[test]
fn rows_columns_wrapping_growing_and_shrinking() {
    check(
        "<!doctype html><body style='margin:0;font:16px/20px Menlo'>\
         <div id=r1 style='display:flex;width:600px'><div id=a1 style='width:100px;height:30px'></div>\
         <div id=a2 style='flex:1'>two words</div><div id=a3 style='padding:5px'>x</div></div>\
         <div id=r2 style='display:flex;justify-content:space-between;align-items:center;height:60px;width:500px'>\
         <span id=b1>left</span><div id=b2 style='height:20px;width:50px'></div><a id=b3>right side</a>\
         </div><div id=r3 style='display:flex;flex-direction:column;width:300px'><div id=c1>one</div>\
         <div id=c2 style='align-self:center'>two</div><div id=c3 style='margin:5px 10px'>three</div>\
         </div><div id=r4 style='display:flex;flex-wrap:wrap;width:250px'>\
         <div id=d1 style='width:100px;height:10px'></div><div id=d2 style='width:100px;height:20px'>\
         </div><div id=d3 style='width:100px;height:10px'></div></div>\
         <div id=r5 style='display:flex;width:200px'>\
         <div id=e1 style='flex:1 1 0'>aaaaaaaaaaaaaaaaaaaaaaaaa</div>\
         <div id=e2 style='flex:1 1 0'>b</div></div><div id=r6 style='display:flex;width:300px'>\
         <div id=g1 style='flex-shrink:1;width:200px'></div>\
         <div id=g2 style='width:200px;flex-shrink:3'></div></div>\
         <div>text <span id=if style='display:inline-flex'><b id=if1>x</b><i id=if2>yy</i>\
         </span> after</div>\
         <div id=r7 style='display:flex;flex-direction:column;height:200px;width:100px'>\
         <div id=h1 style='flex:1'></div><div id=h2 style='height:50px'></div></div>\
         <div id=r8 style='display:flex;width:300px;align-items:flex-start'>\
         <div id=k1 style='margin-left:auto;width:50px;height:5px'></div></div></body> ",
        &[
            ("r1", [0.0, 0.0, 600.0, 30.0]),
            ("a1", [0.0, 0.0, 100.0, 30.0]),
            ("a2", [100.0, 0.0, 480.359, 30.0]),
            ("a3", [580.359, 0.0, 19.641, 30.0]),
            ("r2", [0.0, 30.0, 500.0, 60.0]),
            ("b1", [0.0, 50.0, 38.531, 20.0]),
            ("b2", [196.094, 50.0, 50.0, 20.0]),
            ("b3", [403.656, 50.0, 96.328, 20.0]),
            ("r3", [0.0, 90.0, 300.0, 70.0]),
            ("c1", [0.0, 90.0, 300.0, 20.0]),
            ("c2", [135.547, 110.0, 28.906, 20.0]),
            ("c3", [10.0, 135.0, 280.0, 20.0]),
            ("r4", [0.0, 160.0, 250.0, 30.0]),
            ("d1", [0.0, 160.0, 100.0, 10.0]),
            ("d2", [100.0, 160.0, 100.0, 20.0]),
            ("d3", [0.0, 180.0, 100.0, 10.0]),
            ("r5", [0.0, 190.0, 200.0, 20.0]),
            ("e1", [0.0, 190.0, 240.828, 20.0]),
            ("e2", [240.828, 190.0, 9.641, 20.0]),
            ("r6", [0.0, 210.0, 300.0, 0.0]),
            ("g1", [0.0, 210.0, 175.0, 0.0]),
            ("g2", [175.0, 210.0, 125.0, 0.0]),
            ("if", [48.156, 210.0, 28.906, 20.0]),
            ("if1", [48.156, 210.0, 9.641, 20.0]),
            ("if2", [57.797, 210.0, 19.266, 20.0]),
            ("r7", [0.0, 230.0, 100.0, 200.0]),
            ("h1", [0.0, 230.0, 100.0, 150.0]),
            ("h2", [0.0, 380.0, 100.0, 50.0]),
            ("r8", [0.0, 430.0, 300.0, 5.0]),
            ("k1", [250.0, 430.0, 50.0, 5.0]),
        ],
    );
}

#[test]
fn gaps_between_items_and_lines() {
    check(
        "<!doctype html><body style='margin:0;font:16px/20px Menlo'>\
         <div id=r1 style='display:flex;gap:10px 20px;width:300px;flex-wrap:wrap'>\
         <div id=a1 style='width:100px;height:10px'></div><div id=a2 style='width:100px;height:10px'>\
         </div><div id=a3 style='width:100px;height:10px'></div></div>\
         <div id=r2 style='display:flex;flex-direction:column;row-gap:5px'><div id=b1>x</div>\
         <div id=b2>y</div></div><span id=r3 style='display:inline-flex;gap:7px'><b id=c1>x</b>\
         <b id=c2>y</b></span></body> ",
        &[
            ("r1", [0.0, 0.0, 300.0, 30.0]),
            ("a1", [0.0, 0.0, 100.0, 10.0]),
            ("a2", [120.0, 0.0, 100.0, 10.0]),
            ("a3", [0.0, 20.0, 100.0, 10.0]),
            ("r2", [0.0, 30.0, 1280.0, 45.0]),
            ("b1", [0.0, 30.0, 1280.0, 20.0]),
            ("b2", [0.0, 55.0, 1280.0, 20.0]),
            ("r3", [0.0, 75.0, 26.281, 20.0]),
            ("c1", [0.0, 75.0, 9.641, 20.0]),
            ("c2", [16.641, 75.0, 9.641, 20.0]),
        ],
    );
}

#[test]
fn a_stretched_item_is_laid_out_again_when_its_contents_read_its_height() {
    check(
        "<!doctype html><body style='margin:0'><div style='display:flex'>\
         <div><div id=p style='height:50%'></div></div><div style='height:100px'></div></div>\
         <div style='display:flex;height:80px'><div><div id=q style='height:50%'>x</div></div></div>",
        &[("p", [0.0, 0.0, 0.0, 50.0]), ("q", [0.0, 100.0, 8.0, 40.0])],
    );
}

#[test]
fn anonymous_items_take_no_edges_of_their_container() {
    let body = |css: &str| {
        format!(
            "<!doctype html><body style='margin:0;font:16px/20px Menlo'>\
             <div style='display:flex;width:400px;{css}'>aaa<div id=q>q</div></div>\
             <div style='display:flex'><div id=a>aaa</div></div>"
        )
    };
    for (css, x) in [("", 0.0), ("margin-left:100px", 100.0), ("padding:0 7px", 7.0)] {
        let html = body(css);
        same_without_reuse(&html, 1280.0, 800.0);
        let dom = parse(&html);
        let st = style(&dom);
        let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
        let (q, a) = (rect(&mut l, &dom, "q"), rect(&mut l, &dom, "a"));
        assert!((q[0] - x - a[2]).abs() <= 0.02, "{css}: q {q:?}, aaa {a:?}");
        assert!(a[2] > 20.0 && a[2] < 40.0, "{a:?}");
    }
}

#[test]
fn items_are_formatting_context_roots() {
    check(
        "<!doctype html><body style='margin:0;font:16px/20px Menlo'>\
         <div style=display:flex><div id=i><p id=p style=margin:10px>x</p></div></div>\
         <div style=display:flex><div id=j><div style='float:left;width:10px;height:50px'></div></div></div>\
         <div id=after>x</div>",
        &[
            ("i", [0.0, 0.0, 29.640625, 40.0]),
            ("p", [10.0, 10.0, 9.640625, 20.0]),
            ("j", [0.0, 40.0, 10.0, 50.0]),
            ("after", [0.0, 90.0, 1280.0, 20.0]),
        ],
    );
}
