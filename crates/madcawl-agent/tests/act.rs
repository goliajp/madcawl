//! What actions need of a page: hit testing in paint order, the checks
//! of a target, what a click does without script, and control state
//! that views and diffs show while a password's value never comes out.

mod common;

use common::Page;
use madcawl_agent::activate::{Activation, activation, form_owner, is_editable, labeled_control, radio_group};
use madcawl_agent::{Check, pick_point, scroll_into_view};

const FORM: &str = r#"<!doctype html><style>body{margin:0}</style>
<form id=f action=/go><label id=l>Name <input id=name value=v></label>
<input id=pw type=password><textarea id=ta></textarea>
<label><input id=cb type=checkbox> Tick</label><input id=r1 type=radio name=g checked><input id=r2 type=radio name=g>
<select id=sel><option>a<option id=ob>b</select><button id=go><span id=inner>Go</span></button>
<button id=off disabled>Off</button><button id=reset type=reset>Reset</button></form>
<input id=outside form=f name=extra><a id=link href="/x"><b id=bold>Link</b></a>
<div style="position:relative;height:30px"><button id=under>Under</button>
<div id=cover style="position:absolute;left:0;top:0;width:300px;height:30px">Cover</div></div>
<p id=gone style="display:none"><a href=/y id=hidden>Hidden</a></p>
<div style="height:3000px"></div><button id=far>Far</button>"#;

#[test]
fn clicks_find_what_their_target_does() {
    let p = Page::new(FORM);
    let d = &p.dom;
    let id = |s: &str| p.by_id(s);
    assert_eq!(activation(d, id("bold")), Activation::Follow(id("link")));
    assert_eq!(activation(d, id("inner")), Activation::Submit { form: id("f"), submitter: id("go") });
    assert_eq!(activation(d, id("cb")), Activation::Toggle(id("cb")));
    assert_eq!(activation(d, id("r2")), Activation::Choose(id("r2")));
    assert_eq!(activation(d, id("reset")), Activation::Reset(id("f")));
    assert_eq!(activation(d, id("name")), Activation::Focus(id("name")));
    assert_eq!(activation(d, id("l")), Activation::Focus(id("name")), "a label forwards to its control");
    assert_eq!(activation(d, id("cover")), Activation::Nothing);
    assert_eq!(form_owner(d, id("outside")), Some(id("f")), "by the form attribute");
    assert_eq!(labeled_control(d, id("l")), Some(id("name")));
    assert_eq!(radio_group(d, id("r1")), [id("r1"), id("r2")]);
    assert!(is_editable(d, id("pw")) && is_editable(d, id("ta")) && !is_editable(d, id("cb")));
}

#[test]
fn hit_testing_follows_paint_order_and_the_checks_say_what_failed() {
    let mut p = Page::new(FORM);
    let s = p.attach();
    p.snapshot(s);
    let geo = p.geometry.as_ref().unwrap();
    let under = p.by_id("under");
    let at = geo.place(under).unwrap().rect;
    let hit = geo.hit(at.x + 2.0, at.y + 2.0).unwrap();
    assert!(p.dom.is_inclusive_ancestor_of(p.by_id("cover"), hit), "the positioned cover paints over the button");
    let check = |p: &Page, n| p.agents.check_target(&p.dom, p.geometry.as_ref().unwrap(), n, true);
    assert!(matches!(check(&p, under), Err(Check::Obscured(Some(_)))));
    assert_eq!(check(&p, p.by_id("off")), Err(Check::Disabled));
    assert_eq!(check(&p, p.by_id("hidden")), Err(Check::NotVisible));
    assert!(check(&p, p.by_id("go")).is_ok());
    let far = p.by_id("far");
    assert!(pick_point(p.geometry.as_ref().unwrap(), &p.dom, far).is_err(), "below the viewport");
    assert!(scroll_into_view(p.geometry.as_mut().unwrap(), far));
    assert!(p.geometry.as_ref().unwrap().viewport.y > 2000.0);
    assert!(check(&p, far).is_ok());
    assert!(!scroll_into_view(p.geometry.as_mut().unwrap(), far), "already in view");
}

#[test]
fn control_state_shows_in_views_and_diffs_but_a_password_never_does() {
    let mut p = Page::new(FORM);
    let s = p.attach();
    let before = p.snapshot(s);
    let seq = p.agents.seq();
    let (name, pw, cb, sel, ob) = (p.by_id("name"), p.by_id("pw"), p.by_id("cb"), p.by_id("sel"), p.by_id("ob"));
    let a = &mut p.agents;
    a.set_control(s, &p.dom, name, |c| c.value = Some("typed name".into())).unwrap();
    a.set_control(s, &p.dom, pw, |c| c.value = Some("hunter2-secret".into())).unwrap();
    a.set_control(s, &p.dom, cb, |c| c.checked = Some(true)).unwrap();
    for o in madcawl_agent::activate::options(&p.dom, sel) {
        a.set_control(s, &p.dom, o, |c| c.selected = Some(o == ob)).unwrap();
    }
    a.set_control(s, &p.dom, sel, |_| {}).unwrap();
    assert_eq!(a.controls().unwrap().value(pw), Some("hunter2-secret"), "kept for the submission");
    let (_, d) = p.diff(s, seq).unwrap();
    assert!(d.text.contains("typed name") && d.text.contains("checkbox \"Tick\"") && d.text.contains("checked"), "{}", d.text);
    assert!(d.text.contains("value=\"b\""), "{}", d.text);
    let after = p.snapshot(s);
    assert_ne!(before, after);
    for text in [d.text, after] {
        assert!(!text.contains("hunter2"), "{text}");
    }
    let s2 = p.attach();
    let found = p.view(s2, madcawl_agent::ViewKind::Find(madcawl_agent::Query { text: Some("hunter2".into()), role: None, limit: 5 }), Default::default());
    assert!(found.1.text.starts_with("0 found"), "{}", found.1.text);
}

#[test]
fn a_reference_from_one_page_is_stale_on_another_of_the_same_host() {
    static GENS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let html = r#"<!doctype html><label>User <input id=u></label><label>Password <input id=p type=password></label>"#;
    let (mut one, mut two) = (Page::new(html), Page::new(html));
    one.agents = madcawl_agent::Agents::sharing_generations(&GENS);
    two.agents = madcawl_agent::Agents::sharing_generations(&GENS);
    let (s1, s2) = (one.attach(), two.attach());
    let user = common::ref_on(&one.snapshot(s1), "textbox \"User");
    assert_eq!(one.resolve(s1, &user).unwrap(), one.by_id("u"));
    two.snapshot(s2);
    let e = two.resolve(s2, &user).unwrap_err();
    assert_eq!(e, madcawl_agent::AgentError::Ref(madcawl_agent::RefError::Stale), "page one's {user} on page two");
}

#[test]
fn a_control_after_its_form_was_closed_belongs_to_the_form_its_submission_reads() {
    let p = Page::new(
        "<!doctype html><div><form id=a><button id=go>Go</button></div><table><tr><td><input id=pw type=password name=p>\
         <input id=r1 type=radio name=g></table><input id=r2 type=radio name=g>",
    );
    let (d, f, pw) = (&p.dom, p.by_id("a"), p.by_id("pw"));
    assert!(!d.is_inclusive_ancestor_of(f, pw), "the parser closed the form with the div but keeps it as the owner");
    assert_eq!(form_owner(d, pw), Some(f));
    assert_eq!(madcawl_agent::activate::form_controls(d, f), [p.by_id("go"), pw, p.by_id("r1"), p.by_id("r2")]);
    assert_eq!(radio_group(d, p.by_id("r1")), [p.by_id("r1"), p.by_id("r2")], "one group: one form owner");
}

#[test]
fn many_controls_naming_a_missing_form_take_one_walk_of_the_document() {
    let mut html = String::from("<!doctype html><form id=f><input id=a type=radio name=a></form>");
    for _ in 0..20_000 {
        html.push_str("<input form=nope>");
    }
    let p = Page::new(&html);
    let t = std::time::Instant::now();
    let f = p.by_id("f");
    assert_eq!(madcawl_agent::activate::form_controls(&p.dom, f), [p.by_id("a")]);
    assert_eq!(radio_group(&p.dom, p.by_id("a")), [p.by_id("a")]);
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
}

#[test]
fn an_invisible_box_over_a_target_does_not_obscure_it() {
    let html = r#"<!doctype html><style>body{margin:0}</style><div style="position:relative;height:30px">
<button id=b>Press</button><div id=ghost style="position:absolute;left:0;top:0;width:300px;height:30px;visibility:hidden">Ghost</div></div>"#;
    let mut p = Page::new(html);
    let s = p.attach();
    p.snapshot(s);
    let b = p.by_id("b");
    let geo = p.geometry.as_ref().unwrap();
    let at = geo.place(b).unwrap().rect;
    let hit = geo.hit(at.x + 2.0, at.y + 2.0).unwrap();
    assert!(p.dom.is_inclusive_ancestor_of(b, hit), "the hidden box takes no hit");
    assert!(p.agents.check_target(&p.dom, geo, b, true).is_ok());
}
