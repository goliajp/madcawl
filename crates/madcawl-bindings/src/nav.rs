//! The document's URL and its session history, as Location and History
//! read and change them, and the window's own objects. The host has no
//! navigation of its own: a navigation script asks for that would leave
//! the document (a URL that differs in more than its fragment, a
//! reload, a traversal) is noted for the embedder
//! ([`DomHost::take_notes`]) and not followed. A fragment
//! navigation and `pushState` / `replaceState` change the URL in place,
//! with no `hashchange` or `popstate` event.

use std::fmt;

use alt_url::Url;
use madcawl_js::{Engine, Root};

use crate::host::DomHost;
use crate::notes::Note;
use crate::side::SideRef;

/// The longest URL a recorded navigation keeps.
const MAX_URL: usize = 2048;

/// What a navigation script asked for and the host did not follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Navigation {
    /// `location.href = url`, `location.assign(url)`
    Assign(String),
    /// `location.replace(url)`
    Replace(String),
    /// `location.reload()`, `history.go(0)`
    Reload,
    /// `history.go(delta)`, `back()`, `forward()`
    Traverse(i32),
}

impl fmt::Display for Navigation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Navigation::Assign(u) => write!(f, "assign {u}"),
            Navigation::Replace(u) => write!(f, "replace {u}"),
            Navigation::Reload => f.write_str("reload"),
            Navigation::Traverse(d) => write!(f, "traverse {d:+}"),
        }
    }
}

pub(crate) struct Nav<E: Engine> {
    pub url: Url,
    /// The session history's entries and the current one's index: this
    /// document's own, with no entries of other documents
    pub length: u32,
    pub index: u32,
    /// The current entry's state, as script gave it: not a structured
    /// clone
    pub state: Option<Root<E>>,
    /// The window's Location, History, Crypto and Performance, each held
    /// by a count from install to the host's end
    pub objects: Option<[SideRef; 4]>,
}

impl<E: Engine> Nav<E> {
    pub fn new() -> Self {
        Nav {
            url: Url::parse("about:blank").expect("a valid URL"),
            length: 1,
            index: 0,
            state: None,
            objects: None,
        }
    }
}

fn cut(u: &Url) -> String {
    let s = u.as_str();
    if s.len() <= MAX_URL {
        return s.to_owned();
    }
    let mut end = MAX_URL;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// `u` without its fragment.
pub(crate) fn without_fragment(u: &Url) -> &str {
    let s = u.as_str();
    &s[..s.find('#').unwrap_or(s.len())]
}

/// Whether a document at `doc` can have its URL rewritten to `target`,
/// as the HTML Standard has it.
pub(crate) fn can_rewrite(doc: &Url, target: &Url) -> bool {
    let parts = |u: &Url| (u.scheme().to_owned(), u.username().to_owned(), u.password().map(str::to_owned), u.host_str().map(str::to_owned), u.port());
    if parts(doc) != parts(target) {
        return false;
    }
    match target.scheme() {
        "http" | "https" => true,
        "file" => doc.path() == target.path(),
        _ => without_fragment(doc) == without_fragment(target),
    }
}

impl<E: Engine> DomHost<E> {
    /// The document's URL, `about:blank` until the embedder sets it.
    pub fn url(&self) -> &str {
        self.nav.url.as_str()
    }

    /// Make `url`, the URL the document was fetched from after its
    /// redirects, the document's URL; `false` when it does not parse.
    pub fn set_url(&mut self, url: &str) -> bool {
        match Url::parse(url) {
            Ok(u) => {
                self.nav.url = u;
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn record_navigation(&mut self, n: Navigation) {
        self.note(Note::Navigation(n));
    }

    /// A new session history entry after the current one, which drops
    /// those after it, with no state.
    pub(crate) fn push_entry(&mut self, e: &mut E) {
        self.nav.index = self.nav.index.saturating_add(1);
        self.nav.length = self.nav.index.saturating_add(1);
        self.set_state(e, None);
    }

    pub(crate) fn set_state(&mut self, e: &mut E, v: Option<E::Value>) {
        let new = v.map(|v| Root::new(e.retain(v)));
        if let Some(old) = std::mem::replace(&mut self.nav.state, new) {
            old.release(e);
        }
    }

    /// The Location-object navigate of the HTML Standard, with no
    /// navigation of the host's own: a fragment navigation changes the
    /// URL in place, in a new entry unless it replaces (asked to, the
    /// document still loading, or the URL the same); anything else is
    /// noted.
    pub(crate) fn location_navigate(&mut self, e: &mut E, url: Url, replace: bool) {
        let replace = replace || !self.is_loaded() || url == self.nav.url;
        if url.fragment().is_some() && without_fragment(&url) == without_fragment(&self.nav.url) {
            if !replace {
                self.push_entry(e);
            }
            self.nav.url = url;
            return;
        }
        let s = cut(&url);
        self.record_navigation(if replace { Navigation::Replace(s) } else { Navigation::Assign(s) });
    }

    pub(crate) fn close_nav(&mut self, e: &mut E) {
        self.set_state(e, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn a_url_without_its_fragment_keeps_everything_before_the_hash() {
        assert_eq!(without_fragment(&u("https://a.test/p?q#f")), "https://a.test/p?q");
        assert_eq!(without_fragment(&u("https://a.test/p#")), "https://a.test/p");
        assert_eq!(without_fragment(&u("https://a.test/p")), "https://a.test/p");
    }

    #[test]
    fn only_a_url_of_the_same_origin_and_credentials_can_be_rewritten_to() {
        let doc = u("https://a.test/x?y#z");
        assert!(can_rewrite(&doc, &u("https://a.test/other?q")));
        assert!(!can_rewrite(&doc, &u("https://b.test/x")));
        assert!(!can_rewrite(&doc, &u("http://a.test/x")));
        assert!(!can_rewrite(&doc, &u("https://a.test:8443/x")));
        assert!(!can_rewrite(&doc, &u("https://me@a.test/x")));
        assert!(can_rewrite(&u("file:///a/b"), &u("file:///a/b?q")));
        assert!(!can_rewrite(&u("file:///a/b"), &u("file:///a/c")));
        assert!(can_rewrite(&u("about:blank"), &u("about:blank#x")));
        assert!(!can_rewrite(&u("about:blank"), &u("about:srcdoc")));
    }
}
