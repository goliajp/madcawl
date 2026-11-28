//! The window's own members, put on the realm's global object: `window`,
//! `self` and `frames`, `parent` and `top` (a top-level window's own),
//! `opener`, `frameElement`, `length` (`frame_window.rs`), `document`, `location`, `history`, `crypto` and `performance`
//! (`objects.rs`), the interface objects, the timers, `queueMicrotask`,
//! `atob` and `btoa`, and the window's listeners. The global is not yet
//! a host object of a bound `Window` interface: these are plain
//! properties, but for the accessors of `objects.rs`, and the functions
//! host functions made at run time.

use madcawl_js::{Bound, ClassDef, Ctx, Engine, ErrorKind, Halt, HostFn, Key, Owned, Root};
use madcawl_str::{Atom, DomString};

use crate::convert::{Thrown, arg, run, throw};
use crate::generated::{
    AbortControllerClass, AbortSignalClass, AttrClass, CSSStyleDeclarationClass, CharacterDataClass, CommentClass, CryptoClass, CustomEventClass,
    DOMExceptionClass, DOMTokenListClass, DocumentClass, DocumentFragmentClass, ElementClass, EventClass, EventTargetClass, HTMLCollectionClass,
    HTMLAnchorElementClass, HTMLBodyElementClass, HTMLButtonElementClass, HTMLDivElementClass, HTMLElementClass,
    HTMLHeadElementClass, HTMLHtmlElementClass, HTMLIFrameElementClass, HTMLImageElementClass, HTMLInputElementClass,
    HTMLLinkElementClass, HTMLObjectElementClass, HTMLScriptElementClass, HTMLSpanElementClass, HTMLStyleElementClass,
    HTMLUnknownElementClass, HistoryClass, LocationClass, NamedNodeMapClass, NodeClass, NodeListClass, PerformanceClass,
    PerformanceObserverClass, TextClass, BlobClass, TextDecoderClass, TextEncoderClass, URLClass, URLSearchParamsClass,
};
use crate::event::Target;
use crate::host::DomHost;
use crate::shell;

type Value<E> = <E as Engine>::Value;

pub(crate) fn put<E: Engine>(e: &mut E, h: &mut DomHost<E>, obj: Value<E>, name: &str, v: Value<E>) {
    let ok = e.set(h, obj, Key::Str(&DomString::from_wtf8(name.as_bytes())), v);
    assert!(ok, "setting {name} on the global object");
}

impl<E: Engine> DomHost<E> {
    /// The interfaces on the global object, in the order of its own
    /// keys.
    pub(crate) const INTERFACES: &'static [&'static ClassDef<E, DomHost<E>>] = &[
        &AbortControllerClass::<E>::DEF,
        &AbortSignalClass::<E>::DEF,
        &AttrClass::<E>::DEF,
        &NamedNodeMapClass::<E>::DEF,
        &DOMTokenListClass::<E>::DEF,
        &CSSStyleDeclarationClass::<E>::DEF,
        &CharacterDataClass::<E>::DEF,
        &TextClass::<E>::DEF,
        &CommentClass::<E>::DEF,
        &DocumentFragmentClass::<E>::DEF,
        &CustomEventClass::<E>::DEF,
        &DOMExceptionClass::<E>::DEF,
        &EventTargetClass::<E>::DEF,
        &NodeClass::<E>::DEF,
        &ElementClass::<E>::DEF,
        &DocumentClass::<E>::DEF,
        &HTMLElementClass::<E>::DEF,
        &HTMLUnknownElementClass::<E>::DEF,
        &HTMLHtmlElementClass::<E>::DEF,
        &HTMLHeadElementClass::<E>::DEF,
        &HTMLBodyElementClass::<E>::DEF,
        &HTMLDivElementClass::<E>::DEF,
        &HTMLSpanElementClass::<E>::DEF,
        &HTMLAnchorElementClass::<E>::DEF,
        &HTMLLinkElementClass::<E>::DEF,
        &HTMLStyleElementClass::<E>::DEF,
        &HTMLImageElementClass::<E>::DEF,
        &HTMLIFrameElementClass::<E>::DEF,
        &HTMLObjectElementClass::<E>::DEF,
        &HTMLButtonElementClass::<E>::DEF,
        &HTMLInputElementClass::<E>::DEF,
        &HTMLScriptElementClass::<E>::DEF,
        &EventClass::<E>::DEF,
        &NodeListClass::<E>::DEF,
        &HTMLCollectionClass::<E>::DEF,
        &LocationClass::<E>::DEF,
        &HistoryClass::<E>::DEF,
        &CryptoClass::<E>::DEF,
        &PerformanceClass::<E>::DEF,
        &PerformanceObserverClass::<E>::DEF,
        &TextDecoderClass::<E>::DEF,
        &TextEncoderClass::<E>::DEF,
        &URLClass::<E>::DEF,
        &URLSearchParamsClass::<E>::DEF,
        &BlobClass::<E>::DEF,
    ];

    /// Put the window's members on the global object of this host's
    /// realm.
    pub fn install_window(&mut self, e: &mut E) {
        let global = e.global(self.realm);
        for name in ["window", "self", "frames", "parent", "top"] {
            put(e, self, global.get(), name, global.get());
        }
        put(e, self, global.get(), "opener", E::null());
        put(e, self, global.get(), "frameElement", E::null());
        let doc = self.dom.document();
        let d = self.wrap(e, doc);
        put(e, self, global.get(), "document", d.get());
        e.release(d);
        let ok = e.define_interfaces(self, self.realm, global.get(), Self::INTERFACES);
        assert!(ok, "putting the interface objects on the global object");
        self.install_functions(e, self.realm, global.get(), 0);
        self.install_length(e, self.realm, global.get(), 0);
        self.install_objects(e, global.get());
        e.release(global);
        self.settle(e);
    }

    /// The functions of window `w` (`frames.rs`), whose global of
    /// `realm` is `global`: each gets `w` as its data word.
    pub(crate) fn install_functions(&mut self, e: &mut E, realm: E::Realm, global: Value<E>, w: u64) {
        let functions: [(&'static str, u8, HostFn); 9] = [
            ("addEventListener", 2, add_listener_abi::<E>),
            ("removeEventListener", 2, remove_listener_abi::<E>),
            ("setTimeout", 1, set_timeout_abi::<E>),
            ("setInterval", 1, set_interval_abi::<E>),
            ("clearTimeout", 0, clear_timer_abi::<E>),
            ("clearInterval", 0, clear_timer_abi::<E>),
            ("queueMicrotask", 1, queue_microtask_abi::<E>),
            ("atob", 1, crate::base64::atob_abi::<E>),
            ("btoa", 1, crate::base64::btoa_abi::<E>),
        ];
        for (name, length, f) in functions {
            // SAFETY: every function here reads its context as E and DomHost<E>
            let bound = unsafe { Bound::<HostFn, E, DomHost<E>>::host_fn(f) };
            let v = e.new_function(realm, name, length, bound, w);
            put(e, self, global, name, v.get());
            e.release(v);
        }
    }
}

impl<E: Engine> DomHost<E> {
    /// Call the window's listeners of type `ty`, each followed by a
    /// checkpoint; as `fire` does for a node.
    pub fn fire_window(&mut self, e: &mut E, ty: Atom) -> usize {
        let roots: Vec<Root<E>> = self
            .window_listeners
            .iter()
            .filter(|l| l.ty == ty)
            .filter_map(|l| l.callback.get())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|cb| Root::new(e.retain(cb)))
            .collect();
        let window = e.global(self.realm);
        let arg = e.string(ty.to_dom_string());
        let mut called = 0;
        for r in roots {
            if e.halt() == Halt::Running {
                called += 1;
                let res = e.call(self, r.get(), window.get(), &[arg.get()]);
                self.finish_task(e, res);
            }
            r.release(e);
        }
        e.release(arg);
        e.release(window);
        called
    }

    pub(crate) fn close_window(&mut self, e: &mut E) {
        self.close_nav(e);
        for mut l in std::mem::take(&mut self.window_listeners) {
            l.callback.clear(e);
        }
    }
}

/// A window's listener list: a window that is gone keeps none.
fn listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, args: &[Value<E>], add: bool) -> Result<(), Thrown> {
    let ty = crate::convert::atom(e, h, arg::<E>(args, 0))?;
    let cb = crate::convert::callback_or_null(e, arg::<E>(args, 1))?;
    let o = crate::listeners::options(e, h, arg::<E>(args, 2), add)?;
    let Some(cb) = cb else { return Ok(()) };
    if w != 0 && !h.frames.live.contains_key(&w) {
        return Ok(());
    }
    if add {
        crate::listeners::add(e, h, Target::Window(w), ty, cb, &o);
    } else {
        h.remove_listener(e, Target::Window(w), ty, o.capture, cb);
    }
    Ok(())
}

fn add_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| listener(e, h, w, args, true))
}

fn remove_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| listener(e, h, w, args, false))
}

/// A timer of window `w`; one of a window that is gone never runs, and
/// is 0.
fn timer<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, args: &[Value<E>], repeat: bool) -> Result<u32, Thrown> {
    let handler = arg::<E>(args, 0);
    let handler = if e.is_callable(handler) {
        e.retain(handler)
    } else {
        let s = e.to_string(h, handler).ok_or(Thrown)?;
        e.string(s)
    };
    let ms = match args.get(1) {
        Some(&v) => match e.to_number(h, v) {
            Some(n) => crate::convert::to_int32(n),
            None => {
                e.release(handler);
                return Err(Thrown);
            }
        },
        None => 0,
    };
    if w != 0 && !h.frames.live.contains_key(&w) {
        e.release(handler);
        return Ok(0);
    }
    let rest = args.iter().skip(2).map(|&v| Root::new(e.retain(v))).collect();
    Ok(h.event_loop.add_timer(Root::new(handler), rest, ms, repeat, w))
}

fn set_timeout<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| timer(e, h, w, args, false).map(f64::from))
}

fn set_interval<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| timer(e, h, w, args, true).map(f64::from))
}

/// Clears a timer of the window it is called for only.
fn clear_timer<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| {
        let n = crate::convert::to_int32(e.to_number(h, arg::<E>(args, 0)).ok_or(Thrown)?);
        if n >= 1
            && let Some(t) = h.event_loop.remove_timer_of(n as u32, w)
        {
            t.handler.release(e);
            for a in t.args {
                a.release(e);
            }
        }
        Ok(())
    })
}

/// Queues in the realm of the window it is called for, which may be
/// gone by then: the callback is then dropped unrun.
fn queue_microtask<E: Engine>(e: &mut E, h: &mut DomHost<E>, w: u64, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| {
        let cb = arg::<E>(args, 0);
        if !e.is_callable(cb) {
            return Err(throw(e, ErrorKind::Type, "queueMicrotask takes a function"));
        }
        if let Some(realm) = h.realm_of(w) {
            let job = e.retain(cb);
            // SAFETY: undefined carries no count
            e.enqueue_microtask(realm, job, unsafe { Owned::from_raw(E::undefined()) });
        }
        Ok(())
    })
}

macro_rules! abi {
    ($($abi:ident => $glue:ident),*) => {$(
        // SAFETY: called as a host function made for E and DomHost<E>
        unsafe extern "C" fn $abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
            unsafe { shell::op_data(ctx, this, argv, argc, $glue::<E>) }
        }
    )*};
}

abi!(
    add_listener_abi => add_listener,
    remove_listener_abi => remove_listener,
    set_timeout_abi => set_timeout,
    set_interval_abi => set_interval,
    clear_timer_abi => clear_timer,
    queue_microtask_abi => queue_microtask
);
