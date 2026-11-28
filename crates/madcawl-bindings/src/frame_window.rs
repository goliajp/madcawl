//! A child navigable's window members (`frames.rs`), the `length` of
//! every window, the iframe's `contentWindow` and `contentDocument`, and
//! the stand-in script gets for a window of another origin: an object of
//! the parent's realm whose accessors answer what the standard lets
//! other origins read (`window`, `self`, `frames`, `parent`, `top`,
//! `opener`, `closed`, `length`), with `postMessage`; reading, writing
//! or defining any other member throws a SecurityError. `location`,
//! `focus`, `blur` and `close` are not done yet.

use madcawl_dom::NodeId;
use madcawl_js::{Bound, Ctx, Engine, HostFn, Owned, Root};

use crate::convert::{plain, throw_dom};
use crate::host::DomHost;
use crate::window::put;

/// The stand-in's members, in the order of its getters.
const STUB_MEMBERS: [&str; 8] = ["window", "self", "frames", "parent", "top", "opener", "closed", "length"];

impl<E: Engine> DomHost<E> {
    /// Put window `w`'s members on its global, before any script of its
    /// own runs.
    pub(crate) fn install_frame_window(&mut self, e: &mut E, w: u64) {
        let f = &self.frames.live[&w];
        let (realm, parent, el, doc) = (f.realm, f.parent, f.element.id(), f.doc.id());
        let global = e.global(realm);
        let g = global.get();
        for name in ["window", "self", "frames"] {
            put(e, self, g, name, g);
        }
        let values = [("parent", self.window_value(e, parent)), ("top", e.global(self.realm)), ("frameElement", self.wrap(e, el)), ("document", self.wrap(e, doc))];
        for (name, v) in values {
            put(e, self, g, name, v.get());
            e.release(v);
        }
        put(e, self, g, "opener", E::null());
        let ok = e.define_interfaces(self, realm, g, Self::INTERFACES);
        assert!(ok, "putting the interface objects on a child window");
        self.install_functions(e, realm, g, w);
        self.install_length(e, realm, g, w);
        e.release(global);
    }

    /// `length` of window `w` on its global of `realm`: how many child
    /// navigables its document has.
    pub(crate) fn install_length(&mut self, e: &mut E, realm: E::Realm, global: E::Value, w: u64) {
        let get = self.window_function(e, realm, "length", 0, member_abi::<E, 7>, w);
        let ok = e.define_accessor(realm, global, "length", get.get(), E::undefined(), false);
        assert!(ok, "defining length on a global object");
        e.release(get);
    }

    fn window_function(&mut self, e: &mut E, realm: E::Realm, name: &'static str, length: u8, f: HostFn, w: u64) -> Owned<E::Value> {
        // SAFETY: every function here reads its context as E and DomHost<E>
        let bound = unsafe { Bound::<HostFn, E, DomHost<E>>::host_fn(f) };
        e.new_function(realm, name, length, bound, w)
    }

    fn child_count(&self, w: u64) -> usize {
        self.frames.live.values().filter(|f| f.parent == w).count()
    }

    /// The iframe's document, when its origin is its parent's.
    pub(crate) fn content_document(&self, el: NodeId) -> Option<NodeId> {
        let f = &self.frames.live[self.frames.by_element.get(&el.index())?];
        f.same_origin.then(|| f.doc.id())
    }

    /// The iframe's window: its WindowProxy, or the stand-in when its
    /// origin is another.
    pub(crate) fn content_window(&mut self, e: &mut E, el: NodeId) -> Option<Owned<E::Value>> {
        let w = *self.frames.by_element.get(&el.index())?;
        if self.frames.live[&w].same_origin {
            return Some(self.window_value(e, w));
        }
        Some(self.stand_in(e, w))
    }

    /// The stand-in for window `w`, one for as long as the window lives:
    /// every realm that reaches it gets the same object, frozen, so none
    /// can leave anything there for another.
    fn stand_in(&mut self, e: &mut E, w: u64) -> Owned<E::Value> {
        let f = &self.frames.live[&w];
        if let Some(s) = &f.stub {
            return e.retain(s.get());
        }
        let realm = self.realm_of(f.parent).expect("a frame's parent outlives it");
        let o = e.new_object(realm);
        let getters: [HostFn; 8] = [
            member_abi::<E, 0>, member_abi::<E, 0>, member_abi::<E, 0>, member_abi::<E, 3>,
            member_abi::<E, 4>, member_abi::<E, 5>, member_abi::<E, 6>, member_abi::<E, 7>,
        ];
        for (name, get) in STUB_MEMBERS.into_iter().zip(getters) {
            let get = self.window_function(e, realm, name, 0, get, w);
            let ok = e.define_accessor(realm, o.get(), name, get.get(), E::undefined(), true);
            assert!(ok, "defining {name} on a window's stand-in");
            e.release(get);
        }
        let post = self.window_function(e, realm, "postMessage", 1, post_message_abi::<E>, w);
        put(e, self, o.get(), "postMessage", post.get());
        let deny = self.window_function(e, realm, "deny", 1, deny_abi::<E>, w);
        let guard = e.new_cross_origin_object(realm, o.get(), deny.get());
        for v in [post, deny, o] {
            e.release(v);
        }
        if !E::is_thrown(guard.get()) {
            let kept = Root::new(e.retain(guard.get()));
            self.frames.live.get_mut(&w).expect("a live frame").stub = Some(kept);
        }
        guard
    }
}

/// `postMessage` of a window of another origin: taken and dropped, as
/// delivering the message to that window is the next step.
fn post_message<E: Engine>(_: &mut E, _: &mut DomHost<E>, _: u64, _: E::Value, _: &[E::Value]) -> Owned<E::Value> {
    plain::<E>(E::undefined())
}

/// Any member of a window of another origin besides those the standard
/// lets it reach.
fn deny<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: u64, _: E::Value, _: &[E::Value]) -> Owned<E::Value> {
    let _thrown = throw_dom(e, h, "SecurityError", "a window of another origin");
    E::thrown()
}

// SAFETY: called as a host function made for E and DomHost<E>
unsafe extern "C" fn post_message_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { crate::shell::op_data(ctx, this, argv, argc, post_message::<E>) }
}

// SAFETY: called as a host function made for E and DomHost<E>
unsafe extern "C" fn deny_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { crate::shell::op_data(ctx, this, argv, argc, deny::<E>) }
}

/// Member `I` of `STUB_MEMBERS` of window `w`; `length` (7) is every
/// window's own too. A window that is gone is closed and has no parent.
fn member<E: Engine, const I: usize>(e: &mut E, h: &mut DomHost<E>, w: u64, _: E::Value, _: &[E::Value]) -> Owned<E::Value> {
    let parent = h.frames.live.get(&w).map(|f| f.parent);
    match I {
        7 => plain::<E>(E::number(h.child_count(w) as f64)),
        6 => plain::<E>(E::boolean(w != 0 && parent.is_none())),
        5 => plain::<E>(E::null()),
        _ if parent.is_none() => plain::<E>(E::null()),
        4 => e.global(h.realm),
        3 => h.window_value(e, parent.expect("checked")),
        _ => h.stand_in(e, w),
    }
}

// SAFETY: called as a host function made for E and DomHost<E>
unsafe extern "C" fn member_abi<E: Engine, const I: usize>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { crate::shell::op_data(ctx, this, argv, argc, member::<E, I>) }
}
