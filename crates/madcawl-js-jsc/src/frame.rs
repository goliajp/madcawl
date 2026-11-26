//! Which isolate and host a callback from the framework belongs to.
//!
//! The framework calls back with nothing but its own context: a host
//! function, a microtask trampoline, an interface object that throws.
//! Every engine call that may reach such a callback takes the host, and
//! puts the isolate and the host here for as long as it is inside the
//! framework; a nested call puts its own and gives the outer one back.

use std::any::TypeId;
use std::cell::Cell;

use crate::isolate::Jsc;
use crate::objects::{Report, report};

#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub engine: *mut Jsc,
    pub host: *mut (),
    pub host_type: TypeId,
    /// Hands the host an exception nothing caught.
    pub report: Report,
}

thread_local! {
    static FRAME: Cell<Option<Frame>> = const { Cell::new(None) };
}

/// Gives the outer frame back when dropped.
pub(crate) struct Entered(Option<Frame>);

impl Drop for Entered {
    fn drop(&mut self) {
        FRAME.with(|f| f.set(self.0));
    }
}

/// Make `engine` and `host` the frame for what runs until the result
/// is dropped.
pub(crate) fn enter<H: madcawl_js::Host<Jsc>>(engine: *mut Jsc, host: *mut H) -> Entered {
    let frame = Frame { engine, host: host.cast(), host_type: TypeId::of::<H>(), report: report::<H> };
    Entered(FRAME.with(|f| f.replace(Some(frame))))
}

/// The frame of the engine call in progress.
pub(crate) fn current() -> Frame {
    FRAME.with(Cell::get).expect("a callback from JavaScriptCore inside an engine call that took the host")
}
