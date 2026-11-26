//! The host side of collection.
//!
//! The framework's collector traces its own heap and sees nothing of
//! the host's. A wrapper of a node in the document is protected; the
//! wrappers of a detached tree keep one another alive there and die
//! together once script holds none (`groups.rs`). Their finalizers queue
//! notices, and when the host next settles it is told, clears the
//! wrapper words, and the nodes' counts drop; each such node outside the
//! document becomes a suspect.
//!
//! What is left for the adapter is the cycles among host objects alone:
//! a detached tree that no wrapper and no outside count holds any more.
//! [`Jsc::collect_host`] runs the trial deletion of `madcawl_js::trial`
//! over the host graph `traverse` reports, from the suspects, and
//! unlinks what is garbage: a tree whose counts the tree's own edges
//! explain. A host object with a live wrapper has a count no `traverse`
//! explains (the wrapper's), so it is never garbage here. Edges to
//! script values are not part of the graph.

use madcawl_js::trial::SideGraph;
use madcawl_js::{Deadline, Host, HostRef, Owned, Step, Tracer, Unlinker};
use madcawl_str::abi;

use crate::isolate::Jsc;
use crate::value::Value;

#[derive(Default)]
pub struct JscTracer {
    hosts: Vec<HostRef>,
}

impl Tracer<Jsc> for JscTracer {
    fn host(&mut self, obj: HostRef) {
        self.hosts.push(obj);
    }

    fn value(&mut self, _: Value) {}
}

#[derive(Default)]
pub struct JscUnlinker {
    released: Vec<Value>,
}

impl Unlinker<Jsc> for JscUnlinker {
    fn release(&mut self, v: Owned<Value>) {
        self.released.push(v.into_raw());
    }
}

fn count_at(obj: HostRef) -> u32 {
    // SAFETY: a host object starts with the 8-byte engine header, and a
    // suspect or a vertex is alive (freed ones are forgotten)
    abi::refcount(unsafe { *(obj.addr() as *const u64) })
}

impl Jsc {
    pub(crate) fn add_suspect(&mut self, obj: HostRef) {
        if self.suspected.insert(obj) {
            self.suspects.push(obj);
        }
    }

    pub(crate) fn forget_host(&mut self, obj: HostRef) {
        if self.suspected.remove(&obj) {
            self.suspects.retain(|&s| s != obj);
        }
    }

    /// One collection over the suspects, whole: the deadline is not
    /// divided yet.
    pub(crate) fn collect_host<H: Host<Jsc>>(&mut self, host: &mut H, _deadline: Deadline) -> Step {
        let todo = std::mem::take(&mut self.suspects);
        self.suspected.clear();
        let mut graph = SideGraph::new();
        let mut stack: Vec<HostRef> = Vec::new();
        for obj in todo {
            if graph.vertex(obj, count_at(obj)).1 {
                stack.push(obj);
            }
        }
        let mut t = JscTracer::default();
        while let Some(obj) = stack.pop() {
            let ops = host.class_of(obj).host_ops();
            let (i, _) = graph.vertex(obj, count_at(obj));
            if (ops.can_skip)(host, obj) {
                graph.pin(obj);
                continue;
            }
            t.hosts.clear();
            (ops.traverse)(host, obj, &mut t);
            for &to in &t.hosts {
                graph.edge(i, to);
                if !graph.contains(to) && graph.vertex(to, count_at(to)).1 {
                    stack.push(to);
                }
            }
        }
        let garbage = graph.garbage().expect("a traverse that reports only counted edges");
        let mut u = JscUnlinker::default();
        for &obj in &garbage {
            (host.class_of(obj).host_ops().unlink)(host, obj, &mut u);
        }
        for v in u.released {
            self.unprotect(v);
        }
        Step::Done
    }
}
