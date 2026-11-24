//! Character data: the byte limit, text nodes and merging into them.
//!
//! Text that merges into an existing text node (the parser adds text
//! next to a text node it made earlier) is appended to a buffer kept
//! for that one node and becomes its `DomString` when the node is read
//! or another node starts merging. Rebuilding the string on every merge
//! would make a page of many small merges cost the square of its size.

use std::ptr::NonNull;

use madcawl_str::DomString;

use crate::dom::{Dom, Refused};
use crate::node::NodeKind;
use crate::tree::Change;

/// The node whose text is `text` (WTF-8), newer than its `DomString`.
pub(crate) struct Tail {
    node: u32,
    text: Vec<u8>,
}

/// Whitespace-only text up to this long shares a string.
const SPACES_LEN: usize = 32;
const SPACES_KEPT: usize = 16;

/// The length of `s` in WTF-8, which is what storing it was counted as.
pub(crate) fn wtf8_len(s: &DomString) -> usize {
    if let Some(b) = s.as_latin1() {
        return b.len() + b.iter().filter(|&&c| c >= 0x80).count();
    }
    let (mut n, mut high) = (0, false);
    for u in s.units() {
        let low = (0xDC00..0xE000).contains(&u);
        n += match u {
            0..0x80 => 1,
            0x80..0x800 => 2,
            // the pair's four bytes: three for the high half, one more
            _ if low && high => 1,
            _ => 3,
        };
        high = (0xD800..0xDC00).contains(&u);
    }
    n
}

/// Whether `s` holds a high surrogate followed by a low one, each as
/// three bytes: the form WTF-8 joins into one four-byte code point.
pub(crate) fn split_pair(s: &[u8]) -> bool {
    !s.is_ascii()
        && s.windows(6)
            .any(|w| w[0] == 0xED && w[1] & 0xF0 == 0xA0 && w[3] == 0xED && w[4] & 0xF0 == 0xB0)
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

impl Dom {
    /// The part of `s` that fits under the byte limit, cut at a
    /// character boundary, and counted.
    pub(crate) fn admit<'a>(&mut self, s: &'a [u8]) -> &'a [u8] {
        let room = self.limits.bytes - self.bytes;
        if s.len() <= room {
            self.bytes += s.len();
            return s;
        }
        let mut k = room;
        while k > 0 && s[k] & 0xC0 == 0x80 {
            k -= 1;
        }
        self.bytes += k;
        self.cut = Some(Refused::Bytes);
        &s[..k]
    }

    /// Give back what `s`, a value no longer stored, was counted as.
    pub(crate) fn refund(&mut self, s: &DomString) {
        self.bytes -= wtf8_len(s);
    }

    /// Count `n` bytes of bookkeeping, if they fit.
    pub(crate) fn charge(&mut self, n: usize) -> bool {
        if self.limits.bytes - self.bytes < n {
            self.cut = Some(Refused::Bytes);
            return false;
        }
        self.bytes += n;
        true
    }

    pub(crate) fn char_data(&self, i: u32) -> &DomString {
        self.settle(i);
        let p = std::ptr::from_ref(&self.n(i).data);
        // SAFETY: a character data node's data word holds a live block
        // pointer, and a `DomString` is `repr(transparent)` over it
        unsafe { &*p.cast::<DomString>() }
    }

    /// Turn node `i`'s pending text into its string.
    pub(crate) fn settle(&self, i: u32) {
        let mut tail = self.tail.borrow_mut();
        if tail.as_ref().is_none_or(|t| t.node != i) {
            return;
        }
        let t = tail.take().expect("just checked");
        let s = DomString::from_wtf8(&t.text).into_raw().as_ptr() as u64;
        let p = self.arena.slot(i);
        // SAFETY: the slot is a live text node; no reference into it is
        // held while its text is pending, since every read settles first
        unsafe {
            let old = std::mem::replace(&mut (*p).data, s);
            drop_string(old);
        }
    }

    /// A string for new text, shared when it is short whitespace.
    fn text_string(&mut self, s: &[u8]) -> DomString {
        if s.len() > SPACES_LEN || !s.iter().all(|&b| is_space(b)) {
            return self.short.get(s);
        }
        if let Some(x) = self.spaces.iter().find(|x| x.as_latin1() == Some(s)) {
            return x.clone();
        }
        let x = DomString::from_latin1(s);
        if self.spaces.len() < SPACES_KEPT {
            self.spaces.push(x.clone());
        }
        x
    }

    pub(crate) fn new_char_data(
        &mut self,
        kind: NodeKind,
        owner: u32,
        s: DomString,
    ) -> Option<u32> {
        let i = self.new_node(kind, owner)?;
        self.m(i).data = s.into_raw().as_ptr() as u64;
        Some(i)
    }

    /// Add text before `before` (or at the end when 0), into the text
    /// node in front of that place if there is one.
    pub(crate) fn add_text(&mut self, parent: u32, before: u32, text: &[u8]) {
        assert!(
            self.kind_of(parent).is_container(),
            "text goes into a container"
        );
        let text = self.admit(text);
        if text.is_empty() {
            return;
        }
        let prev = if before == 0 {
            self.n(parent).last()
        } else {
            self.n(before).prev
        };
        if prev != 0 && self.kind_of(prev) == NodeKind::Text {
            self.merge(prev, text);
            self.notify(Change::Text(prev));
            return;
        }
        let s = self.text_string(text);
        let t = self
            .new_char_data(NodeKind::Text, self.doc_of(parent), s)
            .expect("the parser stops while there is room for its nodes");
        self.insert(parent, t, before);
    }

    fn merge(&mut self, t: u32, text: &[u8]) {
        let tail = self.tail.get_mut();
        if let Some(tail) = tail.as_mut().filter(|tail| tail.node == t) {
            tail.text.extend_from_slice(text);
            return;
        }
        if let Some(other) = tail.as_ref().map(|tail| tail.node) {
            self.settle(other);
        }
        let mut buf = Vec::new();
        self.char_data(t).write_wtf8(&mut buf);
        buf.extend_from_slice(text);
        *self.tail.get_mut() = Some(Tail { node: t, text: buf });
    }

    /// Give back what node `i` owns besides its slot: its string or its
    /// attribute block.
    pub(crate) fn release_contents(&mut self, i: u32) {
        let n = self.n(i);
        let (kind, data, attrs) = (n.kind(), n.data, n.attrs);
        if kind.is_some_and(NodeKind::is_char_data) {
            if self.tail.get_mut().as_ref().is_some_and(|t| t.node == i) {
                *self.tail.get_mut() = None;
            }
            // SAFETY: the node's own count on its string, given up as
            // the node goes
            unsafe { drop_string(data) };
        } else if attrs != 0 {
            self.drop_entries(i);
            for u in self.attrs.attrs(attrs) {
                u.drop_value();
            }
            self.attrs.release(attrs);
        }
    }
}

/// # Safety
/// `bits` is a block pointer that owns one count, not used again.
pub(crate) unsafe fn drop_string(bits: u64) {
    // SAFETY: the caller's
    drop(unsafe { DomString::from_raw(NonNull::new_unchecked(bits as *mut u8)) });
}
