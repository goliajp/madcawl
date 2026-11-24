mod common;

use common::*;
use madcawl_layout::Layout;

fn check(html: &str, want: &[(&str, [f32; 4])]) {
    same_without_reuse(html, 1280.0, 800.0);
    let dom = parse(html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    for (id, r) in want {
        assert_eq!(rect(&mut l, &dom, id), *r, "#{id}");
    }
}

#[test]
fn body_margin_and_auto_width() {
    check(
        "<!doctype html><body id=b><div id=d style='height:10px'></div>",
        &[
            ("b", [8.0, 8.0, 1264.0, 10.0]),
            ("d", [8.0, 8.0, 1264.0, 10.0]),
        ],
    );
}

#[test]
fn margins_collapse_between_siblings_and_through_parents() {
    check(
        "<!doctype html><body style='margin:0'>\
         <div id=a style='height:10px;margin-bottom:20px'></div>\
         <div id=b style='height:10px;margin-top:30px'></div>\
         <div id=p style='margin-top:5px'><div id=c style='height:10px;margin-top:40px'></div></div>\
         <div id=e style='margin:15px 0'></div>\
         <div id=f style='height:10px;margin-top:-5px'></div>",
        &[
            ("a", [0.0, 0.0, 1280.0, 10.0]),
            ("b", [0.0, 40.0, 1280.0, 10.0]),
            // the child's 40px collapses through its parent with the parent's 5px
            ("p", [0.0, 90.0, 1280.0, 10.0]),
            ("c", [0.0, 90.0, 1280.0, 10.0]),
            // an empty box's margins collapse through it: max(15, 15) - 5
            ("f", [0.0, 110.0, 1280.0, 10.0]),
        ],
    );
}

#[test]
fn borders_and_padding_stop_collapsing() {
    check(
        "<!doctype html><body style='margin:0'>\
         <div id=p style='margin-top:5px;padding-top:1px'><div id=c style='height:10px;margin-top:40px'></div></div>",
        &[
            ("p", [0.0, 5.0, 1280.0, 51.0]),
            ("c", [0.0, 46.0, 1280.0, 10.0]),
        ],
    );
}

#[test]
fn widths_box_sizing_min_max_and_auto_margins() {
    check(
        "<!doctype html><body style='margin:0;width:1000px'>\
         <div id=a style='width:50%;padding:10px;border:5px solid'></div>\
         <div id=b style='width:50%;padding:10px;border:5px solid;box-sizing:border-box'></div>\
         <div id=c style='width:100px;margin:0 auto'></div>\
         <div id=d style='max-width:300px;min-width:400px'></div>\
         <div id=e style='width:200px;margin-left:auto'></div>\
         <div id=f style='margin:0 10%'></div>",
        &[
            ("a", [0.0, 0.0, 530.0, 30.0]),
            ("b", [0.0, 30.0, 500.0, 30.0]),
            ("c", [450.0, 60.0, 100.0, 0.0]),
            ("d", [0.0, 60.0, 400.0, 0.0]),
            ("e", [800.0, 60.0, 200.0, 0.0]),
            ("f", [100.0, 60.0, 800.0, 0.0]),
        ],
    );
}

#[test]
fn percentage_heights_need_a_definite_base() {
    check(
        "<!doctype html><html style='height:100%'><body style='margin:0;height:50%'>\
         <div id=a style='height:50%'></div><div id=w><div id=b style='height:50%'></div></div>",
        &[
            ("a", [0.0, 0.0, 1280.0, 200.0]),
            ("b", [0.0, 200.0, 1280.0, 0.0]),
        ],
    );
}

#[test]
fn min_and_max_height() {
    check(
        "<!doctype html><body style='margin:0'>\
         <div id=a style='min-height:30px'></div><div id=b style='height:100px;max-height:20px'></div>",
        &[
            ("a", [0.0, 0.0, 1280.0, 30.0]),
            ("b", [0.0, 30.0, 1280.0, 20.0]),
        ],
    );
}

#[test]
fn display_none_and_contents() {
    let html = "<!doctype html><body style='margin:0'><div id=n style='display:none'><div id=x>x</div></div>\
                <div id=c style='display:contents'><div id=y style='height:7px'></div></div>";
    same_without_reuse(html, 1280.0, 800.0);
    let dom = parse(html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert_eq!(l.border_box(by_id(&dom, "n")), None);
    assert_eq!(l.border_box(by_id(&dom, "x")), None);
    assert_eq!(l.border_box(by_id(&dom, "c")), None);
    assert_eq!(rect(&mut l, &dom, "y"), [0.0, 0.0, 1280.0, 7.0]);
}

#[test]
fn relative_and_absolute_positions() {
    check(
        "<!doctype html><body style='margin:0'>\
         <div id=r style='position:relative;left:10px;top:5px;height:50px;margin-top:100px'>\
         <div id=a style='position:absolute;right:0;bottom:10px;width:20px;height:20px'></div>\
         <div id=s style='position:absolute;width:30px;height:5px'></div></div>\
         <div id=f style='position:fixed;left:3px;top:4px;width:5px;height:6px'></div>\
         <div style='position:relative;width:100px;height:60px'>\
         <div id=i style='position:absolute;top:5px;bottom:10px;left:1px;right:2px;padding:1px'></div></div>",
        &[
            ("r", [10.0, 105.0, 1280.0, 50.0]),
            ("a", [1270.0, 125.0, 20.0, 20.0]),
            ("s", [10.0, 105.0, 30.0, 5.0]),
            ("f", [3.0, 4.0, 5.0, 6.0]),
            // inset on all sides: the box takes what they leave
            ("i", [1.0, 155.0, 97.0, 45.0]),
        ],
    );
}

// what WebKit gives the same markup
#[test]
fn absolute_boxes_after_blocks_take_their_static_position() {
    let at = |html: &str, id: &str| {
        same_without_reuse(html, 1280.0, 800.0);
        let dom = parse(html);
        let st = style(&dom);
        let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
        let r = rect(&mut l, &dom, id);
        [r[0], r[1]]
    };
    let page = "<!doctype html><body style='margin:0'><div style='position:relative;padding:10%'>\
                <p style='margin:0;height:50px'>a</p><p style='margin:0;height:50px'>b</p>";
    // an inline one is wrapped in an anonymous block below the blocks
    let html = format!("{page}<span id=s style='position:absolute'>x</span></div>");
    assert_eq!(at(&html, "s"), [128.0, 228.0]);
    let html = format!("{page}<div id=d style='position:absolute'>x</div><p>c</p></div>");
    assert_eq!(at(&html, "d"), [128.0, 228.0]);
    // in a line, where the text before it ends
    let html = "<!doctype html><body style='margin:0;font:16px/20px Menlo'><div style='position:relative'>\
                <p style='margin:0;height:50px'>a</p>ab<span id=i style='position:absolute'>x</span></div>";
    let i = at(html, "i");
    assert_eq!(i[1], 50.0);
    assert!((i[0] - 19.265625).abs() <= 0.02, "{i:?}");
}

// percentages truncate to 1/64 px as WebKit's layout units do
#[test]
fn percentages_truncate_to_the_unit() {
    check(
        "<!doctype html><body style='margin:0'><div id=a style='width:33.3333%;height:10px'></div>\
         <div id=b style='width:66.6667%;padding-left:12.3457%;height:10px'></div>",
        &[
            ("a", [0.0, 0.0, 426.65625, 10.0]),
            ("b", [0.0, 10.0, 1011.34375, 10.0]),
        ],
    );
}
