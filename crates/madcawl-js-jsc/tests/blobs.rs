//! Blob and blob URLs on JavaScriptCore: a blob from parts, its type and
//! slices, its bytes read through promises settled in a task, and the
//! blob URLs a window makes, bounded and gone with the window.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page() -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    assert!(h.set_url("https://a.test/dir/page"));
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, b"<body>");
    while h.run_tasks(&mut e, 100) != 0 {}
    (e, h)
}

/// The result of `src` as a string, or what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"blobs");
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
fn a_blob_is_made_of_strings_buffers_and_blobs() {
    let (mut e, mut h) = page();
    let src = "var inner = new Blob(['xy']), b = new Blob(['a\\u00e9', new Uint8Array([65, 66]), new DataView(new Uint8Array([1, 67]).buffer, 1), inner,
  new Uint16Array([0x4544]).buffer, 'lone\\ud800'], { type: 'Text/Plain;Charset=UTF-8' });
function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
return [b.size, b.type, b instanceof Blob, new Blob().size, new Blob([]).type, new Blob(['x'], { type: 'a\\u00e9' }).type,
  new Blob(['a\\r\\nb\\rc\\n'], { endings: 'native' }).size, new Blob(['a\\r\\nb'], { endings: 'transparent' }).size,
  name(() => new Blob('abc')), name(() => new Blob([], { endings: 'x' })), name(() => new Blob([], 1)), name(() => Blob()),
  Object.prototype.toString.call(b)].join();";
    let want = "17,text/plain;charset=utf-8,true,0,,,6,4,TypeError,TypeError,TypeError,TypeError,[object Blob]";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn text_and_array_buffer_settle_in_a_task_and_slices_share_the_bytes() {
    let (mut e, mut h) = page();
    let src = "window.log = []; var b = new Blob(['\\ufeffhello, w\\u00f6rld']);
var p = b.text(); log.push(p instanceof Promise);
p.then(t => log.push('text:' + t));
b.slice(3, 8).text().then(t => log.push('slice:' + t));
b.slice(-6, -1, 'X/Y').arrayBuffer().then(a => log.push('buf:' + (a instanceof ArrayBuffer) + ':' + new Uint8Array(a).join(' ')));
var s = b.slice(-3); log.push(s.size, s.type, b.slice(5, 2).size, b.slice(NaN, 1e300).size, b.slice(2, undefined).size,
  b.slice(0, 3, 'A\\u00e9').type);
Promise.resolve().then(() => log.push('microtask'));
return '';";
    eval(&mut e, &mut h, src);
    assert_eq!(eval(&mut e, &mut h, "return log.join('|');"), "true|3||0|16|14||microtask");
    while h.run_tasks(&mut e, 100) != 0 {}
    let want = "true|3||0|16|14||microtask|text:hello, w\u{f6}rld|slice:hello|buf:true:119 195 182 114 108";
    assert_eq!(eval(&mut e, &mut h, "return log.join('|');"), want);
    h.close(&mut e);
}

#[test]
fn a_blob_url_names_its_blob_until_revoked() {
    let (mut e, mut h) = page();
    let src = "window.u = URL.createObjectURL(new Blob(['<p>hi'], { type: 'text/html' }));
function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } }
return [/^blob:https:\\/\\/a\\.test\\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(u),
  new URL(u).origin, name(() => URL.createObjectURL({})), name(() => URL.createObjectURL())].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,https://a.test,TypeError,TypeError");
    let u = eval(&mut e, &mut h, "return u;");
    assert_eq!(h.blob_url(&format!("{u}#frag")), Some((b"<p>hi".as_slice(), "text/html")));
    eval(&mut e, &mut h, "URL.revokeObjectURL('blob:https://b.test/' + u.slice(-36)); URL.revokeObjectURL('nope'); return '';");
    assert_eq!(h.blob_url_count(), 1, "another origin's URL and a bad one revoke nothing");
    eval(&mut e, &mut h, "URL.revokeObjectURL(u); return '';");
    assert_eq!(h.blob_url(&u), None);
    assert_eq!(h.blob_url_count(), 0);
    h.close(&mut e);
}

#[test]
fn a_window_holds_a_bounded_number_of_blob_urls_which_go_with_it() {
    let (mut e, mut h) = page();
    let src = "var b = new Blob(['x']), n = 0;
try { for (;;) { URL.createObjectURL(b); n++; } } catch (x) { return n + ':' + x.name; }";
    assert_eq!(eval(&mut e, &mut h, src), "4096:QuotaExceededError");
    let src = "var f = document.body.appendChild(document.createElement('iframe')); var U = f.contentWindow.URL;
window.child = U.createObjectURL(new Blob(['c'])); window.U = U; return child.slice(0, 20);";
    assert_eq!(eval(&mut e, &mut h, src), "blob:https://a.test/");
    assert_eq!(h.blob_url_count(), 4097);
    assert_eq!(eval(&mut e, &mut h, "document.querySelector('iframe').remove(); return '';"), "");
    assert_eq!(h.blob_url_count(), 4096, "the iframe's window took its blob URL with it");
    let child = eval(&mut e, &mut h, "return child;");
    assert!(h.blob_url(&child).is_none());
    let src = "function name(f) { try { f(); return 'none'; } catch (x) { return x.name; } } return name(() => U.createObjectURL(new Blob([])));";
    assert_eq!(eval(&mut e, &mut h, src), "TypeError");
    h.close(&mut e);
}

#[test]
fn an_iframe_s_blob_settles_promises_of_its_own_realm_and_none_once_it_is_gone() {
    let (mut e, mut h) = page();
    let src = "window.log = []; var w = document.body.appendChild(document.createElement('iframe')).contentWindow, b = new w.Blob(['ab']);
var p = b.text(), q = b.arrayBuffer(); log.push(p instanceof w.Promise, p instanceof Promise);
q.then(a => log.push(a instanceof w.ArrayBuffer, new Uint8Array(a).join(' '), a.transfer().byteLength, a.byteLength));
var g = document.body.appendChild(document.createElement('iframe')); g.contentWindow.Blob.prototype.text.call(b).then(() => log.push('gone'));
g.remove(); return '';";
    assert_eq!(eval(&mut e, &mut h, src), "");
    while h.run_tasks(&mut e, 100) != 0 {}
    assert_eq!(eval(&mut e, &mut h, "return log.join();"), "true,false,true,97 98,2,0");
    h.close(&mut e);
}
