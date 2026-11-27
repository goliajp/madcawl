//! An element's `style` on JavaScriptCore: the declaration block of its
//! style attribute, read and changed through CSSStyleDeclaration and its
//! property attributes, written back to the attribute, parsed again when
//! the attribute is set, and kept by its element.

use madcawl_bindings::DomHost;
use madcawl_dom::{Dom, Mutation};
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
    let url = DomString::from_wtf8(b"style");
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
fn style_reads_the_declarations_of_the_attribute() {
    let (mut e, mut h) = page("<p id=a style='display: None; COLOR: Red; margin: 1px 2px; width: 3px !important; float: left'>x</p>");
    let src = "var p = document.getElementById('a'), s = p.style, P = CSSStyleDeclaration.prototype;
return [s === p.style, s instanceof CSSStyleDeclaration, s.display, s.color, s.margin, s.marginTop, s['margin-right'], s.length,
  s[0], s.item(1), s.item(99), String(s[99]), s.getPropertyValue('MARGIN-LEFT'), s.getPropertyPriority('width'),
  s.getPropertyPriority('color'), s.cssFloat, s.float, String(s.parentRule), 'display' in P, s.hasOwnProperty('display'),
  typeof s.notAProperty, JSON.stringify(s.cursor), s.cssText].join('|');";
    let want = "true|true|none|red|1px 2px|1px|2px|8|display|color||undefined|2px|important||left|left|null|true|false|undefined|\"\"|\
display: none; color: red; margin: 1px 2px; width: 3px !important; float: left;";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn changes_are_written_to_the_attribute_and_the_attribute_is_read_again() {
    let (mut e, mut h) = page("<p id=a style='color: red'>x</p><i id=i></i>");
    let src = "var p = document.getElementById('a'), s = p.style, out = [];
s.display = 'none'; out.push(p.getAttribute('style'));
s.setProperty('Width', '10PX', 'IMPORTANT'); out.push(p.getAttribute('style'));
s.setProperty('height', '1px', 'bogus'); s.width = 'red'; s.notAProperty = 'x'; s.setProperty('not-a-property', 'x');
out.push(p.getAttribute('style'), s.notAProperty);
out.push(s.removeProperty('color'), s.removeProperty('color'), p.getAttribute('style'));
s.width = null; out.push(p.getAttribute('style'));
s.webkitBoxSizing = 'border-box'; s.backgroundColor = 'rgb(0 0 255)'; out.push(p.getAttribute('style'));
s.cssText = 'top: 1px; nope: 2'; out.push(p.getAttribute('style'), s.length);
p.style = 'left: 2px'; out.push(p.getAttribute('style'), s.top);
p.setAttribute('style', 'bottom: 3px; margin: 4px'); out.push(s.bottom, s.left, s.margin, s.length);
p.removeAttribute('style'); out.push(s.length, s.cssText === '');
var i = document.getElementById('i'); i.style.removeProperty('color'); out.push(i.hasAttribute('style'));
i.style.color = ''; out.push(i.hasAttribute('style'));
i.style.cssText = ''; out.push(JSON.stringify(i.getAttribute('style')));
return out.join('|');";
    let want = "color: red; display: none;|color: red; display: none; width: 10px !important;|\
color: red; display: none; width: 10px !important;|x|red||display: none; width: 10px !important;|display: none;|\
display: none; box-sizing: border-box; background-color: rgb(0, 0, 255);|top: 1px;|1|left: 2px;||3px||4px|5|0|true|false|false|\"\"";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn every_change_marks_the_element_once_and_no_change_marks_nothing() {
    let (mut e, mut h) = page("<p id=a style='color: red'>x</p>");
    assert_eq!(eval(&mut e, &mut h, "globalThis.s = document.getElementById('a').style; return s.color;"), "red");
    let d = h.dom.document();
    let p = h.dom.descendants(d).find(|&n| h.dom.attribute(n, madcawl_str::atom!("id")).is_some_and(|v| v.eq_str("a"))).expect("p");
    h.dom.observe(64);
    let mut m = Vec::new();
    eval(&mut e, &mut h, "s.color; s.cssText; s.color = 'red'; s.width = 'bogus'; s.notAProperty = 'x'; s.removeProperty('top'); return '';");
    assert!(h.dom.take_mutations(&mut m));
    assert!(m.is_empty(), "reads and changes to nothing: {m:?}");
    eval(&mut e, &mut h, "s.display = 'none'; s.margin = '1px'; return '';");
    assert!(h.dom.take_mutations(&mut m));
    assert_eq!(m, [Mutation::Attribute(p), Mutation::Attribute(p)]);
    h.close(&mut e);
}

#[test]
fn property_accessors_check_their_receiver() {
    let (mut e, mut h) = page("<p id=a>x</p>");
    let src = "var out = ['color' in CSSStyleDeclaration.prototype]; document.body.style;
var d = Object.getOwnPropertyDescriptor(CSSStyleDeclaration.prototype, 'backgroundColor');
out.push(typeof d.get, typeof d.set, d.enumerable, d.configurable);
try { d.get.call({}); out.push('none'); } catch (x) { out.push(x.name); }
try { d.set.call(document.body, 'red'); out.push('none'); } catch (x) { out.push(x.name); }
out.push('webkitFlexDirection' in CSSStyleDeclaration.prototype, '-webkit-flex-direction' in CSSStyleDeclaration.prototype,
  'marginInlineStart' in CSSStyleDeclaration.prototype, 'gridArea' in CSSStyleDeclaration.prototype);
return out.join();";
    assert_eq!(eval(&mut e, &mut h, src), "false,function,function,true,true,TypeError,TypeError,true,true,true,true");
    h.close(&mut e);
}

#[test]
fn an_element_keeps_its_style_and_cycles_through_it_are_freed() {
    let (mut e, mut h) = page("<p id=a style='color: red'>x</p>");
    collect(&mut e, &mut h);
    let (nodes, sides) = (h.dom.node_count(), h.side_objects());
    let src = "var p = globalThis.p = document.getElementById('a'); p.style.tag = 'kept';
for (let i = 0; i < 200; i++) { const el = document.createElement('div'); el.style.width = i + 'px'; el.style.el = el; el.s = el.style; }
return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    assert!(h.side_objects() >= sides + 200, "the loop made its declarations");
    collect(&mut e, &mut h);
    assert_eq!(eval(&mut e, &mut h, "return p.style.tag + p.style.color;"), "keptred");
    assert_eq!(h.dom.node_count(), nodes);
    assert_eq!(h.side_objects(), sides + 1, "only p's style is left");
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

#[test]
fn properties_the_cascade_does_not_compute_are_kept_and_have_attributes() {
    let (mut e, mut h) = page("<p id=a style='transform: scale(2); cursor: pointer; color: red'>x</p>");
    let src = "var p = document.getElementById('a'), s = p.style; s.display = 'none';
var out = [p.getAttribute('style'), s.transform.match(/scale\\((\\d)\\)/)[1], s.cursor, JSON.stringify(s.transition),
  JSON.stringify(s.pointerEvents), JSON.stringify(s.animation)];
s.pointerEvents = 'none'; s.webkitUserSelect = 'none'; out.push(p.getAttribute('style'));
return out.join('|');";
    let want = "transform: scale(2); cursor: pointer; color: red; display: none;|2|pointer|\"\"|\"\"|\"\"|\
transform: scale(2); cursor: pointer; color: red; display: none; pointer-events: none; -webkit-user-select: none;";
    assert_eq!(eval(&mut e, &mut h, src), want);
    h.close(&mut e);
}

#[test]
fn a_prototype_script_froze_or_took_names_of_first_is_left_as_it_is() {
    let (mut e, mut h) = page("<p id=a style='color: red'>x</p>");
    let src = "Object.defineProperty(CSSStyleDeclaration.prototype, 'color', { value: 'mine', configurable: false });
var s = document.getElementById('a').style;
return [s.color, s.display === '', s.getPropertyValue('color')].join();";
    assert_eq!(eval(&mut e, &mut h, src), "mine,true,red");
    h.close(&mut e);
    let (mut e, mut h) = page("<p id=a style='color: red'>x</p>");
    let src = "Object.freeze(CSSStyleDeclaration.prototype); var s = document.getElementById('a').style;
return [typeof s.color, s.getPropertyValue('color'), s.cssText].join();";
    assert_eq!(eval(&mut e, &mut h, src), "undefined,red,color: red;");
    h.close(&mut e);
}

#[test]
fn webkit_names_of_uncomputed_properties_have_attributes() {
    let (mut e, mut h) = page("<p id=a>x</p>");
    let src = "var s = document.getElementById('a').style; s.webkitTransform = 'scale(2)';
return [s.transform, s.WebkitTransform, s['-webkit-transform'], s.cssText].join('|');";
    assert_eq!(eval(&mut e, &mut h, src), "scale(2)|scale(2)|scale(2)|transform: scale(2);");
    h.close(&mut e);
}
