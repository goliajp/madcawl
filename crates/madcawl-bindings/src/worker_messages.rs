//! What goes between a worker and its window: messages, cloned into
//! the other realm when posted (the engine's structured clone) and
//! delivered as MessageEvents in a task, and the errors its script does
//! not catch, an ErrorEvent at its global and, when that is not canceled,
//! one at its Worker object, then a note in the page's request log when
//! that is not canceled either.

use madcawl_js::{Engine, Owned};
use madcawl_str::{DomString, atom};

use crate::convert::{Thrown, throw_dom};
use crate::event::{EventState, Target};
use crate::event_kinds::{ErrorPlace, Extra};
use crate::host::DomHost;
use crate::notes::Note;
use crate::report::ScriptError;
use crate::side::SideData;
use crate::workers::WorkerTask;

impl<E: Engine> DomHost<E> {
    /// `v` cloned into `realm`, or a DataCloneError for a value that
    /// cannot be (a function, a symbol, a platform object); what a getter
    /// threw passes on.
    pub(crate) fn clone_into(&mut self, e: &mut E, realm: E::Realm, v: E::Value) -> Result<Owned<E::Value>, Thrown> {
        match e.clone_value(self, realm, v) {
            Some(c) if E::is_thrown(c.get()) => {
                let _sentinel = c.into_raw();
                Err(Thrown)
            }
            Some(c) => Ok(c),
            None => Err(throw_dom(e, self, "DataCloneError", "the value cannot be cloned")),
        }
    }

    /// `postMessage` on a Worker object: for its global, once it has run
    /// its script.
    pub(crate) fn post_to_worker(&mut self, e: &mut E, w: u64, message: E::Value) -> Result<(), Thrown> {
        let Some(k) = self.workers.live.get(&w).filter(|k| !k.closing) else { return Ok(()) };
        let realm = k.realm;
        let v = madcawl_js::Root::new(self.clone_into(e, realm, message)?);
        let k = self.workers.live.get_mut(&w).expect("cloning runs no script of the worker's");
        if k.started {
            self.queue_worker(WorkerTask::ToWorker(w, v));
        } else {
            k.inbox.push(v);
        }
        Ok(())
    }

    /// `postMessage` on worker `w`'s global: for its Worker object.
    pub(crate) fn post_to_object(&mut self, e: &mut E, w: u64, message: E::Value) -> Result<(), Thrown> {
        let Some((owner, object)) = self.workers.live.get(&w).map(|k| (k.owner, k.object)) else { return Ok(()) };
        let realm = self.realm_of(owner).expect("a worker's window outlives it");
        let v = madcawl_js::Root::new(self.clone_into(e, realm, message)?);
        let Some(k) = self.workers.live.get(&w) else {
            v.release(e);
            return Ok(());
        };
        let o = madcawl_js::Root::new(e.retain(k.wrapper.get()));
        self.queue_worker(WorkerTask::ToObject { worker: w, owner, object, wrapper: o, data: v });
        Ok(())
    }

    /// Fire a MessageEvent with `data` at `target`, its wrapper made in
    /// the realm of whoever receives it.
    pub(crate) fn deliver_message(&mut self, e: &mut E, target: Target, data: E::Value) {
        let mut ev = EventState::new(atom!("message"), self.now_ms());
        ev.trusted = true;
        ev.extra = Extra::Message { origin: DomString::empty() };
        let r = self.sides.add(SideData::Event(ev));
        let realm = self.target_realm(target);
        let w = self.wrap_side_in(e, r, realm);
        e.release(w);
        self.set_value(e, r, data);
        self.fire_side(e, r, target);
    }

    /// The realm an event fired at `target` is made in: a worker's own, or
    /// that of the window of a worker's Worker object.
    fn target_realm(&self, target: Target) -> E::Realm {
        let w = match target {
            Target::Window(w) => w,
            Target::Side(r) => match self.sides.get(r).data {
                SideData::Worker(w) => self.workers.live.get(&w).map_or(0, |k| k.owner),
                _ => 0,
            },
            Target::Node(_) => 0,
        };
        self.realm_of(w).unwrap_or(self.realm)
    }

    /// An exception nothing caught in worker `w`'s script: an ErrorEvent
    /// at its global, which may cancel it; one thrown while that is
    /// dispatched is noted for the page instead.
    pub(crate) fn worker_exception(&mut self, e: &mut E, w: u64, x: E::Value) {
        let realm = self.workers.live[&w].realm;
        let err = self.describe(e, realm, x);
        let place = ErrorPlace {
            message: DomString::from_wtf8(format!("Uncaught {}", err.message).as_bytes()),
            filename: DomString::from_wtf8(err.source.as_bytes()),
            line: err.line,
            column: err.column,
        };
        let saved = self.workers.current.take();
        let not_canceled = self.fire_error(e, Target::Window(w), place.clone(), Some(x));
        self.workers.current = saved;
        if not_canceled && self.workers.live.contains_key(&w) {
            self.queue_worker(WorkerTask::Error(w, Box::new(place)));
        }
    }

    /// The error worker `w`'s global did not cancel, at its Worker object.
    pub(crate) fn worker_error_at_object(&mut self, e: &mut E, w: u64, place: ErrorPlace) {
        let Some(object) = self.workers.live.get(&w).map(|k| k.object) else { return };
        let (message, source, line, column) = (crate::scripts::text(&place.message), crate::scripts::text(&place.filename), place.line, place.column);
        if self.fire_error(e, Target::Side(object), place, None) {
            self.note(Note::Exception(ScriptError { message: format!("in a worker: {message}"), source, line, column }));
        }
    }

    /// Fire a trusted, cancelable ErrorEvent at `target`; whether no
    /// listener canceled it.
    fn fire_error(&mut self, e: &mut E, target: Target, place: ErrorPlace, error: Option<E::Value>) -> bool {
        let mut ev = EventState::new(atom!("error"), self.now_ms());
        (ev.trusted, ev.cancelable) = (true, true);
        ev.extra = Extra::Error(Box::new(place));
        let r = self.sides.add(SideData::Event(ev));
        let realm = self.target_realm(target);
        let w = self.wrap_side_in(e, r, realm);
        e.release(w);
        if let Some(x) = error {
            self.set_value(e, r, x);
        }
        self.fire_side(e, r, target)
    }
}
