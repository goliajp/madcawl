//! How much memory reading a frame takes, measured by a counting
//! allocator; the only test in this binary, so nothing else allocates
//! while it measures.

use madcawl_net::wire::{Frame, MAX_FRAME, kind, tag};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(n: usize) {
    let now = LIVE.fetch_add(n, Ordering::SeqCst) + n;
    PEAK.fetch_max(now, Ordering::SeqCst);
}

// SAFETY: every call goes on to the system allocator unchanged
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        grew(l.size());
        // SAFETY: the caller's layout, passed on
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::SeqCst);
        // SAFETY: allocated by System with this layout
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // a growing realloc may move and hold both blocks for a moment;
        // a shrinking one is counted as done in place
        if new > l.size() {
            grew(new);
        }
        // SAFETY: allocated by System with this layout
        let q = unsafe { System.realloc(p, l, new) };
        LIVE.fetch_sub(if new > l.size() { l.size() } else { l.size() - new }, Ordering::SeqCst);
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

/// The most memory held at once beyond what was live before `f` ran.
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let out = f();
    (out, PEAK.load(Ordering::SeqCst) - base)
}

#[test]
fn reading_a_frame_takes_memory_for_what_arrives_and_no_second_copy() {
    // a header claiming the largest frame, then the stream ends
    let head = (MAX_FRAME as u32).to_le_bytes();
    let (r, peak) = peak_of(|| Frame::read_from(&mut &head[..]).map(|_| ()));
    assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
    assert!(peak < 1 << 20, "{peak} bytes for a frame that never came");

    let body = vec![7u8; 4 << 20];
    let mut wire = Vec::new();
    Frame::new(kind::FETCH).num(tag::ID, 1).str(tag::URL, "http://a/").bytes(tag::BODY, &body).encode(&mut wire);
    let (f, peak) = peak_of(|| Frame::read_from(&mut &wire[..]).unwrap().unwrap());
    // the body arrives into one buffer that grows; holding it and a copy
    // of it at once would take twice its size
    assert!(peak < body.len() * 7 / 4, "{peak} bytes to read a {} byte frame", wire.len());
    let mut f = f;
    assert_eq!(f.get_str(tag::URL), Some("http://a/"));
    assert_eq!(f.take(tag::BODY), Some(body));
    assert_eq!(f.field(tag::BODY), None);
}
