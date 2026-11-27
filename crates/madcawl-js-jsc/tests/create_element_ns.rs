//! `document.createElementNS` on JavaScriptCore: the DOM Standard's name
//! and namespace checks, and elements of the namespaces the DOM keeps.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn eval(html: &str, src: &str) -> String {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.parse_document(&mut e, html.as_bytes());
    let url = DomString::from_wtf8(b"ns");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(&mut h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()));
    let mut b = Vec::new();
    e.to_string(&mut h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.close(&mut e);
    String::from_utf8(b).unwrap()
}

#[test]
fn elements_of_the_kept_namespaces() {
    let src = "var S = 'http://www.w3.org/2000/svg', s = document.createElementNS(S, 'linearGradient'),
  h = document.createElementNS('http://www.w3.org/1999/xhtml', 'DIV'), m = document.createElementNS('http://www.w3.org/1998/Math/MathML', 'mi');
s.setAttribute('id', 'g'); document.body.appendChild(s);
return [s.localName, s.tagName, h.localName, h.tagName, m.localName, s instanceof Element,
  document.getElementById('g') === s, s.style.display = 'none', s.getAttribute('style')].join();";
    assert_eq!(eval("<body>", src), "linearGradient,linearGradient,DIV,DIV,mi,true,true,none,display: none;");
}

#[test]
fn bad_names_and_namespaces_throw_in_the_standard_order() {
    let src = "var out = [], S = 'http://www.w3.org/2000/svg';
for (const [ns, n] of [[S, ''], [S, 'a b'], [S, ':a'], [S, 'a:'], [null, 'p:a'], ['', 'p:a'], [S, 'xml:a'], [S, 'xmlns'],
  ['http://www.w3.org/2000/xmlns/', 'a'], [S, 'p:a'], ['urn:x', 'a'], [null, 'a'], [S, 'a:b:c']]) {
  try { document.createElementNS(ns, n); out.push('ok'); } catch (x) { out.push(x.name); }
}
return out.join();";
    let want = "InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,NamespaceError,NamespaceError,\
NamespaceError,NamespaceError,NamespaceError,NotSupportedError,NotSupportedError,NotSupportedError,NotSupportedError";
    assert_eq!(eval("<body>", src), want);
}
