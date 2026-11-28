//! URL and URLSearchParams on JavaScriptCore: parsing against a base,
//! the parts and their setters as the URL Standard has them, and
//! `searchParams` kept in step with the query both ways.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Deadline, Engine, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page() -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, b"<body>");
    (e, h)
}

/// The result of `src` as a string, or what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"url-class");
    let realm = h.realm;
    let src = format!("String((function () {{ try {{ {src} }} catch (x) {{ return 'threw ' + x.name; }} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw");
    let mut b = Vec::new();
    e.to_string(h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

#[test]
fn a_url_parses_against_a_base_and_reads_its_parts() {
    let (mut e, mut h) = page();
    let src = "var u = new URL('../b/c?x=1#f', 'https://user:pw@EXAMPLE.com:8080/a/z');
function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash,
  String(u), u.toJSON(), JSON.stringify({ u }), name(() => new URL('nope')), name(() => new URL('/x', 'nope')),
  URL.canParse('/x', 'https://a/'), URL.canParse('/x'), URL.parse('https://a/b') instanceof URL, URL.parse('x'),
  new URL('blob:https://a.test/1234').origin, new URL('data:text/plain,hi').origin, new URL('mailto:x@y').pathname].join('|');";
    let want = "https://user:pw@example.com:8080/b/c?x=1#f|https://example.com:8080|https:|user|pw|example.com:8080|example.com|8080|/b/c|?x=1|#f|\
https://user:pw@example.com:8080/b/c?x=1#f|https://user:pw@example.com:8080/b/c?x=1#f|{\"u\":\"https://user:pw@example.com:8080/b/c?x=1#f\"}|\
TypeError|TypeError|true|false|true||https://a.test|null|x@y";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn the_setters_change_one_part_or_nothing_as_the_standard_says() {
    let (mut e, mut h) = page();
    let src = "function set(href, k, v) { var u = new URL(href); u[k] = v; return u.href; }
return [
  set('http://a/p', 'protocol', 'https:x'), set('http://a/p', 'protocol', 'foo'), set('foo://a/p', 'protocol', 'http'),
  set('http://a:443/p', 'protocol', 'https'), set('http://u@a/p', 'protocol', 'file'), set('http://a/p', 'protocol', '1x'),
  set('http://a/p', 'username', 'a b@'), set('http://a/p', 'password', 'p:w'), set('file:///p', 'username', 'u'),
  set('http://a/p', 'host', 'b.test:81/zz'), set('http://a:9/p', 'host', 'b.test:x'), set('http://a/p', 'host', ''),
  set('http://a/p', 'hostname', 'b:1'), set('http://a/p', 'hostname', 'B.Test'), set('foo://a/p', 'host', ''),
  set('http://a/p', 'host', 'x@y'), set('mailto:x', 'host', 'b'), set('file://h/p', 'host', 'localhost'),
  set('http://a/p', 'port', '8080abc'), set('http://a:1/p', 'port', ''), set('http://a/p', 'port', '80'),
  set('http://a/p', 'port', '65536'), set('http://a/p', 'port', 'x'),
  set('http://a/p', 'pathname', 'x y/../z?#'), set('http://a/p', 'pathname', ''), set('foo://a/p', 'pathname', ''),
  set('foo:/p', 'pathname', '//x'), set('http://a/p', 'pathname', 'a b/c'),
  set('http://a/p', 'search', '?a b#c'), set('http://a/p?q', 'search', ''), set('http://a/p', 'search', \"'\"),
  set('foo://a/p', 'search', \"'\"),
  set('http://a/p', 'hash', '#a b#c'), set('http://a/p#f', 'hash', ''),
  set('http://a/p', 'href', 'https://b/'),
].join('\\n');";
    let want = [
        "https://a/p", "http://a/p", "foo://a/p",
        "https://a/p", "http://u@a/p", "http://a/p",
        "http://a%20b%40@a/p", "http://:p%3Aw@a/p", "file:///p",
        "http://b.test:81/p", "http://b.test:9/p", "http://a/p",
        "http://a/p", "http://b.test/p", "foo:///p",
        "http://a/p", "mailto:x", "file:///p",
        "http://a:8080/p", "http://a/p", "http://a/p",
        "http://a/p", "http://a/p",
        "http://a/z%3F%23", "http://a/", "foo://a",
        "foo:/.//x", "http://a/a%20b/c",
        "http://a/p?a%20b%23c", "http://a/p", "http://a/p?%27",
        "foo://a/p?'",
        "http://a/p#a%20b#c", "http://a/p",
        "https://b/",
    ]
    .join("\n");
    assert_eq!(eval(&mut e, &mut h, src), want);
    let src = "var u = new URL('http://a/'); function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
return [name(() => { u.href = 'nope'; }), u.href].join();";
    assert_eq!(eval(&mut e, &mut h, src), "TypeError,http://a/");
    h.close(&mut e);
}

#[test]
fn search_params_and_the_query_follow_each_other() {
    let (mut e, mut h) = page();
    let src = "var u = new URL('http://a/?b=2&a=1&b=3#f'), p = u.searchParams;
var log = [p === u.searchParams, p instanceof URLSearchParams, p.size, p.get('b'), p.getAll('b').join('+'), p.has('a'), p.has('b', '3'), p.get('z')];
p.append('c', 'x y&'); log.push(u.href);
p.delete('b', '2'); log.push(u.search);
p.set('b', 'n'); log.push(u.search);
p.sort(); log.push(u.search);
u.search = '?q=%41+b'; log.push(p.get('q'), p.size);
p.delete('q'); log.push(u.href);
u.href = 'http://z/?k=v'; log.push(p.toString());
return log.join('|');";
    let want = "true|true|3|2|2+3|true|true||http://a/?b=2&a=1&b=3&c=x+y%26#f|?a=1&b=3&c=x+y%26|?a=1&b=n&c=x+y%26|\
?a=1&b=n&c=x+y%26|A b|1|http://a/#f|k=v";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn url_search_params_take_a_string_pairs_or_a_record_and_iterate() {
    let (mut e, mut h) = page();
    let src = "function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
var a = new URLSearchParams('?x=1&y=%zz&&z'), b = new URLSearchParams([['k', 'v'], new Set(['s', 't'])]);
var c = new URLSearchParams({ r: 1, q: 'two', [Symbol('s')]: 3 }), d = new URLSearchParams(new URLSearchParams('m=n'));
var seen = []; b.forEach(function (v, k, o) { seen.push(k + '=' + v + ':' + (o === b) + ':' + (this === seen)); if (k === 'k') b.append('late', '1'); }, seen);
return [a.toString(), b.toString(), c.toString(), d.toString(), seen.join(' '), [...c].join(' '), [...c.keys()].join(' '),
  [...c.values()].join(' '), Array.from(c.entries()).length, URLSearchParams.prototype[Symbol.iterator] === URLSearchParams.prototype.entries,
  name(() => new URLSearchParams([['x']])), name(() => new URLSearchParams([1])), new URLSearchParams(null).toString(),
  new URLSearchParams().size, name(() => b.forEach(1))].join('|');";
    let want = "x=1&y=%25zz&z=|k=v&s=t&late=1|r=1&q=two|m=n|k=v:true:true s=t:true:true late=1:true:true|r,1 q,two|r q|1 two|2|true|\
TypeError|TypeError|null=|0|TypeError";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn search_params_iterate_through_the_realm_s_own_array_iterator() {
    let (mut e, mut h) = page();
    let src = "var w = document.body.appendChild(document.createElement('iframe')).contentWindow, p = new w.URLSearchParams('a=1&b=2');
var AIP = Object.getPrototypeOf([][Symbol.iterator]()), WAIP = Object.getPrototypeOf(w.Array.prototype.values.call([]));
Array.prototype.values = w.Array.prototype.values = Array.prototype[Symbol.iterator] = w.Array.prototype[Symbol.iterator] = () => 'patched';
var it = p.entries(), first = it.next().value;
return [Object.getPrototypeOf(it) === WAIP, Object.getPrototypeOf(it) === AIP, first instanceof w.Array, first.join('='),
  [...p.keys()].join(' '), p.getAll('a') instanceof w.Array].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,false,true,a=1,a b,true");
    h.close(&mut e);
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}

/// A URL and its `searchParams` hold each other; once script drops both
/// the collector frees the pair.
#[test]
fn a_url_and_its_search_params_are_freed_together() {
    let (mut e, mut h) = page();
    let before = h.side_objects();
    let src = "for (var i = 0; i < 200; i++) { var u = new URL('http://a/?i=' + i); u.searchParams.append('x', i); u.searchParams.foo = i; }
window.kept = new URL('http://k/?a=1'); kept.searchParams.bar = 7; return '';";
    eval(&mut e, &mut h, src);
    for _ in 0..3 {
        scrub();
        e.collect_now();
        while h.collect(&mut e, Deadline::Work(u32::MAX)) != Step::Done {}
    }
    assert!(h.side_objects() <= before + 4, "{} side objects left of {before}", h.side_objects());
    assert_eq!(eval(&mut e, &mut h, "return [kept.searchParams.bar, kept.searchParams.get('a')].join();"), "7,1");
    h.close(&mut e);
}
