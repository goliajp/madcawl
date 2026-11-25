//! Table layout; the expected rectangles are what WebKit gives the same markup (to 1/64 px).

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

/// The separated border model: spacing around rows and columns, header and footer groups moved to the top and
/// bottom, a caption below, cells spanning rows and columns.
#[test]
fn separated_borders_sections_captions_and_spans() {
    check(
        "<table id=t style='border-spacing:10px 6px;border:3px solid;padding:4px'>\
         <caption id=cap style='caption-side:bottom'>c</caption><tfoot id=f><tr id=r4>\
         <td id=d colspan=2 style='padding:0'>foot</td></tr></tfoot><tbody id=b><tr id=r2><td id=b1>y</td>\
         <td id=b2 rowspan=2>z<br>z<br>z<br>z</td></tr><tr id=r3><td id=c1 style='vertical-align:top'>w</td>\
         </tr></tbody><thead id=h><tr id=r1><td id=a>x</td></tr></thead></table><table id=t2>\
         <caption id=cap2>top caption wider than cells</caption><tr><td id=e1>a</td>\
         <td id=e2 rowspan=0>b</td></tr><tr><td id=e3 colspan=1000>c</td></tr></table>",
        &[
            ("t", [0.0, 0.0, 72.531, 182.0]),
            ("cap", [0.0, 162.0, 72.531, 20.0]),
            ("f", [7.0, 129.0, 58.531, 26.0]),
            ("r4", [7.0, 129.0, 58.531, 20.0]),
            ("d", [17.0, 129.0, 38.531, 20.0]),
            ("b", [7.0, 41.0, 58.531, 88.0]),
            ("r2", [7.0, 41.0, 58.531, 22.0]),
            ("b1", [17.0, 41.0, 14.266, 22.0]),
            ("b2", [41.266, 41.0, 14.266, 82.0]),
            ("r3", [7.0, 69.0, 58.531, 54.0]),
            ("c1", [17.0, 69.0, 14.266, 54.0]),
            ("h", [7.0, 7.0, 58.531, 34.0]),
            ("r1", [7.0, 13.0, 58.531, 22.0]),
            ("a", [17.0, 13.0, 14.266, 22.0]),
            ("t2", [0.0, 182.0, 67.438, 150.0]),
            ("cap2", [0.0, 182.0, 67.438, 100.0]),
            ("e1", [2.0, 284.0, 29.719, 22.0]),
            ("e2", [33.719, 284.0, 29.719, 46.0]),
            ("e3", [2.0, 308.0, 63.438, 22.0]),
        ],
    );
}

/// Column widths from cells and column elements: fixed, percentage and auto columns sharing the table's
/// width, a spanning cell spreading what its columns lack, a percentage column given back what the others
/// need.
#[test]
fn column_widths_of_the_automatic_layout() {
    check(
        "<table id=t1><col id=cl style='width:100px'><tr><td id=e style='width:30px'>a</td><td id=e2>b</td>\
         </tr></table><table id=t2 style='width:300px'><tr><td id=g1>aaa</td><td id=g2>b</td>\
         <td id=g3 style='width:50%'>c</td></tr></table><table id=t3><tr>\
         <td id=w1 colspan=3>wide spanning cell content</td></tr><tr><td id=w2>a</td><td id=w3>bb</td>\
         <td id=w4>c</td></tr></table><table id=t4 style='width:100%' cellpadding=4 cellspacing=0><tr>\
         <td id=n1>left</td><td id=n2 style='text-align:right' width=200>right</td></tr></table>\
         <div style='width:704px'><table id=t5 style='width:100%'><colgroup><col><col style='width: 50%'>\
         <col></colgroup><tr><td id=p1>a</td><td id=p2>b</td><td id=p3><div style='width:400px'>c</div></td>\
         </tr></table></div><table id=t6><tr><td id=y1></td><td id=y2>x</td></tr></table>",
        &[
            ("t1", [0.0, 0.0, 117.641, 26.0]),
            ("cl", [0.0, 0.0, 0.0, 0.0]),
            ("e", [2.0, 2.0, 100.0, 22.0]),
            ("e2", [104.0, 2.0, 11.641, 22.0]),
            ("t2", [0.0, 26.0, 300.0, 26.0]),
            ("g1", [2.0, 28.0, 106.062, 22.0]),
            ("g2", [110.062, 28.0, 39.953, 22.0]),
            ("g3", [152.016, 28.0, 146.0, 22.0]),
            ("w1", [2.0, 54.0, 252.484, 22.0]),
            ("w2", [2.0, 78.0, 64.938, 22.0]),
            ("w3", [68.938, 78.0, 118.609, 22.0]),
            ("w4", [189.547, 78.0, 64.938, 22.0]),
            ("t4", [0.0, 102.0, 1280.0, 28.0]),
            ("n1", [0.0, 102.0, 1072.0, 28.0]),
            ("n2", [1072.0, 102.0, 208.0, 28.0]),
            ("t5", [0.0, 130.0, 704.0, 26.0]),
            ("p1", [2.0, 132.0, 11.641, 22.0]),
            ("p2", [15.641, 132.0, 282.359, 22.0]),
            ("p3", [300.0, 132.0, 402.0, 22.0]),
            ("t6", [0.0, 156.0, 19.641, 26.0]),
            ("y1", [2.0, 158.0, 2.0, 22.0]),
            ("y2", [6.0, 158.0, 11.641, 22.0]),
        ],
    );
}

/// `table-layout: fixed`: the first row's widths, the rest shared equally, content not widening the columns.
#[test]
fn fixed_table_layout() {
    check(
        "<table id=t1 style='width:300px;table-layout:fixed'><tr><td id=k1 style='width:50px'>aaa</td>\
         <td id=k2>bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb</td><td id=k3>c</td></tr><tr>\
         <td id=k4 style='width:200px'>x</td><td id=k5>y</td><td id=k6>z</td></tr></table>\
         <table id=t2 style='width:400px;table-layout:fixed'><col style='width:25%'><tr><td id=m1>a</td>\
         <td id=m2>b</td></tr></table>",
        &[
            ("t1", [0.0, 0.0, 300.0, 50.0]),
            ("k1", [2.0, 2.0, 52.0, 22.0]),
            ("k2", [56.0, 2.0, 120.0, 22.0]),
            ("k3", [178.0, 2.0, 120.0, 22.0]),
            ("k4", [2.0, 26.0, 52.0, 22.0]),
            ("k5", [56.0, 26.0, 120.0, 22.0]),
            ("k6", [178.0, 26.0, 120.0, 22.0]),
            ("t2", [0.0, 50.0, 400.0, 26.0]),
            ("m1", [2.0, 52.0, 98.5, 22.0]),
            ("m2", [102.5, 52.0, 295.5, 22.0]),
        ],
    );
}

/// The collapsing border model: the widest border at each edge, the table's at its outer edges, `hidden`
/// suppressing borders, row and row group borders, halves split as WebKit splits them.
#[test]
fn collapsing_borders() {
    check(
        "<table id=t1 border=1 style='border-collapse:collapse'><tr><td id=m1>a</td>\
         <td id=m2 style='border:5px solid'>b</td></tr><tr><td id=m3>c</td><td id=m4>d</td></tr></table>\
         <table id=t2 style='border-collapse:collapse;border:3px solid'><tr>\
         <td id=a style='border:1px solid'>a</td><td id=b style='border:4px solid'>b</td></tr><tr>\
         <td id=c style='border:2px solid'>c</td><td id=d style='border:1px solid'>d</td></tr></table>\
         <table id=t3 style='border-collapse:collapse;border-style:hidden hidden none hidden;width:600px'>\
         <thead id=h style='border-bottom:solid'><tr><th id=th1 style='border:solid;\
         border-bottom:solid thin'>Name</th><th id=th2 style='border:solid;\
         border-bottom:solid thin'>Desc</th></tr></thead><tbody id=bd style='border-bottom:solid'><tr id=r1>\
         <td id=c1 style='border-left:solid;border-right:solid;border-bottom:solid thin'>one</td>\
         <td id=c2 style='border-left:solid;border-right:solid;border-bottom:solid thin'>two words</td></tr>\
         </tbody></table><table id=t4 style='border:2px solid;border-collapse:collapse'>\
         <tr id=s1 style='border:6px solid'><td id=g style='border:1px solid'>g</td><td id=g2>h</td></tr>\
         <tbody id=s2 style='border:4px solid'><tr><td id=k>k</td></tr></tbody></table>",
        &[
            ("t1", [0.0, 0.0, 34.281, 55.0]),
            ("m1", [0.0, 2.0, 14.641, 27.0]),
            ("m2", [14.641, 2.0, 16.641, 27.0]),
            ("m3", [0.0, 29.0, 14.641, 25.0]),
            ("m4", [14.641, 29.0, 16.641, 25.0]),
            ("t2", [0.0, 55.0, 34.281, 55.0]),
            ("a", [1.0, 57.0, 15.641, 26.0]),
            ("b", [16.641, 57.0, 15.641, 26.0]),
            ("c", [1.0, 83.0, 15.641, 25.0]),
            ("d", [16.641, 83.0, 15.641, 25.0]),
            ("t3", [0.0, 110.0, 600.0, 50.0]),
            ("h", [0.0, 110.0, 600.0, 23.0]),
            ("bd", [0.0, 133.0, 600.0, 25.0]),
            ("r1", [0.0, 133.0, 600.0, 25.0]),
            ("t4", [0.0, 160.0, 36.281, 60.0]),
            ("s1", [3.0, 163.0, 30.281, 28.0]),
            ("g", [3.0, 163.0, 14.641, 28.0]),
            ("g2", [17.641, 163.0, 15.641, 28.0]),
            ("s2", [3.0, 191.0, 30.281, 27.0]),
            ("k", [3.0, 191.0, 14.641, 27.0]),
        ],
    );
}

/// Anonymous tables, rows and cells around table-internal boxes and the content between them.
#[test]
fn anonymous_table_objects() {
    check(
        "<div id=t1 style='display:table;border-spacing:4px'><div id=g1>loose text</div>\
         <span id=g2>and</span> words</div><div id=t2><div id=h1 style='display:table-row'>\
         <div id=h2 style='display:table-cell'>r1</div></div><div id=h3 style='display:table-row'>bare</div>\
         </div><ul id=t3 style='display:table;padding:0;margin:0'><li id=k1 style='display:table-cell;\
         padding:3px'>a</li><li id=k2 style='display:table-cell;padding:3px'>bb</li></ul><div id=t4>\
         <span id=q1 style='display:table-cell'>c1</span><span id=q2 style='display:table-cell;\
         vertical-align:middle;height:50px'>c2</span></div><div style='width:300px'>\
         <div id=t5 style='display:table;width:100%'><div id=m1 style='display:table-cell;width:30%'>x</div>\
         <div id=m2 style='display:table-cell'>y</div></div></div>",
        &[
            ("t1", [0.0, 0.0, 104.328, 48.0]),
            ("g1", [4.0, 4.0, 96.328, 20.0]),
            ("g2", [4.0, 24.688, 28.898, 18.625]),
            ("t2", [0.0, 48.0, 1280.0, 40.0]),
            ("h1", [0.0, 48.0, 38.531, 20.0]),
            ("h2", [0.0, 48.0, 38.531, 20.0]),
            ("h3", [0.0, 68.0, 38.531, 20.0]),
            ("t3", [0.0, 88.0, 40.906, 26.0]),
            ("k1", [0.0, 88.0, 15.641, 26.0]),
            ("k2", [15.641, 88.0, 25.266, 26.0]),
            ("t4", [0.0, 114.0, 1280.0, 50.0]),
            ("q1", [0.0, 114.0, 19.266, 50.0]),
            ("q2", [19.266, 114.0, 19.266, 50.0]),
            ("t5", [0.0, 164.0, 300.0, 20.0]),
            ("m1", [0.0, 164.0, 90.0, 20.0]),
            ("m2", [90.0, 164.0, 210.0, 20.0]),
        ],
    );
}

/// `vertical-align` in cells (baseline, top, middle, bottom), rows as tall as their cells need, a table's
/// height shared out over its rows, an inline table on its first row's baseline.
#[test]
fn vertical_alignment_in_cells_and_row_heights() {
    check(
        "<table id=t1><tr><td id=s1 style='font-size:30px;line-height:40px'>Big</td><td id=s2>small</td>\
         <td id=s3 style='vertical-align:bottom'>bot</td><td id=s4 style='vertical-align:top'>top</td></tr>\
         </table><table id=t2 style='height:100px'><tr><td id=x1>a</td></tr><tr><td id=x2>b</td></tr></table>\
         <table id=t3><tr><td id=d1>z<br>z</td><td id=d2 rowspan=2>z<br>z<br>z<br>z</td></tr><tr>\
         <td id=d3 style='height:30px'>q</td></tr></table><p>before <span id=t4 style='display:inline-table'>\
         <span id=u1 style='display:table-row'><span id=u2 style='display:table-cell'>it</span></span>\
         </span> after</p>",
        &[
            ("t1", [0.0, 0.0, 178.172, 46.0]),
            ("s1", [2.0, 2.0, 56.188, 42.0]),
            ("s2", [60.188, 2.0, 50.172, 42.0]),
            ("s3", [112.359, 2.0, 30.906, 42.0]),
            ("s4", [145.266, 2.0, 30.906, 42.0]),
            ("t2", [0.0, 46.0, 15.641, 100.0]),
            ("x1", [2.0, 48.0, 11.641, 47.0]),
            ("x2", [2.0, 97.0, 11.641, 47.0]),
            ("t3", [0.0, 146.0, 29.281, 86.0]),
            ("d1", [2.0, 148.0, 11.641, 42.0]),
            ("d2", [15.641, 148.0, 11.641, 82.0]),
            ("d3", [2.0, 192.0, 11.641, 38.0]),
            ("t4", [67.422, 248.0, 19.266, 20.0]),
            ("u1", [67.422, 248.0, 19.266, 20.0]),
            ("u2", [67.422, 248.0, 19.266, 20.0]),
        ],
    );
}

/// Tables as flex items, in floats, centred by auto margins, and holding absolutely positioned boxes.
#[test]
fn tables_in_other_layouts() {
    check(
        "<div id=t1 style='display:flex'><table id=n1><tr><td>flex item table</td></tr></table>\
         <div id=n2>next</div></div><div id=t2 style='float:left'><table id=p1 style='width:100%'><tr>\
         <td id=p2>in float</td></tr></table></div><div style='clear:both'></div>\
         <table id=t3 style='margin:10px auto;width:50%'><tr><td id=v1>centered</td></tr></table>\
         <table id=t4 style='position:relative'><tr><td id=q1>a<span id=q2 style='position:absolute;left:0;\
         top:0'>abs</span></td></tr></table>",
        &[
            ("t1", [0.0, 0.0, 1280.0, 26.0]),
            ("n1", [0.0, 0.0, 150.5, 26.0]),
            ("n2", [150.5, 0.0, 38.531, 26.0]),
            ("t2", [0.0, 26.0, 83.062, 26.0]),
            ("p1", [0.0, 26.0, 83.062, 26.0]),
            ("p2", [2.0, 28.0, 79.062, 22.0]),
            ("t3", [320.0, 62.0, 640.0, 26.0]),
            ("v1", [322.0, 64.0, 636.0, 22.0]),
            ("t4", [0.0, 98.0, 15.641, 26.0]),
            ("q1", [2.0, 100.0, 11.641, 22.0]),
            ("q2", [0.0, 98.0, 28.906, 20.0]),
        ],
    );
}

#[test]
fn anonymous_table_boxes_wrap_what_their_parents_cannot_hold() {
    use madcawl_layout::tree::Kind;
    let dom = parse(
        "<!doctype html><body><div id=a><div style='display:table-row'>text</div>\
         <div style='display:table-cell'>c</div></div></body>",
    );
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    let t = l.boxes();
    let a = by_id(&dom, "a");
    let div = (0..t.len() as u32)
        .find(|&b| t.get(b).node == a)
        .expect("a box for #a");
    let kids: Vec<u32> = t.children(div).collect();
    // one anonymous table around the row and the cell, which gets an anonymous row of its own
    assert_eq!(kids.len(), 1, "{kids:?}");
    assert_eq!(t.get(kids[0]).kind, Kind::AnonBlock);
    let rows: Vec<u32> = t.children(kids[0]).collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(t.get(rows[0]).kind, Kind::Block);
    assert_eq!(t.get(rows[1]).kind, Kind::AnonBlock);
    // the row's text is in an anonymous cell
    let cells: Vec<u32> = t.children(rows[0]).collect();
    assert_eq!(cells.len(), 1);
    assert_eq!(t.get(cells[0]).kind, Kind::AnonBlock);
    assert_eq!(
        t.get(t.children(cells[0]).next().expect("text")).kind,
        Kind::Text
    );
}

#[test]
fn big_tables_lay_each_cell_out_a_bounded_number_of_times() {
    let html = |rows: usize| {
        format!(
            "<!doctype html><table>{}</table>",
            "<tr><td>a</td><td rowspan=2>b b</td><td colspan=2>c</td></tr><tr><td>d</td><td>e</td><td>f</td></tr>"
                .repeat(rows)
        )
    };
    same_without_reuse(&html(4), 1280.0, 800.0);
    let dom = parse(&html(500));
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert!(l.fragments().is_some());
    let c = l.counters();
    assert!(c.laid <= 3 * 500 * 9, "{c:?}");
}

#[test]
fn deeply_nested_anonymous_tables_stay_within_the_depth_layout_recurses_to() {
    // each cell nested in a cell gets an anonymous row and table around it
    let depth = 300;
    let html = format!(
        "<!doctype html><body>{}x{}",
        "<div style='display:table-cell'>".repeat(depth),
        "</div>".repeat(depth)
    );
    let dom = parse(&html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert!(l.fragments().is_some());
    let tables = "<table><tr><td>".repeat(100) + "x";
    let dom = parse(&format!("<!doctype html><body>{tables}"));
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    assert!(l.fragments().is_some());
}

/// A generated box of a row takes the pseudo-element's display, not the row's: a block `::before` is
/// wrapped in an anonymous cell.
#[test]
fn a_rows_generated_block_is_wrapped_in_an_anonymous_cell() {
    let html = "<!doctype html><meta charset=utf-8><body style='margin:0;font:16px Helvetica'>\
                <style>tr::before{content:\"\";display:block;width:30px;height:10px}</style>\
                <table id=t style='border-spacing:0'><tr id=r><td id=c>a</td></tr></table>";
    let dom = parse(html);
    let st = style(&dom);
    let mut l = Layout::new(&dom, &st, 1280.0, 800.0);
    for (id, r) in [("t", [0.0, 0.0, 40.906, 20.0]), ("c", [30.0, 0.0, 10.906, 20.0])] {
        let got = rect(&mut l, &dom, id);
        let close = got.iter().zip(r).all(|(a, b)| (a - b).abs() <= 0.02);
        assert!(close, "#{id}: {got:?}, webkit {r:?}");
    }
}

/// A cell spanning a fixed-width column and an auto one widens the auto one, after the fixed one's minimum
/// reaches its width; with a percentage column instead of the auto one, both in proportion to their
/// maximums (WebKit then widens the table for the percentage, `e` at 60.563, which is not done yet).
#[test]
fn a_spanning_cell_widens_auto_columns_first() {
    check(
        "<table style='border-spacing:0'><tr><td id=a style='width:50px;padding:0'>a</td>\
         <td id=b style='padding:0'>b</td></tr><tr><td colspan=2 id=c style='padding:0'>\
         xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx</td></tr></table>\
         <table style='border-spacing:0'><tr><td id=d style='width:50px;padding:0'>a</td>\
         <td id=e style='width:20%;padding:0'>b</td></tr><tr><td colspan=2 id=f style='padding:0'>\
         xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx</td></tr></table>",
        &[
            ("a", [0.0, 0.0, 50.0, 20.0]),
            ("b", [50.0, 0.0, 238.984, 20.0]),
            ("c", [0.0, 20.0, 288.984, 20.0]),
            ("d", [0.0, 40.0, 242.281, 20.0]),
        ],
    );
}

/// Collapsed borders split in whole device pixels: at 2x the halves of odd widths are halves of a px.
#[test]
fn collapsed_borders_split_in_device_pixels() {
    let html = "<!doctype html><meta charset=utf-8><body style='margin:0;font:16px/20px Menlo'>\
                <table id=t style='border-collapse:collapse;border:3px solid'><tr>\
                <td id=a style='border:1px solid;padding:0'>a</td><td id=b style='border:3px solid;padding:0'>b</td>\
                <td id=c style='border:2px solid;padding:0'>c</td></tr><tr>\
                <td id=d style='border:5px solid;padding:0'>d</td><td id=e style='padding:0'>e</td>\
                <td id=f style='padding:0;border:1px solid'>f</td></tr></table>";
    let dom = parse(html);
    let at = |scale: f32| {
        let device = madcawl_css::style::Device {
            scale: scale.into(),
            ..Default::default()
        };
        let st = madcawl_css::style::style_document(&dom, device, &mut |_| None);
        let mut l = Layout::new(&dom, &st, 1280.0, 800.0).with_scale(scale);
        ["t", "a", "b", "e", "f"].map(|id| rect(&mut l, &dom, id))
    };
    let want1 = [
        [0.0, 0.0, 42.922, 53.0],
        [1.0, 1.0, 14.641, 24.0],
        [15.641, 1.0, 12.641, 24.0],
        [15.641, 25.0, 12.641, 25.0],
        [28.281, 25.0, 12.641, 25.0],
    ];
    let want2 = [
        [0.0, 0.0, 42.922, 53.0],
        [1.5, 1.5, 14.641, 24.0],
        [16.141, 1.5, 12.641, 24.0],
        [16.141, 25.5, 12.641, 25.0],
        [28.781, 25.5, 12.641, 25.0],
    ];
    for (scale, want) in [(1.0, want1), (2.0, want2)] {
        let got = at(scale);
        for (g, w) in got.iter().zip(&want) {
            let close = g.iter().zip(w).all(|(a, b)| (a - b).abs() <= 0.02);
            assert!(close, "{scale}x: {got:?}, webkit {want:?}");
        }
    }
}
