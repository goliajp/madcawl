//! `document.cookie`: the embedder hands the host a store, the network
//! service's jar in the document's cookie partition, which the host
//! reads and writes as a non-HTTP API, so `HttpOnly` cookies are never
//! seen or replaced by script and the jar's rules on `Secure`, `Path`,
//! `Domain`, expiry, `SameSite` and sizes apply. A host given no store
//! keeps a document with no cookies, as a cookie-averse one.

use madcawl_dom::NodeId;
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::host::DomHost;

/// The cookie jar a document's script reads and writes.
pub trait CookieStore {
    /// The cookies script may see for `url`, as `document.cookie` shows
    /// them: `name=value` pairs joined with `; `.
    fn cookies(&mut self, url: &str) -> String;
    /// Stores the cookie of one set-cookie-string `line` received for
    /// `url` from script; the store ignores what its rules refuse.
    fn set_cookie(&mut self, url: &str, line: &str);
    /// A turn of the document's script has ended: what the store holds
    /// only while script runs may go.
    fn idle(&mut self) {}
}

impl<E: Engine> DomHost<E> {
    /// Give the document's script `store` for `document.cookie`.
    pub fn set_cookie_store(&mut self, store: Box<dyn CookieStore>) {
        self.cookies = Some(store);
    }

    /// Tell the store a turn of script has ended.
    pub fn cookies_idle(&mut self) {
        if let Some(c) = self.cookies.as_deref_mut() {
            c.idle();
        }
    }

    /// The store for `doc`, unless it is cookie-averse: a document other
    /// than the window's has no browsing context, and one whose URL is
    /// not HTTP(S) has no cookies. An HTTP(S) URL never has an opaque
    /// origin here (no sandboxing), so no SecurityError is thrown.
    fn cookie_store(&mut self, doc: NodeId) -> Option<(&mut dyn CookieStore, &str)> {
        if doc != self.dom.document() || !matches!(self.nav.url.scheme(), "http" | "https") {
            return None;
        }
        Some((self.cookies.as_deref_mut()?, self.nav.url.as_str()))
    }

    pub(crate) fn document_cookie(&mut self, doc: NodeId) -> DomString {
        match self.cookie_store(doc) {
            Some((store, url)) => DomString::from_wtf8(store.cookies(url).as_bytes()),
            None => DomString::empty(),
        }
    }

    /// Stores `value` as a set-cookie-string: converted as a USVString,
    /// lone surrogates to U+FFFD, then UTF-8. One the store would ignore
    /// for its size is dropped here.
    pub(crate) fn set_document_cookie(&mut self, doc: NodeId, value: &DomString) {
        if value.len() > MAX_LINE {
            return;
        }
        let Some((store, url)) = self.cookie_store(doc) else { return };
        let line: String = char::decode_utf16(value.units()).map(|c| c.unwrap_or('\u{FFFD}')).collect();
        if !oversized(&line) {
            store.set_cookie(url, &line);
        }
    }
}

/// The longest name and value a cookie has together, in bytes (RFC 6265bis).
const MAX_PAIR: usize = 4096;
/// The longest set-cookie-string passed on, in bytes: the pair and the
/// attributes the store reads, each value at most 1024 bytes.
const MAX_LINE: usize = 16 << 10;

/// Whether the store would ignore `line` for its size: longer than
/// `MAX_LINE`, or a name and value longer than `MAX_PAIR` together, as
/// they are once split at the first `=` and trimmed.
fn oversized(line: &str) -> bool {
    let pair = line.split(';').next().unwrap_or("");
    let (name, value) = pair.split_once('=').unwrap_or(("", pair));
    let ws = |c: char| c == ' ' || c == '\t';
    line.len() > MAX_LINE || name.trim_matches(ws).len() + value.trim_matches(ws).len() > MAX_PAIR
}

#[cfg(test)]
mod tests {
    use super::oversized;

    #[test]
    fn a_cookie_too_long_for_the_store_is_dropped_before_it_is_sent() {
        let pair = |n: usize, v: usize| format!("{}={}; Path=/", "n".repeat(n), "v".repeat(v));
        assert!(!oversized(&pair(96, 4000)));
        assert!(oversized(&pair(97, 4000)));
        assert!(!oversized(&format!("  {} ", pair(96, 4000))), "whitespace around the pair is not counted");
        assert!(oversized(&"x".repeat(4097)), "a pair with no `=` is all value");
        assert!(!oversized(&"x".repeat(4096)));
        assert!(oversized(&format!("a=b; Comment={}", "x".repeat(16 << 10))));
        assert!(!oversized(&format!("a=b; Comment={}", "x".repeat(8 << 10))));
    }
}
