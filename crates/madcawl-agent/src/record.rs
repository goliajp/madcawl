//! One node's semantic record, 16 bytes in a side table indexed by the
//! node's slot, and the states it keeps as flags.

use std::mem::size_of;

use crate::role::Role;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Record {
    /// The name, in the string pool; 0 when none
    pub name: u32,
    /// The current value, a link's address: in the string pool
    pub value: u32,
    /// Bumped each time the slot is freed, so references to the node
    /// that had it go stale
    pub generation: u32,
    pub role: Role,
    /// A heading's level, 0 for other roles
    pub level: u8,
    pub flags: u16,
}

const _: () = assert!(size_of::<Record>() <= 16);

/// The node is in the document.
pub const IN_TREE: u16 = 1;
/// Shown to a user: rendered, `visibility: visible`, not fully
/// transparent, not without area, not clipped away.
pub const VISIBLE: u16 = 1 << 1;
pub const DISABLED: u16 = 1 << 2;
pub const CHECKED: u16 = 1 << 3;
/// Checked, or pressed, is "mixed"
pub const MIXED: u16 = 1 << 4;
pub const EXPANDED: u16 = 1 << 5;
/// `aria-expanded="false"` or a closed disclosure
pub const COLLAPSED: u16 = 1 << 6;
pub const PRESSED: u16 = 1 << 7;
pub const REQUIRED: u16 = 1 << 8;
pub const READONLY: u16 = 1 << 9;
pub const SELECTED: u16 = 1 << 10;
pub const CURRENT: u16 = 1 << 11;
/// The name came from an attribute (`aria-label`, `aria-labelledby`,
/// `title`) and is not the text a user sees
pub const NAME_ATTR: u16 = 1 << 12;
/// A password field: its value is never given out
pub const PASSWORD: u16 = 1 << 13;
/// The name may come from the content: a change below changes it
pub const NAME_CONTENT: u16 = 1 << 14;

/// The states a diff or a view writes, in order, with their words.
pub(crate) const STATE_WORDS: [(u16, &str); 10] = [
    (DISABLED, "disabled"),
    (CHECKED, "checked"),
    (MIXED, "mixed"),
    (EXPANDED, "expanded"),
    (COLLAPSED, "collapsed"),
    (PRESSED, "pressed"),
    (REQUIRED, "required"),
    (READONLY, "readonly"),
    (SELECTED, "selected"),
    (CURRENT, "current"),
];

/// The flags that change what a view shows of the node.
pub(crate) const SHOWN_FLAGS: u16 = IN_TREE
    | VISIBLE
    | DISABLED
    | CHECKED
    | MIXED
    | EXPANDED
    | COLLAPSED
    | PRESSED
    | REQUIRED
    | READONLY
    | SELECTED
    | CURRENT
    | NAME_ATTR;

impl Record {
    pub fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }

    /// In the document and shown.
    pub fn shown(&self) -> bool {
        self.flags & (IN_TREE | VISIBLE) == IN_TREE | VISIBLE
    }

    /// The same as `other` as far as a view can tell.
    pub(crate) fn same_view(&self, other: &Record) -> bool {
        self.name == other.name
            && self.value == other.value
            && self.role == other.role
            && self.level == other.level
            && self.flags & SHOWN_FLAGS == other.flags & SHOWN_FLAGS
    }
}
