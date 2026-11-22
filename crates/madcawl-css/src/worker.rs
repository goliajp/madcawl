//! Parsing stylesheets off the main thread: one worker thread, a queue
//! of jobs and a queue of results, both bounded. Submitting never
//! blocks: a full queue hands the source back. The parsed sheets come
//! back in the order they were submitted. The thread waits on its queue
//! when there is nothing to do, and exits when the `Worker` is dropped.
//!
//! A parsed sheet's names are the sheet's own; the main thread turns
//! them into atoms (`Names::atoms`) when it takes the sheet.

use std::cell::Cell;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::thread::JoinHandle;

use madcawl_html::Encoding;

use crate::bytes::decode_stylesheet;
use crate::{SourceTooLarge, Stylesheet};

pub enum Source {
    Text(String),
    /// Bytes to decode first, with the response's charset label and the
    /// referring document's encoding
    Bytes {
        bytes: Vec<u8>,
        protocol: Option<Vec<u8>>,
        environment: Option<Encoding>,
    },
}

impl Source {
    /// Whether the text may fit the 4 GiB a stylesheet can have. Bytes
    /// decode to at most three times as many: a byte of a legacy
    /// encoding can become three bytes of UTF-8.
    fn fits(&self) -> bool {
        match self {
            Source::Text(t) => t.len() <= u32::MAX as usize,
            Source::Bytes { bytes, .. } => bytes.len() <= u32::MAX as usize / 3,
        }
    }
}

pub struct Parsed {
    pub id: u64,
    pub sheet: Result<Stylesheet, SourceTooLarge>,
}

/// Why `submit` did not take a source; it comes back.
pub enum Refused {
    /// The queue is full: take some results first
    Full(Source),
    TooLarge(Source),
}

pub struct Worker {
    jobs: Option<SyncSender<(u64, Source)>>,
    done: Option<Receiver<Parsed>>,
    thread: Option<JoinHandle<()>>,
    /// Submitted and not yet received
    pending: Cell<usize>,
}

impl Worker {
    /// A worker whose queues hold `queue` jobs and `queue` results.
    pub fn new(queue: usize) -> Worker {
        let (jobs, inbox) = sync_channel::<(u64, Source)>(queue);
        let (out, done) = sync_channel(queue);
        let thread = std::thread::Builder::new()
            .name("css-parse".into())
            .spawn(move || {
                for (id, source) in inbox {
                    let text = match source {
                        Source::Text(t) => t,
                        Source::Bytes {
                            bytes,
                            protocol,
                            environment,
                        } => decode_stylesheet(&bytes, protocol.as_deref(), environment).0,
                    };
                    let sheet = Stylesheet::parse(text);
                    if out.send(Parsed { id, sheet }).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn the stylesheet worker");
        Worker {
            jobs: Some(jobs),
            done: Some(done),
            thread: Some(thread),
            pending: Cell::new(0),
        }
    }

    /// Queue a stylesheet; `id` comes back with it.
    pub fn submit(&self, id: u64, source: Source) -> Result<(), Refused> {
        if !source.fits() {
            return Err(Refused::TooLarge(source));
        }
        let jobs = self.jobs.as_ref().expect("a live worker has a queue");
        match jobs.try_send((id, source)) {
            Ok(()) => {
                self.pending.set(self.pending.get() + 1);
                Ok(())
            }
            Err(TrySendError::Full((_, s))) => Err(Refused::Full(s)),
            Err(TrySendError::Disconnected(_)) => panic!("the stylesheet worker died"),
        }
    }

    fn results(&self) -> &Receiver<Parsed> {
        self.done.as_ref().expect("a live worker has its results")
    }

    /// The next parsed sheet, waiting for it; `None` when nothing
    /// submitted is still to come.
    pub fn recv(&self) -> Option<Parsed> {
        if self.pending.get() == 0 {
            return None;
        }
        let p = self.results().recv().expect("the stylesheet worker died");
        self.pending.set(self.pending.get() - 1);
        Some(p)
    }

    /// The next parsed sheet, if one is ready.
    pub fn try_recv(&self) -> Option<Parsed> {
        match self.results().try_recv() {
            Ok(p) => {
                self.pending.set(self.pending.get() - 1);
                Some(p)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!("the stylesheet worker died"),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // with both ends gone the thread's next receive or send fails,
        // also when it waits to hand back a result
        self.jobs = None;
        self.done = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
