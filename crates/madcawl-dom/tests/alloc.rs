//! The parser's insert path allocates nothing on the heap: a node
//! without attributes or text takes a slot and links, nothing else.
//! Text appended to an existing text node grows one buffer, so its
//! allocations grow with the logarithm of the text, not with the number
//! of appends.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use madcawl_dom::Dom;
use madcawl_html::{Attrs, Namespace, TreeSink, atom};

struct Counting;

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: forwards to the system allocator, counting on the way
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.with(|c| c.set(c.get() + 1));
        // SAFETY: the caller's contract, passed on
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, passed on
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.with(|c| c.set(c.get() + 1));
        // SAFETY: the caller's contract, passed on
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn allocs() -> usize {
    ALLOCS.with(Cell::get)
}

#[test]
fn creating_and_inserting_elements_allocates_nothing() {
    let mut dom = Dom::new();
    let doc = dom.document();
    let body = dom.create_element(Namespace::Html, atom!("body"), Attrs::empty());
    dom.append(doc, body);
    let before = allocs();
    let mut parent = body;
    for i in 0..10_000 {
        let el = dom.create_element(Namespace::Html, atom!("div"), Attrs::empty());
        dom.append(parent, el);
        if i % 100 == 0 {
            parent = el;
        }
    }
    let c = dom.create_comment(b"");
    dom.insert_before(body, c, dom.first_child(body).unwrap());
    assert_eq!(allocs() - before, 0, "heap allocations while inserting");
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn appending_to_an_existing_text_node_allocates_rarely() {
    let mut dom = Dom::new();
    let doc = dom.document();
    let body = dom.create_element(Namespace::Html, atom!("body"), Attrs::empty());
    dom.append(doc, body);
    dom.insert_text(body, None, b"start");
    let before = allocs();
    for _ in 0..10_000 {
        dom.insert_text(body, None, b"abcd");
    }
    let n = allocs() - before;
    assert!(n <= 24, "{n} allocations for 10 000 appends");
    let text = dom.last_child(body).unwrap();
    assert_eq!(dom.text(text).unwrap().len(), 5 + 40_000);
}
