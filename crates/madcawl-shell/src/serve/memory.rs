//! The content process's memory cap. Past the soft cap (three quarters
//! of the hard one) the process drops what it can make again: the
//! shared stylesheet cache, whose sheets are parsed again when a page
//! asks. It does so once, and again only after the footprint has gone
//! back below the rearm mark (three fifths), so a process sitting above
//! the soft cap does not throw its sheets away on every request. Past the hard cap it exits with `EXIT_CODE`, so its host loads
//! its pages again in a new process (`proxy`) instead of the machine
//! running short. The footprint is read after every request and, while a
//! request runs, by the watchdog's periodic check; an idle process reads
//! nothing.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::rusage;

/// The exit status of a content process that went over its hard cap.
pub const EXIT_CODE: i32 = 88;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cap {
    pub hard: u64,
    pub soft: u64,
    pub rearm: u64,
}

impl Cap {
    pub fn new(hard: u64) -> Cap {
        Cap { hard, soft: hard / 4 * 3, rearm: hard / 5 * 3 }
    }

    /// The default: an eighth of the machine's memory, at least 1 GiB.
    pub fn for_machine() -> Cap {
        Cap::new((rusage::physical_memory() / 8).max(1 << 30))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Level {
    Under,
    Soft,
    Hard,
}

pub fn level(cap: Cap, footprint: u64) -> Level {
    match footprint {
        f if f >= cap.hard => Level::Hard,
        f if f >= cap.soft => Level::Soft,
        _ => Level::Under,
    }
}

/// Set once the process is on its way out, so only one thread says so.
static LEAVING: AtomicBool = AtomicBool::new(false);
/// Whether passing the soft cap drops the stylesheet cache.
static ARMED: AtomicBool = AtomicBool::new(true);

/// Whether a footprint `fp` at the soft cap drops the cache now; one
/// under the rearm mark lets the next one do so.
fn drop_now(armed: &AtomicBool, cap: Cap, fp: u64) -> bool {
    match level(cap, fp) {
        Level::Soft => armed.swap(false, Ordering::SeqCst),
        _ if fp < cap.rearm => {
            armed.store(true, Ordering::SeqCst);
            false
        }
        _ => false,
    }
}

/// Reads this process's footprint against `cap`: logs what was dropped
/// past the soft cap, and past the hard one logs and exits.
pub fn check(cap: Cap, session: u32) {
    let fp = rusage::footprint();
    match level(cap, fp) {
        Level::Hard => {
            if !LEAVING.swap(true, Ordering::SeqCst) {
                eprintln!(
                    "madcawl-shell: session {session}: memory {} MiB, over the cap of {} MiB: exiting so the pages load again in a new process",
                    fp >> 20,
                    cap.hard >> 20
                );
                std::process::exit(EXIT_CODE);
            }
        }
        _ if drop_now(&ARMED, cap, fp) => match super::doc::drop_sheets() {
            0 => {}
            w => eprintln!(
                "madcawl-shell: session {session}: memory over the soft cap of {} MiB: dropped the stylesheet cache ({} MiB by its estimate)",
                cap.soft >> 20,
                w >> 20
            ),
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels() {
        let cap = Cap::new(400);
        assert_eq!(cap.soft, 300);
        assert_eq!(level(cap, 299), Level::Under);
        assert_eq!(level(cap, 300), Level::Soft);
        assert_eq!(level(cap, 400), Level::Hard);
        assert!(Cap::for_machine().hard >= 1 << 30);
    }

    #[test]
    fn the_cache_is_dropped_once_past_the_soft_cap_until_the_footprint_falls_back() {
        let (cap, armed) = (Cap::new(1000), AtomicBool::new(true));
        assert_eq!((cap.soft, cap.rearm), (750, 600));
        assert!(drop_now(&armed, cap, 800));
        assert!(!drop_now(&armed, cap, 900), "still above: not again");
        assert!(!drop_now(&armed, cap, 700), "below the soft cap, above the rearm mark");
        assert!(!drop_now(&armed, cap, 800), "not rearmed yet");
        assert!(!drop_now(&armed, cap, 599));
        assert!(drop_now(&armed, cap, 760), "rearmed");
    }
}
