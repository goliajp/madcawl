//! Acting on pages through madcawl-mcp as an agent would: navigation and
//! history, clicks by hit testing with their checks, controls, form
//! submission, uploads and downloads, and the deterministic wait.

mod act_site;

use act_site::{act, host, page_of, path_arg, ref_on, site};

#[test]
fn links_navigate_and_history_goes_back_and_forward() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let opened = m.ok("open", &path_arg(&format!("{origin}/")));
    let p = page_of(&opened);
    assert!(opened.contains("Site"), "{opened}");
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let link = ref_on(&snap, "Form");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{link}""#));
    assert!(!err, "{t}");
    assert!(t.contains(" · stable · dispatch "), "{t}");
    assert!(t.contains(&format!("the page is now {origin}/form")), "{t}");
    let (t, err) = act(&mut m, &p, r#""action":"back""#);
    assert!(!err && t.contains(&format!("the page is now {origin}/\n")), "{t}");
    let (t, err) = act(&mut m, &p, r#""action":"forward""#);
    assert!(!err && t.contains("/form"), "{t}");
    let (t, err) = act(&mut m, &p, r#""action":"forward""#);
    assert!(err && t.contains("no history entry"), "{t}");
    let (t, _) = act(&mut m, &p, r#""action":"reload""#);
    assert!(t.contains("/form"), "{t}");
    let (stale, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{link}""#));
    assert!(err && stale.contains("stale"), "{stale}");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"navigate","value":"{origin}/missing""#));
    assert!(!err && t.contains("status 404"), "{t}");
    let log = m.ok("requests", &format!(r#"{{"page":"{p}"}}"#));
    assert!(log.contains(&format!("document GET {origin}/form 200")), "{log}");
    assert!(log.contains(&format!("error: http 404: GET {origin}/missing")), "{log}");
}

#[test]
fn a_click_checks_its_target_first() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/form"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}","include_hidden":true}}"#));
    let (t, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{}""#, ref_on(&snap, "Frozen")));
    assert!(err && t.contains("check failed: disabled"), "{t}");
    let hidden = snap.lines().find(|l| l.contains("link [e") && l.ends_with(" hidden")).unwrap();
    let hidden = hidden.split('[').nth(1).unwrap().split(']').next().unwrap();
    let (t, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{hidden}""#));
    assert!(err && t.contains("check failed: not visible"), "{t}");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{}""#, ref_on(&snap, "Under")));
    assert!(err && t.contains("check failed: obscured by [e"), "{t}");
    let cover = t.split("obscured by [").nth(1).unwrap().split(']').next().unwrap();
    let found = m.ok("find", &format!(r#"{{"page":"{p}","text":"Cover"}}"#));
    assert!(found.contains(&format!("[{cover}]")), "the cover's own ref: {found}");

    let long = page_of(&m.ok("open", &path_arg(&format!("{origin}/long"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{long}"}}"#));
    let far = ref_on(&snap, "Far link");
    let view = m.ok("find", &format!(r#"{{"page":"{long}","text":"Far link","scope":"viewport"}}"#));
    assert!(!view.contains(&format!("[{far}]")), "{view}");
    let (t, err) = act(&mut m, &long, &format!(r#""action":"click","ref":"{far}""#));
    assert!(!err && t.contains(&format!("the page is now {origin}/")), "{t}");
}

#[test]
fn controls_take_input_and_the_diff_shows_it() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/form"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let title = ref_on(&snap, "Title");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"type","ref":"{title}","value":" two""#));
    assert!(!err && t.contains("typed 4 characters"), "{t}");
    assert!(t.contains(&format!("~ textbox \"Title\" [{title}]")) && t.contains("draft two"), "{t}");
    let (t, _) = act(&mut m, &p, r#""action":"press","value":"Backspace""#);
    assert!(t.contains("draft tw\""), "{t}");
    let notify = ref_on(&snap, "Notify me");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"click","ref":"{notify}""#));
    assert!(!err && t.contains("checked"), "{t}");
    let large = ref_on(&snap, "Large");
    let (t, _) = act(&mut m, &p, &format!(r#""action":"check","ref":"{large}""#));
    assert!(t.contains("radio is now checked"), "{t}");
    let after = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    assert!(ref_line(&after, "Small").contains("radio") && !ref_line(&after, "Small").contains("checked"), "{after}");
    let lang = ref_on(&snap, "Lang");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"select","ref":"{lang}","value":"py""#));
    assert!(!err && t.contains("selected \"Python\""), "{t}");
    let (t, err) = act(&mut m, &p, &format!(r#""action":"select","ref":"{lang}","value":"Go""#));
    assert!(err && t.contains("no option"), "{t}");
}

fn ref_line<'a>(reply: &'a str, text: &str) -> &'a str {
    reply.lines().find(|l| l.contains(text)).unwrap()
}

#[test]
fn a_form_is_filled_submitted_with_a_file_and_a_download_is_saved() {
    let origin = site();
    let f = host(&origin);
    let up = f.dir.join("up").join("report.txt");
    std::fs::write(&up, "uploaded bytes").unwrap();
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/form"))));
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let body = ref_on(&snap, "Body");
    let forms = m.ok("extract", &format!(r#"{{"page":"{p}","what":"forms"}}"#));
    let file = forms.lines().find(|l| l.contains(r#""type":"file""#)).unwrap_or_else(|| panic!("{forms}"));
    let file = file.split(r#""ref":""#).nth(1).unwrap().split('"').next().unwrap().to_owned();
    let (t, err) = act(&mut m, &p, &format!(r#""action":"upload","ref":"{file}","value":"/etc/hosts""#));
    assert!(err && t.contains("approved for uploads"), "{t}");
    let fields = format!(r#""fields":{{"{body}":"hello\nworld","{file}":"{}"}}"#, up.display());
    let (t, err) = act(&mut m, &p, &format!(r#""action":"fill_form",{fields},"submit":true"#));
    assert!(!err && t.contains("filled 2 fields") && t.contains(&format!("the page is now {origin}/echo")), "{t}");
    let t = m.ok("read", &format!(r#"{{"page":"{p}"}}"#));
    assert!(t.contains("name=\"title\"") && t.contains("draft"), "{t}");
    assert!(t.contains("filename=\"report.txt\"") && t.contains("uploaded bytes"), "{t}");
    assert!(t.contains("hello"), "{t}");

    let (t, err) = act(&mut m, &p, &format!(r#""action":"navigate","value":"{origin}/notes.txt""#));
    assert!(!err && t.contains("downloaded notes.txt (18 bytes)"), "{t}");
    let saved = t.split(" to ").nth(1).unwrap().split_whitespace().next().unwrap();
    assert!(saved.starts_with(&format!("{}/s1-", f.dir.join("downloads").display())), "{saved}");
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "line one\nline two\n");
    assert!(t.contains(&format!("the page is now {origin}/echo")) || t.contains("diff "), "{t}");
}

#[test]
fn a_wait_that_runs_out_says_what_is_missing_and_can_be_taken_up_again() {
    let origin = site();
    let f = host(&origin);
    let mut m = f.mcp();
    let p = page_of(&m.ok("open", &path_arg(&format!("{origin}/"))));
    let (t, err) = act(&mut m, &p, &format!(r#""action":"navigate","value":"{origin}/slow","timeout_ms":100"#));
    assert!(!err && t.contains("NOT stable"), "{t}");
    assert!(t.contains("missing: a navigation is in flight"), "{t}");
    assert!(t.contains(&format!("1 requests in flight: GET {origin}/slow (document")), "{t}");
    let (t, err) = m.call("wait", &format!(r#"{{"page":"{p}","until_text":"Slow page"}}"#));
    assert!(!err && t.contains(" · stable") && t.contains("the page is now"), "{t}");
    let (t, _) = m.call("wait", &format!(r#"{{"page":"{p}","until_text":"never here","until_ref":"e999"}}"#));
    assert!(t.contains("\"never here\" is not on the page") && t.contains("[e999] names no node"), "{t}");
}
