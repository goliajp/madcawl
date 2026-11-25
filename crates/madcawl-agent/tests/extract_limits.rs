mod common;

use std::time::{Duration, Instant};

use common::Page;
use madcawl_agent::extract::{ExtractOpts, Format, Records, What};
use madcawl_agent::{Frame, Inputs, Query, ViewKind, ViewOpts, kind, tag};

fn records(p: &mut Page, opts: &ExtractOpts) -> Records {
    p.attach();
    let inputs = p.inputs();
    p.agents
        .tree()
        .expect("attached")
        .extract(&p.dom, inputs, opts)
}

fn text(p: &mut Page, opts: &ExtractOpts) -> String {
    let s = p.attach();
    let inputs = Inputs {
        styles: Some(&p.styles),
        geometry: p.geometry.as_ref(),
    };
    p.agents
        .extract(s, &mut p.dom, inputs, opts)
        .expect("a session")
        .1
        .text
}

/// Runs `f` and fails when it takes longer than `limit`: these cases
/// took seconds while their work grew with the square of the input.
fn within<T>(limit: Duration, what: &str, f: impl FnOnce() -> T) -> T {
    let t0 = Instant::now();
    let out = f();
    let took = t0.elapsed();
    assert!(took < limit, "{what} took {took:?}, over {limit:?}");
    out
}

#[test]
fn a_password_named_by_reference_leaks_into_no_view_and_no_extract() {
    let mut p = Page::new(
        "<form><input id=a aria-labelledby=pw>\
         <input type=password id=pw value=sekrit>\
         <button aria-labelledby=pw>b</button>\
         <label for=c>pin <input type=password value=sekrit></label><input id=c>\
         <table><tr><th aria-labelledby=pw>h</th></tr><tr><td>1</td></tr></table>\
         <ul><li><a href=/x aria-labelledby=pw>x</a><li><a href=/y>y</a><li><a href=/z>z</a></ul>\
         </form>",
    );
    let s = p.attach();
    let views = [
        ViewKind::Outline,
        ViewKind::Snapshot,
        ViewKind::Markdown,
        ViewKind::Find(Query {
            text: None,
            role: None,
            limit: 0,
        }),
    ];
    for v in views {
        let t = p.view(s, v.clone(), ViewOpts::default()).1.text;
        assert!(!t.contains("sekrit"), "{v:?}:\n{t}");
    }
    for what in [
        What::Table,
        What::List,
        What::Links,
        What::Images,
        What::Forms,
    ] {
        for format in [Format::JsonLines, Format::Markdown] {
            let mut o = ExtractOpts::new(what);
            o.format = format;
            o.include_hidden = true;
            let t = text(&mut p, &o);
            assert!(!t.contains("sekrit"), "{what:?} {format:?}:\n{t}");
        }
    }
    let mut o = ExtractOpts::new(What::Table);
    o.target = Some(p.by_id("pw"));
    let mut table = ExtractOpts::new(What::Table);
    table.target = p
        .dom
        .descendants(p.dom.document())
        .find(|&n| p.dom.local_name(n) == Some(madcawl_str::atom!("table")));
    for o in [o, table] {
        let t = text(&mut p, &o);
        assert!(!t.contains("sekrit"), "{t}");
    }
}

#[test]
fn a_table_spanning_more_cells_than_the_cap_is_cut_and_says_so() {
    let rows = "<tr><td>y</td></tr>".repeat(2000);
    let mut p = Page::new(&format!(
        "<table id=t><tr><td rowspan=0 colspan=1000>x</td></tr>{rows}</table>"
    ));
    let mut o = ExtractOpts::new(What::Table);
    o.target = Some(p.by_id("t"));
    o.budget = Some(2000);
    let t = within(Duration::from_secs(1), "a 2001 x 1000 table", || {
        text(&mut p, &o)
    });
    let first = t.lines().next().unwrap();
    assert!(first.contains("cut at row"), "{first}");
    let recs = records(&mut p, &o);
    assert!(recs.rows.len() * recs.columns.len() <= 1_000_000);
    assert!(recs.rows.len() >= 999, "{}", recs.rows.len());
}

#[test]
fn a_page_of_a_long_table_builds_only_its_own_records() {
    let rows: String = (0..20_000)
        .map(|i| {
            let rich = "<td><b>a</b> <i>b</i> <u>c</u> <s>d</s></td>".repeat(3);
            format!("<tr><td>{i}{rich}</tr>")
        })
        .collect();
    let mut p = Page::new(&format!("<table id=t>{rows}</table>"));
    let mut o = ExtractOpts::new(What::Table);
    o.target = Some(p.by_id("t"));
    o.budget = Some(200);
    // the first page and the cost of the grid, then twenty pages more
    let first = text(&mut p, &o);
    assert!(first.contains("20000 records"), "{first}");
    within(
        Duration::from_millis(1500),
        "twenty pages of a table",
        || {
            for k in 0..20 {
                o.offset = 1000 * k;
                let t = text(&mut p, &o);
                assert!(t.contains(&format!("\"c1\":\"{}\"", 1000 * k)), "{t}");
            }
        },
    );
}

#[test]
fn many_controls_naming_their_form_are_listed_in_linear_time() {
    let controls = "<input form=f name=n>".repeat(4000);
    let filler = "<div><span>t</span></div>".repeat(4000);
    let mut p = Page::new(&format!("{filler}{controls}<form id=f></form>"));
    let recs = within(Duration::from_secs(1), "4000 form= controls", || {
        records(&mut p, &ExtractOpts::new(What::Forms))
    });
    assert_eq!(recs.rows.len(), 4000);
    assert!(
        recs.rows
            .iter()
            .all(|r| r.get("form_name").and_then(|v| v.as_text()) == Some("f"))
    );
}

#[test]
fn a_thousand_equal_headers_get_numbered_keys_quickly() {
    let head = "<th>x</th>".repeat(1000);
    let body = "<td>1</td>".repeat(1000);
    let mut p = Page::new(&format!(
        "<table id=t><thead><tr>{head}</tr></thead><tr>{body}</tr></table>"
    ));
    let mut o = ExtractOpts::new(What::Table);
    o.target = Some(p.by_id("t"));
    let recs = within(Duration::from_millis(400), "1000 equal headers", || {
        records(&mut p, &o)
    });
    assert_eq!(recs.columns[0], "x");
    assert_eq!(recs.columns[999], "x (1000)");
}

#[test]
fn a_long_list_is_split_into_records_in_linear_time() {
    let items = "<li>an item</li><li class=d>its detail</li>".repeat(30_000);
    let mut p = Page::new(&format!(
        "<ul><li id=s>an item</li><li class=d>d</li>{items}</ul>"
    ));
    let mut o = ExtractOpts::new(What::List);
    o.target = Some(p.by_id("s"));
    let recs = within(Duration::from_millis(1500), "30000 items", || {
        records(&mut p, &o)
    });
    assert_eq!(recs.rows.len(), 30_001);
}

#[test]
fn deeply_nested_tables_are_indexed_without_rereading_each_level() {
    let depth = 1000;
    let words = "<i>t</i> ".repeat(20);
    // named rows and cells keep the tree's own names from reading down
    let open =
        format!("<table aria-label=t><tr aria-label=r><td aria-label=c>{words}").repeat(depth);
    let close = "</td></tr></table>".repeat(depth);
    let mut p = Page::new(&format!("{open}{close}"));
    // building the tree for so deep a page takes its own while
    p.attach();
    let inputs = p.inputs();
    let tree = p.agents.tree().expect("attached");
    let recs = within(
        Duration::from_millis(150),
        "indexing 1000 nested tables",
        || tree.extract(&p.dom, inputs, &ExtractOpts::new(What::Table)),
    );
    assert!(recs.rows.len() >= 500, "{}", recs.rows.len());
}

fn request(p: &mut Page, what: &str, format: &str) -> Frame {
    let s = p.attach();
    let f = Frame::new(kind::READ)
        .num(tag::SESSION, s as u64)
        .num(tag::VIEW, 4)
        .str(tag::WHAT, what)
        .str(tag::FORMAT, format);
    let inputs = Inputs {
        styles: Some(&p.styles),
        geometry: p.geometry.as_ref(),
    };
    p.agents.handle(&mut p.dom, inputs, &f).expect("known")
}

#[test]
fn an_unknown_what_or_format_is_an_error_frame() {
    let mut p = Page::new("<a href=/a>A</a>");
    let bad_what = request(&mut p, "tables", "json");
    assert_eq!(
        bad_what.kind,
        kind::ERROR,
        "{:?}",
        bad_what.get_str(tag::TEXT)
    );
    let bad_format = request(&mut p, "links", "csv");
    assert_eq!(bad_format.kind, kind::ERROR);
    assert_eq!(request(&mut p, "links", "json").kind, kind::RESULT);
    assert_eq!(request(&mut p, "links", "markdown").kind, kind::RESULT);
}
