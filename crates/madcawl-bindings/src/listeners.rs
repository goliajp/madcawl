//! Event listener lists, a node's or the window's, as the DOM Standard's
//! "add an event listener" and "remove an event listener" keep them.
//!
//! A node's callbacks are members of the node ([`Member::of`]): on an
//! engine that keeps members beside the wrapper they die with it, so a
//! listener whose closure holds its own detached node does not keep the
//! node. The window lives as long as the host, so its callbacks are
//! plain counts. Each listener has an id of its own: a dispatch that
//! snapshotted the list skips one removed meanwhile, also when an equal
//! one was added again (the standard's removed flag).

use madcawl_dom::NodeId;
use madcawl_js::{Engine, ErrorKind, Key, Member, ValueKind};
use madcawl_str::{Atom, DomString, atom};

use crate::convert::{Thrown, throw};
use crate::event::Target;
use crate::host::DomHost;

pub(crate) struct Listener<E: Engine> {
    pub ty: Atom,
    pub capture: bool,
    /// Removed before its first call.
    pub once: bool,
    /// Its calls cannot cancel the event.
    pub passive: bool,
    pub id: u64,
    pub callback: Member<E>,
}

/// The options of an add or a remove, flattened.
pub(crate) struct Options {
    pub capture: bool,
    pub once: bool,
    pub passive: Option<bool>,
}

/// The third argument: a boolean (capture), or an options dictionary
/// whose members are read in the order Web IDL converts it (the
/// inherited `capture`, then the rest by name). No `AbortSignal` is
/// bound yet, so a `signal` that is present is of the wrong type.
pub(crate) fn options<E: Engine>(e: &mut E, h: &mut DomHost<E>, v: E::Value, add: bool) -> Result<Options, Thrown> {
    let mut o = Options { capture: false, once: false, passive: None };
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
        e.release(x);
        match name {
            "capture" => o.capture = b,
            "once" => o.once = b,
            "passive" if kind != ValueKind::Undefined => o.passive = Some(b),
            "signal" if kind != ValueKind::Undefined => {
                return Err(throw(e, ErrorKind::Type, "the signal is not an AbortSignal"));
            }
            _ => {}
        }
    }
    Ok(o)
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
            Target::Window => &self.window_listeners,
        }
    }

    /// Whether listener `id` is still on `t`'s list.
    pub(crate) fn listening(&self, t: Target, id: u64) -> bool {
        self.list_of(t).iter().any(|l| l.id == id)
    }

    pub(crate) fn add_listener(&mut self, e: &mut E, t: Target, ty: Atom, cb: E::Value, o: &Options) {
        let same = |l: &Listener<E>| l.ty == ty && l.capture == o.capture && l.callback.get() == Some(cb);
        if self.list_of(t).iter().any(same) {
            return;
        }
        let passive = o.passive.unwrap_or_else(|| self.passive_by_default(t, ty));
        self.next_listener += 1;
        let id = self.next_listener;
        let callback = match t {
            Target::Node(n) => {
                // held beside the node's wrapper
                let owner = self.wrap(e, n);
                let m = Member::of(e, owner.get(), cb);
                e.release(owner);
                m
            }
            Target::Window => Member::new(e.retain(cb)),
        };
        let l = Listener { ty, capture: o.capture, once: o.once, passive, id, callback };
        match t {
            Target::Node(n) => self.listeners.entry(n.index()).or_default().push(l),
            Target::Window => self.window_listeners.push(l),
        }
    }

    /// The standard's default passive value: touch and wheel listeners
    /// on the window, the document, its element and its body.
    fn passive_by_default(&self, t: Target, ty: Atom) -> bool {
        let kinds = [atom!("touchstart"), atom!("touchmove"), atom!("wheel")];
        if !kinds.contains(&ty) && !(ty.as_static().is_none() && spelled(ty, b"mousewheel")) {
            return false;
        }
        let Target::Node(n) = t else { return true };
        let doc = self.dom.document();
        let root = self.dom.children(doc).find(|&c| self.dom.kind(c) == madcawl_dom::NodeKind::Element);
        let body = root.and_then(|r| self.dom.children(r).find(|&c| self.dom.local_name(c) == Some(atom!("body"))));
        n == doc || Some(n) == root || Some(n) == body
    }

    pub(crate) fn remove_listener(&mut self, e: &mut E, t: Target, ty: Atom, capture: bool, cb: E::Value) {
        let at = |l: &Listener<E>| l.ty == ty && l.capture == capture && l.callback.get() == Some(cb);
        let list = match t {
            Target::Node(n) => match self.listeners.get_mut(&n.index()) {
                Some(list) => list,
                None => return,
            },
            Target::Window => &mut self.window_listeners,
        };
        let Some(i) = list.iter().position(at) else { return };
        let mut l = list.remove(i);
        if let Target::Node(n) = t
            && list.is_empty()
        {
            self.listeners.remove(&n.index());
        }
        l.callback.clear(e);
    }
}

fn spelled(a: Atom, name: &[u8]) -> bool {
    let mut b = Vec::new();
    a.write_wtf8(&mut b);
    b == name
}
