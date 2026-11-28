//! Event listener lists, a node's or the window's, as the DOM Standard's
//! "add an event listener" and "remove an event listener" keep them.
//!
//! A node's callbacks are members of the node ([`Member::of`]): on an
//! engine that keeps members beside the wrapper they die with it, so a
//! listener whose closure holds its own detached node does not keep the
//! node. The window lives as long as the host, so its callbacks are
//! plain counts. Each listener has an id of its own: a dispatch that
//! snapshotted the list skips one removed meanwhile, also when an equal
//! one was added again (the standard's removed flag). An event handler
//! (`onabort`) is a listener of the list too, in the place it was first
//! set, which `removeEventListener` does not find.

use madcawl_dom::NodeId;
use madcawl_js::{Engine, Key, Member, Owned, ValueKind};
use madcawl_str::{Atom, DomString, atom};

use crate::convert::{IntoJs, Thrown, side};
use crate::event::Target;
use crate::generated::AbortSignalClass;
use crate::host::DomHost;
use crate::side::SideRef;

pub(crate) struct Listener<E: Engine> {
    pub ty: Atom,
    pub capture: bool,
    /// Removed before its first call.
    pub once: bool,
    /// Its calls cannot cancel the event.
    pub passive: bool,
    pub id: u64,
    /// An event handler's: called with no `handleEvent`, and a result of
    /// false cancels the event.
    pub handler: bool,
    pub callback: Member<E>,
}

/// The options of an add or a remove, flattened.
pub(crate) struct Options {
    pub capture: bool,
    pub once: bool,
    pub passive: Option<bool>,
    pub signal: Option<SideRef>,
}

/// The third argument: a boolean (capture), or an options dictionary
/// whose members are read in the order Web IDL converts it (the
/// inherited `capture`, then the rest by name).
pub(crate) fn options<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value, add: bool) -> Result<Options, Thrown> {
    let mut o = Options { capture: false, once: false, passive: None, signal: None };
    if !matches!(e.kind(v), ValueKind::Object | ValueKind::Function) {
        o.capture = e.to_boolean(v);
        return Ok(o);
    }
    let names: &[&str] = if add { &["capture", "once", "passive", "signal"] } else { &["capture"] };
    for &name in names {
        let x = e.get(h, v, Key::Str(&DomString::from_wtf8(name.as_bytes())));
        if E::is_thrown(x.get()) {
            let _sentinel = x.into_raw();
            return Err(Thrown);
        }
        let (kind, b) = (e.kind(x.get()), e.to_boolean(x.get()));
        let signal = match name {
            "signal" if kind != ValueKind::Undefined => side(e, h, x.get(), &AbortSignalClass::<E>::DEF).map(Some),
            _ => Ok(None),
        };
        e.release(x);
        match name {
            "capture" => o.capture = b,
            "once" => o.once = b,
            "passive" if kind != ValueKind::Undefined => o.passive = Some(b),
            _ => o.signal = signal?,
        }
    }
    Ok(o)
}

/// `addEventListener` on `t` with options `o`: none when the signal is
/// aborted already, and one the signal removes when it aborts.
pub(crate) fn add<E: Engine>(e: &mut E, h: &mut DomHost<E>, t: Target, ty: Atom, cb: E::Value, o: &Options) {
    if o.signal.is_some_and(|s| h.signal(s).aborted) {
        return;
    }
    if let Some(id) = h.add_listener(e, t, ty, cb, o)
        && let Some(s) = o.signal
    {
        h.remove_on_abort(s, t, id);
    }
}

impl<E: Engine> DomHost<E> {
    pub(crate) fn listeners_of(&self, n: NodeId) -> impl Iterator<Item = &Listener<E>> {
        self.listeners.get(&n.index()).into_iter().flatten()
    }

    pub(crate) fn take_listeners(&mut self, n: NodeId) -> Vec<Listener<E>> {
        self.listeners.remove(&n.index()).unwrap_or_default()
    }

    pub(crate) fn list_of(&self, t: Target) -> &[Listener<E>] {
        match t {
            Target::Node(n) => self.listeners.get(&n.index()).map_or(&[], Vec::as_slice),
            Target::Side(r) => self.side_listeners.get(&r).map_or(&[], Vec::as_slice),
            Target::Window(0) => &self.window_listeners,
            Target::Window(w) => self.frames.listeners(w),
        }
    }

    /// Whether listener `id` is still on `t`'s list.
    pub(crate) fn listening(&self, t: Target, id: u64) -> bool {
        self.list_of(t).iter().any(|l| l.id == id)
    }

    /// Add a listener unless an equal one is on the list; its id.
    pub(crate) fn add_listener(&mut self, e: &mut E, t: Target, ty: Atom, cb: E::Value, o: &Options) -> Option<u64> {
        let same = |l: &Listener<E>| !l.handler && l.ty == ty && l.capture == o.capture && l.callback.get() == Some(cb);
        if self.list_of(t).iter().any(same) {
            return None;
        }
        let passive = o.passive.unwrap_or_else(|| self.passive_by_default(t, ty));
        let callback = self.member_of(e, t, cb);
        self.next_listener += 1;
        let id = self.next_listener;
        let l = Listener { ty, capture: o.capture, once: o.once, passive, id, handler: false, callback };
        self.list_mut(t).push(l);
        Some(id)
    }

    /// `cb` held by `t`'s list: a member of the target, or for the window
    /// a plain count.
    fn member_of(&mut self, e: &mut E, t: Target, cb: E::Value) -> Member<E> {
        match t {
            // held beside the target's wrapper
            Target::Node(_) | Target::Side(_) => {
                let owner = t.into_js(e, self);
                let m = Member::of(e, owner.get(), cb);
                e.release(owner);
                m
            }
            Target::Window(_) => Member::new(e.retain(cb)),
        }
    }

    fn list_mut(&mut self, t: Target) -> &mut Vec<Listener<E>> {
        match t {
            Target::Node(n) => self.listeners.entry(n.index()).or_default(),
            Target::Side(r) => self.side_listeners.entry(r).or_default(),
            Target::Window(0) => &mut self.window_listeners,
            Target::Window(w) => self.frames.listeners_mut(w),
        }
    }

    /// The standard's default passive value: touch and wheel listeners
    /// on the window, the document, its element and its body.
    fn passive_by_default(&self, t: Target, ty: Atom) -> bool {
        let kinds = [atom!("touchstart"), atom!("touchmove"), atom!("wheel")];
        if !kinds.contains(&ty) && !(ty.as_static().is_none() && spelled(ty, b"mousewheel")) {
            return false;
        }
        let n = match t {
            Target::Node(n) => n,
            Target::Window(_) => return true,
            Target::Side(_) => return false,
        };
        let doc = self.dom.owner_document(n);
        let root = self.dom.children(doc).find(|&c| self.dom.kind(c) == madcawl_dom::NodeKind::Element);
        let body = root.and_then(|r| self.dom.children(r).find(|&c| self.dom.local_name(c) == Some(atom!("body"))));
        n == doc || Some(n) == root || Some(n) == body
    }

    pub(crate) fn remove_listener(&mut self, e: &mut E, t: Target, ty: Atom, capture: bool, cb: E::Value) {
        self.remove_where(e, t, |l| !l.handler && l.ty == ty && l.capture == capture && l.callback.get() == Some(cb));
    }

    /// Remove listener `id` of `t`, if it is still there.
    pub(crate) fn remove_listener_id(&mut self, e: &mut E, t: Target, id: u64) {
        self.remove_where(e, t, |l| l.id == id);
    }

    fn remove_where(&mut self, e: &mut E, t: Target, at: impl Fn(&Listener<E>) -> bool) {
        let list = match t {
            Target::Node(n) => self.listeners.get_mut(&n.index()),
            Target::Side(r) => self.side_listeners.get_mut(&r),
            Target::Window(0) => Some(&mut self.window_listeners),
            Target::Window(w) => self.frames.live.get_mut(&w).map(|f| &mut f.listeners),
        };
        let Some(list) = list else { return };
        let Some(i) = list.iter().position(at) else { return };
        let mut l = list.remove(i);
        if list.is_empty() {
            match t {
                Target::Node(n) => drop(self.listeners.remove(&n.index())),
                Target::Side(r) => drop(self.side_listeners.remove(&r)),
                Target::Window(_) => {}
            }
        }
        l.callback.clear(e);
    }

    /// The event handler of type `ty` on `t`: its value, or null.
    pub(crate) fn event_handler(&mut self, e: &mut E, t: Target, ty: Atom) -> Owned<E::Value> {
        match self.list_of(t).iter().find(|l| l.handler && l.ty == ty).and_then(|l| l.callback.get()) {
            Some(v) => e.retain(v),
            // SAFETY: null carries no count
            None => unsafe { Owned::from_raw(E::null()) },
        }
    }

    /// Set the event handler of type `ty` on `t`: one already set keeps
    /// its place in the list, null takes it out, and one set after that
    /// goes at the end.
    pub(crate) fn set_event_handler(&mut self, e: &mut E, t: Target, ty: Atom, v: Option<E::Value>) {
        let Some(v) = v else {
            return self.remove_where(e, t, |l| l.handler && l.ty == ty);
        };
        let m = self.member_of(e, t, v);
        if let Some(l) = self.list_mut(t).iter_mut().find(|l| l.handler && l.ty == ty) {
            let mut old = std::mem::replace(&mut l.callback, m);
            return old.clear(e);
        }
        self.next_listener += 1;
        let id = self.next_listener;
        let passive = self.passive_by_default(t, ty);
        let l = Listener { ty, capture: false, once: false, passive, id, handler: true, callback: m };
        self.list_mut(t).push(l);
    }
}

fn spelled(a: Atom, name: &[u8]) -> bool {
    let mut b = Vec::new();
    a.write_wtf8(&mut b);
    b == name
}
