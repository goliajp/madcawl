//! Strings that live for the whole program, readable from any thread.

use crate::abi;

use crate::block::{Block, View};

/// A string block marked `FLAG_STATIC`: never counted, never freed and
/// never written, so any thread may read it at any time. The static
/// atoms are these, in read-only data.
#[derive(Clone, Copy)]
pub struct StaticStr(Block);

// SAFETY: the block is immutable for the life of the program; nothing
// writes its header, length or payload, so reads from many threads
// never race with a write
unsafe impl Send for StaticStr {}
// SAFETY: as for `Send`
unsafe impl Sync for StaticStr {}

impl StaticStr {
    /// # Safety
    /// `block` is a string block with `FLAG_STATIC` set that stays
    /// valid and unchanged for the rest of the program.
    pub(crate) unsafe fn new(block: Block) -> Self {
        debug_assert!(block.flags() & abi::FLAG_STATIC_LITERAL != 0);
        Self(block)
    }

    pub(crate) fn block(self) -> Block {
        self.0
    }

    fn view(self) -> View<'static> {
        // SAFETY: a static block outlives every borrow
        unsafe { self.0.view() }
    }

    /// Length in UTF-16 code units.
    pub fn len(self) -> usize {
        self.view().len()
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn is_latin1(self) -> bool {
        self.view().latin1
    }

    pub fn is_ascii(self) -> bool {
        self.view().ascii
    }

    pub fn as_latin1(self) -> Option<&'static [u8]> {
        self.view().as_latin1()
    }

    pub fn as_ascii_str(self) -> Option<&'static str> {
        self.view().as_ascii_str()
    }

    pub fn unit(self, i: usize) -> u16 {
        self.view().unit(i)
    }

    pub fn units(self) -> impl ExactSizeIterator<Item = u16> {
        self.view().units()
    }

    pub fn write_wtf8(self, out: &mut Vec<u8>) {
        self.view().write_wtf8(out);
    }

    pub fn eq_str(self, s: &str) -> bool {
        self.view().eq_str(s)
    }
}

impl PartialEq for StaticStr {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 || self.view().eq(other.view())
    }
}

impl Eq for StaticStr {}

impl std::hash::Hash for StaticStr {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.view().hash(h);
    }
}

impl std::fmt::Debug for StaticStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut v = Vec::new();
        self.write_wtf8(&mut v);
        write!(f, "{:?}", String::from_utf8_lossy(&v))
    }
}
