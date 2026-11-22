//! A count of the tokenizer's work on this thread: every byte a token
//! spans, and one for each token, so a byte tokenized again (a rule body
//! parsed after it was skipped, a declaration tried again as a rule)
//! counts each time. The fuzz driver reads it to find inputs whose work
//! grows faster than their length.

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
