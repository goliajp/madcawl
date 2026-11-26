//! The content process's memory cap. Past the soft cap (three quarters
//! of the hard one) the process drops what it can make again: the
//! shared stylesheet cache, whose sheets are parsed again when a page
//! asks. Past the hard cap it exits with `EXIT_CODE`, so its host loads
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
}

impl Cap {
    pub fn new(hard: u64) -> Cap {
        Cap { hard, soft: hard / 4 * 3 }
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

/// Reads this process's footprint against `cap`: logs what was dropped
/// past the soft cap, and past the hard one logs and exits.
pub fn check(cap: Cap, session: u32) {
    let fp = rusage::footprint();
    match level(cap, fp) {
        Level::Under => {}
        Level::Soft => match super::doc::drop_sheets() {
            0 => {}
            w => eprintln!(
                "madcawl-shell: session {session}: memory over the soft cap of {} MiB: dropped the stylesheet cache ({} MiB by its estimate)",
                cap.soft >> 20,
                w >> 20
            ),
        },
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
}
