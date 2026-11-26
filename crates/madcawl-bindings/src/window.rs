//! The window's own members, put on the realm's global object: `window`
//! and `self`, `parent` and `top` (a top-level window's own), `opener`,
//! `document`, the interface objects, the timers, `queueMicrotask`, and
//! the window's listeners. The global is not yet a host object of a
//! bound `Window` interface: these are plain properties, and the
//! functions host functions made at run time.

use madcawl_js::{Bound, ClassDef, Ctx, Engine, ErrorKind, Halt, HostFn, Key, Owned, Root};
use madcawl_str::{Atom, DomString};

use crate::convert::{Thrown, arg, run, throw};
use crate::generated::{
    AbortControllerClass, AbortSignalClass, AttrClass, CharacterDataClass, CommentClass, CustomEventClass, DOMExceptionClass,
    DocumentClass, DocumentFragmentClass, ElementClass, EventClass, EventTargetClass, HTMLCollectionClass, HTMLScriptElementClass,
    NamedNodeMapClass,
    NodeClass, NodeListClass, TextClass,
};
use crate::event::Target;
use crate::host::DomHost;
use crate::shell;

type Value<E> = <E as Engine>::Value;

fn put<E: Engine>(e: &mut E, h: &mut DomHost<E>, obj: Value<E>, name: &str, v: Value<E>) {
    let ok = e.set(h, obj, Key::Str(&DomString::from_wtf8(name.as_bytes())), v);
    assert!(ok, "setting {name} on the global object");
}

impl<E: Engine> DomHost<E> {
    /// Put the window's members on the global object of this host's
    /// realm.
    pub fn install_window(&mut self, e: &mut E) {
        let global = e.global(self.realm);
        for name in ["window", "self", "parent", "top"] {
            put(e, self, global.get(), name, global.get());
        }
        put(e, self, global.get(), "opener", E::null());
        let doc = self.dom.document();
        let d = self.wrap(e, doc);
        put(e, self, global.get(), "document", d.get());
        e.release(d);
        let classes: [&'static ClassDef<E, DomHost<E>>; 18] = [
            &AbortControllerClass::<E>::DEF,
            &AbortSignalClass::<E>::DEF,
            &AttrClass::<E>::DEF,
            &NamedNodeMapClass::<E>::DEF,
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
            &HTMLScriptElementClass::<E>::DEF,
            &EventClass::<E>::DEF,
            &NodeListClass::<E>::DEF,
            &HTMLCollectionClass::<E>::DEF,
        ];
        for c in classes {
            let i = e.interface_object(self.realm, c);
            put(e, self, global.get(), c.name, i.get());
            e.release(i);
        }
        let functions: [(&'static str, u8, HostFn); 7] = [
            ("addEventListener", 2, add_listener_abi::<E>),
            ("removeEventListener", 2, remove_listener_abi::<E>),
            ("setTimeout", 1, set_timeout_abi::<E>),
            ("setInterval", 1, set_interval_abi::<E>),
            ("clearTimeout", 0, clear_timer_abi::<E>),
            ("clearInterval", 0, clear_timer_abi::<E>),
            ("queueMicrotask", 1, queue_microtask_abi::<E>),
        ];
        for (name, length, f) in functions {
            // SAFETY: every function here reads its context as E and DomHost<E>
            let bound = unsafe { Bound::<HostFn, E, DomHost<E>>::host_fn(f) };
            let v = e.new_function(self.realm, name, length, bound, 0);
            put(e, self, global.get(), name, v.get());
            e.release(v);
        }
        e.release(global);
        self.settle(e);
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
        for mut l in std::mem::take(&mut self.window_listeners) {
            l.callback.clear(e);
        }
    }
}

fn listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, args: &[Value<E>], add: bool) -> Result<(), Thrown> {
    let ty = crate::convert::atom(e, h, arg::<E>(args, 0))?;
    let cb = crate::convert::callback_or_null(e, arg::<E>(args, 1))?;
    let o = crate::listeners::options(e, h, arg::<E>(args, 2), add)?;
    let Some(cb) = cb else { return Ok(()) };
    if add {
        crate::listeners::add(e, h, Target::Window, ty, cb, &o);
    } else {
        h.remove_listener(e, Target::Window, ty, o.capture, cb);
    }
    Ok(())
}

fn add_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| listener(e, h, args, true))
}

fn remove_listener<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| listener(e, h, args, false))
}

fn timer<E: Engine>(e: &mut E, h: &mut DomHost<E>, args: &[Value<E>], repeat: bool) -> Result<u32, Thrown> {
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
    let rest = args.iter().skip(2).map(|&v| Root::new(e.retain(v))).collect();
    Ok(h.event_loop.add_timer(Root::new(handler), rest, ms, repeat))
}

fn set_timeout<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| timer(e, h, args, false).map(f64::from))
}

fn set_interval<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| timer(e, h, args, true).map(f64::from))
}

fn clear_timer<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, h| {
        let n = crate::convert::to_int32(e.to_number(h, arg::<E>(args, 0)).ok_or(Thrown)?);
        if n >= 1
            && let Some(t) = h.event_loop.remove_timer(n as u32)
        {
            t.handler.release(e);
            for a in t.args {
                a.release(e);
            }
        }
        Ok(())
    })
}

fn queue_microtask<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: Value<E>, args: &[Value<E>]) -> Owned<Value<E>> {
    run(e, h, |e, _| {
        let cb = arg::<E>(args, 0);
        if !e.is_callable(cb) {
            return Err(throw(e, ErrorKind::Type, "queueMicrotask takes a function"));
        }
        let job = e.retain(cb);
        // SAFETY: undefined carries no count
        e.enqueue_microtask(job, unsafe { Owned::from_raw(E::undefined()) });
        Ok(())
    })
}

macro_rules! abi {
    ($($abi:ident => $glue:ident),*) => {$(
        // SAFETY: called as a host function made for E and DomHost<E>
        unsafe extern "C" fn $abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
            unsafe { shell::op(ctx, this, argv, argc, $glue::<E>) }
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
