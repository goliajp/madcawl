//! Reading a property of an element's `style` again, while its style
//! attribute is unchanged, makes no heap block on the Rust side: the
//! declarations are not parsed again and the value is the one served
//! before.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_js_jsc::Jsc;
use madcawl_str::DomString;

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: passes every call on to the system allocator
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// The Rust heap blocks made while `src` runs.
fn allocs(e: &mut Jsc, h: &mut DomHost<Jsc>, src: &str) -> usize {
    let url = DomString::from_wtf8(b"alloc");
    let src = DomString::from_wtf8(src.as_bytes());
    let realm = h.realm;
    let before = ALLOCS.load(Ordering::Relaxed);
    let r = e.evaluate(h, realm, &src, &ScriptOrigin { url: &url, line: 1, column: 0 });
    let n = ALLOCS.load(Ordering::Relaxed) - before;
    assert!(!Jsc::is_thrown(r.get()));
    e.release(r);
    n
}

#[test]
fn reading_an_unchanged_style_again_makes_no_heap_block() {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.use_virtual_time();
    h.parse_document(&mut e, b"<p id=a style='display: none; margin: 1px 2px'>x</p>");
    allocs(&mut e, &mut h, "var s = document.getElementById('a').style, k = 0; s.display; s.margin; s.width;");
    let few = allocs(&mut e, &mut h, "for (let i = 0; i < 10; i++) k += s.display.length + s.margin.length + s.width.length;");
    let many = allocs(&mut e, &mut h, "for (let i = 0; i < 10010; i++) k += s.display.length + s.margin.length + s.width.length;");
    assert_eq!(many, few, "30000 more reads, {} more blocks", many - few);
    // a change makes the next read serve a new value, once
    let changed = allocs(&mut e, &mut h, "s.display = 'block'; for (let i = 0; i < 10; i++) k += s.display.length;");
    assert!(changed > 0);
    let again = allocs(&mut e, &mut h, "for (let i = 0; i < 10010; i++) k += s.display.length;");
    let base = allocs(&mut e, &mut h, "for (let i = 0; i < 10; i++) k += s.display.length;");
    assert_eq!(again, base);
    h.close(&mut e);
}
