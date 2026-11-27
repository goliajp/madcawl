//! An element's `classList` on JavaScriptCore: the ordered set of its
//! `class` tokens, written back by the update steps, one object per
//! element that its element keeps, and freed with it.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Deadline, Engine, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

fn page(html: &str) -> (Jsc, DomHost<Jsc>) {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, html.as_bytes());
    (e, h)
}

/// The result of `src` as a string, or the name of what it threw.
fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"classlist");
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
fn class_list_reads_the_ordered_set_of_class_tokens() {
    let (mut e, mut h) = page("<p id=a class='  b a\tb  c'>x</p>");
    let src = "var p = document.getElementById('a'), l = p.classList;
return [l === p.classList, l instanceof DOMTokenList, l.length, l[0], l.item(2), String(l[3]), String(l.item(3)), l.value, String(l),
  l.contains('a'), l.contains('d'), [...l].join('+'), Array.from(l.entries()).length].join();";
    assert_eq!(eval(&mut e, &mut h, src), "true,true,3,b,c,undefined,null,  b a\tb  c,  b a\tb  c,true,false,b+a+c,3");
    h.close(&mut e);
}

#[test]
fn class_list_changes_are_written_back_to_the_attribute() {
    let (mut e, mut h) = page("<p id=a class='b a b'>x</p><i id=i></i>");
    let src = "var p = document.getElementById('a'), l = p.classList, out = [];
l.add('c', 'a', 'd'); out.push(p.className);
l.remove('b', 'x'); out.push(p.className);
out.push(l.toggle('a'), l.toggle('a', false), l.toggle('z', true), l.toggle('z', true), l.toggle('a', undefined), p.className);
out.push(l.replace('nope', 'q'), l.replace('c', 'd'), p.className, l.replace('z', 'n'), p.className, l.replace('n', 'n'), p.className);
var i = document.getElementById('i'); i.classList.remove('x'); out.push(i.hasAttribute('class'));
i.classList.toggle('x'); i.classList.toggle('x'); out.push(i.hasAttribute('class'), JSON.stringify(i.className));
p.classList = 'k  k j'; out.push(p.className, l.length, l.value);
l.value = 'v'; out.push(p.getAttribute('class'));
return out.join();";
    let want = "b a c d,a c d,false,false,true,true,true,c d z a,false,true,d z a,true,d n a,true,d n a,false,true,\"\",k  k j,2,k  k j,v";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn class_list_refuses_bad_tokens_before_changing_anything() {
    let (mut e, mut h) = page("<p id=a class=x>x</p>");
    let src = "var l = document.getElementById('a').classList, out = [];
for (const f of [() => l.add('y', ''), () => l.add('y', 'a b'), () => l.remove('x', ''), () => l.toggle('a\\nb'),
  () => l.replace('', 'y'), () => l.replace('x', 'a b'), () => l.supports('x'), () => l.item()]) {
  try { f(); out.push('none'); } catch (x) { out.push(x.name); }
}
out.push(l.value, l.contains(''), l.contains('a b'));
return out.join();";
    let want = "SyntaxError,InvalidCharacterError,SyntaxError,InvalidCharacterError,SyntaxError,InvalidCharacterError,TypeError,TypeError,x,false,false";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn an_element_keeps_its_class_list_and_cycles_through_it_are_freed() {
    let (mut e, mut h) = page("<p id=a class=x>x</p>");
    collect(&mut e, &mut h);
    let (nodes, sides) = (h.dom.node_count(), h.side_objects());
    let src = "var p = globalThis.p = document.getElementById('a'); p.classList.tag = 'kept';
for (let i = 0; i < 200; i++) { const el = document.createElement('div'); el.classList.add('c' + i); el.classList.el = el; el.list = el.classList; }
return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    assert!(h.side_objects() >= sides + 200, "the loop made its lists");
    collect(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return p.classList.tag;"), "kept");
    assert_eq!(h.dom.node_count(), nodes);
    assert_eq!(h.side_objects(), sides + 1, "only p's list is left");
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}
