//! What the event interfaces besides Event and CustomEvent carry: a
//! ProgressEvent's counts, a MessageEvent's origin (its data, like a
//! CustomEvent's detail, is a member in the host's `details`), and an
//! ErrorEvent's message and place (its error is in `details` too).

use madcawl_js::{ClassDef, Engine};
use madcawl_str::DomString;

use crate::event::EventState;
use crate::host::DomHost;

pub(crate) enum Extra {
    None,
    Progress { computable: bool, loaded: f64, total: f64 },
    Message { origin: DomString },
    Error(Box<ErrorPlace>),
}

#[derive(Clone)]
pub(crate) struct ErrorPlace {
    pub message: DomString,
    pub filename: DomString,
    pub line: u32,
    pub column: u32,
}

impl EventState {
    /// The class of an event's wrapper.
    pub(crate) fn class<E: Engine>(&self) -> &'static ClassDef<E, DomHost<E>> {
        use crate::generated as g;
        match &self.extra {
            _ if self.custom => &g::CustomEventClass::<E>::DEF,
            Extra::Progress { .. } => &g::ProgressEventClass::<E>::DEF,
            Extra::Message { .. } => &g::MessageEventClass::<E>::DEF,
            Extra::Error(_) => &g::ErrorEventClass::<E>::DEF,
            Extra::None => &g::EventClass::<E>::DEF,
        }
    }
}
