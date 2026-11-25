mod common;

use common::{Page, ref_on};
use madcawl_agent::{
    AgentError, Envelope, Frame, Inputs, Query, Role, Scope, ViewKind, ViewOpts, kind,
    read_request, tag,
};
use madcawl_str::{DomString, atom};

#[test]
fn a_budget_folds_long_lists_and_the_reference_expands_them() {
    let mut items = String::new();
    for i in 0..60 {
        items.push_str(&format!("<li><a href=#{i}>Item number {i}</a></li>"));
    }
    let mut p = Page::new(&format!(
        "<h1>List</h1><ul id=u>{items}</ul><p>after the list</p>"
    ));
    let s = p.attach();
    let opts = ViewOpts {
        budget: Some(120),
        ..ViewOpts::default()
    };
    let (_, v) = p.view(s, ViewKind::Snapshot, opts);
    assert!(v.tokens <= 120, "{} tokens:\n{}", v.tokens, v.text);
    assert!(
        v.text.contains("Item number 2") && !v.text.contains("Item number 3\""),
        "{}",
        v.text
    );
    assert!(v.text.contains("… 57 more [e"), "{}", v.text);
    assert!(v.text.contains("after the list"), "{}", v.text);
    let list_ref = ref_on(&v.text, "… 57 more");
    let n = p.resolve(s, &list_ref).unwrap();
    let sub = ViewOpts {
        scope: Scope::Subtree(n),
        ..ViewOpts::default()
    };
    let (_, all) = p.view(s, ViewKind::Snapshot, sub);
    assert!(all.text.contains("Item number 59"), "{}", all.text);
}

#[test]
fn markdown_has_headings_lists_tables_code_and_links_by_reference() {
    let mut p = Page::new(
        r#"<h1>Guide</h1><p>Read <a href=/docs>the docs</a> first.</p>
        <ul><li>one</li><li>two</li></ul>
        <table><tr><th>Name</th><th>Since</th></tr><tr><td>Safari</td><td>17</td></tr></table>
        <pre><code class="language-rust">fn main() {}</code></pre>
        <button>Save</button>"#,
    );
    let s = p.attach();
    let md = p.view(s, ViewKind::Markdown, ViewOpts::default()).1.text;
    for want in [
        "# Guide\n",
        "Read [the docs](e",
        ") first.",
        "- one\n- two",
        "| Name | Since |\n| --- | --- |\n| Safari | 17 |",
        "```rust\nfn main() {}\n```",
        "[button: Save](e",
    ] {
        assert!(md.contains(want), "{want:?} not in\n{md}");
    }
}

#[test]
fn the_outline_lists_landmarks_with_counts_and_headings() {
    let mut p = Page::new(
        r#"<title>Site</title><header><a href=/>Home</a></header>
        <nav aria-label=Main><a href=/a>A</a><a href=/b>B</a></nav>
        <main><h1>Welcome</h1><h2>Part</h2><button>Go</button></main>"#,
    );
    let s = p.attach();
    let v = p.view(s, ViewKind::Outline, ViewOpts::default()).1;
    assert!(
        v.text.starts_with("document \"Site\" (4 interactive)"),
        "{}",
        v.text
    );
    assert!(
        v.text.contains("navigation \"Main\" [") && v.text.contains("(2 interactive)"),
        "{}",
        v.text
    );
    assert!(v.text.contains("    heading \"Welcome\" ["), "{}", v.text);
    assert!(v.tokens <= 500);
}

#[test]
fn find_returns_references_with_where_they_are() {
    let mut p =
        Page::new(r#"<nav aria-label=Top><a href=/x>Pricing</a></nav><p>See pricing below</p>"#);
    let s = p.attach();
    let q = Query {
        text: Some("pricing".into()),
        ..Query::default()
    };
    let v = p.view(s, ViewKind::Find(q), ViewOpts::default()).1.text;
    assert!(v.starts_with("3 found"), "{v}");
    assert!(
        v.contains("link \"Pricing\" [") && v.contains(" in navigation \"Top\" ["),
        "{v}"
    );
    let q = Query {
        role: Some(Role::Link),
        ..Query::default()
    };
    let v = p.view(s, ViewKind::Find(q), ViewOpts::default()).1.text;
    assert!(v.starts_with("1 found"), "{v}");
}

#[test]
fn a_diff_lists_what_changed_since_a_view() {
    let mut p = Page::new(r#"<button id=b>Save</button><p id=p>old</p><ul id=u><li>a</li></ul>"#);
    let s = p.attach();
    let (seq, snap) = p.view(s, ViewKind::Snapshot, ViewOpts::default());
    let b = p.by_id("b");
    p.dom.set_attribute(b, atom!("disabled"), b"");
    let t = p.dom.first_child(p.by_id("p")).unwrap();
    p.dom.set_text(t, DomString::from_wtf8(b"new"));
    let li = p.dom.new_element(b"li").unwrap();
    let lt = p.dom.new_text(DomString::from_wtf8(b"b")).unwrap();
    p.dom.insert_node(li, lt, None);
    let u = p.by_id("u");
    p.dom.insert_node(u, li, None);
    p.relayout();
    let (seq2, d) = p.diff(s, seq).unwrap();
    assert!(seq2 > seq);
    let button = ref_on(&snap.text, "button");
    assert!(
        d.text
            .contains(&format!("~ button \"Save\" [{button}] disabled")),
        "{}",
        d.text
    );
    assert!(d.text.contains("~ text \"new\""), "{}", d.text);
    assert!(d.text.contains("+ listitem ["), "{}", d.text);
    assert!(d.text.len() < snap.text.len() * 2);
    let (_, none) = p.diff(s, seq2).unwrap();
    assert!(
        none.text
            .starts_with(&format!("diff {seq2}..{seq2}: 0 changes")),
        "{}",
        none.text
    );
    p.dom.remove_node(b);
    let (_, gone) = p.diff(s, seq2).unwrap();
    assert!(
        gone.text.contains(&format!("- [{button}]")),
        "{}",
        gone.text
    );
}

#[test]
fn a_session_too_far_behind_is_told_to_take_a_snapshot() {
    let mut p = Page::new(r#"<p id=p>x</p>"#);
    let s = p.attach();
    let (seq, _) = p.view(s, ViewKind::Snapshot, ViewOpts::default());
    let t = p.dom.first_child(p.by_id("p")).unwrap();
    for i in 0..madcawl_agent::RING_CAP + 1 {
        p.dom
            .set_text(t, DomString::from_wtf8(format!("v{i}").as_bytes()));
        p.diff(s, u64::MAX).ok();
    }
    assert_eq!(p.diff(s, seq).unwrap_err(), AgentError::TooFarBehind);
}

#[test]
fn requests_come_in_as_frames_and_answers_go_out_as_frames() {
    let mut p = Page::new("<h1>Hi</h1><p>[e1] is not a reference</p>");
    let inputs = Inputs {
        styles: Some(&p.styles),
        geometry: p.geometry.as_ref(),
    };
    let attached = p
        .agents
        .handle(&mut p.dom, inputs, &Frame::new(kind::ATTACH))
        .unwrap();
    let session = attached.get_num(tag::SESSION).unwrap() as u32;
    let req = read_request(session, "markdown", "page", "");
    let mut wire = Vec::new();
    req.encode(&mut wire);
    let (back, _) = Frame::decode(&wire).unwrap();
    let res = p.agents.handle(&mut p.dom, inputs, &back).unwrap();
    assert_eq!(res.kind, kind::RESULT);
    let text = res.get_str(tag::TEXT).unwrap();
    assert!(text.starts_with("# Hi"), "{text}");
    let wrapped = Envelope::new(7).wrap("file:///x.html", text);
    assert!(wrapped.contains("\\[e1\\] is not a reference"), "{wrapped}");
    assert!(
        p.agents
            .handle(&mut p.dom, inputs, &Frame::new(999))
            .is_none()
    );
    let bad = read_request(session + 9, "snapshot", "page", "");
    let err = p.agents.handle(&mut p.dom, inputs, &bad).unwrap();
    assert_eq!(err.kind, kind::ERROR);
    let stale = read_request(session, "snapshot", "subtree", "e999");
    let err = p.agents.handle(&mut p.dom, inputs, &stale).unwrap();
    assert_eq!(err.get_num(tag::CODE), Some(3));
}

#[test]
fn scopes_cover_the_viewport_and_the_main_content() {
    let mut body = String::from("<nav><a href=/>Home</a></nav><main><h1>Top</h1>");
    for i in 0..80 {
        body.push_str(&format!("<p>paragraph {i}</p>"));
    }
    body.push_str("</main>");
    let mut p = Page::new(&body);
    let s = p.attach();
    let vp = ViewOpts {
        scope: Scope::Viewport,
        ..ViewOpts::default()
    };
    let v = p.view(s, ViewKind::Snapshot, vp).1.text;
    assert!(
        v.contains("paragraph 0") && !v.contains("paragraph 79"),
        "{v}"
    );
    let main = ViewOpts {
        scope: Scope::Main,
        ..ViewOpts::default()
    };
    let m = p.view(s, ViewKind::Markdown, main).1.text;
    assert!(m.starts_with("# Top") && !m.contains("Home"), "{m}");
}

#[test]
fn a_freed_slot_taken_by_a_new_node_gets_a_new_reference() {
    let mut p = Page::new("<div id=d><button id=b>Old</button></div>");
    let s = p.attach();
    let (seq, snap) = p.view(s, ViewKind::Snapshot, ViewOpts::default());
    let old = ref_on(&snap.text, "button \"Old\"");
    let b = p.by_id("b");
    p.dom.remove_node(b);
    assert!(p.dom.reclaim() > 0);
    // the freed slots are taken last freed first: the text's, then the button's
    let t = p.dom.new_text(DomString::from_wtf8(b"New")).unwrap();
    let nb = p.dom.new_element(b"button").unwrap();
    assert_eq!(nb, b, "the slot is used again");
    p.dom.insert_node(nb, t, None);
    let d = p.by_id("d");
    p.dom.insert_node(d, nb, None);
    p.relayout();
    let new = ref_on(&p.snapshot(s), "button \"New\"");
    assert_ne!(old, new);
    assert!(new.contains('x'), "{new}");
    assert!(p.resolve(s, &old).is_err());
    assert_eq!(p.resolve(s, &new).unwrap(), nb);
    let (_, diff) = p.diff(s, seq).unwrap();
    assert!(
        diff.text.contains(&format!("- [{old}]"))
            && diff.text.contains(&format!("+ button \"New\" [{new}]")),
        "{}",
        diff.text
    );
}
