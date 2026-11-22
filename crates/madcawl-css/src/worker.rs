//! Parsing stylesheets off the main thread: one worker thread, a
//! bounded queue of jobs (submitting blocks while it is full), and the
//! parsed sheets handed back in the order they were submitted. The
//! thread waits on its queue when there is nothing to do, and exits
//! when the `Worker` is dropped.
//!
//! A parsed sheet's names are the sheet's own; the main thread turns
//! them into atoms (`Names::atoms`) when it takes the sheet.

use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, channel, sync_channel};
use std::thread::JoinHandle;

use madcawl_html::Encoding;

use crate::Stylesheet;
use crate::bytes::decode_stylesheet;

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

pub struct Parsed {
    pub id: u64,
    pub sheet: Stylesheet,
}

pub struct Worker {
    jobs: Option<SyncSender<(u64, Source)>>,
    done: Receiver<Parsed>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// A worker whose queue holds `queue` jobs.
    pub fn new(queue: usize) -> Worker {
        let (jobs, inbox) = sync_channel::<(u64, Source)>(queue);
        let (out, done) = channel();
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
            done,
            thread: Some(thread),
        }
    }

    /// Queue a stylesheet; `id` comes back with it.
    pub fn submit(&self, id: u64, source: Source) {
        let jobs = self.jobs.as_ref().expect("a live worker has a queue");
        jobs.send((id, source)).expect("the worker thread is alive");
    }

    /// The next parsed sheet, waiting for it.
    pub fn recv(&self) -> Parsed {
        self.done.recv().expect("the worker thread is alive")
    }

    /// The next parsed sheet, if one is ready.
    pub fn try_recv(&self) -> Option<Parsed> {
        match self.done.try_recv() {
            Ok(p) => Some(p),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!("the stylesheet worker died"),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
