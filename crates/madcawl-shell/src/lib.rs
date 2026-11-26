//! The windowless host's library: loading a local page, styling it, and
//! reading it back as a line-oriented structure an agent can read and
//! later act on by reference (`read`), with the checks that what the
//! cascade hides stays out of it (`audit`); and painting it offscreen
//! to a PNG (`shot`); and serving windowless pages to agents over a
//! Unix socket (`serve`); and fetching a page over the network
//! (`fetch`).

pub mod audit;
pub mod fetch;
pub mod load;
pub mod read;
pub mod rusage;
pub mod serve;
pub mod sheet_files;
pub mod shot;
pub mod shown;

pub use read::{Item, Kind, Read, read};
pub use shown::Shown;
