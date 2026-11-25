//! What an agent must not be able to do with actions: touch another
//! session's page, find a password it typed anywhere the host writes,
//! submit a password to an origin the user did not approve (on the first
//! hop or after a redirect), use one page's reference on another, have a
//! page from the network open a local file, or learn which paths exist.

mod act_site;

use act_site::{Fixture, LOCALHOST_POSTS, act, host, page_of, path_arg, ref_on, site};
use std::sync::atomic::Ordering;

#[test]
fn another_sessions_page_is_refused_and_the_refusal_audited() {
    let origin = site();
    let f = host(&origin);
    let (mut a, mut b) = (f.mcp(), f.mcp());
    let p = page_of(&a.ok("open", &path_arg(&format!("{origin}/"))));
    let snap = a.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let link = ref_on(&snap, "Form");
    let (t, err) = act(&mut b, &p, &format!(r#""action":"click","ref":"{link}""#));
    assert!(err && t.contains("may not touch that page"), "{t}");
    for tool in ["snapshot", "requests", "wait", "close"] {
        let (t, err) = b.call(tool, &format!(r#"{{"page":"{p}"}}"#));
        assert!(err && t.contains("may not touch"), "{tool}: {t}");
    }
    assert!(a.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#)).contains("Form"), "the page is still a's");
    let audit = f.audit();
    for action in ["refused act:click", "refused snapshot", "refused requests", "refused wait", "refused close"] {
        assert!(audit.lines().any(|l| l.contains(&format!("\t{action}\t")) && l.ends_with("error 21")), "{action}: {audit}");
    }
}

#[test]
fn a_typed_password_is_found_nowhere() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/login"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let (user, pass) = (ref_on(&snap, "User"), ref_on(&snap, "Password"));
    let seq: u64 = snap.split(" · seq ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
    let (typed, err) = act(&mut m, &p, &format!(r#""action":"type","ref":"{pass}","value":"s3cret""#));
    assert!(!err && typed.contains("its value is never shown"), "{typed}");
    let mut seen = vec![typed];
    for (tool, args) in [
        ("snapshot", String::new()),
        ("read", String::new()),
        ("find", r#","text":"s3cret""#.into()),
        ("find", r#","role":"textbox""#.into()),
        ("extract", r#","what":"forms""#.into()),
        ("diff", format!(r#","since":{seq}"#)),
    ] {
        let t = m.ok(tool, &format!(r#"{{"page":"{p}"{args}}}"#));
        seen.push(t);
    }
    assert!(seen[3].contains("\n0 found"), "{}", seen[3]);
    let fields = format!(r#""fields":{{"{user}":"alice","{pass}":"s3cret"}},"submit":true"#);
    let (t, err) = act(&mut m, &p, &format!(r#""action":"fill_form",{fields}"#));
    assert!(!err && t.contains("Welcome alice"), "{t}");
    seen.push(t);
    seen.push(m.ok("requests", &format!(r#"{{"page":"{p}"}}"#)));
    drop(m);
    seen.push(f.audit());
    for s in &seen {
        assert!(!s.contains("s3cret"), "{s}");
    }
}

#[test]
fn a_password_form_goes_only_to_an_approved_origin() {
    let origin = site();
    let mut f = Fixture::new();
    f.config.net = true;
    f.start();
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/login"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let pass = ref_on(&snap, "Password");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"fill_form","fields":{{"{pass}":"x"}},"submit":true"#));
    assert!(err && t.contains(&format!("--approve {origin}")), "{t}");
    let file = ref_on(&m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#)), "User");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"upload","ref":"{file}","value":"/etc/hosts""#));
    assert!(err && t.contains("not a file input"), "{t}");
    drop(m);
    assert!(f.audit().lines().any(|l| l.contains("act:fill_form") && l.ends_with("error 22")), "{}", f.audit());
}

#[test]
fn a_password_in_a_get_form_is_masked_wherever_the_page_url_goes() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/gate"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let (user, pin) = (ref_on(&snap, "User"), ref_on(&snap, "PIN"));
    let fields = format!(r#""fields":{{"{user}":"al","{pin}":"s3cret"}},"submit":true"#);
    let (t, err) = act(&mut m, &p, &format!(r#""action":"fill_form",{fields}"#));
    assert!(!err && t.contains("Gate open") && t.contains("the page is now") && t.contains("pin=••••••"), "{t}");
    let mut seen = vec![t];
    seen.push(m.ok("requests", &format!(r#"{{"page":"{p}"}}"#)));
    seen.push(m.ok("pages", "{}"));
    let (t, err) = act(&mut m, &p, r#""action":"reload""#);
    assert!(err && t.contains("re-submit the form"), "reloading would send the mask as the PIN: {t}");
    seen.push(t);
    assert!(seen[1].contains("stylesheet GET") && seen[2].contains("/gated?user=al&pin="), "{seen:?}");
    let (t, err) = act(&mut m, &p, r#""action":"back""#);
    assert!(!err && t.contains("Gate"), "{t}");
    let (t, err) = act(&mut m, &p, r#""action":"forward""#);
    assert!(err && t.contains("re-submit the form"), "{t}");
    seen.push(t);
    drop(m);
    seen.push(f.audit());
    for s in &seen {
        assert!(!s.contains("s3cret"), "{s}");
    }
}

#[test]
fn a_password_field_the_parser_left_outside_its_form_still_needs_approval() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/moved"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let (secret, go) = (ref_on(&snap, "Secret\""), ref_on(&snap, "Go"));
    let fields = format!(r#""fields":{{"{secret}":"s3cret"}},"submit":true,"ref":"{go}""#);
    let (t, err) = act(&mut m, &p, &format!(r#""action":"fill_form",{fields}"#));
    assert!(err && t.contains("--approve http://evil"), "{t}");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{go}""#));
    assert!(err && t.contains("--approve http://evil"), "{t}");
    drop(m);
    assert!(!f.audit().contains("s3cret"), "{}", f.audit());
}

#[test]
fn a_password_field_naming_an_id_that_is_not_a_forms_is_in_no_form() {
    let origin = site();
    let mut f = Fixture::new();
    f.config.net = true;
    f.start();
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/notform"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let (user, secret, send) = (ref_on(&snap, "User"), ref_on(&snap, "Secret\""), ref_on(&snap, "Send"));
    let fields = format!(r#""fields":{{"{user}":"al","{secret}":"s3cret"}},"submit":true,"ref":"{send}""#);
    let (t, err) = act(&mut m, &p, &format!(r#""action":"fill_form",{fields}"#));
    assert!(!err && t.contains("/echo?user=al (status 200)"), "the form owns no password, so needs no approval: {t}");
    assert!(!t.contains("s3cret"), "{t}");
}

#[test]
fn a_redirect_does_not_carry_a_password_form_to_an_origin_not_approved() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/hop"))));
    let pass = ref_on(&m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#)), "Password");
    let before = LOCALHOST_POSTS.load(Ordering::SeqCst);
    let (t, _) = act(&mut m, &p, &format!(r#""action":"fill_form","fields":{{"{pass}":"s3cret"}},"submit":true"#));
    assert!(t.contains("the load failed") && t.contains("not approved"), "{t}");
    assert_eq!(LOCALHOST_POSTS.load(Ordering::SeqCst), before, "the body went on to localhost");
}

#[test]
fn a_reference_from_another_page_is_stale() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let one = page_of(&m.ok("open", &path_arg(&format!("{origin}/login"))));
    let two = page_of(&m.ok("open", &path_arg(&format!("{origin}/login"))));
    let user = ref_on(&m.ok("snapshot", &format!(r#"{{"page":"{one}"}}"#)), "User");
    m.ok("snapshot", &format!(r#"{{"page":"{two}"}}"#));
    let (t, err) = act(&mut m, &two, &format!(r#""action":"type","ref":"{user}","value":"x""#));
    assert!(err && t.contains("stale"), "{one}'s {user} on {two}: {t}");
    let (t, err) = act(&mut m, &one, &format!(r#""action":"type","ref":"{user}","value":"x""#));
    assert!(!err && t.contains("typed 1 characters"), "{t}");
}

#[test]
fn a_page_from_the_network_cannot_lead_to_a_local_file() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let local = f.page();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/files?p={local}"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let (t, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{}""#, ref_on(&snap, "Local")));
    assert!(err && t.contains("may not lead to a local file"), "{t}");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{}""#, ref_on(&snap, "Go local")));
    assert!(err && t.contains("Scheme(\"file\")"), "a form submits only to http and https: {t}");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"navigate","value":"file://{local}""#));
    assert!(!err && t.contains("Vec in std"), "the agent's own navigation: {t}");
}

#[test]
fn a_missing_upload_path_is_refused_as_one_outside_the_approved_directories() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/form"))));
    let forms = m.ok("extract", &format!(r#"{{"page":"{p}","what":"forms"}}"#));
    let file = forms.lines().find(|l| l.contains(r#""type":"file""#)).unwrap();
    let file = file.split(r#""ref":""#).nth(1).unwrap().split('"').next().unwrap().to_owned();
    let refusal = |m: &mut act_site::Mcp, path: &str| act(m, &p, &format!(r#""action":"upload","ref":"{file}","value":"{path}""#));
    let (outside, missing) = (refusal(&mut m, "/etc/hosts"), refusal(&mut m, "/no/such/dir/x.txt"));
    assert!(outside.1 && missing.1, "{outside:?} {missing:?}");
    let line = |t: &str| t.lines().next().unwrap_or("").to_owned();
    assert_eq!(line(&outside.0), line(&missing.0));
    assert!(missing.0.contains("approved for uploads"), "{}", missing.0);
}
