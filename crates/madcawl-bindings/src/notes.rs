//! What script did that the embedder is told of and the host did not
//! act on: exceptions nothing caught and navigations not followed, in
//! the order they happened, the last [`MAX_NOTES`] of them. Kept for the
//! embedder, which decides where they go: they may hold URLs, and so
//! what a page put in them.

use std::collections::VecDeque;
use std::fmt;

use madcawl_js::Engine;

use crate::host::DomHost;
use crate::nav::Navigation;
use crate::report::ScriptError;

/// Notes a host keeps; older ones are dropped.
pub const MAX_NOTES: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    Exception(ScriptError),
    Navigation(Navigation),
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Note::Exception(x) => write!(f, "uncaught exception: {x}"),
            Note::Navigation(n) => write!(f, "navigation not followed: {n}"),
        }
    }
}

#[derive(Default)]
pub(crate) struct Notes(VecDeque<Note>);

impl<E: Engine> DomHost<E> {
    /// The notes since the last call, oldest first.
    pub fn take_notes(&mut self) -> Vec<Note> {
        self.notes.0.drain(..).collect()
    }

    pub(crate) fn note(&mut self, n: Note) {
        if self.notes.0.len() == MAX_NOTES {
            self.notes.0.pop_front();
        }
        self.notes.0.push_back(n);
    }
}
