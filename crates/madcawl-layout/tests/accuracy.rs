//! Geometry that pages lean on, checked against what WebKit gives the same markup (to 0.05 px): fonts,
//! replaced elements that do not load, flex sizing, generated content, sticky and inline positioning.

mod common;

use common::*;
use madcawl_layout::Layout;

fn check(html: &str, want: &[(&str, [f32; 4])]) {
    let dom = parse(html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    for (id, r) in want {
        let got = rect(&mut l, &dom, id);
        let close = got.iter().zip(r).all(|(a, b)| (a - b).abs() <= 0.05);
        assert!(close, "#{id}: {got:?}, webkit {r:?}");
    }
}

const HEAD: &str = "<!doctype html><meta charset=utf-8><body style='margin:0;font:16px Helvetica'>";

#[test]
fn system_fonts_by_weight_and_the_monospaced_design() {
    check(
        &format!(
            "{HEAD}<div style='font-size:12px'><span id=m style='font-family:ui-monospace'>abcdefghij\
             </span><br><span id=b style='font-family:system-ui;font-weight:bold'>abcdefghij</span></div>"
        ),
        &[
            ("m", [0.0, 0.434, 74.18, 14.133]),
            ("b", [0.0, 15.434, 63.858, 14.133]),
        ],
    );
}

#[test]
fn a_wbr_is_a_break_opportunity_without_a_box() {
    let dom = parse(&format!("{HEAD}<div>long<wbr id=w>word</div>"));
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert_eq!(l.border_box(by_id(&dom, "w")), None);
}

#[test]
fn images_that_do_not_load_take_their_alternative_text_or_attribute_ratio() {
    check(
        &format!(
            "{HEAD}<div><img id=a src=x.png alt='hello world'></div>\
             <div style='font-size:30px'><img id=b src=x.png alt='hello world'></div>\
             <div><img id=e src=x.png alt=hello width=30></div>\
             <div><img id=j src=x.png width=200 height=100 style='height:auto'></div>\
             <div style='width:50px'><img id=k src=x.png width=200 height=100 \
             style='max-width:100%;height:auto'></div>\
             <div><img id=l src=x.png style='aspect-ratio:2/1;width:80px'></div>"
        ),
        &[
            ("a", [0.0, 0.0, 81.0, 22.0]),
            ("b", [0.0, 25.672, 148.0, 39.0]),
            ("e", [0.0, 71.562, 30.0, 17.359]),
            ("j", [0.0, 92.594, 200.0, 100.0]),
            ("k", [0.0, 196.266, 50.0, 25.0]),
            ("l", [0.0, 224.938, 80.0, 40.0]),
        ],
    );
}

#[test]
fn an_svg_root_takes_its_ratio_from_its_view_box_and_fills_the_width_without_a_size() {
    check(
        &format!(
            "{HEAD}<div><svg id=n width=50></svg></div>\
             <div style='width:200px'><svg id=r viewBox='0 0 100 50'></svg></div>\
             <div style='display:flex'><svg id=q viewBox='0 0 100 50'></svg></div>"
        ),
        &[
            ("n", [0.0, 0.0, 50.0, 150.0]),
            ("r", [0.0, 153.672, 200.0, 100.0]),
            ("q", [0.0, 257.344, 1280.0, 640.0]),
        ],
    );
}

#[test]
fn flex_sizing_keywords_limits_and_baselines() {
    check(
        &format!(
            "{HEAD}<div style='display:flex;width:300px'><div id=x style='max-width:100%'>\
             <span style='white-space:nowrap'>a very long line of text that does not wrap at all here\
             </span></div></div>\
             <div style='display:flex;flex-direction:column;align-items:center;width:400px'>\
             <div id=f style='width:fit-content;margin:0 auto'></div>\
             <div id=g style='width:fit-content'>abc</div>\
             <div id=h style='width:max-content'>abc def</div></div>\
             <div id=l1>x <span id=l style='display:inline-flex;overflow:hidden'>\
             <span style='display:block;width:20px;height:20px;margin-top:6px'></span></span></div>\
             <div id=fc style='display:flex;align-items:center;min-height:68px'>\
             <div id=t style='height:32px;width:10px'></div></div>"
        ),
        &[
            ("x", [0.0, 0.0, 300.0, 18.0]),
            ("f", [200.0, 18.0, 0.0, 0.0]),
            ("g", [187.094, 18.0, 25.797, 18.0]),
            ("h", [173.75, 36.0, 52.484, 18.0]),
            ("l1", [0.0, 54.0, 1280.0, 29.672]),
            ("t", [0.0, 101.672, 10.0, 32.0]),
        ],
    );
}

#[test]
fn before_and_after_generate_boxes_from_their_content() {
    check(
        "<!doctype html><style>#b::after{content:attr(data-x) '!';display:inline-block;width:30px;\
         border:1px solid}#c::before{content:'';display:block;height:20px}.f:before{content:'\\201C'}\
         #g::before{content:none}#i{display:flex}#i::before{content:'ab'}#i::after{content:'cd';flex:1}\
         #j::before{content:open-quote}#j::after{content:close-quote}</style>\
         <body style='margin:0;font:16px Helvetica'><div><span id=b data-x=X>b</span></div>\
         <div id=c>c</div><div><span id=f class=f>f</span></div><div><span id=g>g</span></div>\
         <div id=i><span id=i1>m</span></div><div><span id=j>j</span></div>",
        &[
            ("b", [0.0, 0.992, 40.898, 18.0]),
            ("c", [0.0, 20.0, 1280.0, 38.0]),
            ("f", [0.0, 58.0, 9.773, 18.0]),
            ("g", [0.0, 76.0, 8.898, 18.0]),
            ("i1", [17.797, 94.0, 13.328, 18.0]),
            ("j", [0.0, 112.0, 14.914, 18.0]),
        ],
    );
}

#[test]
fn aspect_ratio_gives_an_auto_height() {
    check(
        &format!(
            "{HEAD}<div id=a style='aspect-ratio:16/9;width:320px'></div>\
             <div id=b style='aspect-ratio:2;width:100px;padding:10px'></div>\
             <div id=c style='aspect-ratio:2;width:100px;padding:10px;box-sizing:border-box'></div>"
        ),
        &[
            ("a", [0.0, 0.0, 320.0, 180.0]),
            ("b", [0.0, 180.0, 120.0, 70.0]),
            ("c", [0.0, 250.0, 100.0, 50.0]),
        ],
    );
}

#[test]
fn sticky_boxes_and_the_body_overflow_the_viewport_takes() {
    check(
        "<!doctype html><body style='margin:0;font:16px Helvetica;overflow-x:hidden'>\
         <div id=m style='margin-top:20px'>m</div>\
         <div id=a style='position:sticky;top:50px;height:20px'>a</div>\
         <div id=p style='height:100px'><div id=b style='position:sticky;top:300px;height:20px'>b</div>\
         </div><div id=c style='position:sticky;bottom:0;height:20px;margin-top:1000px'>c</div>\
         <div id=s style='overflow:auto;height:100px;margin-top:10px'><div style='height:30px'></div>\
         <div id=s2 style='position:sticky;top:10px;height:10px'></div><div style='height:300px'></div>\
         </div>",
        &[
            ("m", [0.0, 20.0, 1280.0, 18.0]),
            ("a", [0.0, 50.0, 1280.0, 20.0]),
            ("b", [0.0, 138.0, 1280.0, 20.0]),
            ("c", [0.0, 1020.0, 1280.0, 20.0]),
            ("s2", [0.0, 1218.0, 1280.0, 10.0]),
        ],
    );
}

#[test]
fn inline_boxes_aligned_to_the_line_edges_relative_and_holding_positioned_boxes() {
    check(
        &format!(
            "{HEAD}<div>a <span id=t1 style='vertical-align:top;font-size:30px'>T<b id=t1b>b</b></span> \
             <span id=u1 style='vertical-align:bottom;line-height:50px'>u</span> z</div>\
             <div>x <span id=r1 style='position:relative;top:-5px;left:3px'>rel<i id=r1i>i</i></span> y\
             </div><div>m <span id=p1 style='position:relative;padding:2px;border:1px solid'>pos\
             <span id=a1 style='position:absolute;top:0;left:0;width:10px;height:5px'></span>\
             <span id=a2 style='position:absolute;bottom:0;right:0;width:4px;height:4px'></span></span>\
             </div>"
        ),
        &[
            ("t1", [13.344, 0.0, 36.65, 35.0]),
            ("t1b", [31.669, 0.0, 18.325, 35.0]),
            ("u1", [54.439, 16.0, 8.898, 18.0]),
            ("r1", [15.445, 45.0, 21.336, 18.0]),
            ("r1i", [33.227, 45.0, 3.555, 18.0]),
            ("p1", [17.773, 65.0, 31.797, 24.0]),
            ("a1", [18.781, 66.0, 10.0, 5.0]),
            ("a2", [44.578, 84.0, 4.0, 4.0]),
        ],
    );
}

#[test]
fn text_in_a_fallback_face_takes_that_faces_line_height() {
    check(
        "<!doctype html><meta charset=utf-8><body style='margin:0;font:14px Helvetica'>\
         <div id=a>中文文字</div><div id=c style='line-height:20px'>中文文字</div>\
         <div id=d><span id=d1>中文</span> x</div><div id=g style='font:16px Times'>emoji 😀 here</div>",
        &[
            ("a", [0.0, 0.0, 1280.0, 20.0]),
            ("c", [0.0, 20.0, 1280.0, 20.0]),
            ("d1", [0.0, 42.26, 28.0, 16.0]),
            ("g", [0.0, 60.0, 1280.0, 26.0]),
        ],
    );
}

#[test]
fn a_replaced_box_sized_by_a_percentage_shrinks_to_nothing_at_min_content() {
    check(
        &format!(
            "{HEAD}<div style='width:50px'><div id=f style='float:left'><img id=i src=x.png width=200 \
             height=100 style='width:50%'></div></div><div style='clear:both;width:50px'><div id=g \
             style='float:left'><img id=j src=x.png width=200 height=100 style='max-width:50%'></div></div>"
        ),
        &[
            ("f", [0.0, 0.0, 50.0, 103.672]),
            ("i", [0.0, 0.0, 25.0, 100.0]),
            ("g", [0.0, 103.672, 50.0, 103.672]),
            ("j", [0.0, 103.672, 25.0, 100.0]),
        ],
    );
}

#[test]
fn a_sticky_box_keeps_to_a_scrollport_inside_borders_of_whole_device_pixels() {
    let html = format!(
        "{HEAD}<div id=s style='overflow:auto;height:100px;border:0.5px solid'><div style='height:199px'>\
         </div><div id=k style='position:sticky;bottom:0;height:10px'></div></div>"
    );
    let dom = parse(&html);
    // computed where half a px is a device pixel, laid out at both scales
    let device = madcawl_css::style::Device {
        scale: 2.0,
        ..Default::default()
    };
    let st = madcawl_css::style::style_document(&dom, device, &mut |_| None);
    for (scale, want) in [(1.0, [1.0, 91.0, 1278.0, 10.0]), (2.0, [0.5, 90.5, 1279.0, 10.0])] {
        let mut l = Layout::new(&dom, &st, 1280.0, 800.0).with_scale(scale as f32);
        let got = rect(&mut l, &dom, "k");
        let close = got.iter().zip(want).all(|(a, b)| (a - b).abs() <= 0.05);
        assert!(close, "{scale}x: {got:?}, webkit {want:?}");
    }
}
