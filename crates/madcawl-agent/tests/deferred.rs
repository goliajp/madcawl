//! A tree built only as far as the page outline reads it answers every
//! request as a tree built whole when the session attached would.

mod common;

use common::Page;
use madcawl_agent::{Agents, Frame, Geometry, Inputs, SemTree, SessionId, ViewOpts, kind, tag};
use madcawl_layout::Layout;
use madcawl_str::{DomString, atom};

const HTML: &str = r#"<title>Deferred</title>
<header><nav aria-label="Top"><a href=/a>Home</a> <a href=/b>About</a></nav></header>
<span id=sec>Section title</span><button aria-labelledby=sec>pre</button>
<main><h1 id=h1>Main <em>head</em></h1>
<section aria-labelledby=sec><h2>Inside</h2><p>para one with <a href=/c>link</a></p></section>
<section><h3>Unnamed section</h3></section>
<form aria-label="Find"><label for=q>Query</label><input id=q value=cats>
<select><option>One<option selected>Two</select><input type=checkbox checked></form>
<h2 style="display:none">Hidden head</h2>
<div style="opacity:0"><h2>Clear head</h2><button>Clear</button></div>
<table><tr><th>A<th>B<tr><td>1<td>2</table><ul><li>x<li>y</ul><img alt=Pic src=p.png>
<aside><h4 aria-labelledby=h1>Aside</h4></aside><div role=region aria-labelledby=sec>r</div></main>
<footer>foot <div style="width:0;height:0;overflow:hidden"><a href=/gone>Clipped</a></div></footer>"#;

/// A page and its session, the tree whole at attach or deferred, laid
/// out or not.
fn page(deferred: bool, laid_out: bool) -> (Page, SessionId) {
    let mut p = Page::new(HTML);
    if deferred {
        p.agents = Agents::new().deferring();
    }
    if !laid_out {
        p.geometry = None;
    }
    let s = p.attach();
    (p, s)
}

fn ask(p: &mut Page, f: &Frame) -> (u64, String) {
    let inputs = Inputs {
        styles: Some(&p.styles),
        geometry: p.geometry.as_ref(),
    };
    let r = p.agents.handle(&mut p.dom, inputs, f).expect("a request");
    let text = r.get_str(tag::TEXT).unwrap_or("").to_owned();
    (r.get_num(tag::SEQ).unwrap_or(u64::MAX), text)
}

fn read(s: SessionId, view: u64, scope: u64) -> Frame {
    Frame::new(kind::READ)
        .num(tag::SESSION, s as u64)
        .num(tag::VIEW, view)
        .num(tag::SCOPE, scope)
}

fn outline(s: SessionId) -> Frame {
    read(s, 0, 0)
}

/// Every kind of read but the page outline.
fn reads(s: SessionId) -> Vec<Frame> {
    let mut out = vec![
        read(s, 1, 0),
        read(s, 1, 0).num(tag::HIDDEN, 1),
        read(s, 1, 3),
        read(s, 0, 3),
        read(s, 2, 0),
        read(s, 3, 0).str(tag::TEXT, "head"),
        read(s, 3, 0).str(tag::ROLE, "link"),
        read(s, 1, 0).num(tag::BUDGET, 40),
        Frame::new(kind::DIFF)
            .num(tag::SESSION, s as u64)
            .num(tag::SINCE, 0),
    ];
    for what in ["table", "list", "links", "images", "forms"] {
        out.push(read(s, 4, 0).str(tag::WHAT, what));
    }
    out
}

/// Lays the page out under the styles it has.
fn lay_out(p: &mut Page) {
    let mut layout = Layout::new(&p.dom, &p.styles, 1280.0, 800.0);
    p.geometry = Some(Geometry::from_layout(&mut layout, &p.styles, 1280.0, 800.0));
}

fn whole(p: &Page) -> bool {
    p.agents.is_attached() && p.agents.tree().is_some()
}

#[test]
fn the_page_outline_comes_from_the_deferred_tree_as_from_a_whole_one() {
    for laid_out in [true, false] {
        let (mut eager, s) = page(false, laid_out);
        let (mut lazy, t) = page(true, laid_out);
        assert_eq!(s, t);
        assert!(!whole(&lazy) && lazy.agents.is_attached());
        assert!(lazy.agents.bytes() < eager.agents.bytes());
        for f in [
            outline(s),
            outline(s).num(tag::HIDDEN, 1),
            outline(s).num(tag::BUDGET, 30),
            outline(s).num(tag::BUDGET, 4),
        ] {
            assert_eq!(
                ask(&mut eager, &f),
                ask(&mut lazy, &f),
                "laid out {laid_out}"
            );
        }
        let o = ask(&mut lazy, &outline(s)).1;
        for want in [
            "region \"Section title\"",
            "heading \"Main head\"",
            "form \"Find\"",
            "navigation \"Top\"",
        ] {
            assert!(o.contains(want), "{want} in\n{o}");
        }
        assert!(!whole(&lazy), "the page outline leaves it deferred");
    }
}

#[test]
fn every_other_read_makes_it_whole_once_and_answers_as_a_whole_tree() {
    for laid_out in [true, false] {
        for i in 0..reads(1).len() {
            let (mut eager, s) = page(false, laid_out);
            let (mut lazy, t) = page(true, laid_out);
            assert_eq!(s, t);
            let f = reads(s).swap_remove(i);
            let o = [ask(&mut eager, &outline(s)), ask(&mut lazy, &outline(s))];
            assert_eq!(o[0], o[1]);
            let pg = lazy.agents.page_gen();
            assert_eq!(
                ask(&mut eager, &f),
                ask(&mut lazy, &f),
                "read {i}, laid out {laid_out}"
            );
            assert!(whole(&lazy), "read {i}");
            assert_eq!(lazy.agents.page_gen(), pg, "completed, not built again");
            for g in reads(s) {
                assert_eq!(ask(&mut eager, &g), ask(&mut lazy, &g), "after read {i}");
            }
            assert_eq!(lazy.agents.page_gen(), pg);
        }
    }
}

/// Changes after the tree is whole come out of the diff as they do of
/// a tree built whole: the elements named by one ID in the same order.
#[test]
fn changes_after_completion_are_diffed_as_by_a_whole_tree() {
    let mut pages = [page(false, true), page(true, true)];
    let mut replies = Vec::new();
    for (p, s) in &mut pages {
        let seq = ask(p, &outline(*s)).0;
        ask(p, &read(*s, 3, 0).str(tag::TEXT, "pre"));
        let sec = p.by_id("sec");
        let text = p.dom.first_child(sec).unwrap();
        p.dom.set_text(text, DomString::from_wtf8(b"Renamed"));
        let q = p.by_id("q");
        p.dom.set_attribute(q, atom!("disabled"), b"");
        let diff = Frame::new(kind::DIFF)
            .num(tag::SESSION, *s as u64)
            .num(tag::SINCE, seq);
        replies.push((ask(p, &diff), ask(p, &read(*s, 1, 0).num(tag::HIDDEN, 1))));
    }
    assert_eq!(replies[0], replies[1]);
    let diff = &replies[1].0.1;
    assert!(
        diff.contains("Renamed") && diff.contains("disabled"),
        "{diff}"
    );
}

#[test]
fn geometry_after_the_outline_is_taken_in_as_by_a_whole_tree() {
    let mut pages = [page(false, false), page(true, false)];
    let mut replies = Vec::new();
    for (p, s) in &mut pages {
        let seq = ask(p, &outline(*s)).0;
        lay_out(p);
        let inputs = Inputs {
            styles: Some(&p.styles),
            geometry: p.geometry.as_ref(),
        };
        p.agents.sync(&mut p.dom, inputs);
        let diff = Frame::new(kind::DIFF)
            .num(tag::SESSION, *s as u64)
            .num(tag::SINCE, seq);
        replies.push((ask(p, &diff), ask(p, &read(*s, 1, 0)), ask(p, &outline(*s))));
    }
    assert_eq!(replies[0], replies[1]);
    // the link without area and its text are no longer shown
    assert!(
        replies[1].0.1.contains("2 changes\n- ["),
        "{}",
        replies[1].0.1
    );
}

/// The DOM changed under a deferred tree without a request between:
/// what it was built from is gone, so it is read again whole under a
/// new page generation, as when the DOM's change record overflows.
#[test]
fn a_dom_changed_under_a_deferred_tree_reads_it_again_whole() {
    let (mut p, s) = page(true, true);
    let o = ask(&mut p, &outline(s)).1;
    let head = common::ref_on(&o, "heading \"Main head\"");
    let pg = p.agents.page_gen();
    let h1 = p.by_id("h1");
    let text = p.dom.first_child(h1).unwrap();
    p.dom.set_text(text, DomString::from_wtf8(b"Moved "));
    let after = ask(&mut p, &outline(s)).1;
    assert!(after.contains("heading \"Moved head\""), "{after}");
    assert_ne!(p.agents.page_gen(), pg);
    assert!(whole(&p));
    assert!(
        p.resolve(s, &head).is_err(),
        "{head} names the tree read before"
    );
    let mut q = Page::new(HTML);
    let h1 = q.by_id("h1");
    let text = q.dom.first_child(h1).unwrap();
    q.dom.set_text(text, DomString::from_wtf8(b"Moved "));
    let inputs = Inputs {
        styles: Some(&q.styles),
        geometry: q.geometry.as_ref(),
    };
    let fresh = SemTree::build(&mut q.dom, inputs, p.agents.page_gen());
    let opts = ViewOpts {
        include_hidden: true,
        ..ViewOpts::default()
    };
    let built = fresh.snapshot(&q.dom, inputs, opts).text;
    assert_eq!(ask(&mut p, &read(s, 1, 0).num(tag::HIDDEN, 1)).1, built);
}

#[test]
fn new_styles_under_a_deferred_tree_read_it_again_whole() {
    let (mut p, s) = page(true, true);
    ask(&mut p, &outline(s));
    let pg = p.agents.page_gen();
    p.relayout();
    ask(&mut p, &outline(s));
    assert!(whole(&p));
    assert_ne!(p.agents.page_gen(), pg);
}
