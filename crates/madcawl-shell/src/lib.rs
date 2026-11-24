//! The windowless host's library: loading a local page, styling it, and
//! reading it back as a line-oriented structure an agent can read and
//! later act on by reference (`read`), with the checks that what the
//! cascade hides stays out of it (`audit`).

pub mod audit;
pub mod load;
pub mod read;
pub mod rusage;
pub mod shown;

pub use read::{Item, Kind, Read, read};
pub use shown::Shown;
