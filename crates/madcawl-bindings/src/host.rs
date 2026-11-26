//! The host side of one document: the DOM, the listeners, and keeping
//! the engine told.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_js::{
    ClassDef, Deadline, Engine, ErrorKind, Halt, Host, HostRef, Member, ModuleRequest, Owned, Root, Step,
};
use madcawl_str::{Atom, HeldAtoms};

use crate::event_loop::EventLoop;
use crate::listeners::Listener;
use crate::generated::{DocumentClass, ElementClass, NodeClass};

pub struct DomHost<E: Engine> {
    pub dom: Dom,
    pub realm: E::Realm,
    /// By node index.
    pub(crate) listeners: HashMap<u32, Vec<Listener<E>>>,
    /// Event types the listeners name.
    pub(crate) names: HeldAtoms,
    /// Exceptions nothing caught, reported to the host.
    pub reported: u32,
    freed: Vec<NodeId>,
    suspects: Vec<NodeId>,
    pub(crate) event_loop: EventLoop<E>,
    /// The parser, while `parse_document` runs.
    pub(crate) parser: Option<Box<crate::parse::Parser>>,
    pub(crate) loader: Option<crate::parse::Loader>,
    /// The window's listeners: the window lives as long as the host, so
    /// they are roots.
    pub(crate) window_listeners: Vec<Listener<E>>,
    /// The id of the last listener added (`listeners.rs`).
    pub(crate) next_listener: u64,
    /// Nodes moved between trees, being placed, and those the engine
    /// could not place yet (`trees.rs`).
    pub(crate) moved: Vec<NodeId>,
    /// The roots of the detached trees placed as held by a dispatch in
    /// progress (`trees.rs`).
    pub(crate) pinned: std::collections::HashSet<NodeId>,
    /// The events being dispatched, innermost last.
    pub(crate) dispatching: Vec<crate::side::SideRef>,
    /// A dispatch began or ended since the trees were last placed.
    pub(crate) pins_changed: bool,
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
    /// The detail of each CustomEvent that has one, a member of the event.
    pub(crate) details: HashMap<crate::side::SideRef, Member<E>>,
    /// The wrapper of each event's target, a member of the event, on an
    /// engine that places wrappers (`event.rs`).
    pub(crate) target_edges: HashMap<crate::side::SideRef, Member<E>>,
}

impl<E: Engine> DomHost<E> {
    /// A host for `dom` in a new realm of `e`, which has no other live
    /// realm: the engine hands every wrapper notice and cycle candidate
    /// of the isolate to the host that settles, which must be the one
    /// whose nodes they are. A host of a closed realm is no obstacle.
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
            parser: None,
            loader: None,
            window_listeners: Vec::new(),
            next_listener: 0,
            moved: Vec::new(),
            pinned: Default::default(),
            dispatching: Vec::new(),
            pins_changed: false,
            unplaced_sides: Vec::new(),
            closed: false,
            collect_due: false,
            sides: Default::default(),
            details: HashMap::new(),
            target_edges: HashMap::new(),
            side_listeners: HashMap::new(),
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
        let v = e.new_wrapper(self.realm, self.class_of(obj), obj);
        let word = E::wrapper_word(v.get());
        self.dom.set_wrapper(n, word);
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
            self.dom.free_dead(&mut self.freed);
            let freed = std::mem::take(&mut self.freed);
            for &n in &freed {
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
    }

    /// Run the cycle collector until `deadline`.
    pub fn collect(&mut self, e: &mut E, deadline: Deadline) -> Step {
        self.settle(e);
        let step = e.collect_cycles(self, deadline);
        self.collect_due = step == Step::More;
        self.settle(e);
        step
    }

    /// Call the listeners of type `ty` on `target`, each held by a root
    /// handle for the call, with a microtask checkpoint after each when
    /// no script is on the stack (a dispatch the user started, not one
    /// script started). The dispatch stops when script is being ended.
    /// The host's own firing, not yet an Event dispatch (that is
    /// `dispatchEvent`'s, `event.rs`): the listener gets the type as its
    /// argument.
    pub fn fire(&mut self, e: &mut E, target: NodeId, ty: Atom) -> usize {
        let roots: Vec<Root<E>> = self
            .listeners_of(target)
            .filter(|l| l.ty == ty)
            .filter_map(|l| l.callback.get())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|cb| Root::new(e.retain(cb)))
            .collect();
        let this = self.wrap(e, target);
        let arg = e.string(ty.to_dom_string());
        let checkpoints = !e.in_script();
        let (mut called, mut ended) = (0, false);
        for r in roots {
            if !ended {
                called += 1;
                let res = e.call(self, r.get(), this.get(), &[arg.get()]);
                if !E::is_thrown(res.get()) {
                    e.release(res);
                } else {
                    let _sentinel = res.into_raw();
                    ended = e.halt() != Halt::Running;
                    if !ended {
                        let ex = e.take_exception().expect("a thrown exception");
                        self.report_exception(e, ex);
                    }
                }
                // halted script skips the task's checkpoint
                if checkpoints && !ended {
                    e.run_microtasks(self);
                }
            }
            r.release(e);
        }
        e.release(this);
        e.release(arg);
        self.settle(e);
        called
    }

    pub fn listener_count(&self, n: NodeId) -> usize {
        self.listeners_of(n).count()
    }

    /// Give back everything this host holds in the engine: listeners,
    /// preserved wrappers (of nodes in the document and out of it), the
    /// realm. What script still holds is the engine's to free; the
    /// engine tells no host about this realm's wrappers again.
    pub fn close(mut self, e: &mut E) {
        self.close_loop(e);
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
            return side_class::<E>(&self.sides.get(r).data);
        }
        match self.dom.kind(self.node(obj)) {
            NodeKind::Document => &DocumentClass::<E>::DEF,
            NodeKind::Element => &ElementClass::<E>::DEF,
            NodeKind::Text => &crate::generated::TextClass::<E>::DEF,
            NodeKind::Comment => &crate::generated::CommentClass::<E>::DEF,
            NodeKind::Fragment => &crate::generated::DocumentFragmentClass::<E>::DEF,
            NodeKind::ProcessingInstruction => &crate::generated::CharacterDataClass::<E>::DEF,
            NodeKind::Doctype => &NodeClass::<E>::DEF,
        }
    }

    fn report_exception(&mut self, e: &mut E, exception: Owned<E::Value>) {
        self.reported += 1;
        e.release(exception);
    }

    fn load_imported_module(&mut self, e: &mut E, _: &ModuleRequest<'_>, payload: E::ModulePayload) {
        let err = e.throw_error(ErrorKind::Type, "modules are not supported yet");
        let _sentinel = err.into_raw();
        let ex = e.take_exception().expect("just thrown");
        e.finish_loading_module(self, payload, ex);
    }
}

/// The class of a side object.
pub(crate) fn side_class<E: Engine>(data: &crate::side::SideData) -> &'static ClassDef<E, DomHost<E>> {
    match data {
        crate::side::SideData::Event(ev) if ev.custom => &crate::generated::CustomEventClass::<E>::DEF,
        crate::side::SideData::Event(_) => &crate::generated::EventClass::<E>::DEF,
        crate::side::SideData::Target => &crate::generated::EventTargetClass::<E>::DEF,
        crate::side::SideData::DomException(_) => &crate::generated::DOMExceptionClass::<E>::DEF,
    }
}
