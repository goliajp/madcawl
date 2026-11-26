//! Script elements on JavaScriptCore, their sources handed in by the
//! test as an embedder would: the order of parser-blocking, `defer`,
//! `async` and script-inserted scripts, `document.currentScript`, the
//! `load` and `error` events, and the end of loading.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Deadline, Engine, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

struct Page {
    e: Jsc,
    h: DomHost<Jsc>,
    /// src as written, to request id
    asked: Vec<(String, u64)>,
}

impl Page {
    fn new(html: &str) -> Page {
        let mut e = Jsc::new();
        let mut h = DomHost::new(&mut e, Dom::new());
        h.install_window(&mut e);
        h.use_virtual_time();
        let mut p = Page { e, h, asked: Vec::new() };
        p.eval("globalThis.log = []; 0");
        p.h.parse_document(&mut p.e, html.as_bytes());
        p.take();
        p
    }

    fn take(&mut self) {
        for (id, src) in self.h.take_script_requests() {
            let mut b = Vec::new();
            src.write_wtf8(&mut b);
            self.asked.push((String::from_utf8(b).unwrap(), id));
        }
    }

    fn asked(&self) -> Vec<&str> {
        self.asked.iter().map(|(s, _)| s.as_str()).collect()
    }

    /// Hand in `src`'s source (`None`: the fetch failed) and run what
    /// that lets run.
    fn arrive(&mut self, src: &str, text: Option<&str>) {
        let i = self.asked.iter().position(|(s, _)| s == src).unwrap_or_else(|| panic!("{src} was not asked for"));
        let (_, id) = self.asked.remove(i);
        self.h.script_fetched(id, text.map(|t| DomString::from_wtf8(t.as_bytes())));
        self.run();
    }

    fn run(&mut self) {
        while self.h.run_tasks(&mut self.e, 100) != 0 {}
        self.take();
    }

    fn eval(&mut self, src: &str) -> String {
        let url = DomString::from_wtf8(b"scripts");
        let realm = self.h.realm;
        let r = self.e.evaluate(&mut self.h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
        let r = if Jsc::is_thrown(r.get()) {
            let _sentinel = r.into_raw();
            let x = self.e.take_exception().expect("an exception");
            let s = self.e.to_string(&mut self.h, x.get()).expect("a string");
            panic!("{src} threw {}", madcawl_bindings_text(&s));
        } else {
            r
        };
        let s = self.e.to_string(&mut self.h, r.get()).expect("a string");
        self.e.release(r);
        self.h.settle(&mut self.e);
        let mut b = Vec::new();
        s.write_wtf8(&mut b);
        String::from_utf8(b).unwrap()
    }

    fn log(&mut self) -> String {
        self.eval("log.join(' ')")
    }

    fn close(mut self) {
        assert_eq!(self.h.dom.audit().mismatches(), 0, "the counts audit");
        assert_eq!(self.h.reported, 0, "uncaught exceptions");
        self.h.close(&mut self.e);
    }
}

fn madcawl_bindings_text(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    String::from_utf8(b).unwrap()
}

fn src(name: &str) -> String {
    format!("log.push('{name}:' + (document.currentScript && document.currentScript.getAttribute('src')));")
}

const ORDER: &str = "<!doctype html><script>
document.addEventListener('DOMContentLoaded', () => log.push('dcl'));
addEventListener('load', () => log.push('load'));
document.addEventListener('load', e => log.push('L:' + e.target.getAttribute('src')), true);
</script>
<script src=a.js></script><script>log.push('inline:' + document.currentScript.localName)</script>
<script defer src=d1.js></script><script defer src=d2.js></script><script async src=s.js></script>
<script async defer src=s2.js></script><p id=end>end</p>";

#[test]
fn scripts_run_in_the_order_of_their_kinds() {
    let mut p = Page::new(ORDER);
    assert_eq!(p.asked(), ["a.js"], "the parser waits for a.js before it reads on");
    assert_eq!(p.eval("String(document.getElementById('end'))"), "null");
    p.arrive("a.js", Some(&src("a")));
    assert_eq!(p.asked(), ["d1.js", "d2.js", "s.js", "s2.js"]);
    p.arrive("d2.js", Some(&src("d2")));
    p.arrive("s2.js", Some(&src("s2")));
    assert_eq!(p.log(), "a:a.js L:a.js inline:script s2:s2.js L:s2.js", "d2 waits for d1; s2 runs at once");
    p.arrive("d1.js", Some(&src("d1")));
    assert!(!p.h.is_loaded(), "s.js is still awaited");
    p.arrive("s.js", Some(&src("s")));
    assert!(p.h.is_loaded());
    let want = "a:a.js L:a.js inline:script s2:s2.js L:s2.js d1:d1.js L:d1.js d2:d2.js L:d2.js dcl s:s.js L:s.js load";
    assert_eq!(p.log(), want);
    assert_eq!(p.eval("String(document.currentScript)"), "null");
    p.close();
}

#[test]
fn a_failed_fetch_fires_error_and_the_parser_reads_on() {
    let html = "<script>document.addEventListener('error', e => log.push('E:' + e.target.getAttribute('src') + ':' + e.bubbles + e.cancelable), true)</script>
<script src=gone.js></script><script>log.push('after')</script><script src=''></script><script async src=gone2.js></script>";
    let mut p = Page::new(html);
    p.arrive("gone.js", None);
    assert_eq!(p.asked(), ["gone2.js"], "an empty src is not fetched");
    p.arrive("gone2.js", None);
    assert_eq!(p.log(), "E:gone.js:falsefalse after E::falsefalse E:gone2.js:falsefalse");
    assert!(p.h.is_loaded());
    p.close();
}

#[test]
fn scripts_inserted_by_script_run_async_unless_told_otherwise() {
    let mut p = Page::new("<p>x</p>");
    p.run();
    let setup = "function add(name, ordered) { var s = document.createElement('script'); s.src = name;
if (ordered) s.async = false; s.onload = () => log.push('L:' + name); document.querySelector('body').appendChild(s); return s; }
var i = document.createElement('script'); i.textContent = \"log.push('inline:' + (document.currentScript === i))\";
document.querySelector('body').appendChild(i);
log.push(String(document.createElement('script').async) + i.async);
add('o1.js', true); add('o2.js', true); add('f1.js', false);
var late = document.createElement('script'); document.querySelector('body').appendChild(late); late.src = 'late.js'; 'ok'";
    assert_eq!(p.eval(setup), "ok");
    p.take();
    assert_eq!(p.asked(), ["o1.js", "o2.js", "f1.js", "late.js"]);
    p.arrive("o2.js", Some("log.push('o2')"));
    p.arrive("f1.js", Some("log.push('f1')"));
    p.arrive("o1.js", Some("log.push('o1')"));
    p.arrive("late.js", Some("log.push('late')"));
    assert_eq!(p.log(), "inline:true truetrue f1 L:f1.js o1 L:o1.js o2 L:o2.js late");
    p.close();
}

#[test]
fn a_script_written_by_a_blocking_script_runs_after_it_and_before_what_follows() {
    let html = "<script src=w.js></script><script>log.push('next')</script>";
    let mut p = Page::new(html);
    p.arrive("w.js", Some("document.write('<script src=x.js><\\/script>'); log.push('after-write')"));
    assert_eq!(p.asked(), ["x.js"]);
    assert_eq!(p.log(), "after-write");
    p.arrive("x.js", Some("log.push('x'); document.write('<i>written</i>')"));
    assert_eq!(p.log(), "after-write x next");
    assert_eq!(p.eval("document.querySelector('i').textContent + ' ' + document.querySelector('i').nextSibling.localName"), "written script");
    p.close();
}

#[test]
fn a_script_removed_before_its_source_arrives_still_runs_and_keeps_its_listener() {
    let mut p = Page::new("<p>x</p>");
    p.run();
    let setup = "(function () { var s = document.createElement('script'); s.src = 'r.js'; s.tag = 'kept';
s.addEventListener('load', () => log.push('L:' + s.tag)); document.querySelector('body').appendChild(s); s.remove(); })(); 'ok'";
    assert_eq!(p.eval(setup), "ok");
    p.take();
    for _ in 0..3 {
        p.e.collect_now();
        while p.h.collect(&mut p.e, Deadline::Work(u32::MAX)) != Step::Done {}
    }
    p.arrive("r.js", Some("log.push('r:' + document.currentScript.isConnected)"));
    assert_eq!(p.log(), "r:false L:kept");
    p.close();
}

#[test]
fn a_script_that_is_not_classic_does_not_run_and_one_that_ran_does_not_run_again() {
    let html = "<script type=module>log.push('module')</script><script type=text/plain>log.push('plain')</script>
<script language=javascript>log.push('lang')</script><script type=' TEXT/JavaScript '>log.push('typed')</script>
<script id=once>log.push('once')</script>";
    let mut p = Page::new(html);
    assert_eq!(p.eval("var o = document.getElementById('once'); document.querySelector('body').appendChild(o); log.join(' ')"), "lang typed once");
    p.close();
}

#[test]
fn closing_a_page_with_scripts_still_awaited_gives_back_what_they_hold() {
    let html = "<script src=never.js></script><p>x</p>";
    let mut p = Page::new(html);
    p.eval("var s = document.createElement('script'); s.src = 'also.js'; s.onload = () => 0; document.querySelector('head').appendChild(s); 0");
    p.take();
    assert_eq!(p.asked(), ["never.js", "also.js"]);
    assert_eq!(p.h.scripts_pending(), 2);
    p.close();
}

#[test]
fn a_tree_removed_while_the_parser_waits_is_freed_once_parsing_is_done() {
    let html = "<script>setTimeout(() => document.getElementById('a').remove(), 0)</script>\
<div id=a><b>x<script src=b.js></script>y</b><i>z</i></div><p id=z>after</p>";
    let mut p = Page::new(html);
    p.run();
    let collect = |p: &mut Page| {
        for _ in 0..3 {
            p.e.collect_now();
            while p.h.collect(&mut p.e, Deadline::Work(u32::MAX)) != Step::Done {}
        }
    };
    collect(&mut p);
    assert_eq!(p.eval("String(document.getElementById('a'))"), "null", "the timer ran while the parser waited");
    p.arrive("b.js", Some("log.push('b:' + document.currentScript.parentNode.localName)"));
    assert_eq!(p.log(), "b:b");
    assert_eq!(p.eval("document.getElementById('z').parentNode.localName"), "body");
    let before = p.h.dom.node_count();
    collect(&mut p);
    assert!(p.h.dom.node_count() + 6 <= before, "{} then {}", before, p.h.dom.node_count());
    p.close();
}

#[test]
fn the_dom_kept_after_closing_has_what_script_built_and_no_wrapper_counts() {
    let html = "<p id=a>x</p><script>var kept = []; for (let i = 0; i < 50; i++) kept.push(document.createElement('div'));
var b = document.createElement('b'); b.textContent = 'built'; document.getElementById('a').appendChild(b);</script>";
    let mut p = Page::new(html);
    p.run();
    let Page { mut e, h, .. } = p;
    let dom = h.into_dom(&mut e);
    drop(e);
    assert_eq!(dom.audit().mismatches(), 0);
    assert!(dom.wrapped().is_empty());
    let mut out = Vec::new();
    dom.write_dat(dom.document(), &mut out);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("\"built\""), "{out}");
    assert!(dom.node_count() < 20, "the 50 divs script kept are freed with the realm: {}", dom.node_count());
}
