//! The value word: JavaScriptCore's own 64-bit encoding, which its C
//! interface passes as `JSValueRef`.
//!
//! ```text
//! 0xFFFE_0000_xxxx_xxxx    i32
//! double + 2^49            other numbers (NaN purified first)
//! 0x0000_xxxx_xxxx_xxx0    pointer to a heap cell (object, string, symbol, bigint)
//! 0x02 null, 0x0A undefined, 0x06 false, 0x07 true
//! 0x00                     never a value: the thrown sentinel (the engine's empty value)
//! 0x04                     never a value: the absent sentinel (the engine's deleted value)
//! ```
//!
//! The layout is checked against the framework when the first isolate
//! is made ([`check_layout`]): an operating system whose engine encodes
//! values otherwise stops there, not at a wrong number later.

use crate::ffi::{self, JSContextRef, JSValueRef};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(transparent)]
pub struct Value(pub(crate) u64);

const NUMBER_TAG: u64 = 0xFFFE_0000_0000_0000;
const DOUBLE_OFFSET: u64 = 1 << 49;
const OTHER_TAG: u64 = 0x02;
const NOT_CELL: u64 = NUMBER_TAG | OTHER_TAG;
const CANONICAL_NAN: u64 = 0x7FF8_0000_0000_0000;

pub(crate) const NULL: Value = Value(0x02);
pub(crate) const UNDEFINED: Value = Value(0x0A);
pub(crate) const FALSE: Value = Value(0x06);
pub(crate) const TRUE: Value = Value(0x07);
pub(crate) const THROWN: Value = Value(0);
pub(crate) const ABSENT: Value = Value(0x04);

impl Value {
    pub(crate) fn number(n: f64) -> Value {
        if n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64 && !(n == 0.0 && n.is_sign_negative()) {
            return Value(NUMBER_TAG | u64::from(n as i32 as u32));
        }
        let bits = if n.is_nan() { CANONICAL_NAN } else { n.to_bits() };
        Value(bits.wrapping_add(DOUBLE_OFFSET))
    }

    pub(crate) fn as_number(self) -> Option<f64> {
        if self.0 & NUMBER_TAG == NUMBER_TAG {
            return Some(f64::from(self.0 as u32 as i32));
        }
        if self.0 & NUMBER_TAG != 0 {
            return Some(f64::from_bits(self.0.wrapping_sub(DOUBLE_OFFSET)));
        }
        None
    }

    pub(crate) fn as_boolean(self) -> Option<bool> {
        match self {
            TRUE => Some(true),
            FALSE => Some(false),
            _ => None,
        }
    }

    /// A pointer to a heap cell: what has a count to protect. Cells are
    /// aligned, which tells them from the two sentinels.
    pub(crate) fn is_cell(self) -> bool {
        self.0 != 0 && self.0 & (NOT_CELL | 0x7) == 0
    }

    pub(crate) fn raw(self) -> JSValueRef {
        self.0 as JSValueRef
    }

    pub(crate) fn from_ref(v: JSValueRef) -> Value {
        Value(v as u64)
    }
}

/// Make the framework encode a few values and compare with the
/// encoding above.
pub(crate) fn check_layout(ctx: JSContextRef) {
    // SAFETY: a live context; making immediates allocates nothing
    let made = unsafe {
        [
            (ffi::JSValueMakeUndefined(ctx), UNDEFINED),
            (ffi::JSValueMakeNull(ctx), NULL),
            (ffi::JSValueMakeBoolean(ctx, true), TRUE),
            (ffi::JSValueMakeBoolean(ctx, false), FALSE),
            (ffi::JSValueMakeNumber(ctx, 7.0), Value::number(7.0)),
            (ffi::JSValueMakeNumber(ctx, -1.0), Value::number(-1.0)),
            (ffi::JSValueMakeNumber(ctx, 1.5), Value::number(1.5)),
            (ffi::JSValueMakeNumber(ctx, -0.0), Value::number(-0.0)),
            (ffi::JSValueMakeNumber(ctx, f64::NAN), Value::number(f64::NAN)),
            (ffi::JSValueMakeNumber(ctx, 1e300), Value::number(1e300)),
        ]
    };
    for (got, want) in made {
        assert_eq!(Value::from_ref(got), want, "JavaScriptCore encodes values as this adapter expects");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_round_trip() {
        for n in [0.0, -0.0, 1.0, -7.0, 1.5, 1e300, f64::INFINITY, i32::MAX as f64, 2147483648.0] {
            let v = Value::number(n);
            assert_eq!(v.as_number().map(f64::to_bits), Some(n.to_bits()), "{n}");
            assert!(!v.is_cell());
        }
        assert!(Value::number(f64::NAN).as_number().unwrap().is_nan());
        for v in [NULL, UNDEFINED, TRUE, FALSE, THROWN, ABSENT] {
            assert!(!v.is_cell() && v.as_number().is_none());
        }
    }
}
