//! A windowless page is laid out on the first question that needs
//! geometry and not before, once; answers that need it are those of a
//! page laid out when it opened.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use madcawl_agent::act::{Action, Until};
use madcawl_agent::{Frame, kind, tag};

use super::run;
use crate::serve::page::{Env, Page};

const PAGE: &str = r#"<!doctype html><title>Lazy</title><button>Start</button>
<a href="/skip" style="position:absolute;left:-9999px">Skip</a>
<h1>Head</h1><p>Some text</p>
<div style="width:0;height:0;overflow:hidden"><a href="/gone">Clipped</a></div>
<label><input type=checkbox> Box</label>
<div style="height:2000px">filler</div><button>Far</button>"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Fixture {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcs-lazy-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.html"), PAGE).unwrap();
        Fixture(dir)
    }

    fn open(&self) -> Page {
        let mut p = Page::blank(1, (1280.0, 800.0), None, 1);
        let path = self.0.join("p.html");
        let url = format!("file://{}", path.display());
        p.load_file(&path, url, None, std::slice::from_ref(&self.0)).unwrap();
        p
    }

    fn env(&self) -> Env<'_> {
        Env { allow: std::slice::from_ref(&self.0), net: None, uploads: &[], approve: &[], owner: 1, run: "t" }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn read(p: &mut Page, view: u64, scope: u64, hidden: u64) -> String {
    let f = Frame::new(kind::READ).num(tag::VIEW, view).num(tag::SCOPE, scope).num(tag::HIDDEN, hidden);
    p.handle(&f).unwrap().get_str(tag::TEXT).unwrap().to_owned()
}

fn outline(p: &mut Page) -> String {
    p.handle(&Frame::new(kind::READ).num(tag::VIEW, 0)).unwrap().get_str(tag::TEXT).unwrap().to_owned()
}

/// The reference on the first line of a find for `text`.
fn ref_of(p: &mut Page, text: &str) -> String {
    let f = Frame::new(kind::READ).num(tag::VIEW, 3).str(tag::TEXT, text);
    let t = p.handle(&f).unwrap().get_str(tag::TEXT).unwrap().to_owned();
    t.split('[').nth(1).and_then(|s| s.split(']').next()).unwrap_or_else(|| panic!("{t}")).to_owned()
}

/// `t` without the page generations of its references and the
/// sequences of its diff headers, which differ from page to page.
fn plain(t: &str) -> String {
    let mut out = String::new();
    for line in t.lines() {
        let line = match line.strip_prefix("diff ").and_then(|r| r.split_once(": ")) {
            Some((_, changes)) => format!("diff: {changes}"),
            None => line.to_owned(),
        };
        let mut skip = false;
        let mut prev = ' ';
        let mut in_ref = false;
        for c in line.chars() {
            if matches!(prev, '[' | '(') && c == 'e' {
                in_ref = true;
            }
            if !c.is_ascii_alphanumeric() {
                in_ref = false;
                skip = false;
            } else if in_ref && c == 'y' {
                skip = true;
            }
            if !skip {
                out.push(c);
            }
            prev = c;
        }
        out.push('\n');
    }
    out
}

fn act(p: &mut Page, f: &Fixture, a: Action) -> String {
    let until = Until { timeout_ms: 1000, ..Until::default() };
    let r = run(p, Some(&a), &until, None, Instant::now(), &f.env()).unwrap_or_else(|e| panic!("{:?}", e.get_str(tag::TEXT)));
    r.get_str(tag::TEXT).unwrap().to_owned()
}

/// What is missing once the page is stable, waiting for `text`.
fn waited(p: &mut Page, f: &Fixture, text: &str) -> Vec<String> {
    let until = Until { text: Some(text.into()), timeout_ms: 1000, ..Until::default() };
    p.wait(&until, Instant::now(), &f.env()).missing
}

/// The reply to Tab pressed with the focus on the first button.
fn tab(p: &mut Page, f: &Fixture) -> String {
    let start = ref_of(p, "Start");
    p.focus = Some(p.resolve(&start).unwrap());
    act(p, f, Action::Press { target: None, key: "Tab".into() })
}

#[test]
fn reading_without_geometry_does_not_lay_out() {
    let f = Fixture::new();
    let mut p = f.open();
    outline(&mut p);
    let s = read(&mut p, 1, 0, 0);
    assert!(s.contains("Skip"), "without geometry only the styles say what is shown: {s}");
    for view in 0..5 {
        read(&mut p, view, 0, 0);
        read(&mut p, view, 3, 1);
    }
    p.handle(&Frame::new(kind::DIFF).num(tag::SINCE, 0)).unwrap();
    let box_ref = ref_of(&mut p, "Box");
    p.target(&box_ref).unwrap();
    assert_eq!(p.doc.layouts, 0);
    assert!(p.doc.geometry.is_none());
}

#[test]
fn each_question_of_geometry_lays_out_once() {
    let f = Fixture::new();
    type Ask = fn(&mut Page, &Fixture);
    let asks: [(&str, Ask); 7] = [
        ("viewport read", |p, _| drop(read(p, 1, 1, 0))),
        ("viewport extract", |p, _| drop(read(p, 4, 1, 0))),
        ("scroll", |p, f| drop(act(p, f, Action::Scroll { target: None, by: 1 }))),
        ("click", |p, f| {
            let r = ref_of(p, "Box");
            act(p, f, Action::Click(r));
        }),
        ("hover", |p, f| {
            let r = ref_of(p, "Far");
            act(p, f, Action::Hover(r));
        }),
        ("wait for text", |p, f| drop(waited(p, f, "some text"))),
        ("tab", |p, f| drop(tab(p, f))),
    ];
    for (name, ask) in asks {
        let mut p = f.open();
        outline(&mut p);
        ask(&mut p, &f);
        assert_eq!(p.doc.layouts, 1, "{name}");
        for (_, again) in &asks {
            again(&mut p, &f);
        }
        assert_eq!(p.doc.layouts, 1, "{name}, then every other question");
    }
}

#[test]
fn answers_with_geometry_are_those_of_a_page_laid_out_at_open() {
    let f = Fixture::new();
    let mut eager = f.open();
    eager.geometry();
    outline(&mut eager);
    let before = read(&mut eager, 1, 0, 0);
    let mut lazy = f.open();
    outline(&mut lazy);
    let viewport = [read(&mut eager, 1, 1, 0), read(&mut lazy, 1, 1, 0)];
    assert_eq!(plain(&viewport[0]), plain(&viewport[1]));
    for (view, scope, hidden) in [(0, 0, 0), (1, 0, 0), (1, 0, 1), (2, 0, 0), (1, 3, 0), (4, 0, 0), (1, 1, 1)] {
        let (a, b) = (read(&mut eager, view, scope, hidden), read(&mut lazy, view, scope, hidden));
        assert_eq!(plain(&a), plain(&b), "view {view} scope {scope} hidden {hidden}");
    }
    assert!(!before.contains("Skip") && !before.contains("Clipped"), "{before}");
}

#[test]
fn an_action_that_lays_out_reports_only_what_it_changed() {
    let f = Fixture::new();
    let mut eager = f.open();
    eager.geometry();
    outline(&mut eager);
    let mut lazy = f.open();
    let seq = lazy.handle(&Frame::new(kind::READ).num(tag::VIEW, 0)).unwrap().get_num(tag::SEQ).unwrap();
    let o = read(&mut lazy, 1, 0, 0);
    assert!(o.contains("Skip") && o.contains("Clipped"), "{o}");
    let (e, l) = (ref_of(&mut eager, "Box"), ref_of(&mut lazy, "Box"));
    let replies = [act(&mut eager, &f, Action::Click(e)), act(&mut lazy, &f, Action::Click(l))];
    assert!(replies[1].contains("checked"), "{}", replies[1]);
    assert!(!replies[1].contains("- ["), "the shown set refined by layout is not the click's: {}", replies[1]);
    assert_eq!(plain(&replies[0]), plain(&replies[1]));
    let all = lazy.handle(&Frame::new(kind::DIFF).num(tag::SINCE, seq)).unwrap();
    let all = all.get_str(tag::TEXT).unwrap();
    assert!(all.contains("checkbox") && !all.contains("- ["), "nor a diff from before it: {all}");
}

#[test]
fn what_is_not_shown_by_geometry_is_skipped_by_tab_and_waits() {
    let f = Fixture::new();
    let mut eager = f.open();
    eager.geometry();
    outline(&mut eager);
    let mut lazy = f.open();
    outline(&mut lazy);
    for text in ["Skip", "Clipped"] {
        let mut fresh = f.open();
        outline(&mut fresh);
        for p in [&mut eager, &mut fresh] {
            let missing = waited(p, &f, text);
            assert!(missing.iter().any(|m| m.contains("is not on the page")), "{text}: {missing:?}");
        }
    }
    let (e, l) = (waited(&mut eager, &f, "some text"), waited(&mut lazy, &f, "some text"));
    assert!(e.is_empty() && l.is_empty(), "{e:?} {l:?}");
    let mut lazy = f.open();
    outline(&mut lazy);
    let replies = [tab(&mut eager, &f), tab(&mut lazy, &f)];
    assert!(replies[1].contains("the focus is on checkbox \"Box\""), "{}", replies[1]);
    assert_eq!(plain(&replies[0]), plain(&replies[1]));
}

#[path = "lazy_tree_tests.rs"]
mod tree;
