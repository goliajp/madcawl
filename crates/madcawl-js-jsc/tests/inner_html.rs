//! `innerHTML` and `outerHTML` on JavaScriptCore: markup written by the
//! fragment serializing algorithm, markup parsed for the right context,
//! scripts it makes that never run, and the nodes it replaces freed.

use madcawl_bindings::DomHost;
use madcawl_dom::{Dom, Limits, NodeId, NodeKind};
use madcawl_js::{Deadline, Engine, Key, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::{DomString, atom};

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
    let url = DomString::from_wtf8(b"inner");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    let r = if Jsc::is_thrown(r.get()) {
        let _sentinel = r.into_raw();
        let x = e.take_exception().expect("an exception");
        let name = e.get(h, x.get(), Key::Str(&DomString::from_wtf8(b"name")));
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
    assert_eq!(h.dom.audit().mismatches(), 0, "{:?}", h.dom.audit());
    h.close(&mut e);
    out
}

#[test]
fn markup_is_read_back_as_the_serializer_writes_it() {
    let src = "const d = document.getElementById('d');
return [d.innerHTML, d.outerHTML, document.querySelector('template').innerHTML, document.querySelector('noscript').innerHTML].join('|');";
    let html = "<div id=d title='a\"b'>x &amp; <br><i>y</i><!--c--></div><template><p>t</template><noscript><b>n</b></noscript>";
    assert_eq!(
        run(html, src),
        "x &amp; <br><i>y</i><!--c-->|<div id=\"d\" title=\"a&quot;b\">x &amp; <br><i>y</i><!--c--></div>|<p>t</p>|<b>n</b>"
    );
}

#[test]
fn setting_inner_html_replaces_the_children_with_the_parsed_markup() {
    let src = "const d = document.getElementById('d'), old = d.firstChild, out = [];
d.innerHTML = 'Ray ID: <code>a1</code><a href=/x target=_blank>y</a>';
out.push(d.childNodes.length, d.querySelector('code').textContent, d.querySelector('a') instanceof HTMLAnchorElement,
  old.parentNode, d.lastChild.ownerDocument === document, d.innerHTML);
d.innerHTML = null; out.push(d.childNodes.length, JSON.stringify(d.innerHTML));
return out.join('|');";
    let want = "3|a1|true||true|Ray ID: <code>a1</code><a href=\"/x\" target=\"_blank\">y</a>|0|\"\"";
    assert_eq!(run("<div id=d><b>old</b></div>", src), want);
}

#[test]
fn the_element_is_the_context_the_markup_is_parsed_for() {
    let src = "const t = document.querySelector('tbody'), ta = document.querySelector('textarea'), tpl = document.querySelector('template'),
  s = document.createElementNS('http://www.w3.org/2000/svg', 'svg'), out = [];
t.innerHTML = '<tr><td>1'; out.push(t.innerHTML);
ta.innerHTML = '<b>x</b>'; out.push(ta.childNodes.length, ta.firstChild.nodeType, ta.textContent);
tpl.innerHTML = '<i>c</i>'; out.push(tpl.childNodes.length, tpl.innerHTML);
s.innerHTML = '<circle/><p>'; out.push(s.firstChild.namespaceURI === undefined ? s.firstChild.tagName : '', s.childNodes.length);
document.body.innerHTML = '<td>1</td>x'; out.push(document.body.innerHTML);
return out.join('|');";
    let want = "<tr><td>1</td></tr>|1|3|<b>x</b>|0|<i>c</i>|circle|2|1x";
    assert_eq!(run("<table><tbody></tbody></table><textarea></textarea><template></template>", src), want);
}

#[test]
fn scripts_made_by_markup_never_run_but_a_script_given_children_may() {
    let src = "const d = document.createElement('div'), out = [];
d.innerHTML = '<script>window.a = 1<\\/script><img src=x>'; document.body.appendChild(d);
out.push(typeof window.a);
d.outerHTML = '<script>window.b = 1<\\/script>'; out.push(typeof window.b);
const s = document.createElement('script'); document.body.appendChild(s);
s.innerHTML = 'window.c = 1'; out.push(window.c);
s.innerHTML = 'window.c = 2'; out.push(window.c);
return out.join();";
    assert_eq!(run("<body>", src), "undefined,undefined,1,1");
}

/// The script elements below `root`, template contents included.
fn scripts_below(dom: &Dom, root: NodeId, out: &mut Vec<NodeId>) {
    for n in dom.descendants(root) {
        if dom.local_name(n) == Some(atom!("script")) {
            out.push(n);
        } else if let Some(c) = dom.contents(n).filter(|_| dom.kind(n) == NodeKind::Element) {
            scripts_below(dom, c, out);
        }
    }
}

#[test]
fn scripts_made_by_markup_in_template_contents_never_run() {
    let (mut e, mut h) = page("<body>");
    let src = "const d = document.createElement('div'); document.body.appendChild(d);
d.innerHTML = '<template><script>window.a = 1<\\/script><template><script>window.b = 1<\\/script></template></template>';
return document.querySelectorAll('script').length;";
    assert_eq!(eval(&mut e, &mut h, src), "0");
    // nothing bound reaches template contents yet: move the scripts out
    // without running them, then insert them again from script
    let (doc, mut scripts) = (h.dom.document(), Vec::new());
    scripts_below(&h.dom, doc, &mut scripts);
    assert_eq!(scripts.len(), 2);
    let body = h.dom.descendants(doc).find(|&n| h.dom.local_name(n) == Some(atom!("body"))).unwrap();
    for s in scripts {
        h.dom.insert_node(body, s, None);
    }
    let src = "for (const s of document.querySelectorAll('script')) { s.remove(); document.body.appendChild(s); }
return [document.querySelectorAll('script').length, typeof window.a, typeof window.b].join();";
    assert_eq!(eval(&mut e, &mut h, src), "2,undefined,undefined");
    assert_eq!(h.dom.audit().mismatches(), 0, "{:?}", h.dom.audit());
    h.close(&mut e);
}

#[test]
fn setting_outer_html_replaces_the_element() {
    let src = "const d = document.getElementById('d'), p = d.parentNode, out = [];
d.outerHTML = '<i>1</i>2'; out.push(p.innerHTML, d.parentNode);
d.outerHTML = '<b>no parent</b>'; out.push(d.outerHTML);
try { document.documentElement.outerHTML = '<p>'; } catch (x) { out.push(x.name); }
const f = document.createDocumentFragment(), e = document.createElement('u'); f.appendChild(e);
e.outerHTML = '<td>1</td>x<tr>'; out.push(f.childNodes.length, f.firstChild.data);
return out.join('|');";
    let want = "a<i>1</i>2b||<div id=\"d\">d</div>|NoModificationAllowedError|1|1x";
    assert_eq!(run("<section>a<div id=d>d</div>b</section>", src), want);
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
fn the_nodes_markup_replaces_are_freed() {
    let (mut e, mut h) = page("<div id=d></div>");
    collect(&mut e, &mut h);
    let nodes = h.dom.node_count();
    let src = "const d = document.getElementById('d');
for (let i = 0; i < 500; i++) { d.innerHTML = '<i>' + i + '</i><b><u>x</u></b>'; if (i % 50 == 0) d.firstChild.tag = i; }
return d.childNodes.length;";
    assert_eq!(eval(&mut e, &mut h, src), "2");
    collect(&mut e, &mut h);
    assert!(h.dom.node_count() <= nodes + 6, "{} nodes left of {nodes}", h.dom.node_count());
    assert_eq!(h.dom.audit().mismatches(), 0, "{:?}", h.dom.audit());
    h.close(&mut e);
}

#[test]
fn markup_for_a_dom_one_node_below_its_limit_throws() {
    let limits = Limits { nodes: 4096, ..Limits::default() };
    let (mut e, mut h) = page_in(Dom::with_limits(limits), "<div id=d>d</div>");
    let doc = h.dom.document();
    let mut filler = Vec::new();
    while h.dom.node_count() < limits.nodes - 1 {
        let el = h.dom.new_element_in(doc, atom!("i")).unwrap();
        filler.push(h.dom.hold(el));
    }
    let src = "const d = document.getElementById('d'), out = [];
try { d.innerHTML = '<b>x</b>'; } catch (x) { out.push(x.name); }
try { d.outerHTML = '<b>x</b>'; } catch (x) { out.push(x.name); }
out.push(d.outerHTML);
return out.join('|');";
    let want = "QuotaExceededError|QuotaExceededError|<div id=\"d\">d</div>";
    assert_eq!(eval(&mut e, &mut h, src), want);
    for f in filler {
        h.dom.let_go(f);
    }
    h.settle(&mut e);
    assert_eq!(eval(&mut e, &mut h, "const d = document.getElementById('d'); d.innerHTML = '<b>x</b>'; return d.innerHTML;"), "<b>x</b>");
    assert_eq!(h.dom.audit().mismatches(), 0, "{:?}", h.dom.audit());
    h.close(&mut e);
}
