//! A windowless page's semantic tree is built at open only as far as
//! the page outline reads it, and made whole by the first request that
//! needs more, once; every answer is that of a page whose tree was built
//! whole when it opened.

use std::time::Instant;

use madcawl_agent::act::{Action, Until};
use madcawl_agent::{Agents, Frame, kind, tag};

use super::{Fixture, act, outline, plain, read, ref_of, run, tab, waited};
use crate::serve::page::Page;

/// The fixture's page with its tree built whole at open.
fn open_whole(f: &Fixture) -> Page {
    let mut p = Page::blank(1, (1280.0, 800.0), None, 1);
    p.agents = Agents::new();
    let path = f.0.join("p.html");
    let url = format!("file://{}", path.display());
    p.load_file(&path, url, None, std::slice::from_ref(&f.0)).unwrap();
    p
}

fn deferred(p: &Page) -> bool {
    p.agents.is_attached() && p.agents.tree().is_none()
}

/// The reference the page outline gives the heading.
fn heading(p: &mut Page) -> String {
    let o = outline(p);
    let line = o.lines().find(|l| l.contains("heading \"Head\"")).unwrap_or_else(|| panic!("{o}"));
    line.split('[').nth(1).and_then(|s| s.split(']').next()).unwrap().to_owned()
}

fn wait_for(p: &mut Page, f: &Fixture, reference: String) -> String {
    let until = Until { reference: Some(reference), timeout_ms: 1000, ..Until::default() };
    let r = run(p, None, &until, None, Instant::now(), &f.env()).unwrap_or_else(|e| e);
    r.get_str(tag::TEXT).unwrap().to_owned()
}

type Ask = fn(&mut Page, &Fixture, String);

/// Every kind of request but the page outline, those that need a
/// reference given the heading's from the outline.
fn asks() -> [(&'static str, Ask); 14] {
    [
        ("snapshot", |p, _, _| drop(read(p, 1, 0, 0))),
        ("markdown", |p, _, _| drop(read(p, 2, 0, 0))),
        ("find", |p, _, _| drop(read(p, 3, 0, 0))),
        ("extract", |p, _, _| drop(read(p, 4, 0, 0))),
        ("main outline", |p, _, _| drop(read(p, 0, 3, 0))),
        ("viewport outline", |p, _, _| drop(read(p, 0, 1, 0))),
        ("subtree", |p, _, r| drop(p.handle(&Frame::new(kind::READ).num(tag::VIEW, 1).num(tag::SCOPE, 2).str(tag::REF, &r)))),
        ("diff", |p, _, _| drop(p.handle(&Frame::new(kind::DIFF).num(tag::SINCE, 0)))),
        ("audit target", |p, _, r| drop(p.target(&r))),
        ("wait for a reference", |p, f, r| drop(wait_for(p, f, r))),
        ("wait for text", |p, f, _| drop(waited(p, f, "some text"))),
        ("hover", |p, f, r| drop(act(p, f, Action::Hover(r)))),
        ("click", |p, f, r| drop(act(p, f, Action::Click(r)))),
        ("scroll", |p, f, _| drop(act(p, f, Action::Scroll { target: None, by: 1 }))),
    ]
}

#[test]
fn the_open_outline_does_not_build_the_whole_tree() {
    let f = Fixture::new();
    let mut p = f.open();
    assert!(deferred(&p), "opening builds only what the outline reads");
    outline(&mut p);
    read(&mut p, 0, 0, 1);
    let budget = Frame::new(kind::READ).num(tag::VIEW, 0).num(tag::BUDGET, 5);
    p.handle(&budget).unwrap();
    assert!(deferred(&p), "nor does the outline, with hidden content or a budget");
    let whole = open_whole(&f);
    assert!(whole.agents.tree().is_some());
    assert!(p.agents.bytes() < whole.agents.bytes(), "{} {}", p.agents.bytes(), whole.agents.bytes());
}

#[test]
fn every_other_request_makes_the_tree_whole_once() {
    let f = Fixture::new();
    for (name, ask) in asks() {
        let mut p = f.open();
        let r = heading(&mut p);
        assert!(deferred(&p), "{name}");
        let (page_gen, seq) = (p.agents.page_gen(), p.agents.seq());
        ask(&mut p, &f, r.clone());
        assert!(!deferred(&p), "{name} makes the tree whole");
        assert_eq!(p.agents.page_gen(), page_gen, "{name}: completed, not built again");
        assert!(p.agents.seq() <= seq + 1, "{name}: only the layout's change, if any");
        for (_, again) in asks() {
            again(&mut p, &f, r.clone());
        }
        assert_eq!(p.agents.page_gen(), page_gen, "{name}, then every other request");
    }
}

#[test]
fn answers_are_those_of_a_tree_built_whole_at_open() {
    let f = Fixture::new();
    let mut eager = open_whole(&f);
    let mut lazy = f.open();
    let seqs = |p: &mut Page| p.handle(&Frame::new(kind::READ).num(tag::VIEW, 0)).unwrap().get_num(tag::SEQ);
    assert_eq!(seqs(&mut eager), seqs(&mut lazy));
    assert_eq!(plain(&outline(&mut eager)), plain(&outline(&mut lazy)));
    let refs = [heading(&mut eager), heading(&mut lazy)];
    // the same node and slot generation; the page generations differ
    assert_eq!(refs[0].split('y').next(), refs[1].split('y').next());
    for (view, scope, hidden) in [(1, 0, 0), (1, 0, 1), (2, 0, 0), (3, 0, 0), (4, 0, 0), (0, 3, 0), (1, 3, 1), (0, 1, 0), (1, 1, 1)] {
        let (a, b) = (read(&mut eager, view, scope, hidden), read(&mut lazy, view, scope, hidden));
        assert_eq!(plain(&a), plain(&b), "view {view} scope {scope} hidden {hidden}");
    }
    let replies = [tab(&mut eager, &f), tab(&mut lazy, &f)];
    assert_eq!(plain(&replies[0]), plain(&replies[1]));
    let mut clicked = Vec::new();
    for p in [&mut eager, &mut lazy] {
        let r = heading(p);
        clicked.push(plain(&act(p, &f, Action::Click(r))));
        let r = ref_of(p, "Box");
        clicked.push(plain(&act(p, &f, Action::Click(r))));
    }
    assert_eq!(clicked[..2], clicked[2..]);
    assert!(clicked[1].contains("checked"), "{}", clicked[1]);
    let d = Frame::new(kind::DIFF).num(tag::SINCE, 0);
    let diffs = [eager.handle(&d).unwrap(), lazy.handle(&d).unwrap()];
    assert_eq!(diffs[0].get_num(tag::SEQ), diffs[1].get_num(tag::SEQ));
    assert_eq!(plain(diffs[0].get_str(tag::TEXT).unwrap()), plain(diffs[1].get_str(tag::TEXT).unwrap()));
    assert_eq!(eager.agents.seq(), lazy.agents.seq());
}
