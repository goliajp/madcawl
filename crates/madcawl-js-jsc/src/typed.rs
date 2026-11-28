//! Typed arrays and buffers: the element type and bytes of one, as the
//! host reads and fills them (`crypto.getRandomValues`, a buffer source),
//! new ones the host makes, and pending promises.

use madcawl_js::{ArrayType, Deferred, Owned};

use crate::ffi;
use crate::helpers::Helper;
use crate::isolate::{Jsc, Realm};
use crate::value::{THROWN, Value};

impl Jsc {
    pub(crate) fn typed_array_bytes(&mut self, v: Value) -> Option<(ArrayType, &mut [u8])> {
        if !v.is_cell() {
            return None;
        }
        let mut exc = std::ptr::null();
        // SAFETY: a type query on a live value
        let ty = match unsafe { ffi::JSValueGetTypedArrayType(self.base, v.raw(), &mut exc) } {
            ffi::kJSTypedArrayTypeNone | ffi::kJSTypedArrayTypeArrayBuffer => return None,
            ffi::kJSTypedArrayTypeInt8Array => ArrayType::Int8,
            ffi::kJSTypedArrayTypeUint8Array => ArrayType::Uint8,
            ffi::kJSTypedArrayTypeUint8ClampedArray => ArrayType::Uint8Clamped,
            ffi::kJSTypedArrayTypeInt16Array => ArrayType::Int16,
            ffi::kJSTypedArrayTypeUint16Array => ArrayType::Uint16,
            ffi::kJSTypedArrayTypeInt32Array => ArrayType::Int32,
            ffi::kJSTypedArrayTypeUint32Array => ArrayType::Uint32,
            ffi::kJSTypedArrayTypeBigInt64Array => ArrayType::BigInt64,
            ffi::kJSTypedArrayTypeBigUint64Array => ArrayType::BigUint64,
            _ => ArrayType::Other,
        };
        let obj = v.0 as ffi::JSObjectRef;
        // asking the framework for the bytes pins the buffer for good: it
        // can no longer be detached or transferred, which script may then
        // see. A view of shared memory needs no refusal here, as the
        // framework's contexts have no SharedArrayBuffer
        // SAFETY: a typed array; the framework's pointer is to the start
        // of its buffer, which it pins, and stays valid while no script
        // runs, which the borrow of the engine keeps so
        let bytes = unsafe {
            let len = ffi::JSObjectGetTypedArrayByteLength(self.base, obj, &mut exc);
            let offset = ffi::JSObjectGetTypedArrayByteOffset(self.base, obj, &mut exc);
            let base = ffi::JSObjectGetTypedArrayBytesPtr(self.base, obj, &mut exc).cast::<u8>();
            if base.is_null() || len == 0 {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(base.add(offset), len)
            }
        };
        Some((ty, bytes))
    }
}

impl Jsc {
    /// The bytes of an ArrayBuffer, typed array or DataView `v`.
    pub(crate) fn buffer_bytes(&mut self, realm: u32, v: Value) -> Option<&[u8]> {
        if !v.is_cell() || self.halted() {
            return None;
        }
        let mut exc = std::ptr::null();
        // SAFETY: a type query on a live value
        match unsafe { ffi::JSValueGetTypedArrayType(self.base, v.raw(), &mut exc) } {
            ffi::kJSTypedArrayTypeArrayBuffer => {
                let obj = v.0 as ffi::JSObjectRef;
                // the framework offers no read that leaves the buffer
                // unpinned (`typed_array_bytes`)
                // SAFETY: an ArrayBuffer; its bytes stay put while no
                // script runs, which the borrow of the engine keeps so
                Some(unsafe {
                    let len = ffi::JSObjectGetArrayBufferByteLength(self.base, obj, &mut exc);
                    let base = ffi::JSObjectGetArrayBufferBytesPtr(self.base, obj, &mut exc).cast::<u8>();
                    if base.is_null() || len == 0 { &[] } else { std::slice::from_raw_parts(base, len) }
                })
            }
            ffi::kJSTypedArrayTypeNone => {
                let view = match self.call_helper(realm, Helper::ViewRange, &[v]) {
                    Ok(view) if view.is_cell() => view,
                    Ok(_) => return None,
                    Err(_) => {
                        self.after_stop();
                        return None;
                    }
                };
                // the view dies with no count; the buffer it shares is
                // held by `v`, which the caller holds
                self.typed_array_bytes(view).map(|(_, b)| &*b)
            }
            _ => self.typed_array_bytes(v).map(|(_, b)| &*b),
        }
    }

    /// A new Uint8Array of `realm` over `bytes`, or its ArrayBuffer. The
    /// framework takes the bytes as they are and frees them when the
    /// buffer dies; nothing asks it for them, so the buffer is not pinned
    /// and script may still detach or transfer it.
    pub(crate) fn make_bytes(&mut self, realm: Realm, bytes: Vec<u8>, buffer: bool) -> Owned<Value> {
        if self.halted() {
            return Jsc::owned(THROWN);
        }
        let ctx = self.realm_ctx(realm);
        let bytes = Box::into_raw(bytes.into_boxed_slice());
        let len = bytes.len();
        let mut exc: ffi::JSValueRef = std::ptr::null();
        // SAFETY: a live realm; the framework owns the box from here, even
        // when it throws, and gives it to `free_bytes` with its length
        let buf = unsafe { ffi::JSObjectMakeArrayBufferWithBytesNoCopy(ctx, bytes.cast(), len, Some(free_bytes), len as *mut _, &mut exc) };
        if !exc.is_null() || buf.is_null() {
            return self.throw_new(madcawl_js::ErrorKind::Range, "the buffer is too long");
        }
        if buffer {
            return self.take(Value(buf as u64));
        }
        // SAFETY: the buffer just made, held by the stack while in use
        let arr = unsafe { ffi::JSObjectMakeTypedArrayWithArrayBuffer(ctx, ffi::kJSTypedArrayTypeUint8Array, buf, &mut exc) };
        if !exc.is_null() || arr.is_null() {
            return self.throw_new(madcawl_js::ErrorKind::Range, "the array is too long");
        }
        self.take(Value(arr as u64))
    }

    pub(crate) fn make_promise(&mut self, realm: Realm) -> Option<Deferred<Value>> {
        if self.halted() {
            return None;
        }
        let ctx = self.realm_ctx(realm);
        let (mut resolve, mut reject): (ffi::JSObjectRef, ffi::JSObjectRef) = (std::ptr::null_mut(), std::ptr::null_mut());
        let mut exc: ffi::JSValueRef = std::ptr::null();
        // SAFETY: a new promise of a live realm
        let p = unsafe { ffi::JSObjectMakeDeferredPromise(ctx, &mut resolve, &mut reject, &mut exc) };
        if !exc.is_null() || p.is_null() {
            return None;
        }
        Some(Deferred { promise: self.take(Value(p as u64)), resolve: self.take(Value(resolve as u64)), reject: self.take(Value(reject as u64)) })
    }
}

/// Frees what `make_bytes` gave the framework; `len` is its length.
unsafe extern "C" fn free_bytes(bytes: *mut std::ffi::c_void, len: *mut std::ffi::c_void) {
    // SAFETY: the box of `len` bytes `make_bytes` let go of
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(bytes.cast::<u8>(), len as usize)) });
}
