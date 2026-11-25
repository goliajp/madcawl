mod common;

use common::Page;
use madcawl_agent::extract::{ExtractOpts, Format, Records, Value, What};
use madcawl_agent::{Frame, Inputs, kind, tag};

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

fn get<'a>(r: &'a madcawl_agent::extract::Row, k: &str) -> &'a str {
    r.get(k)
        .and_then(Value::as_text)
        .unwrap_or_else(|| panic!("no {k} in {r:?}"))
}

fn table_of(p: &mut Page, id: &str) -> Records {
    let mut o = ExtractOpts::new(What::Table);
    o.target = Some(p.by_id(id));
    records(p, &o)
}

#[test]
fn a_table_is_keyed_by_its_two_row_header_with_spans_expanded() {
    let mut p = Page::new(
        "<table id=t><thead>\
         <tr><th rowspan=2>Name<th colspan=2>2026</tr>\
         <tr><th>Jan<th>Feb</tr></thead>\
         <tbody><tr id=r1><td rowspan=2>a<td>1<td>2</tr>\
         <tr><td colspan=2>3</tr></tbody></table>",
    );
    let t = table_of(&mut p, "t");
    assert_eq!(t.columns, ["Name", "2026 / Jan", "2026 / Feb"]);
    assert_eq!(t.rows.len(), 2);
    let row = |i: usize, k: &str| get(&t.rows[i], k).to_owned();
    assert_eq!(
        [row(0, "Name"), row(0, "2026 / Jan"), row(0, "2026 / Feb")],
        ["a", "1", "2"]
    );
    assert_eq!(
        [row(1, "Name"), row(1, "2026 / Jan"), row(1, "2026 / Feb")],
        ["a", "3", "3"]
    );
    // a row's reference names its row
    let s = p.attach();
    let r1 = p.by_id("r1");
    assert_eq!(p.resolve(s, &t.rows[0].reference), Ok(r1));
}

#[test]
fn a_title_row_names_the_table_and_a_table_without_header_gets_column_numbers() {
    let mut p = Page::new(
        "<table id=t><tr><th colspan=2>Rust</tr>\
         <tr><th>Paradigms<td>multi</tr><tr><th>Typing<td>static</tr>\
         <tr></tr><tr><th colspan=2>Influenced by</tr></table>",
    );
    let t = table_of(&mut p, "t");
    assert_eq!(t.label, "Rust");
    assert_eq!(t.columns, ["c1", "c2"]);
    assert_eq!(t.rows.len(), 3, "the empty row is no record");
    assert_eq!(
        (get(&t.rows[0], "c1"), get(&t.rows[0], "c2")),
        ("Paradigms", "multi")
    );
    assert_eq!(get(&t.rows[2], "c2"), "Influenced by");
}

#[test]
fn a_cell_seeds_its_table_and_no_target_lists_the_tables() {
    let mut p = Page::new(
        "<table><caption>First</caption><tr><th>A<th>A<tr><td>1<td id=c>2</table>\
         <table style='display:none'><tr><td>x</table>\
         <table><tr><td>y</table>",
    );
    let mut o = ExtractOpts::new(What::Table);
    o.target = Some(p.by_id("c"));
    let t = records(&mut p, &o);
    assert_eq!(t.label, "First");
    assert_eq!(t.columns, ["A", "A (2)"]);
    let all = records(&mut p, &ExtractOpts::new(What::Table));
    assert_eq!(all.rows.len(), 2, "the hidden table is left out");
    assert_eq!(get(&all.rows[0], "name"), "First");
    assert_eq!(
        all.rows[0].get("header"),
        Some(&Value::List(vec!["A".into(), "A (2)".into()]))
    );
}

#[test]
fn links_carry_their_address_and_landmark_and_hidden_ones_are_asked_for() {
    let html = "<nav aria-label=Site><a href=/home>Home</a></nav>\
                <main><p><a href='https://x.test/a'>A <b>b</b></a>\
                <a href=/secret style='display:none'>secret</a></main>";
    let mut p = Page::new(html);
    let l = records(&mut p, &ExtractOpts::new(What::Links));
    let got: Vec<(&str, &str, &str)> = l
        .rows
        .iter()
        .map(|r| (get(r, "text"), get(r, "url"), get(r, "landmark")))
        .collect();
    assert_eq!(
        got,
        [
            ("Home", "/home", "navigation Site"),
            ("A b", "https://x.test/a", "main")
        ]
    );
    let mut o = ExtractOpts::new(What::Links);
    o.include_hidden = true;
    assert_eq!(records(&mut p, &o).rows.len(), 3);
}

#[test]
fn images_include_decorative_ones_but_not_hidden_ones() {
    let mut p = Page::new(
        "<img src=a.png alt='A cat' width=10><img src=b.png alt=''>\
         <img src=c.png alt=gone style='display:none'><div role=img aria-label=Chart></div>",
    );
    let i = records(&mut p, &ExtractOpts::new(What::Images));
    let got: Vec<(&str, &str)> = i
        .rows
        .iter()
        .map(|r| (get(r, "alt"), get(r, "src")))
        .collect();
    assert_eq!(got, [("A cat", "a.png"), ("", "b.png"), ("Chart", "")]);
    assert_eq!(get(&i.rows[0], "width"), "10");
}

#[test]
fn form_fields_are_listed_and_a_password_value_never_is() {
    let html = "<form id=f name=login action=/in>\
                <label>User <input name=u required value=ann></label>\
                <input type=password name=pw value=hunter2>\
                <input type=hidden name=csrf value=tok123>\
                <select name=s><option>a<option selected>b</select>\
                <input type=checkbox name=c checked>\
                <button>Go</button></form>\
                <input form=f name=outside>";
    let mut p = Page::new(html);
    let f = records(&mut p, &ExtractOpts::new(What::Forms));
    let names: Vec<&str> = f.rows.iter().map(|r| get(r, "name")).collect();
    assert_eq!(
        names,
        ["u", "pw", "s", "c", "", "outside"],
        "the hidden input is left out"
    );
    let u = &f.rows[0];
    assert_eq!(
        (get(u, "label"), get(u, "value"), get(u, "form_name")),
        ("User", "ann", "login")
    );
    assert_eq!(u.get("required"), Some(&Value::Flag(true)));
    assert_eq!(get(&f.rows[1], "value"), "••••••");
    assert_eq!(get(&f.rows[2], "value"), "b");
    assert_eq!(
        f.rows[2].get("options"),
        Some(&Value::List(vec!["a".into(), "b".into()]))
    );
    assert_eq!(f.rows[3].get("checked"), Some(&Value::Flag(true)));
    assert_eq!(get(&f.rows[4], "type"), "submit");
    assert_eq!(
        get(&f.rows[5], "form"),
        get(u, "form"),
        "owned through the form attribute"
    );
    let mut o = ExtractOpts::new(What::Forms);
    o.include_hidden = true;
    let out = text(&mut p, &o);
    assert!(out.contains("\"csrf\"") && out.contains("tok123"), "{out}");
    assert!(!out.contains("hunter2"), "{out}");
}

#[test]
fn page_text_cannot_forge_a_reference_in_either_format() {
    let html = "<table id=t><tr><th>k<th>v<tr><td>see [e1]<td>a|b \"q\"</table>";
    let mut p = Page::new(html);
    let mut o = ExtractOpts::new(What::Table);
    o.target = Some(p.by_id("t"));
    let json = text(&mut p, &o);
    assert!(json.contains(r#""k":"see [\\e1]""#), "{json}");
    assert!(json.contains(r#""v":"a|b \"q\"""#), "{json}");
    o.format = Format::Markdown;
    let md = text(&mut p, &o);
    assert!(md.contains("| ref | k | v |"), "{md}");
    assert!(md.contains(r#"see [\e1] | a\|b "q" |"#), "{md}");
}

#[test]
fn a_budget_cuts_between_records_and_the_offset_goes_on() {
    let rows: String = (0..50)
        .map(|i| format!("<tr><td>row {i}</td></tr>"))
        .collect();
    let mut p = Page::new(&format!("<table id=t><tr><th>n</th></tr>{rows}</table>"));
    let mut o = ExtractOpts::new(What::Table);
    o.target = Some(p.by_id("t"));
    o.budget = Some(60);
    let first = text(&mut p, &o);
    let shown = first.lines().filter(|l| l.starts_with('{')).count();
    assert!(shown > 0 && shown < 50, "{first}");
    assert!(
        first
            .lines()
            .next()
            .unwrap()
            .ends_with(&format!("more from offset {shown}")),
        "{first}"
    );
    o.offset = shown;
    o.budget = None;
    let rest = text(&mut p, &o);
    assert_eq!(
        rest.lines().filter(|l| l.starts_with('{')).count(),
        50 - shown
    );
    assert!(
        rest.contains(&format!("\"row {shown}\"")) && rest.contains("\"row 49\""),
        "{rest}"
    );
    assert!(!rest.contains("more from offset"), "{rest}");
}

#[test]
fn an_extract_request_frame_is_answered() {
    let mut p = Page::new("<ul><li><a href=/a>A</a><li><a href=/b>B</a></ul>");
    let s = p.attach();
    let f = Frame::new(kind::READ)
        .num(tag::SESSION, s as u64)
        .num(tag::VIEW, 4)
        .str(tag::WHAT, "links")
        .str(tag::FORMAT, "markdown");
    let inputs = Inputs {
        styles: Some(&p.styles),
        geometry: p.geometry.as_ref(),
    };
    let r = p
        .agents
        .handle(&mut p.dom, inputs, &f)
        .expect("a known request");
    assert_eq!(r.kind, kind::RESULT);
    let t = r.get_str(tag::TEXT).unwrap();
    assert!(t.starts_with("extract links · 2 records"), "{t}");
    assert!(t.contains("| A | /a |"), "{t}");
}
