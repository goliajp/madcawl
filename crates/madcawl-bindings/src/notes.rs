//! What script did that the embedder is told of and the host did not
//! act on: exceptions nothing caught and navigations not followed, in
//! the order they happened, the last [`MAX_NOTES`] of them. Kept for the
//! embedder, which decides where they go: they may hold URLs, and so
//! what a page put in them.

use std::collections::VecDeque;
use std::fmt;

use madcawl_js::{AbortCause, Engine, Halt};

use crate::host::DomHost;
use crate::nav::Navigation;
use crate::report::ScriptError;

/// Notes a host keeps; older ones are dropped.
pub const MAX_NOTES: usize = 32;
/// Script stopped in a promise job the engine ran as a call returned.
const JOB_STOPPED: &str = "script stopped: a promise job ran past the time limit; \
                           the jobs queued after it were dropped";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    Exception(ScriptError),
    Navigation(Navigation),
    /// An iframe got no navigable: as many as a host keeps were alive.
    FrameLimit,
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Note::Exception(x) => write!(f, "uncaught exception: {x}"),
            Note::Navigation(n) => write!(f, "navigation not followed: {n}"),
            Note::FrameLimit => write!(f, "iframe left without a document: {} are alive", crate::frames::MAX_FRAMES),
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

    /// Script the engine aborted, once no script is on the stack:
    /// reported as an exception nothing caught, and the isolate resumed,
    /// so the page's next script runs; whether it was.
    pub(crate) fn recover(&mut self, e: &mut E) -> bool {
        if e.in_script() || e.halt() != Halt::Aborted {
            return false;
        }
        let message = match e.abort_cause() {
            AbortCause::Asked => "script stopped",
            AbortCause::TimeLimit => "script stopped: it ran past the time limit",
            AbortCause::TimeLimitInJob => JOB_STOPPED,
        }
        .to_owned();
        self.note(Note::Exception(ScriptError { message, source: String::new(), line: 0, column: 0 }));
        e.resume().expect("an engine that aborts resumes");
        true
    }

    /// A microtask checkpoint, recovered from an abort in it.
    pub(crate) fn checkpoint(&mut self, e: &mut E) {
        e.run_microtasks(self);
        self.recover(e);
    }

    pub(crate) fn note(&mut self, n: Note) {
        if self.notes.0.len() == MAX_NOTES {
            self.notes.0.pop_front();
        }
        self.notes.0.push_back(n);
    }
}
