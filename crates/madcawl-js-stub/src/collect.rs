//! The cycle collector: trial deletion over a side graph, built in
//! batches by deadline, checked again in one step, then unlinked in
//! batches. The step that decides also marks the garbage wrappers dead,
//! so that `wrapper_upgrade` refuses them while the unlinking goes on.
//!
//! Candidates are cells whose count dropped and stayed above zero, and
//! host objects the host suspects. The graph's vertices are cells and
//! host objects; strings close no cycle and are left out. A host object
//! the host says can be skipped (a node in an active document) is not
//! entered: an edge to it counts as held from outside.
//!
//! The headers are only read: colours and the buffered mark live in the
//! collector's own tables, since the header bits for them are not yet
//! agreed with the engine. A vertex freed while a collection runs is
//! forgotten (`forget`) and counts as alive; the final check over the
//! garbage set reads counts and edges again, so what script did between
//! batches cannot make a live object look dead.

use std::collections::HashSet;
use std::time::Instant;

use madcawl_abi as abi;
use madcawl_js::trial::SideGraph;
use madcawl_js::{Deadline, Host, HostRef, Owned, Step, Tracer, Unlinker};

use crate::cell::{Body, Cell};
use crate::isolate::Stub;
use crate::value::Value;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Vertex {
    Cell(Value),
    Host(HostRef),
}

/// What the last finished collection did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollectStats {
    /// Vertices put in the graph.
    pub visited: u32,
    /// Host objects not entered because the host could skip them.
    pub skipped: u32,
    /// Vertices the final check found to be garbage and unlinked.
    pub garbage: u32,
    /// Garbage cells still alive after the unlinks were released: an
    /// edge some `traverse` reported but does not count. Must be 0.
    pub survivors: u32,
    pub batches: u32,
}

#[derive(Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Build {
        graph: SideGraph<Vertex>,
        todo: Vec<Vertex>,
    },
    Unlink {
        garbage: Vec<Vertex>,
        next: usize,
    },
}

#[derive(Default)]
pub(crate) struct Collector {
    pub phase: Phase,
    /// A collection is under way (its phase may be taken out while a
    /// batch runs).
    pub running: bool,
    /// Vertices freed while this collection runs.
    pub gone: HashSet<Vertex>,
    pub stats: CollectStats,
    pub last: CollectStats,
}

impl Collector {
    pub(crate) fn forget(&mut self, v: Vertex) {
        if self.running {
            self.gone.insert(v);
        }
    }
}

#[derive(Default)]
pub struct StubTracer {
    edges: Vec<Vertex>,
}

impl StubTracer {
    /// The host objects reported so far.
    pub fn hosts(&self) -> impl Iterator<Item = HostRef> + '_ {
        self.edges.iter().filter_map(|v| match v {
            Vertex::Host(h) => Some(*h),
            Vertex::Cell(_) => None,
        })
    }

    /// The script values reported so far.
    pub fn values(&self) -> impl Iterator<Item = Value> + '_ {
        self.edges.iter().filter_map(|v| match v {
            Vertex::Cell(c) => Some(*c),
            Vertex::Host(_) => None,
        })
    }
}

impl Tracer<Stub> for StubTracer {
    fn host(&mut self, obj: HostRef) {
        self.edges.push(Vertex::Host(obj));
    }

    fn value(&mut self, v: Value) {
        if v.is_cell() {
            self.edges.push(Vertex::Cell(v));
        }
    }
}

#[derive(Default)]
pub struct StubUnlinker {
    released: Vec<Value>,
}

impl Unlinker<Stub> for StubUnlinker {
    fn release(&mut self, v: Owned<Value>) {
        self.released.push(v.into_raw());
    }
}

struct Budget {
    deadline: Deadline,
    used: u32,
}

impl Budget {
    fn spent(&mut self) -> bool {
        self.used += 1;
        match self.deadline {
            Deadline::Work(n) => self.used > n,
            Deadline::At(t) => self.used.is_multiple_of(64) && Instant::now() >= t,
        }
    }
}

fn count_at(v: Vertex) -> u32 {
    let addr = match v {
        Vertex::Cell(c) => c.0 as usize,
        Vertex::Host(h) => h.addr(),
    };
    // SAFETY: a live vertex starts with the engine object header
    abi::refcount(unsafe { *(addr as *const u64) })
}

impl Stub {
    /// What the last finished collection did.
    pub fn last_collection(&self) -> CollectStats {
        self.collector.last
    }

    /// Whether a collection stopped between two batches of unlinks.
    pub fn unlinking(&self) -> bool {
        matches!(self.collector.phase, Phase::Unlink { .. })
    }

    /// The counted edges out of `v`, as they are now.
    fn edges<H: Host<Stub>>(&self, host: &H, v: Vertex, out: &mut StubTracer) {
        match v {
            Vertex::Cell(c) => {
                let mut held = Vec::new();
                // SAFETY: a live cell, read only
                let cell = unsafe { Cell::at(c) };
                cell.held(&mut held);
                for h in held {
                    out.value(h);
                }
                if let Body::Wrapper { obj, orphan: false, .. } = cell.body {
                    out.host(obj);
                }
            }
            Vertex::Host(h) => (host.class_of(h).host_ops().traverse)(host, h, out),
        }
    }

    /// Whether the collector enters `v`.
    fn enters<H: Host<Stub>>(&mut self, host: &H, v: Vertex) -> bool {
        if self.collector.gone.contains(&v) {
            return false;
        }
        match v {
            Vertex::Cell(_) => true,
            Vertex::Host(h) => {
                let skip = (host.class_of(h).host_ops().can_skip)(host, h);
                self.collector.stats.skipped += skip as u32;
                !skip
            }
        }
    }

    pub(crate) fn collect<H: Host<Stub>>(&mut self, host: &mut H, deadline: Deadline) -> Step {
        let mut budget = Budget { deadline, used: 0 };
        self.collector.stats.batches += 1;
        loop {
            match std::mem::take(&mut self.collector.phase) {
                Phase::Idle => {
                    if !self.start(host) {
                        return Step::Done;
                    }
                }
                Phase::Build { mut graph, mut todo } => {
                    while let Some(v) = todo.pop() {
                        // freed since it was queued, in an earlier batch
                        if self.collector.gone.contains(&v) {
                            continue;
                        }
                        let mut t = StubTracer::default();
                        self.edges(host, v, &mut t);
                        let (from, _) = graph.vertex(v, 0);
                        for e in t.edges {
                            graph.edge(from, e);
                            if !graph.contains(e) && self.enters(host, e) {
                                graph.vertex(e, count_at(e));
                                todo.push(e);
                            }
                        }
                        if budget.spent() && !todo.is_empty() {
                            self.collector.phase = Phase::Build { graph, todo };
                            return Step::More;
                        }
                    }
                    for &g in &self.collector.gone {
                        graph.pin(g);
                    }
                    self.collector.stats.visited = graph.len() as u32;
                    let white = graph.garbage_batched();
                    let garbage = self.check(host, &white);
                    self.collector.phase = Phase::Unlink { garbage, next: 0 };
                }
                Phase::Unlink { garbage, mut next } => {
                    self.deferred = Some(Vec::new());
                    while next < garbage.len() {
                        self.unlink(host, garbage[next]);
                        next += 1;
                        if budget.spent() && next < garbage.len() {
                            self.flush();
                            self.collector.phase = Phase::Unlink { garbage, next };
                            return Step::More;
                        }
                    }
                    self.flush();
                    self.finish(&garbage);
                    return Step::Done;
                }
            }
        }
    }

    /// Take the candidates into a new graph; false when there are none.
    fn start<H: Host<Stub>>(&mut self, host: &H) -> bool {
        let roots: Vec<Vertex> = std::mem::take(&mut self.buffer)
            .into_iter()
            .filter(|v| self.buffered.remove(v))
            .collect();
        if roots.is_empty() {
            return false;
        }
        self.collector.stats = CollectStats { batches: 1, ..CollectStats::default() };
        self.collector.running = true;
        let mut graph = SideGraph::new();
        let mut todo = Vec::new();
        for v in roots {
            if !graph.contains(v) && self.enters(host, v) {
                graph.vertex(v, count_at(v));
                todo.push(v);
            }
        }
        // a root entered later as another's edge already has its count
        self.collector.phase = Phase::Build { graph, todo };
        true
    }

    /// The final check: the same decision over the white set alone,
    /// with counts and edges read now.
    fn check<H: Host<Stub>>(&mut self, host: &H, white: &[Vertex]) -> Vec<Vertex> {
        let mut g = SideGraph::new();
        let live: Vec<Vertex> = white
            .iter()
            .copied()
            .filter(|v| !self.collector.gone.contains(v))
            .collect();
        for &v in &live {
            g.vertex(v, count_at(v));
        }
        for &v in &live {
            let mut t = StubTracer::default();
            self.edges(host, v, &mut t);
            let (from, _) = g.vertex(v, 0);
            for e in t.edges {
                g.edge(from, e);
            }
        }
        let garbage = g.garbage().unwrap_or_else(|e| panic!("{e:?}"));
        // in the same step as the verdict: between the unlink batches to
        // come, a node's wrapper word must not hand script a wrapper whose
        // properties a later batch takes
        for &v in &garbage {
            if let Vertex::Cell(c) = v {
                self.clear_weak(c);
                // SAFETY: a live cell
                if let Body::Wrapper { condemned, .. } = &mut unsafe { Cell::at(c) }.body {
                    *condemned = true;
                }
            }
        }
        garbage
    }

    fn unlink<H: Host<Stub>>(&mut self, host: &mut H, v: Vertex) {
        if self.collector.gone.contains(&v) {
            return;
        }
        match v {
            Vertex::Cell(c) => {
                let mut held = Vec::new();
                // SAFETY: a live cell
                unsafe { Cell::at(c) }.take_held(&mut held);
                self.deferred.as_mut().expect("in a batch").extend(held);
            }
            Vertex::Host(h) => {
                let mut u = StubUnlinker::default();
                (host.class_of(h).host_ops().unlink)(host, h, &mut u);
                self.deferred.as_mut().expect("in a batch").extend(u.released);
            }
        }
    }

    /// Release what the batch's unlinks gave up.
    fn flush(&mut self) {
        for v in self.deferred.take().unwrap_or_default() {
            self.release_value(v);
        }
    }

    fn finish(&mut self, garbage: &[Vertex]) {
        let mut stats = self.collector.stats;
        stats.garbage = garbage.len() as u32;
        stats.survivors = garbage
            .iter()
            .filter(|v| match v {
                Vertex::Cell(c) if !self.collector.gone.contains(v) => {
                    // SAFETY: not freed, so a live cell
                    !matches!(unsafe { Cell::at(*c) }.body, Body::Wrapper { dead: true, .. })
                }
                _ => false,
            })
            .count() as u32;
        self.collector.last = stats;
        self.collector.gone.clear();
        self.collector.running = false;
    }
}
