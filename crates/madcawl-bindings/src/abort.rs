//! Abort signals, as the DOM Standard's "signal abort": a signal is
//! aborted once, with a reason (a member of its wrapper, kept where a
//! CustomEvent keeps its detail); then the listeners added with it are
//! removed and an `abort` event is fired at it. `AbortSignal.timeout`
//! aborts its signal from a timer of the event loop that holds the
//! signal's wrapper until it fires.

use madcawl_js::{Engine, Owned};
use madcawl_str::atom;

use crate::event::{EventState, Target};
use crate::host::DomHost;
use crate::side::{SideData, SideRef};

#[derive(Default)]
pub(crate) struct SignalState {
    pub aborted: bool,
    /// The listeners added with this signal, by target and listener id,
    /// removed when it aborts. A listener removed otherwise stays here
    /// until the list is pruned.
    algorithms: Vec<(Target, u64)>,
    /// The list's length after it was last pruned.
    pruned: usize,
}

impl<E: Engine> DomHost<E> {
    pub(crate) fn signal(&self, r: SideRef) -> &SignalState {
        match &self.sides.get(r).data {
            SideData::Signal(s) => s,
            _ => unreachable!("a signal's side object"),
        }
    }

    fn signal_mut(&mut self, r: SideRef) -> &mut SignalState {
        match &mut self.sides.get_mut(r).data {
            SideData::Signal(s) => s,
            _ => unreachable!("a signal's side object"),
        }
    }

    pub(crate) fn new_signal(&mut self) -> SideRef {
        self.sides.add(SideData::Signal(SignalState::default()))
    }

    /// Remove listener `id` of `t` when signal `r` aborts. Each time the
    /// list has doubled since it was last pruned, the listeners already
    /// gone leave it, so a signal kept while listeners come and go holds
    /// at most twice as many entries as there are live listeners.
    pub(crate) fn remove_on_abort(&mut self, r: SideRef, t: Target, id: u64) {
        let s = self.signal(r);
        if s.algorithms.len() >= (2 * s.pruned).max(8) {
            let mut list = std::mem::take(&mut self.signal_mut(r).algorithms);
            list.retain(|&(t, id)| self.listening(t, id));
            let s = self.signal_mut(r);
            s.pruned = list.len();
            s.algorithms = list;
        }
        self.signal_mut(r).algorithms.push((t, id));
    }

    /// Abort signal `r` with `reason`, or a new "AbortError" DOMException
    /// when there is none; nothing when it is aborted already.
    pub(crate) fn signal_abort(&mut self, e: &mut E, r: SideRef, reason: Option<E::Value>) {
        if self.signal(r).aborted {
            return;
        }
        self.set_aborted(e, r, reason);
        for (t, id) in std::mem::take(&mut self.signal_mut(r).algorithms) {
            self.remove_listener_id(e, t, id);
        }
        let mut ev = EventState::new(atom!("abort"), self.now_ms());
        ev.trusted = true;
        let ev = self.sides.add(SideData::Event(ev));
        let _not_canceled = self.dispatch(e, ev, Target::Side(r));
    }

    fn set_aborted(&mut self, e: &mut E, r: SideRef, reason: Option<E::Value>) {
        self.signal_mut(r).aborted = true;
        let reason = match reason {
            Some(v) => e.retain(v),
            None => self.dom_exception(e, "AbortError", "signal is aborted without reason"),
        };
        self.set_value(e, r, reason.get());
        e.release(reason);
    }

    /// A signal aborted already, with `reason` or an "AbortError": it
    /// has no listeners, so no event is fired.
    pub(crate) fn aborted_signal(&mut self, e: &mut E, reason: Option<E::Value>) -> SideRef {
        let r = self.new_signal();
        self.set_aborted(e, r, reason);
        r
    }

    /// A signal that aborts with a "TimeoutError" after `ms`
    /// milliseconds. Its timer cannot be cleared, and it keeps the
    /// signal's wrapper, with what is on it (listeners, their closures),
    /// until it fires, as the standard's lifetime has it: a page that
    /// asks for many timeouts far out (`ms` up to 2^53 - 1) holds one
    /// timer and one signal each until window `w`, which asked, goes.
    pub(crate) fn timeout_signal(&mut self, e: &mut E, ms: u64, w: u64) -> SideRef {
        let r = self.new_signal();
        let wrapper = self.wrap_side(e, r);
        self.event_loop.add_host_timer(madcawl_js::Root::new(wrapper), ms, crate::event_loop::TimerKind::Abort, w);
        r
    }

    /// The timer of a timeout signal fired: `signal` is its wrapper.
    pub(crate) fn timed_out(&mut self, e: &mut E, signal: E::Value) {
        let obj = e.host_of(signal, &crate::generated::AbortSignalClass::<E>::DEF).expect("a signal's wrapper");
        let r = self.sides.of(obj).expect("a live signal");
        let reason = self.dom_exception(e, "TimeoutError", "signal timed out");
        self.signal_abort(e, r, Some(reason.get()));
        e.release(reason);
    }

    /// The reason of signal `r`: undefined until it aborts.
    pub(crate) fn reason(&mut self, e: &mut E, r: SideRef) -> Owned<E::Value> {
        match self.details.get(&r).and_then(madcawl_js::Member::get) {
            Some(v) => e.retain(v),
            // SAFETY: undefined carries no count
            None => unsafe { Owned::from_raw(E::undefined()) },
        }
    }
}

#[cfg(test)]
mod tests {
    use madcawl_dom::Dom;
    use madcawl_js_stub::Stub;

    use crate::event::Target;
    use crate::host::DomHost;

    #[test]
    fn listeners_already_gone_leave_a_signals_list_as_it_grows() {
        let mut e = Stub::new();
        let mut h = DomHost::new(&mut e, Dom::new());
        let s = h.new_signal();
        for id in 0..1000 {
            h.remove_on_abort(s, Target::Window(0), 1_000_000 + id);
        }
        assert!(h.signal(s).algorithms.len() <= 8, "{} entries", h.signal(s).algorithms.len());
        h.close(&mut e);
    }
}
