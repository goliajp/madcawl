//! A count of the parser's work on this thread: every code point read
//! (again after it was put back, too) and every entry looked at in a
//! scan of the open elements, the active formatting elements or a tag's
//! attributes. The fuzz driver reads it to find inputs whose work grows
//! faster than their length.

use std::cell::Cell;

thread_local! {
    static STEPS: Cell<u64> = const { Cell::new(0) };
}

pub(crate) fn add(n: u64) {
    STEPS.with(|s| s.set(s.get() + n));
}

/// The count since the last call, and start again from zero.
pub fn take() -> u64 {
    STEPS.with(|s| s.replace(0))
}
