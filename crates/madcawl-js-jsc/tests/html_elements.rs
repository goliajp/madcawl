//! The HTML element interfaces on JavaScriptCore: which interface an
//! element of each name gets, reflected attributes, `hidden`, the nonce
//! and the event handlers HTMLElement and the body element carry.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::{DomString, atom};

fn page(html: &str) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    assert!(h.set_url("https://example.test/dir/page?q"));
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, html.as_bytes());
    (e, h)
}

/// The result of `src` as a string, or the name of what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"html");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    let r = if Jsc::is_thrown(r.get()) {
        let _sentinel = r.into_raw();
        let x = e.take_exception().expect("an exception");
        let name = e.get(h, x.get(), madcawl_js::Key::Str(&DomString::from_wtf8(b"name")));
        e.release(x);
        name
    } else {
        r
    };
    let mut b = Vec::new();
    e.to_string(h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

fn run(html: &str, src: &str) -> String {
    let (mut e, mut h) = page(html);
    let out = eval(&mut e, &mut h, src);
    h.close(&mut e);
    out
}

#[test]
fn each_name_gets_the_interface_the_standard_gives_it_when_bound() {
    let src = "const names = ['div', 'span', 'a', 'img', 'script', 'link', 'style', 'input', 'button', 'iframe', 'object',
  'body', 'head', 'html', 'p', 'code', 'center', 'x-foo', 'accept-charset', 'foo', 'applet', 'svg', 'menuitem', 'font-face', 'x-\\u00e9'];
const out = names.map(n => Object.getPrototypeOf(document.createElement(n)).constructor.name);
const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
out.push(Object.getPrototypeOf(s).constructor.name, s instanceof HTMLElement, document.body instanceof HTMLBodyElement,
  document.documentElement instanceof HTMLHtmlElement, document.createElement('img') instanceof HTMLElement,
  HTMLElement.prototype.isPrototypeOf(document.createElement('x-y')), Object.getPrototypeOf(HTMLElement.prototype) === Element.prototype);
return out.join();";
    let want = "HTMLDivElement,HTMLSpanElement,HTMLAnchorElement,HTMLImageElement,HTMLScriptElement,HTMLLinkElement,\
HTMLStyleElement,HTMLInputElement,HTMLButtonElement,HTMLIFrameElement,HTMLObjectElement,HTMLBodyElement,HTMLHeadElement,\
HTMLHtmlElement,HTMLElement,HTMLElement,HTMLElement,HTMLElement,HTMLElement,HTMLUnknownElement,HTMLUnknownElement,\
HTMLUnknownElement,HTMLUnknownElement,HTMLUnknownElement,HTMLElement,Element,false,true,true,true,true,true";
    assert_eq!(run("<body>", src), want);
}

#[test]
fn a_member_of_one_interface_refuses_an_element_of_another() {
    let src = "const href = Object.getOwnPropertyDescriptor(HTMLAnchorElement.prototype, 'href').get,
  lang = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'lang').get, out = [];
for (const [f, el] of [[href, document.createElement('img')], [lang, document.createElementNS('http://www.w3.org/2000/svg', 'g')]]) {
  try { f.call(el); out.push('ok'); } catch (x) { out.push(x.name); }
}
out.push(lang.call(document.createElement('a')));
return out.join();";
    assert_eq!(run("<body>", src), "TypeError,TypeError,");
}

#[test]
fn string_url_and_keyword_attributes_reflect() {
    let src = "const a = document.createElement('a'), img = document.createElement('img'), f = document.createElement('iframe'),
  s = document.createElement('script'), out = [a.href === '', a.rel, img.src === ''];
a.href = '../x?y#z'; a.rel = 'noopener noreferrer'; a.target = '_blank'; a.title = 't'; a.lang = 'en';
out.push(a.href, a.getAttribute('href'), a.rel, a.target, a.title, a.lang);
a.setAttribute('href', 'http://[bad'); out.push(a.href);
img.src = '/favicon.ico'; img.alt = 'x'; out.push(img.src, img.alt);
f.width = '100'; f.height = 'auto'; f.name = 'n'; f.src = 'about:blank'; out.push(f.getAttribute('width'), f.height, f.name, f.src);
s.src = 'a.js'; out.push(s.src, s.getAttribute('src'));
for (const v of ['RTL', 'ltr', 'Auto', 'up', '']) { a.dir = v; out.push(a.dir); }
a.removeAttribute('dir'); out.push(a.dir === '');
return out.join();";
    let want = "true,,true,https://example.test/x?y#z,../x?y#z,noopener noreferrer,_blank,t,en,http://[bad,\
https://example.test/favicon.ico,x,100,auto,n,about:blank,https://example.test/dir/a.js,a.js,rtl,ltr,auto,,,true";
    assert_eq!(run("<body>", src), want);
}

#[test]
fn a_url_attribute_is_parsed_against_its_own_documents_url() {
    let (mut e, mut h) = page("<template><a href=x></a><a href='https://a.test/y'></a></template>");
    let doc = h.dom.document();
    let t = h.dom.descendants(doc).find(|&n| h.dom.local_name(n) == Some(atom!("template"))).unwrap();
    let a = h.dom.first_child(h.dom.contents(t).unwrap()).unwrap();
    // nothing bound reaches template contents yet: hand script the element
    let w = h.wrap(&mut e, a);
    let g = e.global(h.realm);
    assert!(e.set(&mut h, g.get(), madcawl_js::Key::Str(&DomString::from_wtf8(b"inert")), w.get()));
    e.release(g);
    e.release(w);
    let src = "const a = document.createElement('a'); a.href = 'x';
return [inert.href, inert.nextSibling.href, a.href].join();";
    assert_eq!(eval(&mut e, &mut h, src), "x,https://a.test/y,https://example.test/dir/x");
    h.close(&mut e);
}

#[test]
fn hidden_converts_its_value_as_the_standard_union_does() {
    let src = "const d = document.createElement('div'), out = [d.hidden];
const state = () => d.hasAttribute('hidden') ? JSON.stringify(d.getAttribute('hidden')) + ':' + d.hidden : 'none:' + d.hidden;
for (const v of [true, false, 1, 0, NaN, -0, 'x', '', 'UNTIL-found', null, 'until-found', undefined, {}, {toString() { return 'until-found'; }}]) {
  d.hidden = v; out.push(state());
}
try { d.hidden = Symbol(); } catch (x) { out.push(x.name, state()); }
d.setAttribute('hidden', 'Until-Found'); out.push(d.hidden);
return out.join('|');";
    let want = "false|\"\":true|none:false|\"\":true|none:false|none:false|none:false|\"\":true|none:false|\
\"until-found\":until-found|none:false|\"until-found\":until-found|none:false|\"\":true|\"until-found\":until-found|TypeError|\
\"until-found\":until-found|until-found";
    assert_eq!(run("<body>", src), want);
}

#[test]
fn the_nonce_is_set_apart_from_its_attribute_until_the_attribute_changes() {
    let src = "const s = document.createElement('script'), out = [JSON.stringify(s.nonce)];
s.setAttribute('nonce', 'a'); out.push(s.nonce);
s.nonce = 'b'; out.push(s.nonce, s.getAttribute('nonce'));
s.setAttribute('nonce', 'c'); out.push(s.nonce);
s.nonce = 'd'; s.getAttributeNode('nonce').value = 'e'; out.push(s.nonce);
s.nonce = 'f'; s.removeAttribute('nonce'); out.push(JSON.stringify(s.nonce));
s.nonce = 'g'; s.toggleAttribute('nonce'); out.push(JSON.stringify(s.nonce));
return out.join();";
    assert_eq!(run("<body>", src), "\"\",a,b,a,c,e,\"\",\"\"");
}

#[test]
fn element_handlers_are_the_elements_and_body_handlers_the_windows() {
    let (mut e, mut h) = page("<body><div id=d></div>");
    let src = "const d = document.getElementById('d'), out = globalThis.out = [d.onclick, d.onerror, d.onload];
d.onclick = () => out.push('click'); d.onerror = () => out.push('error');
d.dispatchEvent(new Event('click')); d.dispatchEvent(new Event('error')); d.dispatchEvent(new Event('load'));
document.body.onload = () => out.push('window load'); document.body.onerror = 'not callable';
out.push(document.body.onload !== null, document.body.onerror, document.createElement('body').onload !== null);
return out.join();";
    assert_eq!(eval(&mut e, &mut h, src), ",,,click,error,true,,true");
    assert_eq!(h.fire_window(&mut e, atom!("load")), 1);
    assert_eq!(eval(&mut e, &mut h, "return out[out.length - 1];"), "window load");
    h.close(&mut e);
}
