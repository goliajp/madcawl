//! The blob URL store: what `URL.createObjectURL` made and not yet
//! revoked, each entry the window's that made it. A window holds at most
//! `MAX_PER_WINDOW`, and its entries go with it (`frames.rs`) and with
//! the host.

use std::collections::HashMap;

use alt_url::{Origin, Url};

use crate::frames::same_origin;
use crate::impls::blob::BlobData;

/// Blob URLs one window may hold at once.
pub(crate) const MAX_PER_WINDOW: usize = 4096;

#[derive(Default)]
pub(crate) struct BlobUrls {
    /// By URL, without a fragment: the window, and the blob.
    entries: HashMap<String, (u64, BlobData)>,
    per_window: HashMap<u64, usize>,
}

impl BlobUrls {
    /// A new blob URL of `origin` (serialized) for `blob`, of window `w`;
    /// `None` once `w` holds as many as it may.
    pub(crate) fn add(&mut self, origin: String, w: u64, blob: BlobData) -> Option<String> {
        let n = self.per_window.entry(w).or_default();
        if *n == MAX_PER_WINDOW {
            return None;
        }
        *n += 1;
        let mut b = [0u8; 16];
        crate::random::fill(&mut b);
        let url = format!("blob:{origin}/{}", crate::impls::crypto::uuid(b));
        self.entries.insert(url.clone(), (w, blob));
        Some(url)
    }

    /// Revoke `url` when it is one of `origin`'s.
    pub(crate) fn revoke(&mut self, url: &str, origin: &Origin) {
        let Ok(u) = Url::parse(url) else { return };
        if u.scheme() != "blob" || !same_origin(&u.origin(), origin) {
            return;
        }
        let key = crate::nav::without_fragment(&u);
        if let Some((w, _)) = self.entries.remove(key) {
            self.forget_one(w);
        }
    }

    fn forget_one(&mut self, w: u64) {
        if let Some(n) = self.per_window.get_mut(&w) {
            *n -= 1;
            if *n == 0 {
                self.per_window.remove(&w);
            }
        }
    }

    /// The blob `url` (fragment ignored) stands for.
    pub(crate) fn get(&self, url: &str) -> Option<&BlobData> {
        let u = Url::parse(url).ok()?;
        self.entries.get(crate::nav::without_fragment(&u)).map(|(_, b)| b)
    }

    /// Window `w` went: its blob URLs go with it.
    pub(crate) fn drop_window(&mut self, w: u64) {
        if self.per_window.remove(&w).is_some() {
            self.entries.retain(|_, (owner, _)| *owner != w);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.per_window.clear();
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

impl<E: madcawl_js::Engine> crate::host::DomHost<E> {
    /// The bytes and type of the blob `url` stands for, while it is not
    /// revoked: what a fetch or a worker of a blob URL reads.
    pub fn blob_url(&self, url: &str) -> Option<(&[u8], &str)> {
        self.blob_urls.get(url).map(|b| (b.bytes(), b.ty.as_str()))
    }

    /// How many blob URLs are not revoked.
    pub fn blob_url_count(&self) -> usize {
        self.blob_urls.len()
    }
}
