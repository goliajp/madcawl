//! The host's own firing of a node's listeners.

use madcawl_dom::NodeId;
use madcawl_js::{Engine, Halt, Host, Root};
use madcawl_str::Atom;

use crate::host::DomHost;

impl<E: Engine> DomHost<E> {
    /// Call the listeners of type `ty` on `target`, each held by a root
    /// handle for the call, with a microtask checkpoint after each when
    /// no script is on the stack (a dispatch the user started, not one
    /// script started). The dispatch stops when script is being ended.
    /// The host's own firing, not yet an Event dispatch (that is
    /// `dispatchEvent`'s, `event.rs`): the listener gets the type as its
    /// argument.
    pub fn fire(&mut self, e: &mut E, target: NodeId, ty: Atom) -> usize {
        let roots: Vec<Root<E>> = self
            .listeners_of(target)
            .filter(|l| l.ty == ty)
            .filter_map(|l| l.callback.get())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|cb| Root::new(e.retain(cb)))
            .collect();
        let this = self.wrap(e, target);
        let arg = e.string(ty.to_dom_string());
        let checkpoints = !e.in_script();
        let (mut called, mut ended) = (0, false);
        for r in roots {
            if !ended {
                called += 1;
                let res = e.call(self, r.get(), this.get(), &[arg.get()]);
                if !E::is_thrown(res.get()) {
                    e.release(res);
                } else {
                    let _sentinel = res.into_raw();
                    ended = e.halt() != Halt::Running;
                    if !ended {
                        let ex = e.take_exception().expect("a thrown exception");
                        self.report_exception(e, ex);
                    }
                }
                // halted script skips the task's checkpoint
                if checkpoints && !ended {
                    e.run_microtasks(self);
                }
            }
            r.release(e);
        }
        e.release(this);
        e.release(arg);
        self.recover(e);
        self.settle(e);
        called
    }

    pub fn listener_count(&self, n: NodeId) -> usize {
        self.listeners_of(n).count()
    }
}
