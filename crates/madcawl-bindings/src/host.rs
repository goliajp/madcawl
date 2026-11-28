//! The host side of one document: the DOM, the listeners, and keeping
//! the engine told.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_js::{
    ClassDef, Deadline, Engine, ErrorKind, Host, HostRef, Member, ModuleRequest, Owned, Step,
};
use madcawl_str::{DomString, HeldAtoms};

use crate::event_loop::EventLoop;
use crate::listeners::Listener;
use crate::generated::{DocumentClass, NodeClass};

pub struct DomHost<E: Engine> {
    pub dom: Dom,
    pub realm: E::Realm,
    /// By node index.
    pub(crate) listeners: HashMap<u32, Vec<Listener<E>>>,
    /// Event types the listeners name.
    pub(crate) names: HeldAtoms,
    /// Exceptions nothing caught, reported to the host.
    pub reported: u32,
    pub(crate) freed: Vec<NodeId>,
    suspects: Vec<NodeId>,
    pub(crate) event_loop: EventLoop<E>,
    /// Script elements, the parser, the document's loading.
    pub(crate) scripts: crate::scripts::Scripts<E>,
    /// The window's listeners: the window lives as long as the host, so
    /// they are roots.
    pub(crate) window_listeners: Vec<Listener<E>>,
    /// The id of the last listener added (`listeners.rs`).
    pub(crate) next_listener: u64,
    /// Nodes moved between trees, being placed, and those the engine
    /// could not place yet (`trees.rs`).
    pub(crate) moved: Vec<NodeId>,
    /// The realm is detached: nothing is placed any more.
    pub(crate) closed: bool,
    /// Side objects whose wrapper is to be placed (again).
    pub(crate) unplaced_sides: Vec<crate::side::SideRef>,
    /// Suspects went to the engine since its last collection finished.
    pub(crate) collect_due: bool,
    /// The host objects that are not nodes (`side.rs`).
    pub(crate) sides: crate::side::Sides,
    /// The listeners of each `EventTarget` script made, members of it.
    pub(crate) side_listeners: HashMap<crate::side::SideRef, Vec<Listener<E>>>,
    /// The detail of each CustomEvent that has one and the reason of each
    /// aborted signal, a member of the side object.
    pub(crate) details: HashMap<crate::side::SideRef, Member<E>>,
    /// The wrapper of each event's target (of a controller: its signal),
    /// a member of the side object, on an engine that places wrappers
    /// (`holds.rs`).
    pub(crate) target_edges: HashMap<crate::side::SideRef, Member<E>>,
    /// The live `childNodes` (false) and `children` (true) of a node, by
    /// node index, while alive (`lists.rs`).
    pub(crate) same_lists: HashMap<(u32, bool), crate::side::SideRef>,
    /// The wrapper of each of those collections, a member of its root's
    /// wrapper, on an engine that places wrappers (`lists.rs`).
    pub(crate) list_edges: HashMap<(u32, bool), Member<E>>,
    /// How many times live collections found their nodes again.
    pub(crate) list_rebuilds: u64,
    /// What fixed collections keep of nodes that leave the document.
    pub(crate) fixed: crate::fixed::FixedLists<E>,
    /// The `Attr`s, `attributes` maps and `classList`s script has
    /// (`attr.rs`).
    pub(crate) attrs: crate::attr::Attrs<E>,
    /// The document's URL, its session history and the window's own
    /// objects (`nav.rs`).
    pub(crate) nav: crate::nav::Nav<E>,
    /// The last exceptions and navigations not followed (`notes.rs`).
    pub(crate) notes: crate::notes::Notes,
    /// Unix milliseconds when the event loop's clock started: the time
    /// origin `performance` counts from.
    pub(crate) time_origin_ms: f64,
    /// Where `document.cookie` goes (`cookie.rs`).
    pub(crate) cookies: Option<Box<dyn crate::cookie::CookieStore>>,
    /// Whether CSSStyleDeclaration's property attributes are defined
    /// (`css_props.rs`).
    pub(crate) css_properties: bool,
    /// The nonce of each HTML element script set one on, by node index,
    /// until its content attribute changes.
    pub(crate) nonces: HashMap<u32, DomString>,
    /// The child navigables (`frames.rs`).
    pub(crate) frames: crate::frames::Frames<E>,
}

impl<E: Engine> DomHost<E> {
    /// A host for `dom` in a new realm of `e`, which has no other live
    /// realm: the engine hands every wrapper notice and cycle candidate
    /// of the isolate to the host that settles, which must be the one
    /// whose nodes they are. A host of a closed realm is no obstacle. The
    /// realms of the host's child navigables are its own (`frames.rs`).
    ///
    /// The host is ended by [`DomHost::close`], never just dropped: the
    /// nodes, collections and events script still holds keep counts and
    /// handles only `close` gives back, and a debug build panics when a
    /// host is dropped while it still holds nodes.
    pub fn new(e: &mut E, dom: Dom) -> Self {
        let () = E::VALUE_IS_ONE_WORD;
        assert_eq!(e.live_realms(), 0, "one live host per isolate");
        let mut h = DomHost {
            dom,
            realm: e.new_realm(),
            listeners: HashMap::new(),
            names: HeldAtoms::new(),
            reported: 0,
            freed: Vec::new(),
            suspects: Vec::new(),
            event_loop: EventLoop::new(),
            scripts: crate::scripts::Scripts::new(),
            window_listeners: Vec::new(),
            next_listener: 0,
            moved: Vec::new(),
            unplaced_sides: Vec::new(),
            closed: false,
            collect_due: false,
            sides: Default::default(),
            details: HashMap::new(),
            target_edges: HashMap::new(),
            side_listeners: HashMap::new(),
            same_lists: HashMap::new(),
            list_edges: HashMap::new(),
            list_rebuilds: 0,
            fixed: Default::default(),
            attrs: Default::default(),
            nav: crate::nav::Nav::new(),
            notes: Default::default(),
            cookies: None,
            css_properties: false,
            nonces: HashMap::new(),
            frames: crate::frames::Frames::new(),
            time_origin_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("a clock after 1970").as_secs_f64() * 1e3,
        };
        if E::PLACES_WRAPPERS {
            h.dom.track_moves();
        }
        h
    }

    pub fn node(&self, obj: HostRef) -> NodeId {
        self.dom.node_at(obj.addr()).expect("a live node of this DOM")
    }

    pub fn host_ref(&self, n: NodeId) -> HostRef {
        HostRef::new(self.dom.address(n)).expect("a node address is never 0")
    }

    /// The wrapper of `n`, made if it has none or the one it has is
    /// dead, with a count for the caller.
    pub fn wrap(&mut self, e: &mut E, n: NodeId) -> Owned<E::Value> {
        let w = self.dom.wrapper(n);
        if w != 0 {
            if let Some(v) = e.wrapper_upgrade(w & !1) {
                return v;
            }
            // the engine sends no notice for a wrapper it refused
            self.let_go_of_wrapper(e, n);
        }
        let obj = self.host_ref(n);
        let realm = self.realm_for(n);
        let v = e.new_wrapper(realm, self.class_of(obj), obj);
        let word = E::wrapper_word(v.get());
        self.dom.set_wrapper(n, word);
        if realm != self.realm {
            self.frames.wrapped.insert(n.index(), (word, self.window_for_doc(self.dom.owner_document(n))));
        }
        self.place_new(e, n, word);
        v
    }

    /// Pass on to the engine what changed: deliver its notices of dead
    /// wrappers, free the nodes no one counts, drop what they held, and
    /// hand over the nodes that left a document still counted. Called
    /// after every call into the engine and every tree change.
    pub fn settle(&mut self, e: &mut E) {
        loop {
            // a side object freed last round may have let go of a held node
            self.regroup(e);
            let delivered = e.drain_host(self);
            self.free_dead();
            let freed = std::mem::take(&mut self.freed);
            for &n in &freed {
                self.forget_script(n);
                if !self.nonces.is_empty() {
                    self.nonces.remove(&n.index());
                }
                if !self.frames.wrapped.is_empty() {
                    self.frames.wrapped.remove(&n.index());
                }
                for mut l in self.take_listeners(n) {
                    l.callback.clear(e);
                }
                e.forget(self.host_ref(n));
            }
            self.dom.take_suspects(&mut self.suspects);
            self.collect_due |= !self.suspects.is_empty();
            for n in std::mem::take(&mut self.suspects) {
                e.suspect(self.host_ref(n));
            }
            let sides = self.settle_sides(e);
            if delivered == 0 && freed.is_empty() && !sides {
                break;
            }
            self.freed = freed;
            self.freed.clear();
        }
        self.settle_fixed(e);
    }

    /// Run the cycle collector until `deadline`.
    pub fn collect(&mut self, e: &mut E, deadline: Deadline) -> Step {
        self.settle(e);
        let step = e.collect_cycles(self, deadline);
        self.collect_due = step == Step::More;
        self.settle(e);
        step
    }

    /// Give back everything this host holds in the engine: listeners,
    /// preserved wrappers (of nodes in the document and out of it), the
    /// realm. What script still holds is the engine's to free; the
    /// engine tells no host about this realm's wrappers again.
    pub fn close(mut self, e: &mut E) {
        self.shut(e);
    }

    pub(crate) fn shut(&mut self, e: &mut E) {
        self.close_frames(e);
        self.close_loop(e);
        self.close_scripts(e);
        self.close_window(e);
        for (_, list) in std::mem::take(&mut self.listeners) {
            for mut l in list {
                l.callback.close(e);
            }
        }
        // wrappers made since the last settle are preserved by it
        self.settle(e);
        self.close_sides(e);
        for n in self.dom.wrapped() {
            let w = self.dom.wrapper(n);
            if w & 1 != 0 {
                self.dom.set_wrapper_preserved(n, false);
                // SAFETY: the count the preserved edge held
                e.release(unsafe { Owned::from_raw(E::wrapper_value(w & !1)) });
            }
        }
        self.closed = true;
        e.detach_realm(self.realm);
        self.settle(e);
    }
}

impl<E: Engine> Host<E> for DomHost<E> {
    fn class_of(&self, obj: HostRef) -> &'static ClassDef<E, Self> {
        if let Some(r) = self.sides.of(obj) {
            return self.sides.get(r).data.class::<E>();
        }
        match self.dom.kind(self.node(obj)) {
            NodeKind::Document => &DocumentClass::<E>::DEF,
            NodeKind::Element => crate::elements::class_of::<E>(&self.dom, self.node(obj)),
            NodeKind::Text => &crate::generated::TextClass::<E>::DEF,
            NodeKind::Comment => &crate::generated::CommentClass::<E>::DEF,
            NodeKind::Fragment => &crate::generated::DocumentFragmentClass::<E>::DEF,
            NodeKind::ProcessingInstruction => &crate::generated::CharacterDataClass::<E>::DEF,
            NodeKind::Doctype => &NodeClass::<E>::DEF,
        }
    }

    fn report_exception(&mut self, e: &mut E, exception: Owned<E::Value>) {
        self.reported += 1;
        self.record_exception(e, exception.get());
        e.release(exception);
    }

    fn load_imported_module(&mut self, e: &mut E, _: &ModuleRequest<'_>, payload: E::ModulePayload) {
        let err = e.throw_error(ErrorKind::Type, "modules are not supported yet");
        let _sentinel = err.into_raw();
        let ex = e.take_exception().expect("just thrown");
        e.finish_loading_module(self, payload, ex);
    }
}
