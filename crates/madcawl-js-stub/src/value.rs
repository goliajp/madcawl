//! The value word: the reference engine's NaN-boxed layout.
//!
//! ```text
//! 0x0007_0000_0000_0000 <= double + offset < 0xFFFE_0000_0000_0000
//! 0xFFFE_0000_xxxx_xxxx    i32
//! 0x0000_xxxx_xxxx_xxx0    pointer to a counted block (8-aligned)
//! 0x02 null, 0x0A undefined, 0x06 false, 0x07 true
//! 0x00                     never a value: the thrown sentinel
//! 0x12                     never a value: the absent sentinel
//! ```
//!
//! A pointer is either a `DomString` block (type tag `TAG_STR`) or one
//! of the stub's cells.

use madcawl_str::abi;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(transparent)]
pub struct Value(pub(crate) u64);

const INT_TAG: u64 = 0xFFFE_0000_0000_0000;
const TOP_16: u64 = 0xFFFF_0000_0000_0000;
const DOUBLE_OFFSET: u64 = 0x0007_0000_0000_0000;
const TYPE_OTHER: u64 = 0x02;

pub(crate) const NULL: Value = Value(0x02);
pub(crate) const UNDEFINED: Value = Value(0x0A);
pub(crate) const FALSE: Value = Value(0x06);
pub(crate) const TRUE: Value = Value(0x07);
pub(crate) const THROWN: Value = Value(0);
/// An index getter's "no such index"; never a value.
pub(crate) const ABSENT: Value = Value(0x12);

impl Value {
    pub(crate) fn number(n: f64) -> Value {
        let i = n as i32;
        if i as f64 == n && !(n == 0.0 && n.is_sign_negative()) {
            Value(INT_TAG | i as u32 as u64)
        } else {
            Value(n.to_bits() + DOUBLE_OFFSET)
        }
    }

    pub(crate) fn as_number(self) -> Option<f64> {
        if self.0 & TOP_16 == INT_TAG {
            Some(self.0 as u32 as i32 as f64)
        } else if self.0 >= DOUBLE_OFFSET {
            Some(f64::from_bits(self.0 - DOUBLE_OFFSET))
        } else {
            None
        }
    }

    pub(crate) fn as_boolean(self) -> Option<bool> {
        match self {
            TRUE => Some(true),
            FALSE => Some(false),
            _ => None,
        }
    }

    /// A pointer to a counted block.
    pub(crate) fn is_block(self) -> bool {
        self.0 & TOP_16 == 0 && self.0 & TYPE_OTHER == 0 && self.0 != 0
    }

    /// A `DomString` block.
    pub(crate) fn is_string(self) -> bool {
        // SAFETY: a block value points at a live block, header first
        self.is_block() && abi::tag(unsafe { *(self.0 as *const u64) }) == abi::TAG_STR
    }

    /// One of the stub's cells.
    pub(crate) fn is_cell(self) -> bool {
        self.is_block() && !self.is_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_round_trip_in_both_forms() {
        for n in [0.0, -0.0, 1.0, -7.0, 0.5, f64::INFINITY, 1e300, 2147483648.0] {
            let v = Value::number(n);
            assert!(!v.is_block());
            assert_eq!(v.as_number().unwrap().to_bits(), n.to_bits());
        }
        assert!(Value::number(f64::NAN).as_number().unwrap().is_nan());
        for v in [NULL, UNDEFINED, TRUE, FALSE, THROWN] {
            assert_eq!(v.as_number(), None);
            assert!(!v.is_block());
        }
    }
}
