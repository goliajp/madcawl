//! The tokenizer allocates nothing: a token is spans of the source.
//! The parser allocates only the stack it counts nesting with, once.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use madcawl_css::{Kind, Parser, Tokenizer};

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

const SHEET: &str = r#"
@charset "utf-8";
@import url("a.css") screen;
@media (min-width: 40em) { .a\:b > #id[href^='x' i] { color: rgb(1 2 3 / 50%) !important } }
/* comment */ a:not(.b, .c):nth-child(2n+1 of .d) { background: url( x\(y.png ) no-repeat; margin: -0.5e1px 3% }
.e { content: "a\"b\
c"; font: 12px/1.5 "x y", sans-serif; unicode-range: U+0025-00FF, u+4??; --x: { a: b } }
<!-- --> .f ~ .g + .h || .i |= .j { width: calc(100% - 2 * var(--x, 1px)) }
"#;

#[test]
fn tokenizing_allocates_nothing() {
    let src = SHEET.repeat(50);
    let before = allocs();
    let mut tok = Tokenizer::new(&src);
    let mut n = 0;
    let mut seen_escape = false;
    loop {
        let t = tok.next_token();
        if t.kind == Kind::Eof {
            break;
        }
        // decoding a value reads the source, it writes nowhere
        seen_escape |= t.is(madcawl_css::ESCAPED) && t.chars(&src).count() > 0;
        n += 1;
    }
    assert_eq!(allocs() - before, 0, "allocations for {n} tokens");
    assert!(n > 5000 && seen_escape);
}

#[test]
fn parsing_rules_and_bodies_allocates_once() {
    let src = SHEET.repeat(50);
    let before = allocs();
    let mut p = Parser::whole(&src);
    let mut rules = 0;
    while let Some(item) = p.next_rule(true) {
        if let madcawl_css::BlockItem::Rule(madcawl_css::ParsedRule::Qualified { block, .. }) = item
        {
            let mut body = Parser::new(&src, block);
            while body.next_block_item().is_some() {}
            rules += 1;
        }
    }
    // the nesting stack of the sheet's parser and of each body's
    assert_eq!(rules, 150);
    assert!(
        allocs() - before <= 1 + rules,
        "{} allocations",
        allocs() - before
    );
}
