//! Insertion points: where `document.write` puts its text while a
//! script the parser stopped for runs.

use super::Input;

impl Input {
    /// A new insertion point just before the next code point.
    pub(crate) fn push_point(&mut self) {
        self.points.push(self.pos);
    }

    pub(crate) fn pop_point(&mut self) {
        self.points.pop();
    }

    pub(crate) fn has_point(&self) -> bool {
        !self.points.is_empty()
    }

    /// `bytes` at the innermost insertion point, which moves past them;
    /// the outer points after it move with the text.
    pub(crate) fn insert(&mut self, bytes: &[u8]) {
        let at = *self.points.last().expect("an insertion point");
        self.buf.splice(at..at, bytes.iter().copied());
        for p in &mut self.points {
            if *p >= at {
                *p += bytes.len();
            }
        }
    }

    /// Run `f` with the input ending at the innermost insertion point,
    /// as the tokenizer runs for `document.write`: what follows it is
    /// set aside and has not arrived.
    pub(crate) fn up_to_point<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let at = *self.points.last().expect("an insertion point");
        let tail = self.buf.split_off(at);
        let done = std::mem::replace(&mut self.done, false);
        let r = f(self);
        self.done = done;
        let cut = self.buf.len() - at;
        self.buf.extend_from_slice(&tail);
        debug_assert_eq!(cut, 0, "nothing arrives while the rest is set aside");
        r
    }
}
