//! Dropped collections and the trees they hold are freed by a full
//! collection, batch after batch, on a page that keeps two others. The
//! test has a binary of its own, so one engine runs in the process: with
//! several engines on threads of one process the framework now and then
//! keeps one dropped tree past every later full collection, also when
//! the other threads call nothing but the framework's own C interface;
//! with one engine per process it has not been seen. Here no tree may be
//! left after any batch, and one that is names its round.

use madcawl_bindings::DomHost;
use madcawl_dom::{Dom, NodeId};
use madcawl_js::{Deadline, Engine, ScriptOrigin, Step};
use madcawl_js_jsc::Jsc;
use madcawl_str::{Atom, DomString};

#[test]
fn a_collection_keeps_its_root_tree_and_dropped_ones_are_freed_after_every_batch() {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, b"<p>x</p>");
    let (nodes, sides) = (h.dom.node_count(), h.side_objects());
    let src = "window.kept = (function () { var d = document.createElement('div'); d.x = 'kept'; d.appendChild(document.createElement('i')); return d.children; })();
window.found = (function () { var d = document.createElement('div'); d.appendChild(document.createElement('i')).y = 'found'; return d.querySelectorAll('i'); })();
return 'ok';";
    assert_eq!(eval(&mut e, &mut h, src), "ok");
    // the two kept collections and their trees: two nodes each
    let kept = (4, 2);
    for batch in 0..6 {
        let src = format!(
            "for (let i = 0; i < 100; i++) {{ const d = document.createElement('div'); d.setAttribute('data-round', 'b{batch}r' + i);
d.appendChild(document.createElement('i')); d.childNodes.length; d.children[0]; d.getElementsByTagName('i').length; d.querySelectorAll('i')[0]; }}
return 'ok';"
        );
        assert_eq!(eval(&mut e, &mut h, &src), "ok");
        collect_everything(&mut e, &mut h);
        let left = (h.dom.node_count() - nodes, h.side_objects() - sides);
        assert_eq!(left, kept, "(nodes, collections) left after batch {batch}; rounds alive: {:?}", surviving_rounds(&h));
    }
    assert_eq!(eval(&mut e, &mut h, "return [kept[0].parentNode.x, found[0].y].join();"), "kept,found");
    assert_eq!(h.dom.audit().mismatches(), 0);
    h.close(&mut e);
}

fn eval(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> String {
    let url = DomString::from_wtf8(b"residue");
    let realm = h.realm;
    let src = format!("String((function () {{ {src} }})())");
    let r = e.evaluate(h, realm, &DomString::from_wtf8(src.as_bytes()), &ScriptOrigin { url: &url, line: 1, column: 0 });
    assert!(!Jsc::is_thrown(r.get()), "{src} threw");
    let mut b = Vec::new();
    e.as_string(r.get()).expect("a string").write_wtf8(&mut b);
    e.release(r);
    h.settle(e);
    String::from_utf8(b).unwrap()
}

fn collect_everything(e: &mut Jsc, h: &mut DomHost<Jsc>) {
    for _ in 0..3 {
        scrub();
        e.collect_now();
        while h.collect(e, Deadline::Work(u32::MAX)) != Step::Done {}
    }
}

/// The `data-round` tags of the trees still alive.
fn surviving_rounds(h: &DomHost<Jsc>) -> Vec<String> {
    let Some(tag) = Atom::lookup(b"data-round") else { return Vec::new() };
    (1..1 << 20)
        .filter_map(NodeId::from_index)
        .filter(|&n| h.dom.is_live(n))
        .filter_map(|n| {
            h.dom.attribute(n, tag).map(|v| {
                let mut b = Vec::new();
                v.write_wtf8(&mut b);
                String::from_utf8_lossy(&b).into_owned()
            })
        })
        .collect()
}

/// Overwrite the stack below the caller, so no stale value there keeps
/// a dropped wrapper alive through the conservative stack scan.
#[inline(never)]
fn scrub() {
    let pad = [0u8; 512 * 1024];
    std::hint::black_box(&pad);
}
