//! What a script the parser stopped for may do to the input:
//! `document.write` text at the insertion point.

use super::{Sink, Step, Tokenizer};

impl Tokenizer {
    /// A script the parser stopped for starts running: what it writes
    /// goes just before the next code point.
    pub fn begin_script(&mut self) {
        self.input.push_point();
    }

    /// The script is done: the insertion point it had goes away.
    pub fn end_script(&mut self) {
        self.input.pop_point();
    }

    /// Whether `document.write` has somewhere to put its text: a script
    /// the parser stopped for is running.
    pub fn has_insertion_point(&self) -> bool {
        self.input.has_point()
    }

    /// `document.write`: WTF-8 bytes at the insertion point. Tokenize
    /// them with [`Tokenizer::run_inserted`].
    pub fn insert(&mut self, bytes: &[u8]) {
        self.input.insert(bytes);
    }

    /// Tokenize up to the insertion point, as `document.write` does
    /// after inserting: `NeedInput` once the inserted text is read,
    /// `Paused` at a script inside it.
    pub fn run_inserted<S: Sink>(&mut self, sink: &mut S) -> Step {
        let mut input = std::mem::take(&mut self.input);
        let step = input.up_to_point(|i| {
            std::mem::swap(&mut self.input, i);
            let step = self.run(sink);
            std::mem::swap(&mut self.input, i);
            step
        });
        self.input = input;
        step
    }
}
