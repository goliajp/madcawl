//! The window's Location, History and Crypto, `atob` and `btoa`, and
//! the exceptions the host keeps, on JavaScriptCore as a page's script
//! uses them.

use madcawl_bindings::{DomHost, MAX_NOTES, Navigation, Note, ScriptError};
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

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
    /// A loaded document at `url`.
    fn at(url: &str) -> Page {
        let mut e = Jsc::new();
        let mut h = DomHost::new(&mut e, Dom::new());
        assert!(h.set_url(url));
        h.install_window(&mut e);
        h.use_virtual_time();
        h.parse_document(&mut e, b"<!doctype html><p>x</p>");
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

    /// The name of what `src` throws.
    fn throws(&mut self, src: &str) -> String {
        self.eval(&format!("(() => {{ try {{ {src}; return 'no exception' }} catch (x) {{ return x.name }} }})()"))
    }

    fn navigations(&mut self) -> Vec<Navigation> {
        let notes = self.h.take_notes().into_iter();
        notes.map(|n| if let Note::Navigation(n) = n { n } else { panic!("a navigation, not {n}") }).collect()
    }

    fn errors(&mut self) -> Vec<ScriptError> {
        let notes = self.h.take_notes().into_iter();
        notes.map(|n| if let Note::Exception(x) = n { x } else { panic!("an exception, not {n}") }).collect()
    }

    fn close(self) {
        let Page { mut e, h } = self;
        h.close(&mut e);
    }
}

#[test]
fn location_reads_the_parts_of_the_document_url() {
    let mut p = Page::at("https://a.test:8443/p/q?x=1#frag");
    let parts = "[location.href, location.origin, location.protocol, location.host, location.hostname, location.port, \
                 location.pathname, location.search, location.hash, String(location)].join(' ')";
    assert_eq!(
        p.eval(parts),
        "https://a.test:8443/p/q?x=1#frag https://a.test:8443 https: a.test:8443 a.test 8443 /p/q ?x=1 #frag https://a.test:8443/p/q?x=1#frag"
    );
    assert_eq!(p.eval("document.location === location && window.location === location && location instanceof Location"), "true");
    assert_eq!(p.eval("Object.prototype.toString.call(history) + ' ' + typeof crypto.getRandomValues"), "[object History] function");
    p.close();
    let mut p = Page::at("http://b.test/");
    assert_eq!(p.eval("[location.port, location.host, location.search, location.hash].join('|')"), "|b.test||");
    p.close();
}

#[test]
fn location_navigates_only_by_fragment_and_records_the_rest() {
    let mut p = Page::at("https://a.test/start?s=1");
    assert_eq!(p.eval("location.hash = 'two'; location.href + ' ' + history.length"), "https://a.test/start?s=1#two 2");
    assert_eq!(p.eval("location.hash = '#two'; history.length"), "2", "the same fragment is no navigation");
    assert_eq!(p.eval("location.href = '#three'; location.hash + ' ' + history.length"), "#three 3");
    assert_eq!(p.eval("location.replace('#four'); location.hash + ' ' + history.length"), "#four 3");
    assert_eq!(p.eval("location.href = location.href; history.length"), "3", "the same URL replaces its entry");
    p.eval("location.assign('/next?t=2'); location.href = 'https://b.test/'; location.replace('other'); location.reload()");
    assert_eq!(p.eval("location.href"), "https://a.test/start?s=1#four", "no navigation left the document");
    assert_eq!(
        p.navigations(),
        [
            Navigation::Assign("https://a.test/next?t=2".into()),
            Navigation::Assign("https://b.test/".into()),
            Navigation::Replace("https://a.test/other".into()),
            Navigation::Reload,
        ]
    );
    assert_eq!(p.throws("location.assign('http://[')"), "SyntaxError");
    assert_eq!(p.throws("location.href = 'http://['"), "SyntaxError");
    assert_eq!(p.eval("location.hash = '#a b\\t '; location.hash"), "#a%20b%20", "spaces are kept, tabs dropped");
    p.close();
}

#[test]
fn assigning_location_navigates_through_href_and_cannot_replace_the_object() {
    let mut p = Page::at("https://a.test/start");
    assert_eq!(p.eval("location = '/x'; location instanceof Location"), "true");
    assert_eq!(p.eval("window.location = '/y'; document.location = '/z'; location === document.location"), "true");
    assert_eq!(
        p.navigations(),
        ["/x", "/y", "/z"].map(|path| Navigation::Assign(format!("https://a.test{path}")))
    );
    let d = "const d = Object.getOwnPropertyDescriptor(globalThis, 'location');
             [d.configurable, d.enumerable, typeof d.get, typeof d.set, delete globalThis.location].join(' ')";
    assert_eq!(p.eval(d), "false true function function false");
    assert_eq!(p.throws("Object.defineProperty(globalThis, 'location', {value: 1})"), "TypeError");
    assert_eq!(p.eval("history = 1; crypto = 2; (history instanceof History) + ' ' + (crypto instanceof Crypto)"), "true true");
    assert_eq!(p.throws("Object.getOwnPropertyDescriptor(Document.prototype, 'location').set.call({}, '/w')"), "TypeError");
    assert!(p.navigations().is_empty());
    p.close();
}

#[test]
fn history_push_and_replace_change_the_url_and_state_in_place() {
    let mut p = Page::at("https://a.test/a");
    assert_eq!(p.eval("history.length + ' ' + history.state"), "1 null");
    assert_eq!(p.eval("history.pushState({n: 1}, '', '/b?q'); [location.href, history.length, history.state.n].join(' ')"), "https://a.test/b?q 2 1");
    assert_eq!(p.eval("history.replaceState('s', '', '#h'); [location.href, history.length, history.state].join(' ')"), "https://a.test/b?q#h 2 s");
    assert_eq!(p.eval("history.pushState(null, ''); [location.href, history.length, String(history.state)].join(' ')"), "https://a.test/b?q#h 3 null");
    assert_eq!(p.throws("history.pushState(1, '', 'https://b.test/')"), "SecurityError");
    assert_eq!(p.throws("history.replaceState(1, '', 'http://[')"), "SecurityError");
    assert_eq!(p.throws("history.pushState(1)"), "TypeError");
    assert_eq!(p.eval("history.length"), "3", "a refused push adds no entry");
    p.eval("history.back(); history.forward(); history.go(-2); history.go()");
    assert_eq!(
        p.navigations(),
        [Navigation::Traverse(-1), Navigation::Traverse(1), Navigation::Traverse(-2), Navigation::Reload]
    );
    p.close();
}

#[test]
fn atob_and_btoa_follow_forgiving_base64() {
    let mut p = Page::at("https://a.test/");
    assert_eq!(p.eval("btoa('hello') + ' ' + atob(' aGVs\\nbG8= ') + ' ' + atob('YQ') + ' ' + btoa('\\xff\\xfe')"), "aGVsbG8= hello a //4=");
    assert_eq!(p.eval("atob(btoa('\\x00\\x01\\x80\\xff')) === '\\x00\\x01\\x80\\xff'"), "true");
    assert_eq!(p.throws("atob('a')"), "InvalidCharacterError");
    assert_eq!(p.throws("atob('YQ=')"), "InvalidCharacterError");
    assert_eq!(p.throws("btoa('\\u0100')"), "InvalidCharacterError");
    assert_eq!(p.throws("atob()"), "TypeError");
    assert_eq!(p.eval("atob.length + ' ' + btoa(undefined)"), "1 dW5kZWZpbmVk");
    p.close();
}

#[test]
fn crypto_fills_integer_arrays_and_makes_uuids() {
    let mut p = Page::at("https://a.test/");
    assert_eq!(p.eval("const a = new Uint32Array(64); crypto.getRandomValues(a) === a && a.some(x => x !== 0)"), "true");
    let view = "const buf = new Uint8Array(32); const v = new Uint8Array(buf.buffer, 8, 8); crypto.getRandomValues(v);
                buf.slice(0, 8).every(x => x === 0) && buf.slice(16).every(x => x === 0) && v.some(x => x !== 0)";
    assert_eq!(p.eval(view), "true", "only the view's own bytes are filled");
    assert_eq!(p.eval("crypto.getRandomValues(new BigUint64Array(4)).length + ' ' + crypto.getRandomValues(new Uint8Array(65536)).length"), "4 65536");
    assert_eq!(p.throws("crypto.getRandomValues(new Float64Array(2))"), "TypeMismatchError");
    assert_eq!(p.throws("crypto.getRandomValues(new Uint8Array(65537))"), "QuotaExceededError");
    assert_eq!(p.throws("crypto.getRandomValues([1, 2])"), "TypeError");
    assert_eq!(p.throws("crypto.getRandomValues(new ArrayBuffer(4))"), "TypeError");
    assert_eq!(p.eval("typeof SharedArrayBuffer"), "undefined", "no view of shared memory to fill");
    let uuid = "const u = crypto.randomUUID(); /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(u) && u !== crypto.randomUUID()";
    assert_eq!(p.eval(uuid), "true");
    p.close();
}

#[test]
fn the_host_keeps_the_message_and_place_of_the_last_uncaught_exceptions() {
    let mut p = Page::at("https://a.test/");
    let run = |p: &mut Page, src: &str| p.h.run_script(&mut p.e, &DomString::from_wtf8(src.as_bytes()), &DomString::from_wtf8(b"inline"));
    run(&mut p, "let x = 1;\n\nthrow new TypeError('boom');");
    run(&mut p, "atob('a')");
    run(&mut p, "throw 'a string'");
    run(&mut p, "throw 1e21");
    run(&mut p, "throw -Infinity");
    run(&mut p, "class Mine extends RangeError {}; throw new Mine('sub')");
    let shown: Vec<String> = p.errors().iter().map(ToString::to_string).collect();
    // the column is the engine's: JavaScriptCore names the call's opening
    // parenthesis
    assert!(shown[0].starts_with("TypeError: boom @inline:3:"), "{}", shown[0]);
    assert!(shown[1].starts_with("InvalidCharacterError: the string is not valid base64"), "{}", shown[1]);
    assert_eq!(shown[2..5], ["a string", "1e+21", "-Infinity"]);
    assert!(shown[5].starts_with("sub @"), "a subclass's prototype is not read: {}", shown[5]);
    assert_eq!(p.h.reported, 6);
    for i in 0..MAX_NOTES + 8 {
        run(&mut p, &format!("throw new Error('e{i}')"));
    }
    let errors = p.errors();
    assert_eq!(errors.len(), MAX_NOTES);
    assert_eq!(errors[0].message, "Error: e8", "the oldest are dropped");
    run(&mut p, &format!("throw new Error('{}')", "x".repeat(2000)));
    run(&mut p, "throw 'y'.repeat(1 << 24)");
    let errors = p.errors();
    assert_eq!(errors[0].message, format!("Error: {}…", "x".repeat(512)), "a long message is cut");
    assert_eq!(errors[1].message, format!("{}…", "y".repeat(512)));
    p.close();
}

/// Page code the report runs would show in the stacks the getters and
/// traps record. The framework itself converts every exception that
/// leaves a call into it with `toString`, which reads `name` and
/// `message` and runs a proxy's `get` trap: those calls come from
/// native code, with no script frame of the host's.
#[test]
fn reporting_an_exception_runs_no_page_code() {
    let mut p = Page::at("https://a.test/");
    let run = |p: &mut Page, src: &str| p.h.run_script(&mut p.e, &DomString::from_wtf8(src.as_bytes()), &DomString::from_wtf8(b"inline"));
    run(
        &mut p,
        "globalThis.calls = []; globalThis.record = () => { calls.push(new Error().stack.split('\\n').slice(2).join(' / ')); };
         globalThis.bump = { get() { record(); return 'got' } }; globalThis.boom = { get() { record(); throw 1 } };
         globalThis.traps = { get() { record() }, getOwnPropertyDescriptor() { record() }, getPrototypeOf() { record() }, ownKeys() { record() }, has() { record() } };",
    );
    run(&mut p, "{ const e = new Error('m'); Object.defineProperty(e, 'message', bump); throw e }");
    run(&mut p, "{ const e = new TypeError('t'); Object.defineProperty(e, 'name', boom); throw e }");
    run(&mut p, "Object.defineProperty(SyntaxError.prototype, 'name', bump); throw new SyntaxError('s')");
    run(&mut p, "throw new Proxy(new Error('p'), traps)");
    run(&mut p, "{ const e = new Error('q'); Object.setPrototypeOf(e, new Proxy(Error.prototype, traps)); throw e }");
    run(&mut p, "throw { get message() { record() }, name: 'Odd' }");
    run(&mut p, "Object.defineProperty(Object.prototype, 'value', bump); Object.defineProperty(Array.prototype, '0', bump); throw new Error('r')");
    let shown: Vec<String> = p.errors().iter().map(|x| x.message.clone()).collect();
    assert_eq!(shown, ["Error", "TypeError: t", "Error: s", "an object", "q", "an object", "Error: r"]);
    let callers = p.eval("calls.filter((c) => c !== '' && c !== 'toString@[native code]').length + ' of ' + calls.length");
    assert!(callers.starts_with("0 of "), "only the framework's toString calls page code: {callers}");
    p.close();
}

#[test]
fn notes_keep_the_order_of_exceptions_and_navigations() {
    let mut p = Page::at("https://a.test/");
    let run = |p: &mut Page, src: &str| p.h.run_script(&mut p.e, &DomString::from_wtf8(src.as_bytes()), &DomString::from_wtf8(b"inline"));
    run(&mut p, "location.assign('/one'); throw new Error('between')");
    run(&mut p, "location.reload()");
    let notes: Vec<String> = p.h.take_notes().iter().map(ToString::to_string).collect();
    assert_eq!(notes[0], "navigation not followed: assign https://a.test/one");
    assert!(notes[1].starts_with("uncaught exception: Error: between @inline:1:"), "{}", notes[1]);
    assert_eq!(notes[2], "navigation not followed: reload");
    p.close();
}
