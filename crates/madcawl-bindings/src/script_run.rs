//! Running script elements once they are ready (`scripts.rs` prepares
//! and fetches them), their `load` and `error` events, and the end of
//! loading: `defer` scripts once parsing is done, `DOMContentLoaded`,
//! then `load` at the window once no script that may still run is
//! awaited. Also where script elements that script inserts, fills or
//! gives a `src` are prepared.

use madcawl_dom::{HeldNode, NodeId};
use madcawl_js::{Deadline, Engine, Halt, Host, Owned};
use madcawl_str::{Atom, DomString, atom};

use crate::event::{EventState, Target};
use crate::event_loop::{COLLECT_SLICE_NS, Task};
use crate::host::DomHost;
use crate::scripts::{Phase, ScriptTask, Wait, is_script};
use crate::side::SideData;

impl<E: Engine> DomHost<E> {
    /// The standard's "execute the script element": `source` `None`
    /// fires `error`; otherwise the script runs with `el` as
    /// `document.currentScript`, then `load` fires when it came from a
    /// file.
    pub(crate) fn execute_script(&mut self, e: &mut E, el: NodeId, source: Option<DomString>, url: &DomString, external: bool) {
        // the trees script may reach are placed before it runs
        self.settle(e);
        let held = self.dom.hold(el);
        match source {
            None => self.fire_simple(e, Target::Node(el), atom!("error"), false),
            Some(src) => {
                let prev = self.scripts.current.replace(el);
                self.run_script(e, &src, url);
                self.scripts.current = prev;
                if external {
                    self.fire_simple(e, Target::Node(el), atom!("load"), false);
                }
            }
        }
        self.dom.let_go(held);
        self.settle(e);
    }

    /// What follows every task: its exception reported, a checkpoint;
    /// whether its script, or a promise job it queued, was aborted.
    pub(crate) fn finish_task(&mut self, e: &mut E, r: Owned<E::Value>) -> bool {
        if E::is_thrown(r.get()) {
            let _sentinel = r.into_raw();
            if e.halt() == Halt::Running
                && let Some(x) = e.take_exception()
            {
                self.report_exception(e, x);
            }
        } else {
            e.release(r);
        }
        let stopped = self.recover(e);
        if e.halt() == Halt::Running {
            self.checkpoint(e);
        }
        self.settle(e);
        // the trees whose last wrapper died since are freed here
        if self.collect_due {
            let deadline = Deadline::At(self.now().saturating_add(COLLECT_SLICE_NS));
            self.collect(e, deadline);
        }
        stopped
    }

    /// Fire a trusted event of type `ty` that cannot be canceled at
    /// `target`, with a checkpoint after it when no script is running.
    pub(crate) fn fire_simple(&mut self, e: &mut E, target: Target, ty: Atom, bubbles: bool) {
        let mut ev = EventState::new(ty, self.now_ms());
        (ev.trusted, ev.bubbles) = (true, bubbles);
        let r = self.sides.add(SideData::Event(ev));
        let _not_canceled = self.dispatch(e, r, target);
        self.recover(e);
        if !e.in_script() && e.halt() == Halt::Running {
            self.checkpoint(e);
        }
        self.settle(e);
    }

    /// Take script `id` off the pending set and run it.
    fn run_pending(&mut self, e: &mut E, id: u64) {
        let p = self.scripts.pending.remove(&id).expect("a pending script");
        if p.wait == Wait::Soon {
            self.scripts.soon -= 1;
        }
        let el = p.el.id();
        let source = p.result.expect("a ready script");
        self.execute_script(e, el, source, &p.url, true);
        self.dom.let_go(p.el);
        if let Some(w) = p.wrapper {
            w.release(e);
        }
    }

    /// The parser's pending parsing-blocking script, ready, runs with an
    /// insertion point for what it writes.
    pub(crate) fn run_blocking(&mut self, e: &mut E, id: u64) {
        self.scripts.blocking = None;
        let p = self.scripts.parser.as_mut().expect("the parser");
        p.tok.begin_script();
        p.nesting += 1;
        self.run_pending(e, id);
        let p = self.scripts.parser.as_mut().expect("the parser");
        p.nesting -= 1;
        p.tok.end_script();
    }

    pub(crate) fn script_task(&mut self, e: &mut E, t: ScriptTask) {
        match t {
            ScriptTask::Fetched(id) => {
                let Some(p) = self.scripts.pending.get(&id) else { return };
                match p.wait {
                    Wait::Blocking => {
                        if self.scripts.blocking == Some(id) && self.scripts.parser.as_ref().is_some_and(|p| p.nesting == 0) {
                            self.resume_parser(e);
                        }
                    }
                    Wait::Soon => self.run_pending(e, id),
                    Wait::InOrder => {
                        while let Some(&id) = self.scripts.in_order.front()
                            && self.scripts.is_ready(id)
                        {
                            self.scripts.in_order.pop_front();
                            self.run_pending(e, id);
                        }
                    }
                    Wait::Defer => {
                        if self.scripts.phase == Phase::Deferring {
                            self.run_defers(e);
                        }
                    }
                }
                self.check_load();
            }
            ScriptTask::ContentLoaded => {
                self.scripts.phase = Phase::ContentLoaded;
                let doc = self.dom.document();
                self.fire_simple(e, Target::Node(doc), atom!("DOMContentLoaded"), true);
                self.check_load();
            }
            ScriptTask::Load => {
                self.scripts.phase = Phase::Complete;
                self.fire_simple(e, Target::Window, atom!("load"), false);
            }
        }
    }

    /// Parsing is done: the `defer` scripts that are ready run, in
    /// order; once none is left, `DOMContentLoaded` is queued.
    pub(crate) fn run_defers(&mut self, e: &mut E) {
        self.scripts.phase = Phase::Deferring;
        while let Some(&id) = self.scripts.defer.front() {
            if !self.scripts.is_ready(id) {
                return;
            }
            self.scripts.defer.pop_front();
            self.run_pending(e, id);
        }
        self.scripts.phase = Phase::ContentLoadedQueued;
        self.event_loop.tasks.push_back(Task::Script(ScriptTask::ContentLoaded));
    }

    /// Queue `load` once `DOMContentLoaded` fired and no script that runs
    /// as soon as possible is awaited.
    fn check_load(&mut self) {
        let s = &self.scripts;
        if s.phase == Phase::ContentLoaded && s.soon == 0 && s.in_order.is_empty() {
            self.scripts.phase = Phase::LoadQueued;
            self.event_loop.tasks.push_back(Task::Script(ScriptTask::Load));
        }
    }

    /// Free the nodes no one counts; those the parser refers to it
    /// counts (`parse.rs`).
    pub(crate) fn free_dead(&mut self) {
        self.dom.free_dead(&mut self.freed);
    }

    /// Close the host and keep its DOM, as it is once script is done
    /// with it: the counts wrappers held are given back, and what no one
    /// counts then is freed. The engine tells no host about the realm
    /// again, so nothing it does later reaches the DOM.
    pub fn into_dom(mut self, e: &mut E) -> madcawl_dom::Dom {
        self.shut(e);
        for n in self.dom.wrapped() {
            self.dom.clear_wrapper(n);
        }
        let mut freed = Vec::new();
        self.dom.free_dead(&mut freed);
        std::mem::take(&mut self.dom)
    }

    /// `node` was freed: it is no script element any more.
    pub(crate) fn forget_script(&mut self, node: NodeId) {
        if !self.scripts.flags.is_empty() {
            self.scripts.flags.remove(&node.index());
        }
    }

    /// Give back what the scripts still awaited hold, and the parser.
    pub(crate) fn close_scripts(&mut self, e: &mut E) {
        self.drop_parser();
        self.scripts.blocking = None;
        self.scripts.defer.clear();
        self.scripts.in_order.clear();
        self.scripts.soon = 0;
        self.scripts.current = None;
        for (_, p) in std::mem::take(&mut self.scripts.pending) {
            self.dom.let_go(p.el);
            if let Some(w) = p.wrapper {
                w.release(e);
            }
        }
    }

    /// A script element was inserted (`nodes`, each with what is below
    /// it) into `parent`, or `parent` got new children: prepare the
    /// connected script elements that were not made by the parser, in
    /// tree order.
    pub(crate) fn scripts_inserted(&mut self, e: &mut E, parent: NodeId, nodes: &[NodeId]) {
        if !self.dom.is_connected(parent) {
            return;
        }
        let mut found = Vec::new();
        if is_script(&self.dom, parent) {
            found.push(parent);
        }
        for &n in nodes {
            let below = std::iter::once(n).chain(self.dom.descendants(n));
            found.extend(below.filter(|&d| is_script(&self.dom, d)));
        }
        self.prepare_found(e, found);
    }

    /// Script element `el` got a `src` attribute it did not have.
    pub(crate) fn script_src_added(&mut self, e: &mut E, el: NodeId) {
        if is_script(&self.dom, el) && self.dom.is_connected(el) {
            self.prepare_found(e, vec![el]);
        }
    }

    fn prepare_found(&mut self, e: &mut E, found: Vec<NodeId>) {
        let found: Vec<NodeId> = found.into_iter().filter(|&n| !self.scripts.flags(n).parser && !self.scripts.flags(n).started).collect();
        if found.is_empty() {
            return;
        }
        // running script can free nodes: hold them until each is prepared
        let held: Vec<HeldNode> = found.iter().map(|&n| self.dom.hold(n)).collect();
        for h in held {
            let n = h.id();
            self.prepare_script(e, n, false);
            self.dom.let_go(h);
        }
    }
}
