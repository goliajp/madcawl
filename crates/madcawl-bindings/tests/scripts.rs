//! Script elements on the stub engine, which runs no script: the order
//! sources are asked for, the parser waiting for a blocking one, the
//! end of loading, and what a page closed with scripts awaited gives
//! back.

mod support;

use madcawl_str::DomString;
use support::Js;

fn asked(js: &mut Js) -> Vec<(u64, String)> {
    js.h.take_script_requests()
        .into_iter()
        .map(|(id, s)| {
            let mut b = Vec::new();
            s.write_wtf8(&mut b);
            (id, String::from_utf8(b).unwrap())
        })
        .collect()
}

#[test]
fn the_parser_waits_for_a_blocking_source_and_the_window_loads_after_the_rest() {
    let mut js = Js::new();
    let html = "<script src=a.js></script><p id=p>x</p><script defer src=d.js></script><script async src=s.js></script>";
    js.h.parse_document(&mut js.e, html.as_bytes());
    let a = asked(&mut js);
    assert_eq!(a.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), ["a.js"]);
    assert_eq!(js.h.dom.descendants(js.h.dom.document()).count(), 3, "html, head, script: nothing past a.js");
    js.h.script_fetched(a[0].0, Some(DomString::from_wtf8(b"0")));
    js.h.run_tasks(&mut js.e, 100);
    let rest = asked(&mut js);
    assert_eq!(rest.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), ["d.js", "s.js"]);
    js.h.script_fetched(rest[0].0, None);
    js.h.run_tasks(&mut js.e, 100);
    assert!(!js.h.is_loaded(), "s.js is awaited");
    assert_eq!(js.h.scripts_pending(), 1);
    js.h.script_fetched(rest[1].0, Some(DomString::from_wtf8(b"0")));
    js.h.script_fetched(rest[1].0, None);
    js.h.run_tasks(&mut js.e, 100);
    assert!(js.h.is_loaded());
    assert_eq!(js.h.dom.audit().mismatches(), 0);
    js.h.close(&mut js.e);
}

#[test]
fn a_page_closed_while_its_parser_waits_gives_back_what_it_holds() {
    let mut js = Js::new();
    js.h.parse_document(&mut js.e, b"<script src=a.js></script><script async src=b.js></script>");
    assert_eq!(js.h.scripts_pending(), 1);
    js.h.close(&mut js.e);
}
