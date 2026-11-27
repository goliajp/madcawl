//! The window's Location, History and Crypto, side objects the host
//! holds from install to its end, and the global's accessors for them.
//! `location` is the standard's [LegacyUnforgeable] and [PutForwards]:
//! a non-configurable accessor whose setter sets `href`, so that
//! `location = url` navigates; `history` and `crypto` are readonly.
//! They are defined through the realm's own builtins, before any page
//! script runs.

use madcawl_js::{Bound, Ctx, Engine, HostFn, Owned};

use crate::convert::{arg, dom_string, need, run};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

/// The object `I` of the three, by its accessor's getter.
fn object<E: Engine, const I: usize>(e: &mut E, h: &mut DomHost<E>, _: E::Value, _: &[E::Value]) -> Owned<E::Value> {
    let r = h.nav.objects.expect("the window's objects, installed with their accessors")[I];
    h.wrap_side(e, r)
}

fn set_location<E: Engine>(e: &mut E, h: &mut DomHost<E>, _: E::Value, args: &[E::Value]) -> Owned<E::Value> {
    run(e, h, |e, h| {
        need(e, args, 1)?;
        let url = dom_string(e, h, arg::<E>(args, 0))?;
        let r = h.location_object().expect("the window's Location, installed with its accessor");
        crate::impls::location::set_href(e, h, r, url)
    })
}

// SAFETY: called as a host function made for E and DomHost<E>
unsafe extern "C" fn object_abi<E: Engine, const I: usize>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { crate::shell::op(ctx, this, argv, argc, object::<E, I>) }
}

// SAFETY: as for object_abi
unsafe extern "C" fn set_location_abi<E: Engine>(ctx: *mut Ctx, this: u64, argv: *const u64, argc: usize) -> u64 {
    unsafe { crate::shell::op(ctx, this, argv, argc, set_location::<E>) }
}

impl<E: Engine> DomHost<E> {
    /// Make the three objects and define their accessors on `global`.
    pub(crate) fn install_objects(&mut self, e: &mut E, global: E::Value) {
        let objects = [SideData::Location, SideData::History, SideData::Crypto].map(|d| {
            let r = self.sides.add(d);
            self.sides.inc(r);
            r
        });
        self.nav.objects = Some(objects);
        let getters: [HostFn; 3] = [object_abi::<E, 0>, object_abi::<E, 1>, object_abi::<E, 2>];
        for (i, (name, get)) in ["location", "history", "crypto"].into_iter().zip(getters).enumerate() {
            let get = self.host_function(e, name, 0, get);
            let set = (i == 0).then(|| self.host_function(e, name, 1, set_location_abi::<E>));
            let set_value = set.as_ref().map_or(E::undefined(), Owned::get);
            let ok = e.define_accessor(self.realm, global, name, get.get(), set_value, i == 0);
            assert!(ok, "defining {name} on the global object");
            e.release(get);
            if let Some(s) = set {
                e.release(s);
            }
        }
    }

    fn host_function(&mut self, e: &mut E, name: &'static str, length: u8, f: HostFn) -> Owned<E::Value> {
        // SAFETY: every function here reads its context as E and DomHost<E>
        let bound = unsafe { Bound::<HostFn, E, DomHost<E>>::host_fn(f) };
        e.new_function(self.realm, name, length, bound, 0)
    }

    pub(crate) fn location_object(&self) -> Option<SideRef> {
        self.nav.objects.map(|o| o[0])
    }
}
