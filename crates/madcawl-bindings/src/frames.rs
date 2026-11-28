//! Child navigables: an `iframe` connected to a document that has a
//! window gets a navigable of its own, with its document in this host's
//! DOM and its window in a realm of its own in the same isolate, the
//! agent cluster they share. Script outside the iframe reaches that
//! window through a WindowProxy (`contentWindow`, the document's
//! `defaultView`, `parent` below it); the window's own script reaches
//! its global, so `contentWindow.window === contentWindow` is false (an
//! engine's WindowProxy is not the global). The top-level window never
//! navigates within a host, so it is its global everywhere.
//!
//! A navigable shows `about:blank` at once, `load` fired at the iframe
//! as it is inserted; `srcdoc` loads in a task, and a `src` of the
//! parent's origin through the embedder ([`DomHost::take_frame_requests`],
//! [`DomHost::frame_fetched`]). A document of another origin (a `src`
//! elsewhere, `sandbox` without `allow-same-origin`) is not loaded:
//! `contentDocument` is null and `contentWindow` a stand-in with the
//! members the standard lets other origins reach (`frame_window.rs`).
//! The first document loaded keeps the window of the initial
//! `about:blank`, as the standard does for one of the same origin. A
//! navigable loading `srcdoc` or `src` delays the `load` of the document
//! its iframe is in until its own iframe's `load`, which waits in turn
//! for the navigables of its document.
//!
//! Not done yet: scripts in these documents (marked started, never run,
//! as with scripting disabled), the child windows' `location`,
//! `history`, `crypto` and `performance`, navigating again when `src` or
//! `srcdoc` changes, `frames[i]`, `name`
//! and `postMessage` (a window of another origin takes a message and
//! delivers nothing). A navigable whose iframe moves within its document
//! during one host call is kept, not made anew.
//!
//! The navigable goes once its iframe is out of that document, checked
//! after every host call: its documents are emptied, its windows'
//! timers and listeners dropped, the wrappers of its realm let go of the
//! nodes still in its document (`frame_wrappers.rs`), and the realm
//! detached, so what script still holds of it throws on use.

use std::collections::HashMap;

use alt_url::{Origin, Url};
use madcawl_dom::{HeldNode, NodeId};
use madcawl_js::{Engine, Halt, Root};

use crate::host::DomHost;
use crate::listeners::Listener;
use crate::notes::Note;

/// Child navigables alive at once in one host, nested ones included:
/// each is a realm, so this bounds what one page can make.
pub(crate) const MAX_FRAMES: usize = 128;

/// What a navigable's next document is waiting for.
pub(crate) enum Load {
    Nothing,
    Srcdoc(Vec<u8>),
    Fetching,
    Fetched(Url, Vec<u8>),
    /// In place; its iframe's `load` waits for the navigables below.
    Parsed,
}

pub(crate) struct Frame<E: Engine> {
    /// The iframe, held while the navigable lives.
    pub element: HeldNode,
    /// The active document, held while it is.
    pub doc: HeldNode,
    pub realm: E::Realm,
    pub proxy: Root<E>,
    /// What script of another origin gets for the window, once asked.
    pub stub: Option<Root<E>>,
    /// The window of the iframe's document, 0 being the top-level one.
    pub parent: u64,
    pub url: Url,
    /// The document's origin is its parent's.
    pub same_origin: bool,
    pub listeners: Vec<Listener<E>>,
    pub load: Load,
    /// The initial `about:blank` once a loaded document replaced it,
    /// held while the navigable lives: its nodes are this window's.
    pub former: Option<HeldNode>,
}

pub(crate) struct Frames<E: Engine> {
    /// By window id, from 1, never used again.
    pub live: HashMap<u64, Frame<E>>,
    pub by_element: HashMap<u32, u64>,
    pub by_doc: HashMap<u32, u64>,
    /// Each live navigable's `former` document, to the window.
    pub former: HashMap<u32, u64>,
    /// The nodes whose wrapper belongs to a child window's realm, with
    /// its word and the window, until the wrapper or the node goes
    /// (`frame_wrappers.rs`).
    pub wrapped: HashMap<u32, (u64, u64)>,
    next: u64,
    /// The DOM's tree version when the iframes were last checked.
    version: u64,
    /// The `src` documents to fetch, not yet taken by the embedder: at
    /// most one for each live navigable, whose going drops it, so never
    /// more than `MAX_FRAMES`.
    pub requests: Vec<(u64, String)>,
}

impl<E: Engine> Frames<E> {
    pub(crate) fn new() -> Self {
        Frames {
            live: HashMap::new(),
            by_element: HashMap::new(),
            by_doc: HashMap::new(),
            former: HashMap::new(),
            wrapped: HashMap::new(),
            next: 0,
            version: 0,
            requests: Vec::new(),
        }
    }

    pub(crate) fn listeners(&self, w: u64) -> &[Listener<E>] {
        self.live.get(&w).map_or(&[], |f| f.listeners.as_slice())
    }

    pub(crate) fn listeners_mut(&mut self, w: u64) -> &mut Vec<Listener<E>> {
        &mut self.live.get_mut(&w).expect("a live window's list").listeners
    }
}

pub(crate) fn blank() -> Url {
    Url::parse("about:blank").expect("a valid URL")
}

/// `about:blank` or `about:srcdoc`, whose document takes its origin and
/// base URL from its parent.
pub(crate) fn inherits(u: &Url) -> bool {
    u.scheme() == "about" && matches!(u.path(), "blank" | "srcdoc")
}

pub(crate) fn same_origin(a: &Origin, b: &Origin) -> bool {
    a.is_tuple() && b.is_tuple() && a.ascii_serialization() == b.ascii_serialization()
}

impl<E: Engine> DomHost<E> {
    /// The window whose document `doc` is: 0 for this host's own.
    pub(crate) fn window_of_doc(&self, doc: NodeId) -> Option<u64> {
        if doc == self.dom.document() { Some(0) } else { self.frames.by_doc.get(&doc.index()).copied() }
    }

    pub(crate) fn doc_of_window(&self, w: u64) -> Option<NodeId> {
        if w == 0 { Some(self.dom.document()) } else { self.frames.live.get(&w).map(|f| f.doc.id()) }
    }

    /// The realm of window `w`, while it lives.
    pub(crate) fn realm_of(&self, w: u64) -> Option<E::Realm> {
        if w == 0 { Some(self.realm) } else { self.frames.live.get(&w).map(|f| f.realm) }
    }

    /// Window `w` as script outside it gets it: its WindowProxy, the
    /// top-level global, or null once it is gone.
    pub(crate) fn window_value(&mut self, e: &mut E, w: u64) -> madcawl_js::Owned<E::Value> {
        match self.frames.live.get(&w) {
            _ if w == 0 => e.global(self.realm),
            Some(f) => e.retain(f.proxy.get()),
            None => crate::convert::plain::<E>(E::null()),
        }
    }

    pub(crate) fn origin_of(&self, w: u64) -> Origin {
        let Some(f) = self.frames.live.get(&w) else { return self.nav.url.origin() };
        match () {
            _ if !f.same_origin => Origin::Opaque,
            _ if inherits(&f.url) => self.origin_of(f.parent),
            _ => f.url.origin(),
        }
    }

    /// The base URL of frame document `doc`, or `None` for a document
    /// that is no navigable's.
    pub(crate) fn frame_base_url(&self, doc: NodeId) -> Option<&Url> {
        let f = &self.frames.live[self.frames.by_doc.get(&doc.index())?];
        if !inherits(&f.url) {
            return Some(&f.url);
        }
        let parent = self.doc_of_window(f.parent).expect("a frame's parent outlives it");
        Some(self.base_url(parent))
    }

    /// Give each of `iframes` that is connected to a document with a
    /// window, and has no navigable, one; in order, each loaded as far
    /// as it loads at once before the next.
    pub(crate) fn frames_connected(&mut self, e: &mut E, iframes: Vec<NodeId>) {
        for el in iframes {
            if let Some(w) = self.new_frame(e, el) {
                self.process_frame_attributes(e, w);
            }
        }
    }

    fn new_frame(&mut self, e: &mut E, el: NodeId) -> Option<u64> {
        let parent = self.window_of_doc(self.dom.owner_document(el))?;
        if !self.dom.is_connected(el) || self.frames.by_element.contains_key(&el.index()) || e.halt() != Halt::Running {
            return None;
        }
        if self.frames.live.len() == MAX_FRAMES {
            self.note(Note::FrameLimit);
            return None;
        }
        let doc = self.dom.new_active_document(true)?;
        let realm = e.new_realm();
        let proxy = e.new_window_proxy(realm);
        if E::is_thrown(proxy.get()) {
            // script was being stopped: no window, as if never inserted
            let _sentinel = proxy.into_raw();
            e.detach_realm(realm);
            self.dom.empty_document(doc.id());
            self.dom.let_go(doc);
            return None;
        }
        self.frames.next += 1;
        let w = self.frames.next;
        self.frames.by_element.insert(el.index(), w);
        self.frames.by_doc.insert(doc.id().index(), w);
        let element = self.dom.hold(el);
        let proxy = Root::new(proxy);
        let f = Frame { element, doc, realm, proxy, stub: None, parent, url: blank(), same_origin: true, listeners: Vec::new(), load: Load::Nothing, former: None };
        self.frames.live.insert(w, f);
        self.install_frame_window(e, w);
        Some(w)
    }

    /// Let the navigables go whose iframe left their document; cheap
    /// while the tree did not change.
    pub(crate) fn check_frames(&mut self, e: &mut E) {
        if self.frames.live.is_empty() || self.dom.tree_version() == self.frames.version {
            return;
        }
        let gone: Vec<u64> = self
            .frames
            .live
            .iter()
            .filter(|(_, f)| {
                let el = f.element.id();
                !self.dom.is_connected(el) || self.doc_of_window(f.parent) != Some(self.dom.owner_document(el))
            })
            .map(|(&w, _)| w)
            .collect();
        for w in gone {
            self.destroy_frame(e, w);
        }
        self.frames.version = self.dom.tree_version();
    }

    /// Destroy navigable `w`, those below it first.
    pub(crate) fn destroy_frame(&mut self, e: &mut E, w: u64) {
        let below: Vec<u64> = self.frames.live.iter().filter(|(_, f)| f.parent == w).map(|(&k, _)| k).collect();
        for k in below {
            self.destroy_frame(e, k);
        }
        let Some(f) = self.frames.live.remove(&w) else { return };
        if !matches!(f.load, Load::Nothing) {
            self.frame_settled(f.parent);
        }
        self.frames.by_element.remove(&f.element.id().index());
        self.frames.by_doc.remove(&f.doc.id().index());
        self.frames.requests.retain(|&(id, _)| id != w);
        for mut l in f.listeners {
            l.callback.clear(e);
        }
        for t in self.event_loop.remove_window_timers(w) {
            t.handler.release(e);
            t.args.into_iter().for_each(|a| a.release(e));
        }
        self.let_go_of_realm_wrappers(e, w, f.doc.id());
        if let Some(old) = f.former {
            self.frames.former.remove(&old.id().index());
            self.dom.let_go(old);
        }
        f.proxy.release(e);
        if let Some(s) = f.stub {
            s.release(e);
        }
        e.detach_realm(f.realm);
        self.dom.empty_document(f.doc.id());
        self.dom.let_go(f.doc);
        self.dom.let_go(f.element);
    }

    /// Every navigable goes: the host is closing.
    pub(crate) fn close_frames(&mut self, e: &mut E) {
        let top: Vec<u64> = self.frames.live.iter().filter(|(_, f)| f.parent == 0).map(|(&w, _)| w).collect();
        for w in top {
            self.destroy_frame(e, w);
        }
        self.frames.requests.clear();
    }
}
