//! Typed arrays: the element type and bytes of one, as the host reads
//! and fills them (`crypto.getRandomValues`).

use madcawl_js::ArrayType;

use crate::ffi;
use crate::isolate::Jsc;
use crate::value::Value;

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
