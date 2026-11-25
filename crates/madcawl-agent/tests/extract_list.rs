mod common;

use common::Page;
use madcawl_agent::extract::{ExtractOpts, Records, Row, Value, What};

fn list(p: &mut Page, seed: Option<&str>) -> Records {
    let mut o = ExtractOpts::new(What::List);
    o.target = seed.map(|id| p.by_id(id));
    p.attach();
    let inputs = p.inputs();
    p.agents
        .tree()
        .expect("attached")
        .extract(&p.dom, inputs, &o)
}

fn field<'a>(r: &'a Row, k: &str) -> &'a str {
    r.get(k)
        .and_then(Value::as_text)
        .unwrap_or_else(|| panic!("no {k} in {r:?}"))
}

fn titles(r: &Records) -> Vec<&str> {
    r.rows.iter().map(|x| field(x, "title")).collect()
}

#[test]
fn a_seed_deep_in_an_item_gives_every_item_of_its_kind() {
    let mut p = Page::new(
        "<ul class=results>\
         <li class=hit><a href=/a><div class=name>alpha-1.0</div><div>first crate</div></a></li>\
         <li class=hit><a href=/b><div class=name id=seed>beta-2.0</div><div>second crate</div></a></li>\
         <li class=hit><a href=/c><div class=name>gamma-3.0</div><div>third crate</div></a></li>\
         <li class=ad>sponsored</li></ul>",
    );
    let l = list(&mut p, Some("seed"));
    assert_eq!(titles(&l), ["alpha-1.0", "beta-2.0", "gamma-3.0"]);
    let b = &l.rows[1];
    assert_eq!(field(b, "url"), "/b");
    assert_eq!(field(b, "text"), "beta-2.0 second crate");
    assert_eq!(
        b.get("parts"),
        Some(&Value::List(vec!["beta-2.0".into(), "second crate".into()]))
    );
}

#[test]
fn without_a_seed_the_items_with_the_most_text_outside_navigation_win() {
    let nav: String = (0..9)
        .map(|i| format!("<li><a href=/n{i}>navigation link number {i}</a></li>"))
        .collect();
    let main: String = (0..4)
        .map(|i| {
            format!(
                "<article><h2>Story {i}</h2><p>a longer paragraph of story text {i}</p></article>"
            )
        })
        .collect();
    let mut p = Page::new(&format!(
        "<nav><ul>{nav}</ul></nav><main><div>{main}</div></main>"
    ));
    let l = list(&mut p, None);
    assert_eq!(titles(&l), ["Story 0", "Story 1", "Story 2", "Story 3"]);
}

#[test]
fn one_item_holding_the_page_does_not_make_its_few_siblings_a_list() {
    let stories: String = (1..=5)
        .map(|i| {
            format!("<tr class=athing><td>{i}.</td><td><a href=/s{i}>Story {i} title</a></td></tr>")
        })
        .collect();
    let html = format!(
        "<table><tr><td>header</td></tr><tr><td><table>{stories}</table></td></tr>\
         <tr><td></td></tr><tr><td>footer</td></tr></table>"
    );
    let mut p = Page::new(&html);
    assert_eq!(list(&mut p, None).rows.len(), 5);
}

#[test]
fn a_regular_run_after_each_item_belongs_to_it() {
    let rows: String = (1..=4)
        .map(|i| {
            format!(
                "<tr class=athing><td>{i}.</td><td><a href=/s{i}>Story {i}</a></td></tr>\
                 <tr><td>{i}0 points by u{i}</td></tr><tr class=spacer></tr>"
            )
        })
        .collect();
    let mut p = Page::new(&format!("<table><tbody>{rows}</tbody></table>"));
    let l = list(&mut p, None);
    assert_eq!(titles(&l), ["Story 1", "Story 2", "Story 3", "Story 4"]);
    assert_eq!(field(&l.rows[3], "text"), "4. Story 4 40 points by u4");
}

#[test]
fn a_dt_before_each_dd_starts_the_record_even_when_the_dd_is_the_kind() {
    let items: String = (1..=5)
        .map(|i| format!("<dt><a href=/abs/{i}>arXiv:{i}</a></dt><dd>Title: paper {i} with a long abstract line</dd>"))
        .collect();
    let mut p = Page::new(&format!("<dl>{items}</dl>"));
    let l = list(&mut p, None);
    assert_eq!(
        titles(&l),
        ["arXiv:1", "arXiv:2", "arXiv:3", "arXiv:4", "arXiv:5"]
    );
    assert_eq!(field(&l.rows[0], "url"), "/abs/1");
    assert_eq!(
        field(&l.rows[0], "text"),
        "arXiv:1 Title: paper 1 with a long abstract line"
    );
}
