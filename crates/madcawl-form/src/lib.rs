//! Form submission without script: which controls a form owns and what
//! they hold, the entry list built from them, its encodings, and the
//! request a submission makes. Nothing here touches the network: the
//! caller sends the request (as a fetch through the network process).
//!
//! Controls hold what their markup says unless `Controls` gives their
//! current state (a value typed, a box ticked, an option chosen, files
//! picked); the DOM keeps no control state of its own.
//!
//! Not covered: encodings other than UTF-8 (`accept-charset` is not
//! consulted), hard wrapping of `textarea` lines, constraint validation,
//! and the date and time types' value sanitization.

mod encode;
mod entries;
mod node;
mod submit;
mod value;

pub use encode::{Files, boundary, multipart, normalize_newlines, pairs, text_plain, urlencoded};
pub use entries::{CHARSET, entry_list, owned_controls, password_controls};
pub use node::{FormOwners, form_owner, is_submit_button};
pub use submit::{Context, Enctype, Method, Submission, submission};

use madcawl_dom::NodeId;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileValue {
    pub filename: String,
    pub content_type: String,
    /// where the bytes are; `None` for the empty entry of a file input
    /// with no file chosen
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    File(FileValue),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub value: Value,
}

impl Entry {
    /// The text value, or a file's name.
    pub fn text(&self) -> &str {
        match &self.value {
            Value::Text(t) => t,
            Value::File(f) => &f.filename,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormError {
    NotAForm,
    /// the submitter is not a submit button
    NotASubmitButton,
    /// `method=dialog` closes a dialog and makes no request
    Dialog,
    BadAction(String),
    /// an action whose scheme makes no request here (`mailto:`, `data:`…)
    Scheme(String),
    FileNotAllowed(PathBuf),
    File(PathBuf, String),
    /// the body is over the caller's cap
    TooLarge,
}

/// The current state of controls where it differs from their markup;
/// `None` means "as the markup says".
pub trait Controls {
    fn value(&self, _n: NodeId) -> Option<String> {
        None
    }
    fn checked(&self, _n: NodeId) -> Option<bool> {
        None
    }
    fn selected(&self, _n: NodeId) -> Option<bool> {
        None
    }
    fn files(&self, _n: NodeId) -> Option<Vec<FileValue>> {
        None
    }
}

/// Every control as its markup says.
pub struct Markup;

impl Controls for Markup {}

/// Control state set by the host, one map per kind.
#[derive(Default)]
pub struct State {
    pub values: HashMap<NodeId, String>,
    pub checked: HashMap<NodeId, bool>,
    pub selected: HashMap<NodeId, bool>,
    pub files: HashMap<NodeId, Vec<FileValue>>,
}

impl Controls for State {
    fn value(&self, n: NodeId) -> Option<String> {
        self.values.get(&n).cloned()
    }
    fn checked(&self, n: NodeId) -> Option<bool> {
        self.checked.get(&n).copied()
    }
    fn selected(&self, n: NodeId) -> Option<bool> {
        self.selected.get(&n).copied()
    }
    fn files(&self, n: NodeId) -> Option<Vec<FileValue>> {
        self.files.get(&n).cloned()
    }
}
