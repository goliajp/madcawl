//! Document members a page's script reaches first: `cookie` over the
//! network stack's jar, read and written as script, and the `head` and
//! `body` elements, on JavaScriptCore.

use std::cell::RefCell;
use std::rc::Rc;

use madcawl_bindings::{CookieStore, DomHost};
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_net::Url;
use madcawl_net::cookie::{Api, CookieJar, JarLimits, SiteContext};
use madcawl_str::DomString;

const NOW: i64 = 1_800_000_000;

struct Jar(Rc<RefCell<CookieJar>>);

impl CookieStore for Jar {
    fn cookies(&mut self, url: &str) -> String {
        let list = self.0.borrow_mut().matching(&Url::parse(url).unwrap(), SiteContext::SameSite, Api::Script, NOW);
        madcawl_net::cookie::serialize(&list)
    }

    fn set_cookie(&mut self, url: &str, line: &str) {
        self.0.borrow_mut().set(&Url::parse(url).unwrap(), line, Api::Script, NOW);
    }
}

struct Page {
    e: Jsc,
    h: DomHost<Jsc>,
}

fn text(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    String::from_utf8(b).unwrap()
}

impl Page {
    /// A loaded document of `html` at `url`, with `jar` as its cookie
    /// store when given.
    fn at(url: &str, html: &str, jar: Option<&Rc<RefCell<CookieJar>>>) -> Page {
        let mut e = Jsc::new();
        let mut h = DomHost::new(&mut e, Dom::new());
        assert!(h.set_url(url));
        if let Some(j) = jar {
            h.set_cookie_store(Box::new(Jar(j.clone())));
        }
        h.install_window(&mut e);
        h.use_virtual_time();
        h.parse_document(&mut e, html.as_bytes());
        while h.run_tasks(&mut e, 100) != 0 {}
        assert!(h.is_loaded());
        Page { e, h }
    }

    fn eval(&mut self, src: &str) -> String {
        let url = DomString::from_wtf8(b"test");
        let realm = self.h.realm;
        let r = self.e.evaluate(&mut self.h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
        if Jsc::is_thrown(r.get()) {
            let _sentinel = r.into_raw();
            let x = self.e.take_exception().expect("an exception");
            let s = self.e.to_string(&mut self.h, x.get()).expect("a string");
            panic!("{src} threw {}", text(&s));
        }
        let s = self.e.to_string(&mut self.h, r.get()).expect("a string");
        self.e.release(r);
        self.h.settle(&mut self.e);
        text(&s)
    }

    fn throws(&mut self, src: &str) -> String {
        self.eval(&format!("(() => {{ try {{ {src}; return 'no exception' }} catch (x) {{ return x.name }} }})()"))
    }

    fn close(self) {
        let Page { mut e, h } = self;
        h.close(&mut e);
    }
}

fn jar() -> Rc<RefCell<CookieJar>> {
    Rc::new(RefCell::new(CookieJar::new(JarLimits::default())))
}

fn http_header(jar: &Rc<RefCell<CookieJar>>, url: &str) -> String {
    jar.borrow_mut().header(&Url::parse(url).unwrap(), SiteContext::SameSite, NOW).unwrap_or_default()
}

#[test]
fn script_reads_and_writes_the_jar_without_seeing_http_only_cookies() {
    let j = jar();
    let site = Url::parse("https://a.test/").unwrap();
    j.borrow_mut().set(&site, "sid=s1; HttpOnly; Path=/", Api::Http, NOW);
    j.borrow_mut().set(&site, "theme=dark; Path=/", Api::Http, NOW);
    let mut p = Page::at("https://a.test/app/page", "<!doctype html><p>x</p>", Some(&j));
    assert_eq!(p.eval("document.cookie"), "theme=dark");
    p.eval("document.cookie = 'js=1; Path=/'; document.cookie = 'sid=forged; Path=/'; document.cookie = 'here=2'");
    assert_eq!(p.eval("document.cookie"), "here=2; theme=dark; js=1", "longest path first, then oldest");
    assert_eq!(http_header(&j, "https://a.test/"), "sid=s1; theme=dark; js=1", "script's cookies go with requests");
    p.eval("document.cookie = 'here=; Max-Age=0'; document.cookie = null; document.cookie = 'big=' + 'x'.repeat(5000)");
    assert_eq!(p.eval("document.cookie"), "theme=dark; js=1", "a deleted, an empty and an oversized cookie");
    p.eval("document.cookie = 'u=\\uD800x'; 0");
    assert_eq!(p.eval("document.cookie === 'u=\\uFFFDx; theme=dark; js=1'"), "true", "a lone surrogate becomes U+FFFD");
    p.close();
}

#[test]
fn an_http_document_sets_no_secure_cookie_and_sees_none() {
    let j = jar();
    j.borrow_mut().set(&Url::parse("https://a.test/").unwrap(), "s=1; Secure", Api::Http, NOW);
    let mut p = Page::at("http://a.test/", "<!doctype html>", Some(&j));
    p.eval("document.cookie = 'plain=1'; document.cookie = 'mine=1; Secure'; document.cookie = '__Host-x=1; Path=/'");
    assert_eq!(p.eval("document.cookie"), "plain=1");
    assert_eq!(p.eval("document.cookie = 'other=1; Domain=b.test'; document.cookie"), "plain=1", "another site's domain");
    p.close();
}

#[test]
fn a_document_without_cookies_reads_empty_and_ignores_writes() {
    let j = jar();
    let mut p = Page::at("file:///tmp/x.html", "<!doctype html>", Some(&j));
    assert_eq!(p.eval("document.cookie = 'a=1'; document.cookie"), "");
    assert!(j.borrow().is_empty(), "a file document does not write the jar");
    p.close();
    let mut p = Page::at("https://a.test/", "<!doctype html>", None);
    assert_eq!(p.eval("document.cookie = 'a=1'; typeof document.cookie + ':' + document.cookie"), "string:");
    p.close();
}

#[test]
fn head_and_body_are_the_html_element_s_children() {
    let mut p = Page::at("https://a.test/", "<!doctype html><title>t</title><p>x</p>", None);
    assert_eq!(p.eval("document.head.localName + ' ' + document.body.localName"), "head body");
    assert_eq!(p.eval("document.body === document.documentElement.lastChild"), "true");
    p.eval("document.documentElement.removeChild(document.body); document.documentElement.removeChild(document.head)");
    assert_eq!(p.eval("String(document.body) + ' ' + String(document.head)"), "null null");
    p.eval("document.documentElement.appendChild(document.createElement('frameset'))");
    assert_eq!(p.eval("document.body.localName"), "frameset", "a frameset is the body element too");
    p.close();
}

#[test]
fn setting_body_replaces_it_or_appends_one() {
    let mut p = Page::at("https://a.test/", "<!doctype html><body id=old><p>x</p>", None);
    assert_eq!(p.throws("document.body = document.createElement('div')"), "HierarchyRequestError");
    assert_eq!(p.throws("document.body = null"), "HierarchyRequestError");
    p.eval("var b = document.createElement('body'); b.id = 'new'; document.body = b");
    assert_eq!(p.eval("document.body.id + ' ' + document.documentElement.children.length + ' ' + document.getElementById('old')"), "new 2 null");
    p.eval("document.documentElement.removeChild(document.body); document.body = document.createElement('body')");
    assert_eq!(p.eval("document.documentElement.lastChild === document.body"), "true", "appended to the html element");
    assert_eq!(p.eval("document.body = document.body; 'same'"), "same");
    p.eval("var nb = document.createElement('body'), sc = document.createElement('script'); sc.textContent = 'seen = document.documentElement.children.length'; nb.appendChild(sc); document.body = nb");
    assert_eq!(p.eval("seen + ' ' + (document.body === nb)"), "2 true", "the old body is gone before the new one's scripts run");
    p.eval("document.removeChild(document.documentElement)");
    assert_eq!(p.throws("document.body = document.createElement('body')"), "HierarchyRequestError");
    p.close();
}

#[test]
fn performance_counts_from_the_time_origin_with_an_empty_entry_buffer() {
    let mut p = Page::at("https://a.test/", "<!doctype html>", None);
    assert_eq!(p.eval("typeof performance.now() + ' ' + (performance instanceof Performance)"), "number true");
    assert_eq!(p.eval("var t = performance.now(); t >= 0 && t === performance.now()"), "true", "virtual time has not moved");
    assert_eq!(p.eval("Math.abs(performance.timeOrigin - Date.now()) < 60000"), "true");
    assert_eq!(p.eval("Number.isInteger(Math.round(performance.timeOrigin * 1e4) / 1e3)"), "true", "coarsened to 100 microseconds");
    assert_eq!(p.eval("[performance.getEntries().length, performance.getEntriesByType('resource').length, Array.isArray(performance.getEntriesByName('x'))].join()"), "0,0,true");
    assert_eq!(p.throws("Performance.prototype.now.call({})"), "TypeError");
    p.close();
}

#[test]
fn a_performance_observer_checks_its_options_and_keeps_its_kind() {
    let mut p = Page::at("https://a.test/", "<!doctype html>", None);
    assert_eq!(p.throws("new PerformanceObserver(1)"), "TypeError");
    assert_eq!(p.throws("PerformanceObserver(() => {})"), "TypeError");
    p.eval("var o = new PerformanceObserver(() => { throw 1 })");
    assert_eq!(p.throws("o.observe({})"), "TypeError");
    assert_eq!(p.throws("o.observe({entryTypes: ['resource'], type: 'resource'})"), "TypeError");
    assert_eq!(p.throws("o.observe({entryTypes: ['resource'], buffered: true})"), "TypeError");
    assert_eq!(p.throws("o.observe({entryTypes: 5})"), "TypeError");
    assert_eq!(p.throws("o.observe({entryTypes: ['resource', 'navigation']})"), "no exception");
    assert_eq!(p.throws("o.observe({type: 'resource'})"), "InvalidModificationError");
    assert_eq!(p.throws("o.observe({entryTypes: ['mark']})"), "no exception");
    p.eval("var s = new PerformanceObserver(function () {}); s.observe({type: 'navigation', buffered: true})");
    assert_eq!(p.throws("s.observe({entryTypes: ['resource']})"), "InvalidModificationError");
    assert_eq!(p.eval("o.takeRecords().length + ' ' + o.disconnect()"), "0 undefined");
    p.close();
}

#[test]
fn a_performance_observer_reads_entry_types_as_an_iterable_and_its_options_in_order() {
    let mut p = Page::at("https://a.test/", "<!doctype html>", None);
    p.eval("var o = new PerformanceObserver(() => {})");
    let t0 = std::time::Instant::now();
    assert_eq!(p.throws("o.observe({entryTypes: {length: 4294967295}})"), "TypeError", "an array-like is not iterable");
    assert!(t0.elapsed() < std::time::Duration::from_secs(1), "{:?}", t0.elapsed());
    assert_eq!(p.throws("o.observe({entryTypes: new Set(['resource'])})"), "no exception");
    p.eval("var n = 0, closed = false; var endless = { [Symbol.iterator]() { return { next: () => ({ value: 't' + n++, done: false }), return() { closed = true; return {} } } } }");
    assert_eq!(p.throws("o.observe({entryTypes: endless})"), "no exception");
    assert_eq!(p.eval("n + ' ' + closed"), "64 true", "read to the cap, then closed");
    assert_eq!(p.throws("o.observe({entryTypes: { [Symbol.iterator]() { return 1 } }})"), "TypeError");
    assert_eq!(p.throws("o.observe({entryTypes: [Symbol()]})"), "TypeError");
    assert_eq!(p.throws("o.observe({entryTypes: ['resource'], durationThreshold: 16})"), "TypeError");
    p.eval("var s = new PerformanceObserver(() => {})");
    assert_eq!(p.throws("s.observe({type: 'event', durationThreshold: Infinity})"), "TypeError");
    assert_eq!(p.throws("s.observe({type: 'event', durationThreshold: 16})"), "no exception");
    p.eval("var log = []; var d = { get type() { log.push('type'); return 'event' }, get entryTypes() { log.push('entryTypes') }, get durationThreshold() { log.push('durationThreshold') }, get buffered() { log.push('buffered') } }");
    assert_eq!(p.throws("new PerformanceObserver(() => {}).observe(d)"), "no exception");
    assert_eq!(p.eval("log.join()"), "buffered,durationThreshold,entryTypes,type");
    p.close();
}
