//! Appends batched into one restyle stay linear in what they reach,
//! the siblings each looks back over count as visits, and ids and
//! classes the DOM gives back and hands out again leave no stale
//! `PathKey` behind.

#[allow(dead_code)]
mod style;

use madcawl_css::style::{Change, ElementStates};
use madcawl_dom::{Dom, NodeId};
use madcawl_html::{Atom, Attrs, Namespace, TreeSink, atom};
use style::{Pair, by_id, check, parse, set, sheets};

fn append_many(dom: &mut Dom, parent: NodeId, names: &[Atom]) -> Vec<Change> {
    names
        .iter()
        .map(|&name| {
            let el = dom.create_element(Namespace::Html, name, Attrs::empty());
            dom.append(parent, el);
            Change::Appended(el)
        })
        .collect()
}

#[test]
fn a_batch_of_appends_under_nth_last_child_visits_linearly() {
    for n in [500u64, 2000] {
        let mut dom = parse(
            "<!doctype html><style>li:nth-last-child(2) { color: red }</style><ul id=u><li></li></ul>",
        );
        let (list, st) = (sheets(&dom), ElementStates::new());
        let c = Pair::new(&list);
        let mut s = c.compute_with(&dom, &st);
        let u = by_id(&dom, "u");
        let ch = append_many(&mut dom, u, &vec![atom!("li"); n as usize]);
        let r = check(&c, &dom, &mut s, &st, &ch);
        assert!(!r.full);
        assert!(r.visited <= 4 * n, "{n}: {r:?}");
    }
}

fn names(prefix: &str, n: usize) -> (String, Vec<String>) {
    let v: Vec<String> = (0..n).map(|k| format!("{prefix}{k}")).collect();
    let html = v.iter().map(|x| format!("<{x}></{x}>")).collect();
    (html, v)
}

#[test]
fn a_batch_of_differently_named_appends_under_last_of_type_visits_linearly() {
    let n = 2000;
    // the names are made atoms by the template, so the appends can use them
    let (html, v) = names("x-e", n);
    let mut dom = parse(&format!(
        "<!doctype html><style>:last-of-type {{ color: red }}</style><div id=u><p></p></div><template>{html}</template>"
    ));
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let u = by_id(&dom, "u");
    let atoms: Vec<Atom> = v
        .iter()
        .map(|x| Atom::lookup(x.as_bytes()).unwrap())
        .collect();
    let ch = append_many(&mut dom, u, &atoms);
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert!(r.visited <= 4 * n as u64, "{r:?}");
}

#[test]
fn looking_back_over_siblings_counts_as_visits() {
    let n = 3000;
    let (html, v) = names("x-f", n);
    let mut dom = parse(&format!(
        "<!doctype html><style>:last-of-type {{ color: red }}</style><div id=u>{html}</div>"
    ));
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let u = by_id(&dom, "u");
    let name = Atom::lookup(v[0].as_bytes()).unwrap();
    let ch = append_many(&mut dom, u, &[name]);
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert!(
        r.visited >= n as u64,
        "the walk back to the first is counted: {r:?}"
    );
}

#[test]
fn whether_the_parent_was_empty_is_asked_only_when_a_selector_does() {
    for (sheet, many) in [
        ("div:empty { color: red }", true),
        ("p { color: red }", false),
    ] {
        let mut dom = parse(&format!(
            "<!doctype html><style>{sheet}</style><div id=u>{}</div>",
            "<!---->".repeat(3000)
        ));
        let (list, st) = (sheets(&dom), ElementStates::new());
        let c = Pair::new(&list);
        let mut s = c.compute_with(&dom, &st);
        let u = by_id(&dom, "u");
        let ch = append_many(&mut dom, u, &[atom!("p")]);
        let r = check(&c, &dom, &mut s, &st, &ch);
        if many {
            assert!(r.visited >= 3000, "{r:?}");
        } else {
            assert!(r.visited <= 2, "{r:?}");
        }
    }
}

#[test]
fn ids_and_classes_given_back_and_handed_out_again_restyle_right() {
    let mut dom = parse(
        "<!doctype html><style>#q { color: red } .a { float: left } .b.a > i { opacity: 0.5 }</style>\
         <div id=p><span id=t><i></i></span><span><i></i></span></div>",
    );
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let t = by_id(&dom, "t");
    let other = dom.next_sibling(t).unwrap();
    let most = dom.table_bytes() + 400;
    for i in 0..3000 {
        let id = if i % 3 == 0 {
            "q".to_string()
        } else {
            format!("x-fresh-{i}")
        };
        let class = match i % 4 {
            0 => format!("a b x-c{i}"),
            1 => "a".to_string(),
            _ => format!("x-c{i} b"),
        };
        let ch = [
            set(&mut dom, t, atom!("id"), &id),
            set(&mut dom, t, atom!("class"), &class),
            set(&mut dom, other, atom!("class"), &class),
        ];
        if i % 25 == 0 {
            check(&c, &dom, &mut s, &st, &ch);
        } else {
            c.keyed.restyle(&dom, &mut s, &st, &ch);
        }
        assert!(dom.table_bytes() <= most, "{i}: {}", dom.table_bytes());
    }
    check(&c, &dom, &mut s, &st, &[]);
}
