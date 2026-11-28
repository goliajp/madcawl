//! Lookups between the windows of a host (0 the top-level one, a child
//! navigable's from 1, `frames.rs`), their documents and their realms.

use alt_url::Origin;
use madcawl_dom::NodeId;
use madcawl_js::{Engine, ErrorKind};

use crate::convert::{Thrown, throw};
use crate::frames::inherits;
use crate::host::DomHost;

impl<E: Engine> DomHost<E> {
    /// The standard's current realm: that of the host function being
    /// run. A TypeError once that function's window is gone: nothing is
    /// made in a detached realm, nor in another in its place.
    pub(crate) fn caller_realm(&self, e: &mut E) -> Result<E::Realm, Thrown> {
        e.current_realm().ok_or_else(|| throw(e, ErrorKind::Type, "the window is gone"))
    }

    /// The window whose document `doc` is: 0 for this host's own.
    pub(crate) fn window_of_doc(&self, doc: NodeId) -> Option<u64> {
        if doc == self.dom.document() { Some(0) } else { self.frames.by_doc.get(&doc.index()).copied() }
    }

    pub(crate) fn doc_of_window(&self, w: u64) -> Option<NodeId> {
        if w == 0 { Some(self.dom.document()) } else { self.frames.live.get(&w).map(|f| f.doc.id()) }
    }

    /// The realm of window or worker `w`, while it lives.
    pub(crate) fn realm_of(&self, w: u64) -> Option<E::Realm> {
        match self.frames.live.get(&w) {
            _ if w == 0 => Some(self.realm),
            Some(f) => Some(f.realm),
            None => self.workers.live.get(&w).map(|k| k.realm),
        }
    }

    /// Whether window or worker `w` lives, and a worker has not closed.
    pub(crate) fn window_alive(&self, w: u64) -> bool {
        w == 0 || self.frames.live.contains_key(&w) || self.workers.live.get(&w).is_some_and(|k| !k.closing)
    }

    /// The window or worker whose realm is `realm`, while it lives.
    pub(crate) fn window_of_realm(&self, realm: E::Realm) -> Option<u64> {
        if realm == self.realm {
            return Some(0);
        }
        let frame = self.frames.live.iter().find(|(_, f)| f.realm == realm).map(|(&w, _)| w);
        frame.or_else(|| self.workers.live.iter().find(|(_, k)| k.realm == realm).map(|(&w, _)| w))
    }

    /// Window `w` as script outside it gets it: its WindowProxy, the
    /// top-level global, a worker's global, or null once it is gone.
    pub(crate) fn window_value(&mut self, e: &mut E, w: u64) -> madcawl_js::Owned<E::Value> {
        match self.frames.live.get(&w) {
            _ if w == 0 => e.global(self.realm),
            Some(f) => e.retain(f.proxy.get()),
            None => match self.workers.live.get(&w) {
                Some(k) => e.global(k.realm),
                None => crate::convert::plain::<E>(E::null()),
            },
        }
    }

    pub(crate) fn origin_of(&self, w: u64) -> Origin {
        if let Some(k) = self.workers.live.get(&w) {
            return k.origin.clone();
        }
        let Some(f) = self.frames.live.get(&w) else { return self.nav.url.origin() };
        match () {
            _ if !f.same_origin => Origin::Opaque,
            _ if inherits(&f.url) => self.origin_of(f.parent),
            _ => f.url.origin(),
        }
    }

    /// The URL of the document whose origin the requests of window or
    /// worker `w` are made with: its own, its parent's for one that
    /// inherits its origin, its owner's for a worker; `None` when that
    /// origin is opaque.
    pub(crate) fn initiator_of(&self, w: u64) -> Option<String> {
        if let Some(k) = self.workers.live.get(&w) {
            return self.initiator_of(k.owner);
        }
        let url = match self.frames.live.get(&w) {
            None => &self.nav.url,
            Some(f) if !f.same_origin => return None,
            Some(f) if inherits(&f.url) => return self.initiator_of(f.parent),
            Some(f) => &f.url,
        };
        url.origin().is_tuple().then(|| url.as_str().to_owned())
    }
}
