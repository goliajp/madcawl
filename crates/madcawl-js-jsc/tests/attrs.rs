//! Attributes as `Attr` objects and an element's `attributes` map, as
//! page script uses them on JavaScriptCore: one object per attribute,
//! values that follow the element, attributes moved between elements,
//! and what the objects keep alive.

use madcawl_bindings::DomHost;
use madcawl_dom::{Dom, Limits};
use madcawl_js::{Deadline, Engine, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page(html: &str) -> (Jsc, DomHost<Jsc>) {
    page_in(Dom::new(), html)
}

fn page_in(dom: Dom, html: &str) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, dom);
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, html.as_bytes());
    (e, h)
}

/// The result of `src` as a string, or the name of what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"attrs");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    let r = if Jsc::is_thrown(r.get()) {
        let _sentinel = r.into_raw();
        e.take_exception().expect("an exception")
    } else {
        r
    };
    let mut b = Vec::new();
    e.to_string(h, r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}

fn collect(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    scrub();
    e.collect_now();
    while h.collect(e, Deadline::Work(u32::MAX)) != Step::Done {}
}

#[test]
fn an_attr_is_one_object_whose_value_follows_its_element() {
    let (mut e, mut h) = page("<p id=a class=x title=t>x</p>");
    let src = "var p = document.getElementById('a'), m = p.attributes, out = [];
out.push(m === p.attributes, m.length, m[0] === m.item(0), m[0] === p.getAttributeNode('id'), m.id === m[0],
  m.getNamedItem('CLASS') === p.getAttributeNode('class'), m[3], m.item(3), m instanceof NamedNodeMap);
var t = p.getAttributeNode('title');
out.push(t.name, t.value, t.localName, t.prefix, t.namespaceURI, t.ownerElement === p, t.specified, t.nodeType, t.nodeName,
  t instanceof Attr, t instanceof Node, t.parentNode, t.textContent, t.ownerDocument === document, t.isConnected, Node.ATTRIBUTE_NODE);
t.value = 'u'; out.push(p.getAttribute('title')); p.setAttribute('title', 'v'); out.push(t.value);
p.removeAttribute('title'); out.push(t.ownerElement, t.value, m.length, p.hasAttribute('title'), p.getAttributeNode('title'));
t.value = 'w'; t.textContent = 'w2'; out.push(p.hasAttribute('title'), t.value);
out.push(p.setAttributeNode(t), p.getAttribute('title'), t.ownerElement === p, m.length, m[2] === t, p.setAttributeNode(t) === t);
return out.map(String).join();";
    let want = "true,3,true,true,true,true,undefined,null,true,title,t,title,null,null,true,true,2,title,true,true,null,t,true,false,2,\
u,v,null,v,2,false,null,false,w2,null,w2,true,3,true,true";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn attrs_move_between_elements_and_maps_take_them_off() {
    let (mut e, mut h) = page("<p id=a class=x>x</p>");
    let src = "var p = document.getElementById('a'), m = p.attributes, out = [];
var c = document.createAttribute('Data-X'); c.value = '1'; out.push(c.name, c.ownerElement, c.value);
out.push(p.setAttributeNode(c), p.getAttribute('data-x'), m.length);
var c2 = document.createAttribute('data-x'); c2.value = '2'; var old = p.setAttributeNode(c2);
out.push(old === c, c.ownerElement, c.value, p.getAttribute('data-x'), m[2] === c2, m.length);
var q = document.createElement('q'), err = [];
try { q.setAttributeNode(c2); } catch (x) { err.push(x.name); }
out.push(p.removeAttributeNode(c2) === c2, c2.ownerElement, p.hasAttribute('data-x'), m.length);
try { p.removeAttributeNode(c2); } catch (x) { err.push(x.name); }
var cls = m.removeNamedItem('class'); out.push(cls.value, cls.ownerElement, p.className === '', m.length);
try { m.removeNamedItem('nope'); } catch (x) { err.push(x.name); }
out.push(q.setAttributeNode(cls), q.className, q.attributes.setNamedItem(c), q.getAttribute('data-x'), q.attributes.length);
out.push(p.toggleAttribute('id'), document.getElementById('a'), p.attributes.length);
try { document.createAttribute('a b'); } catch (x) { err.push(x.name); }
try { p.appendChild(c); } catch (x) { err.push(x.name); }
try { Node.prototype.hasChildNodes.call(c); } catch (x) { err.push(x.name); }
return out.map(String).concat(err).join();";
    let want = "data-x,null,1,null,1,3,true,null,1,2,true,3,true,null,false,2,x,null,true,1,null,x,null,1,2,false,null,0,\
InUseAttributeError,NotFoundError,NotFoundError,InvalidCharacterError,TypeError,TypeError";
    assert_eq!(eval(&mut e, &mut h, src), want);
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

#[test]
fn namespaced_attributes_are_found_by_namespace_and_local_name() {
    let (mut e, mut h) = page("<svg><a xlink:href=u xml:lang=en></a></svg>");
    let src = "var a = document.querySelector('a'), m = a.attributes, X = 'http://www.w3.org/1999/xlink', out = [];
var h = m[0]; out.push(h.name, h.prefix, h.localName, h.namespaceURI, h.value);
out.push(a.getAttributeNodeNS(X, 'href') === h, m.getNamedItemNS(X, 'href') === h, m.getNamedItemNS(null, 'href'),
  m.getNamedItemNS('urn:x', 'href'), m.getNamedItem('xlink:href') === h, m[1].namespaceURI);
var gone = m.removeNamedItemNS(X, 'href'); out.push(gone === h, h.ownerElement, h.value, m.length);
out.push(a.setAttributeNodeNS(h), m[1] === h, a.getAttributeNS === undefined);
try { m.removeNamedItemNS(X, 'nope'); } catch (x) { out.push(x.name); }
var log = []; h.addEventListener('x', function (ev) { log.push(ev.target === h, this === h); }); h.dispatchEvent(new Event('x', { bubbles: true }));
out.push(log.join(' '));
return out.map(String).join();";
    let want = "xlink:href,xlink,href,http://www.w3.org/1999/xlink,u,true,true,null,null,true,http://www.w3.org/XML/1998/namespace,\
true,null,u,1,null,true,true,NotFoundError,true true";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn an_element_keeps_its_attrs_properties_and_cycles_through_them_are_freed() {
    let (mut e, mut h) = page("<p id=a title=t>x</p>");
    let src = "var p = globalThis.p = document.getElementById('a'); p.getAttributeNode('title').tag = 'attr'; p.attributes.tag = 'map';
var d = globalThis.d = document.createElement('div'); d.setAttribute('k', 'v'); d.getAttributeNode('k').tag = 'detached';
globalThis.loose = document.createElement('b'); loose.setAttribute('z', '1'); globalThis.z = loose.getAttributeNode('z'); loose = null;
for (let i = 0; i < 200; i++) {
  const el = document.createElement('div'); el.setAttribute('a', String(i)); el.setAttribute('b', 'x');
  const at = el.getAttributeNode('a'); at.self = el; el.at = at; el.attributes.el = el;
  at.addEventListener('x', () => el); if (i % 2) el.removeAttributeNode(at);
}
return 'ok';";
    collect(&mut e, &mut h);
    let (nodes, sides) = (h.dom.node_count(), h.side_objects());
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    assert!(h.dom.node_count() >= nodes + 200 && h.side_objects() >= sides + 300, "the loop made its elements and attrs");
    collect(&mut e, &mut h);
    let src = "return [p.getAttributeNode('title').tag, p.attributes.tag, d.getAttributeNode('k').tag, z.ownerElement.tagName, z.value].join();";
    assert_eq!(eval(&mut e, &mut h, src), "attr,map,detached,B,1");
    collect(&mut e, &mut h);
    assert!(h.dom.node_count() <= nodes + 4, "{} nodes left of {nodes}", h.dom.node_count());
    assert!(h.side_objects() <= sides + 12, "{} side objects left of {sides}", h.side_objects());
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

#[test]
fn past_the_byte_limit_an_attr_is_on_its_element_exactly_when_its_attribute_is() {
    let dom = Dom::with_limits(Limits { bytes: 4096, ..Limits::default() });
    let (mut e, mut h) = page_in(dom, "<p id=a yy=old>x</p>");
    let src = "var p = document.getElementById('a'), old = p.getAttributeNode('yy'), out = [];
try { for (let i = 0; ; i++) p.setAttribute('f' + i, 'x'.repeat(100)); } catch (x) { out.push(x.name); }
var c = document.createAttribute('zz'); c.value = 'v';
try { p.setAttributeNode(c); out.push('none'); } catch (x) { out.push(x.name); }
out.push(c.ownerElement, p.hasAttribute('zz'), c.value, p.attributes.zz);
var c2 = document.createAttribute('yy'); c2.value = 'a value longer than the room left';
try { p.setAttributeNode(c2); out.push('none'); } catch (x) { out.push(x.name); }
out.push(c2.ownerElement === p, p.getAttribute('yy') === c2.value, c2.value.length < 30, p.getAttributeNode('yy') === c2,
  old.ownerElement, old.value);
p.removeAttribute('f0'); p.setAttributeNode(c); out.push(c.ownerElement === p, p.getAttribute('zz'));
return out.map(String).join();";
    let want = "QuotaExceededError,QuotaExceededError,null,false,v,undefined,QuotaExceededError,true,true,true,true,null,old,true,v";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}
