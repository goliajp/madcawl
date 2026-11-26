//! Events and their dispatch, as the DOM Standard's "dispatch" and
//! "inner invoke" algorithms, for node targets: the path is the target
//! and its ancestors, then the window when the target is in the
//! document; capture listeners run from the top down, then the target's,
//! then bubbling ones from the bottom up when the event bubbles.

use madcawl_dom::{HeldNode, NodeId};
use madcawl_js::{Engine, Halt, Host, Key, Root};
use madcawl_str::{Atom, DomString};

use crate::convert::{IntoJs, Thrown, throw_dom};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

pub const NONE: u16 = 0;
pub const CAPTURING_PHASE: u16 = 1;
pub const AT_TARGET: u16 = 2;
pub const BUBBLING_PHASE: u16 = 3;

/// An event target as an event refers to it.
pub(crate) enum Held {
    Node(HeldNode),
    Window,
}

/// An event target as an attribute returns it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Node(NodeId),
    Window,
}

impl Held {
    pub(crate) fn target(&self) -> Target {
        match self {
            Held::Node(n) => Target::Node(n.id()),
            Held::Window => Target::Window,
        }
    }
}

pub(crate) struct EventState {
    pub ty: Atom,
    pub bubbles: bool,
    pub cancelable: bool,
    pub composed: bool,
    pub trusted: bool,
    pub initialized: bool,
    pub dispatching: bool,
    pub stop: bool,
    pub stop_now: bool,
    pub canceled: bool,
    pub phase: u16,
    pub time: f64,
    pub target: Option<Held>,
    pub current: Option<Held>,
    /// The path while dispatching, for `composedPath`.
    pub path: Vec<Held>,
}

impl EventState {
    pub fn new(ty: Atom, time: f64) -> Self {
        EventState {
            ty,
            bubbles: false,
            cancelable: false,
            composed: false,
            trusted: false,
            initialized: true,
            dispatching: false,
            stop: false,
            stop_now: false,
            canceled: false,
            phase: NONE,
            time,
            target: None,
            current: None,
            path: Vec::new(),
        }
    }

    pub fn nodes(&self, f: &mut impl FnMut(NodeId)) {
        let all = self.target.iter().chain(&self.current).chain(&self.path);
        for h in all {
            if let Held::Node(n) = h {
                f(n.id());
            }
        }
    }

    pub fn release(&mut self, out: &mut Vec<HeldNode>) {
        let all = self.target.take().into_iter().chain(self.current.take()).chain(std::mem::take(&mut self.path));
        for h in all {
            if let Held::Node(n) = h {
                out.push(n);
            }
        }
    }

    /// Cancel, when a listener may.
    pub fn cancel(&mut self) {
        if self.cancelable {
            self.canceled = true;
        }
    }
}

impl<E: Engine> IntoJs<E> for Target {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> madcawl_js::Owned<E::Value> {
        match self {
            Target::Node(n) => h.wrap(e, n),
            Target::Window => e.global(h.realm),
        }
    }
}

impl<E: Engine> IntoJs<E> for SideRef {
    fn into_js(self, e: &mut E, h: &mut DomHost<E>) -> madcawl_js::Owned<E::Value> {
        h.wrap_side(e, self)
    }
}

impl<E: Engine> DomHost<E> {
    pub(crate) fn event(&self, r: SideRef) -> &EventState {
        let SideData::Event(ev) = &self.sides.get(r).data;
        ev
    }

    pub(crate) fn event_mut(&mut self, r: SideRef) -> &mut EventState {
        let SideData::Event(ev) = &mut self.sides.get_mut(r).data;
        ev
    }

    fn hold_target(&mut self, t: Target) -> Held {
        match t {
            Target::Node(n) => Held::Node(self.dom.hold(n)),
            Target::Window => Held::Window,
        }
    }

    fn set_held(&mut self, r: SideRef, which: fn(&mut EventState) -> &mut Option<Held>, t: Option<Target>) {
        let new = t.map(|t| self.hold_target(t));
        if let Some(Held::Node(old)) = std::mem::replace(which(self.event_mut(r)), new) {
            self.dom.let_go(old);
        }
    }

    /// Dispatch event `r` at node `target`; whether no listener canceled
    /// it. Listeners' exceptions are reported, not thrown.
    pub(crate) fn dispatch(&mut self, e: &mut E, r: SideRef, target: NodeId) -> Result<bool, Thrown> {
        let ev = self.event(r);
        if ev.dispatching || !ev.initialized {
            return Err(throw_dom(e, self, "InvalidStateError", "the event is being dispatched or was not initialized"));
        }
        self.event_mut(r).dispatching = true;
        self.set_held(r, |ev| &mut ev.target, Some(Target::Node(target)));
        let mut path: Vec<Target> = std::iter::successors(Some(target), |&n| self.dom.parent(n)).map(Target::Node).collect();
        if self.dom.in_main_tree(target) {
            path.push(Target::Window);
        }
        let held: Vec<Held> = path.iter().map(|&t| self.hold_target(t)).collect();
        self.event_mut(r).path = held;
        let event = self.wrap_side(e, r);
        for (i, &t) in path.iter().enumerate().rev() {
            let phase = if i == 0 { AT_TARGET } else { CAPTURING_PHASE };
            self.invoke(e, r, event.get(), t, phase, true);
        }
        for (i, &t) in path.iter().enumerate() {
            let phase = if i == 0 { AT_TARGET } else { BUBBLING_PHASE };
            if i == 0 || self.event(r).bubbles {
                self.invoke(e, r, event.get(), t, phase, false);
            }
        }
        e.release(event);
        let mut gone = Vec::new();
        let ev = self.event_mut(r);
        (ev.phase, ev.dispatching, ev.stop, ev.stop_now) = (NONE, false, false, false);
        for h in std::mem::take(&mut ev.path).into_iter().chain(ev.current.take()) {
            if let Held::Node(n) = h {
                gone.push(n);
            }
        }
        let canceled = ev.canceled;
        for n in gone {
            self.dom.let_go(n);
        }
        Ok(!canceled)
    }

    /// Call the listeners of `t` for the event in `phase`: the capture
    /// ones or the others, as they were when this target was reached,
    /// skipping those removed meanwhile.
    fn invoke(&mut self, e: &mut E, r: SideRef, event: E::Value, t: Target, phase: u16, capture: bool) {
        if self.event(r).stop || e.halt() != Halt::Running {
            return;
        }
        let ty = self.event(r).ty;
        let list: Vec<(Root<E>, bool)> = match t {
            Target::Node(n) => self
                .listeners_of(n)
                .filter(|l| l.ty == ty && l.capture == capture)
                .filter_map(|l| l.callback.get().map(|cb| (Root::new(e.retain(cb)), l.once)))
                .collect(),
            // the window's listeners are not split by phase yet: they run
            // as it is left
            Target::Window if !capture => {
                self.window_listeners.iter().filter(|(a, _)| *a == ty).map(|(_, cb)| (Root::new(e.retain(cb.get())), false)).collect()
            }
            Target::Window => Vec::new(),
        };
        if list.is_empty() {
            return;
        }
        self.event_mut(r).phase = phase;
        self.set_held(r, |ev| &mut ev.current, Some(t));
        let this = t.into_js(e, self);
        for (cb, once) in list {
            let live = match t {
                Target::Node(n) => self.listeners_of(n).any(|l| l.ty == ty && l.capture == capture && l.callback.get() == Some(cb.get())),
                Target::Window => true,
            };
            if live && !self.event(r).stop_now && e.halt() == Halt::Running {
                if once && let Target::Node(n) = t {
                    self.remove_listener(e, n, ty, capture, cb.get());
                }
                self.call_listener(e, cb.get(), this.get(), event);
            }
            cb.release(e);
        }
        e.release(this);
    }

    fn call_listener(&mut self, e: &mut E, cb: E::Value, this: E::Value, event: E::Value) {
        let (f, this) = if e.is_callable(cb) {
            (e.retain(cb), this)
        } else {
            let f = e.get(self, cb, Key::Str(&DomString::from_wtf8(b"handleEvent")));
            if E::is_thrown(f.get()) {
                return self.report_thrown(e, f);
            }
            (f, cb)
        };
        let res = e.call(self, f.get(), this, &[event]);
        e.release(f);
        if E::is_thrown(res.get()) {
            self.report_thrown(e, res);
        } else {
            e.release(res);
        }
    }

    fn report_thrown(&mut self, e: &mut E, r: madcawl_js::Owned<E::Value>) {
        let _sentinel = r.into_raw();
        if e.halt() == Halt::Running
            && let Some(x) = e.take_exception()
        {
            self.report_exception(e, x);
        }
    }

    /// The milliseconds since the window's time origin, as `timeStamp`.
    pub(crate) fn now_ms(&self) -> f64 {
        self.event_loop.now_ns() as f64 / 1e6
    }
}
