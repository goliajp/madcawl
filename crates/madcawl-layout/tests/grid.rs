//! Grid layout; the expected rectangles are what WebKit gives the same markup (to 1/64 px).

mod common;

use common::*;
use madcawl_layout::Layout;

fn check(html: &str, want: &[(&str, [f32; 4])]) {
    let html = format!("<!doctype html><body style='margin:0;font:16px/20px Menlo'>{html}");
    same_without_reuse(&html, 1280.0, 800.0);
    let dom = parse(&html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    for (id, r) in want {
        let got = rect(&mut l, &dom, id);
        let close = got.iter().zip(r).all(|(a, b)| (a - b).abs() <= 0.02);
        assert!(close, "#{id}: {got:?}, webkit {r:?}");
    }
}

/// Fixed, `fr` and percentage tracks and gaps; an item spanning columns and one spanning rows.
#[test]
fn fixed_flexible_and_percentage_tracks_with_gaps() {
    check(
        "<div id=g1 style='display:grid;grid-template-columns:100px 1fr 2fr;gap:10px;width:600px'>\
         <div id=a1>one</div><div id=a2>two words here</div><div id=a3 style='height:50px'>x</div>\
         <div id=a4>four</div></div><div id=g2 style='display:grid;grid-template-columns:repeat(3, 1fr);\
         grid-template-rows:40px auto;width:300px;column-gap:5%'>\
         <div id=b1 style='grid-column:2/4'>wide</div><div id=b2 style='grid-row:span 2'>tall</div>\
         <div id=b3>c</div><div id=b4>d</div><div id=b5>e</div></div><div id=g3 style='display:grid;\
         grid-template-columns:repeat(3,1fr);width:500px;gap:8px'><b id=f1>x</b><b id=f2>y</b><b id=f3>z</b>\
         </div>",
        &[
            ("g1", [0.0, 0.0, 600.0, 80.0]),
            ("a1", [0.0, 0.0, 100.0, 50.0]),
            ("a2", [110.0, 0.0, 160.0, 50.0]),
            ("a3", [280.0, 0.0, 320.0, 50.0]),
            ("a4", [0.0, 60.0, 100.0, 20.0]),
            ("g2", [0.0, 80.0, 300.0, 80.0]),
            ("b1", [105.0, 80.0, 195.0, 40.0]),
            ("b2", [0.0, 120.0, 90.0, 40.0]),
            ("b3", [105.0, 120.0, 90.0, 20.0]),
            ("b4", [210.0, 120.0, 90.0, 20.0]),
            ("b5", [105.0, 140.0, 90.0, 20.0]),
            ("g3", [0.0, 160.0, 500.0, 20.0]),
            ("f1", [0.0, 160.0, 161.328, 20.0]),
            ("f2", [169.328, 160.0, 161.328, 20.0]),
            ("f3", [338.656, 160.0, 161.344, 20.0]),
        ],
    );
}

/// Areas from `grid-template-areas` and the `grid-template` shorthand, named lines, negative lines and
/// implicit tracks before the explicit grid.
#[test]
fn template_areas_and_named_lines() {
    check(
        "<div id=g1 style='display:grid;grid-template-areas:\"h h\" \"s m\" \"f f\";\
         grid-template-columns:150px 1fr;width:500px;row-gap:4px'>\
         <header id=c1 style='grid-area:h'>head</header><nav id=c2 style='grid-area:s'>side</nav>\
         <main id=c3 style='grid-area:m;height:100px'>main</main>\
         <footer id=c4 style='grid-area:f'>foot</footer></div><div id=g2 style='display:grid;\
         grid-template-columns:[a] 50px [b] 60px [c];'><i id=p1 style='grid-column:b / c'>x</i>\
         <i id=p2 style='grid-column:a'>y</i><i id=p3 style='grid-column: -1'>z</i>\
         <i id=p4 style='grid-column: span 2 / -1; grid-row: 3'>w</i></div><div id=g3 style='display:grid;\
         grid-template:\"a b\" 30px \"c b\" 40px / 1fr 1fr;width:200px;height:100px'>\
         <i id=q1 style='grid-area:b'>b</i><i id=q2 style='grid-area:c'>c</i><i id=q3>auto</i></div>\
         <div id=g4 style='display:grid;grid-template-columns:repeat(4, 1fr);\
         grid-auto-rows:minmax(30px,auto);width:400px'><i id=k1 style='grid-column:-3/-1'>a</i>\
         <i id=k2 style='grid-row:2;grid-column:1/-1'>b</i><i id=k3 style='grid-column:-6'>c</i></div>",
        &[
            ("g1", [0.0, 0.0, 500.0, 148.0]),
            ("c1", [0.0, 0.0, 500.0, 20.0]),
            ("c2", [0.0, 24.0, 150.0, 100.0]),
            ("c3", [150.0, 24.0, 350.0, 100.0]),
            ("c4", [0.0, 128.0, 500.0, 20.0]),
            ("g2", [0.0, 148.0, 1280.0, 60.0]),
            ("p1", [50.0, 148.0, 60.0, 20.0]),
            ("p2", [0.0, 168.0, 50.0, 20.0]),
            ("p3", [110.0, 168.0, 1170.0, 20.0]),
            ("p4", [0.0, 188.0, 110.0, 20.0]),
            ("g3", [0.0, 208.0, 200.0, 100.0]),
            ("q1", [100.0, 208.0, 100.0, 70.0]),
            ("q2", [0.0, 238.0, 100.0, 40.0]),
            ("q3", [0.0, 208.0, 100.0, 30.0]),
            ("g4", [0.0, 308.0, 400.0, 60.0]),
            ("k1", [204.812, 308.0, 195.188, 30.0]),
            ("k2", [9.641, 338.0, 390.359, 30.0]),
            ("k3", [0.0, 338.0, 9.641, 30.0]),
        ],
    );
}

/// The auto-placement algorithm: by rows, by columns, dense packing, and text in an anonymous item.
#[test]
fn auto_placement_by_rows_columns_and_dense() {
    check(
        "<div id=g1 style='display:grid;grid-auto-flow:column;grid-template-rows:20px 20px'><i id=e1>1</i>\
         <i id=e2>2</i><i id=e3>3</i></div><div id=g2 style='display:grid;grid-template-columns:1fr 1fr 1fr;\
         grid-auto-flow:dense'><i id=q1 style='grid-column:span 2'>a</i>\
         <i id=q2 style='grid-column:span 2'>b</i><i id=q3>c</i></div><div id=g3 style='display:grid;\
         grid-template-columns:1fr 1fr 1fr'><i id=r1 style='grid-column:span 2'>a</i>\
         <i id=r2 style='grid-column:span 2'>b</i><i id=r3>c</i><i id=r4 style='grid-row:1'>d</i></div>\
         <div id=g4 style='display:grid;grid-template-columns:100px 1fr;\
         width:400px'>text <b id=p1>bold</b> more</div>",
        &[
            ("g1", [0.0, 0.0, 1280.0, 40.0]),
            ("e1", [0.0, 0.0, 640.0, 20.0]),
            ("e2", [0.0, 20.0, 640.0, 20.0]),
            ("e3", [640.0, 0.0, 640.0, 20.0]),
            ("g2", [0.0, 40.0, 1280.0, 40.0]),
            ("q1", [0.0, 40.0, 853.328, 20.0]),
            ("q2", [0.0, 60.0, 853.328, 20.0]),
            ("q3", [853.328, 40.0, 426.672, 20.0]),
            ("g3", [0.0, 80.0, 1280.0, 40.0]),
            ("r1", [426.656, 80.0, 853.344, 20.0]),
            ("r2", [0.0, 100.0, 853.328, 20.0]),
            ("r3", [853.328, 100.0, 426.672, 20.0]),
            ("r4", [0.0, 80.0, 426.656, 20.0]),
            ("g4", [0.0, 120.0, 400.0, 40.0]),
            ("p1", [100.0, 120.0, 300.0, 20.0]),
        ],
    );
}

/// `auto`, `min-content`, `max-content`, `fit-content()` and `minmax()` tracks sized from their items, and a
/// grid's own intrinsic widths.
#[test]
fn intrinsic_track_sizes() {
    check(
        "<div id=g1 style='display:grid;grid-template-columns:auto auto auto;width:400px;\
         justify-content:center'><span id=d1>a</span><span id=d2>bbbbbb</span><span id=d3>cc</span></div>\
         <span id=g2 style='display:inline-grid;grid-template-columns:min-content max-content'>\
         <b id=k1>a b c</b><b id=k2>d e</b></span><div style='width:200px'><div id=g3 style='display:grid;\
         grid-template-columns:1fr 1fr'><div id=m1>averyveryverylongword</div><div id=m2>x</div></div></div>\
         <div id=g4 style='display:grid;\
         grid-template-columns:fit-content(100px) minmax(50px, max-content) 1fr;width:600px'>\
         <p id=n1 style='margin:0'>some text that is quite long here</p><p id=n2 style='margin:0'>ab</p>\
         <p id=n3 style='margin:0'>c</p></div><div id=g5 style='display:grid;\
         grid-template-columns:max-content auto;width:300px'><dt id=s1>term</dt>\
         <dd id=s2 style='margin:0'>definition text</dd><dt id=s3>another term</dt>\
         <dd id=s4 style='margin:0'>x</dd></div><div id=g6 style='display:grid;\
         grid-template-columns:minmax(0,1fr) minmax(0,1fr);width:300px'><div id=f1 style='display:grid;\
         grid-template-columns:auto 1fr'><i id=f11>nested</i><i id=f12>q</i></div>\
         <div id=f2>long long long long long long long text wrapping</div></div>",
        &[
            ("g1", [0.0, 0.0, 400.0, 20.0]),
            ("d1", [156.641, 0.0, 9.641, 20.0]),
            ("d2", [166.281, 0.0, 57.797, 20.0]),
            ("d3", [224.078, 0.0, 19.266, 20.0]),
            ("g2", [0.0, 20.0, 38.547, 60.0]),
            ("k1", [0.0, 20.0, 9.641, 60.0]),
            ("k2", [9.641, 20.0, 28.906, 60.0]),
            ("g3", [0.0, 80.0, 200.0, 20.0]),
            ("m1", [0.0, 80.0, 202.297, 20.0]),
            ("m2", [202.297, 80.0, 9.641, 20.0]),
            ("g4", [0.0, 100.0, 600.0, 80.0]),
            ("n1", [0.0, 100.0, 100.0, 80.0]),
            ("n2", [100.0, 100.0, 50.0, 80.0]),
            ("n3", [150.0, 100.0, 450.0, 80.0]),
            ("g5", [0.0, 180.0, 300.0, 40.0]),
            ("s1", [0.0, 180.0, 115.594, 20.0]),
            ("s2", [115.594, 180.0, 184.406, 20.0]),
            ("s3", [0.0, 200.0, 115.594, 20.0]),
            ("s4", [115.594, 200.0, 184.406, 20.0]),
            ("g6", [0.0, 220.0, 300.0, 80.0]),
            ("f1", [0.0, 220.0, 150.0, 80.0]),
            ("f11", [0.0, 220.0, 57.797, 80.0]),
            ("f12", [57.797, 220.0, 92.203, 80.0]),
            ("f2", [150.0, 220.0, 150.0, 80.0]),
        ],
    );
}

/// `repeat(auto-fill)` counting the tracks that fit, and `auto-fit` collapsing the empty ones.
#[test]
fn repeat_auto_fill_and_auto_fit() {
    check(
        "<div id=g1 style='display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));\
         width:500px;gap:8px'><b id=f1>x</b><b id=f2>y</b><b id=f3>z</b><b id=f4>w</b><b id=f5>v</b></div>\
         <div id=g2 style='display:grid;grid-template-columns:repeat(auto-fit,100px);width:500px;\
         justify-content:center'><b id=a1>x</b><b id=a2>y</b></div><ul id=g3 style='display:grid;\
         grid-template-columns:repeat(3,1fr);gap:1em;list-style:none;padding:0;width:600px'>\
         <li id=n1>one</li><li id=n2>two</li><li id=n3>three</li><li id=n4>four</li></ul>",
        &[
            ("g1", [0.0, 0.0, 500.0, 48.0]),
            ("f1", [0.0, 0.0, 161.328, 20.0]),
            ("f2", [169.328, 0.0, 161.328, 20.0]),
            ("f3", [338.656, 0.0, 161.344, 20.0]),
            ("f4", [0.0, 28.0, 161.328, 20.0]),
            ("f5", [169.328, 28.0, 161.328, 20.0]),
            ("g2", [0.0, 48.0, 500.0, 20.0]),
            ("a1", [150.0, 48.0, 100.0, 20.0]),
            ("a2", [250.0, 48.0, 100.0, 20.0]),
            ("g3", [0.0, 84.0, 600.0, 56.0]),
            ("n1", [0.0, 84.0, 189.328, 20.0]),
            ("n2", [205.328, 84.0, 189.328, 20.0]),
            ("n3", [410.656, 84.0, 189.344, 20.0]),
            ("n4", [0.0, 120.0, 189.328, 20.0]),
        ],
    );
}

/// `justify-items`, `align-items`, their `-self` overrides and auto margins in the grid area; `align-content`
/// over rows of a definite height; rows stretched to a minimum height.
#[test]
fn alignment_of_items_and_tracks() {
    check(
        "<div id=g1 style='display:grid;grid-template-columns:100px 100px;justify-items:center;\
         align-items:end;grid-auto-rows:50px'><i id=h1>x</i><i id=h2 style='justify-self:end'>y</i>\
         <i id=h3 style='align-self:start;margin:auto'>zz</i></div><div id=g2 style='display:grid;\
         height:200px;grid-template-rows:1fr 2fr;align-content:center'><i id=r1>a</i><i id=r2>b</i></div>\
         <div id=g3 style='display:grid;height:100px;grid-template-rows:30px 30px;\
         align-content:space-between;grid-template-columns:100px'><i id=s1>a</i><i id=s2>b</i></div>\
         <div id=g4 style='display:grid;grid-template-columns:auto 1fr;align-items:center;width:400px;\
         min-height:60px'><img id=h4 style='width:30px;height:30px'><span id=h5>title text</span></div>\
         <div id=g5 style='display:grid;grid-template-columns:100px;justify-items:start'>\
         <div id=w1 style='width:fit-content'>fit</div><div id=w2 style='width:max-content'>max content</div>\
         </div>",
        &[
            ("g1", [0.0, 0.0, 1280.0, 100.0]),
            ("h1", [45.172, 30.0, 9.641, 20.0]),
            ("h2", [190.359, 30.0, 9.641, 20.0]),
            ("h3", [40.359, 65.0, 19.266, 20.0]),
            ("g2", [0.0, 100.0, 1280.0, 200.0]),
            ("r1", [0.0, 100.0, 1280.0, 66.656]),
            ("r2", [0.0, 166.656, 1280.0, 133.344]),
            ("g3", [0.0, 300.0, 1280.0, 100.0]),
            ("s1", [0.0, 300.0, 100.0, 30.0]),
            ("s2", [0.0, 370.0, 100.0, 30.0]),
            ("g4", [0.0, 400.0, 400.0, 60.0]),
            ("h4", [0.0, 415.0, 30.0, 30.0]),
            ("h5", [30.0, 420.0, 370.0, 20.0]),
            ("g5", [0.0, 460.0, 1280.0, 40.0]),
            ("w1", [0.0, 460.0, 28.906, 20.0]),
            ("w2", [0.0, 480.0, 105.969, 20.0]),
        ],
    );
}

/// Items with margins, padding and percentage widths; a relatively positioned item; an absolutely positioned
/// child; percentage rows and item heights against the grid area.
#[test]
fn items_with_edges_percentages_and_positioning() {
    check(
        "<div id=g1 style='display:grid;grid-template-columns:1fr 3fr;padding:10px;border:2px solid;\
         margin:5px 20px'><div id=d1 style='margin:7px;padding:3px'>m</div>\
         <div id=d2 style='width:50%'>w</div><div id=d3 style='position:relative;top:5px'>r</div></div>\
         <div id=g2 style='display:grid;grid-template-columns:100px 100px;position:relative'><i id=e1>a</i>\
         <i id=e2 style='position:absolute;right:0;top:0'>abs</i><i id=e3>c</i></div>\
         <div id=g3 style='display:grid;grid-template-columns:200px;grid-template-rows:50%;height:300px'>\
         <i id=c1 style='height:50%'>x</i><i id=c2>y</i></div>",
        &[
            ("g1", [20.0, 5.0, 1240.0, 84.0]),
            ("d1", [39.0, 24.0, 290.0, 26.0]),
            ("d2", [336.0, 17.0, 456.0, 40.0]),
            ("d3", [32.0, 62.0, 304.0, 20.0]),
            ("g2", [0.0, 94.0, 1280.0, 20.0]),
            ("e1", [0.0, 94.0, 100.0, 20.0]),
            ("e2", [1251.094, 94.0, 28.906, 20.0]),
            ("e3", [100.0, 94.0, 100.0, 20.0]),
            ("g3", [0.0, 114.0, 1280.0, 300.0]),
            ("c1", [0.0, 114.0, 200.0, 75.0]),
            ("c2", [0.0, 264.0, 200.0, 150.0]),
        ],
    );
}

/// Lines far past the explicit grid clamp to ten thousand tracks per axis, where WebKit clamps at a million
/// (it gives these rectangles scaled by a hundred for `2000000`), to bound the tracks a container holds
#[test]
fn lines_past_the_cap_clamp_to_it() {
    check(
        "<div id=g1 style='display:grid;grid-auto-rows:1px'><i id=a style='grid-row:20000'></i>\
         <i id=b style='grid-row:1'></i></div><div id=g2 style='display:grid;grid-auto-columns:1px'>\
         <i id=c style='grid-column:-20000'></i><i id=d style='grid-column:1'></i></div>",
        &[
            ("g1", [0.0, 0.0, 1280.0, 10000.0]),
            ("a", [0.0, 9999.0, 1280.0, 1.0]),
            ("b", [0.0, 0.0, 1280.0, 1.0]),
            ("g2", [0.0, 10000.0, 1280.0, 0.0]),
            ("c", [0.0, 10000.0, 1.0, 0.0]),
            ("d", [9999.0, 10000.0, 1.0, 0.0]),
        ],
    );
}

#[test]
fn nested_grids_lay_each_box_out_a_bounded_number_of_times() {
    for open in [
        "<div style='display:grid'>",
        "<div style='display:grid;grid-template-columns:auto 1fr'><i>a</i>",
        "<div style='display:grid;grid-template-columns:1fr 1fr;align-items:center'><i>a</i>",
        "<div style='display:grid;grid-template-rows:1fr;height:50%'>",
    ] {
        let html = |depth: usize| {
            format!(
                "<!doctype html><html style='height:100%'><body style='height:100%'>{}x{}",
                open.repeat(depth),
                "</div>".repeat(depth)
            )
        };
        same_without_reuse(&html(6), 1280.0, 800.0);
        let depth = 24;
        let dom = parse(&html(depth));
        let st = style(&dom);
        let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
        assert!(l.fragments().is_some());
        let c = l.counters();
        let boxes = depth * (1 + open.matches("<i>").count()) + 4;
        assert!(
            c.laid <= 6 * boxes as u64,
            "{open}: {c:?} for {boxes} boxes"
        );
    }
}

#[test]
fn many_auto_placed_items_are_each_laid_out_a_bounded_number_of_times() {
    let laid = |n: usize| {
        let html = format!(
            "<!doctype html><div style='display:grid;grid-template-columns:repeat(4,1fr)'>{}</div>",
            "<i>x</i><b style='grid-column:span 2'>y</b>".repeat(n)
        );
        let dom = parse(&html);
        let st = style(&dom);
        let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
        assert!(l.fragments().is_some());
        l.counters().laid
    };
    let (a, b) = (laid(500), laid(2000));
    // two items a repeat, each measured for its row and laid out in its area
    assert!(b <= 3 * 2 * 2000 + 8, "{a} then {b} boxes laid out");
    assert!(b <= 5 * a, "{a} then {b} boxes laid out");
}

#[test]
fn deeply_nested_grids_stay_within_the_stack() {
    for open in [
        "<div style='display:grid'>",
        "<div style='display:inline-grid;grid-template-columns:auto 1fr'>",
        "<div style='display:grid;grid-template-columns:min-content'>",
    ] {
        let html = format!("<!doctype html><body>{}x", open.repeat(2000));
        let dom = parse(&html);
        let st = style(&dom);
        let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
        assert!(l.fragments().is_some(), "{open}");
    }
}

/// An automatic minimum is no larger than the fixed maximums of the tracks the item spans; a
/// `fit-content()` argument does not limit it.
#[test]
fn an_automatic_minimum_stops_at_fixed_track_maximums() {
    check(
        "<div style='display:grid;grid-template-columns:minmax(auto,100px) 50px'>\
         <i id=gi>xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx</i><i id=gj>a</i></div>\
         <div style='display:grid;grid-template-columns:fit-content(80px)'>\
         <i id=hi>xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx</i></div>",
        &[
            ("gi", [0.0, 0.0, 100.0, 20.0]),
            ("gj", [100.0, 0.0, 50.0, 20.0]),
            ("hi", [0.0, 20.0, 288.984, 20.0]),
        ],
    );
}

/// `place-items` and `place-self` split where both halves parse: `first baseline`, `safe end` and
/// `center legacy left` are one value for both axes or an align value and a justify value.
#[test]
fn place_shorthands_take_multi_word_values() {
    check(
        "<div style='display:grid;grid-template-columns:200px;place-items:safe end;height:50px'>\
         <i id=q2>a</i></div>\
         <div style='display:grid;grid-template-columns:200px;place-items:center legacy left;height:50px'>\
         <i id=q3>a</i></div>\
         <div style='display:grid;grid-template-columns:200px;height:50px'>\
         <i id=q4 style='place-self:center safe end'>a</i></div>",
        &[
            ("q2", [190.359, 30.0, 9.641, 20.0]),
            ("q3", [0.0, 65.0, 9.641, 20.0]),
            ("q4", [190.359, 115.0, 9.641, 20.0]),
        ],
    );
}
