//! The node arena: one reservation of 2^24 slots of 64 bytes, aligned
//! to its size, so that index and address convert by a shift and the
//! arena of an address is found by a mask.

use std::ptr::NonNull;

use crate::node::Node;
use crate::sys;

/// Slots in the reservation; slot 0 is never used, index 0 is none.
pub(crate) const SLOTS: usize = 1 << 24;
const SLOT_SHIFT: u32 = 6;
pub(crate) const RESERVED: usize = SLOTS << SLOT_SHIFT;
/// Slots in a 16 KiB page, the unit memory is given back in.
pub(crate) const PAGE_SLOTS: usize = 256;

pub(crate) struct Arena {
    base: NonNull<Node>,
    /// Slots below this have been handed out at least once.
    next: u32,
    free: Vec<u32>,
    /// Live nodes per page.
    live: Box<[u16]>,
    count: u32,
    /// Arena pages per system page: 1 where the system page is 16 KiB
    /// or less, 4 on 64 KiB pages.
    group: usize,
}

impl Arena {
    pub(crate) fn new() -> Self {
        let p = sys::reserve_aligned(RESERVED);
        Arena {
            base: NonNull::new(p.cast()).expect("mmap gives a nonnull mapping"),
            next: 1,
            free: Vec::new(),
            live: vec![0; SLOTS / PAGE_SLOTS].into_boxed_slice(),
            count: 0,
            group: (sys::page_size() / (PAGE_SLOTS << SLOT_SHIFT)).max(1),
        }
    }

    /// The arena that `addr`, the address of one of its nodes, is in.
    pub(crate) fn base_of(addr: usize) -> usize {
        addr & !(RESERVED - 1)
    }

    pub(crate) fn addr(&self, i: u32) -> usize {
        self.base.as_ptr() as usize + ((i as usize) << SLOT_SHIFT)
    }

    pub(crate) fn index(&self, addr: usize) -> u32 {
        ((addr - self.base.as_ptr() as usize) >> SLOT_SHIFT) as u32
    }

    /// Live nodes.
    pub(crate) fn count(&self) -> u32 {
        self.count
    }

    /// One past the highest slot ever used.
    pub(crate) fn high(&self) -> u32 {
        self.next
    }

    /// A slot holding `node`, or `None` when all `limit` are taken.
    pub(crate) fn alloc(&mut self, node: Node, limit: u32) -> Option<u32> {
        if self.count >= limit {
            return None;
        }
        let i = match self.free.pop() {
            Some(i) => i,
            None => {
                let i = self.next;
                self.next += 1;
                i
            }
        };
        // SAFETY: i < SLOTS, inside the reservation, and no reference
        // to a free slot is held
        unsafe { self.slot(i).write(node) };
        self.live[i as usize / PAGE_SLOTS] += 1;
        self.count += 1;
        Some(i)
    }

    /// Free slot `i`, whose contents the caller has already released.
    /// A system page left without live nodes is given back.
    pub(crate) fn free(&mut self, i: u32) {
        // SAFETY: as in alloc; a zero kind marks the slot free
        unsafe { (*self.slot(i)).flags = 0 };
        self.free.push(i);
        self.count -= 1;
        let page = i as usize / PAGE_SLOTS;
        self.live[page] -= 1;
        let first = page / self.group * self.group;
        if self.live[first..first + self.group].iter().all(|&n| n == 0) {
            // SAFETY: the range is one system page of the reservation
            // (the reservation is aligned to 1 GiB); every slot in it is
            // free and is written whole before it is read again
            unsafe {
                sys::give_back(
                    self.slot((first * PAGE_SLOTS) as u32).cast(),
                    (self.group * PAGE_SLOTS) << SLOT_SHIFT,
                );
            }
        }
    }

    pub(crate) fn slot(&self, i: u32) -> *mut Node {
        // SAFETY: masked below SLOTS, so the slot is inside the
        // reservation whatever the index
        unsafe { self.base.as_ptr().add(i as usize & (SLOTS - 1)) }
    }

    /// Whether slot `i` holds a node.
    pub(crate) fn is_live(&self, i: u32) -> bool {
        i != 0 && i < self.next && self.get(i).kind().is_some()
    }

    pub(crate) fn get(&self, i: u32) -> &Node {
        // SAFETY: the slot is mapped memory; a slot never written reads
        // as zeros, a valid (free) node
        unsafe { &*self.slot(i) }
    }

    pub(crate) fn get_mut(&mut self, i: u32) -> &mut Node {
        // SAFETY: as in get, and `&mut self` makes the borrow unique
        unsafe { &mut *self.slot(i) }
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: the reservation made in new
        unsafe { sys::release(self.base.as_ptr().cast(), RESERVED) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::NodeKind;

    #[test]
    fn index_and_address_convert_by_shift_and_mask() {
        let a = Arena::new();
        assert_eq!(a.addr(0) % RESERVED, 0, "aligned to its size");
        let addr = a.addr(12345);
        assert_eq!(a.index(addr), 12345);
        assert_eq!(Arena::base_of(addr + 17), a.addr(0));
    }

    #[test]
    fn freed_slots_are_used_again_and_an_empty_page_is_given_back() {
        let mut a = Arena::new();
        let ids: Vec<u32> = (0..PAGE_SLOTS * 2)
            .map(|_| a.alloc(Node::new(NodeKind::Comment, 1), u32::MAX).unwrap())
            .collect();
        assert_eq!(ids[0], 1);
        for &i in &ids[PAGE_SLOTS..] {
            a.free(i);
        }
        assert_eq!(a.count() as usize, PAGE_SLOTS);
        assert!(!a.is_live(ids[PAGE_SLOTS + 3]));
        let again = a.alloc(Node::new(NodeKind::Text, 1), u32::MAX).unwrap();
        assert!(ids[PAGE_SLOTS..].contains(&again));
        assert_eq!(a.get(again).kind(), Some(NodeKind::Text));
        assert_eq!(a.get(again).count(), 0);
    }

    #[test]
    fn no_slot_past_the_limit() {
        let mut a = Arena::new();
        assert!(a.alloc(Node::new(NodeKind::Text, 1), 1).is_some());
        assert!(a.alloc(Node::new(NodeKind::Text, 1), 1).is_none());
    }
}
