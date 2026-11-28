//! Members: the script values a host object holds (its listeners), kept
//! in a container hung on its wrapper as a private property, so the
//! framework's collector sees the edge from wrapper to value and frees a
//! listener whose closure holds its own node's wrapper together with
//! that wrapper. The record mirrors the container's slots, uncounted, to
//! find a value without asking the framework.

use crate::isolate::Jsc;
use crate::objects::WrapRec;
use crate::protos::Helper;
use crate::value::{UNDEFINED, Value};
use crate::{ffi, private};

fn record<'a>(word: u64) -> &'a mut WrapRec {
    // SAFETY: a word the host holds names a record not yet freed
    unsafe { &mut *(word as *mut WrapRec) }
}

impl Jsc {
    /// Keep `v` beside `owner`, a live wrapper of a live realm.
    pub(crate) fn add_member(&mut self, owner: Value, v: Value) {
        let rec = record(Self::word_of(owner));
        assert!(!rec.gone, "a member of a detached realm's wrapper");
        let ctx = self.realms[rec.realm as usize].ctx;
        if rec.container == Value(0) {
            // SAFETY: a plain object of a live realm, with no prototype, so
            // that putting a slot runs no script
            let c = unsafe {
                let c = ffi::JSObjectMake(ctx, std::ptr::null_mut(), std::ptr::null_mut());
                ffi::JSObjectSetPrototype(ctx, c, ffi::JSValueMakeNull(ctx));
                Value(c as u64)
            };
            private::set_private_property(ctx, owner, self.members_name, c);
            rec.container = c;
        }
        let i = rec.members.len() as u32;
        rec.members.push(v);
        // SAFETY: the container its wrapper holds, alive while the wrapper is
        unsafe { ffi::JSObjectSetPropertyAtIndex(ctx, rec.container.0 as _, i, v.raw(), std::ptr::null_mut()) };
    }

    /// Drop one `v` kept beside the wrapper whose word is `owner`;
    /// nothing once that wrapper is dead (the value went with it) or its
    /// realm is gone. A wrapper that is not strong may have been found
    /// dead by a collection whose sweep has not reached it, which only
    /// its `WeakRef` tells (`wrap.rs`): its container is not written
    /// then, nor when script was being stopped as it was read.
    pub(crate) fn remove_member(&mut self, owner: u64, v: Value) {
        let rec = record(owner);
        if rec.dead || rec.gone {
            return;
        }
        // held while its container is written, which may allocate
        let held = if rec.strong {
            UNDEFINED
        } else {
            match self.call_helper(rec.realm, Helper::Deref, &[rec.weak]) {
                Ok(w) if w != UNDEFINED => {
                    self.protect(w);
                    w
                }
                r => {
                    if r.is_err() {
                        self.after_stop();
                    }
                    return;
                }
            }
        };
        let rec = record(owner);
        let i = rec.members.iter().rposition(|&m| m == v).expect("a member of this wrapper");
        let last = rec.members.len() - 1;
        rec.members.swap_remove(i);
        self.member_writes += 1;
        let ctx = self.realms[rec.realm as usize].ctx;
        let c = rec.container.0 as ffi::JSObjectRef;
        // SAFETY: as in add_member
        unsafe {
            if i != last {
                ffi::JSObjectSetPropertyAtIndex(ctx, c, i as u32, rec.members[i].raw(), std::ptr::null_mut());
            }
            ffi::JSObjectSetPropertyAtIndex(ctx, c, last as u32, UNDEFINED.raw(), std::ptr::null_mut());
        }
        self.unprotect(held);
    }
}

#[cfg(feature = "collect-now")]
impl Jsc {
    /// How many members were taken out of their wrappers' containers so
    /// far: for this crate's tests, as `collect_now`.
    pub fn member_writes(&self) -> u64 {
        self.member_writes
    }

    /// How many wrapper words were given a tree so far.
    pub fn words_placed(&self) -> u64 {
        self.words_placed
    }
}
