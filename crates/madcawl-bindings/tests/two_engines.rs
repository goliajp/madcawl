//! The binding test batch (`batch/`) on the stub engine. The same batch
//! runs on the transition engine from its adapter's crate.

mod batch;

use madcawl_bindings::DomHost;
use madcawl_js::{Engine, Key, Owned};
use madcawl_js_stub::{Stub, Value};
use madcawl_str::DomString;

type E = Stub;

fn counting(e: &mut Stub, h: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> Owned<Value> {
    let calls = DomString::from_wtf8(b"calls");
    let n = e.get(h, caps[0], Key::Str(&calls));
    let k = e.as_number(n.get()).unwrap_or(0.0);
    e.release(n);
    assert!(e.set(h, caps[0], Key::Str(&calls), Stub::number(k + 1.0)));
    let fail = e.get(h, caps[0], Key::Str(&DomString::from_wtf8(b"fail")));
    let failing = e.to_boolean(fail.get());
    e.release(fail);
    if failing {
        return e.throw_error(madcawl_js::ErrorKind::Type, "the listener fails");
    }
    // SAFETY: undefined carries no count
    unsafe { Owned::from_raw(Stub::undefined()) }
}

impl batch::Driver for Stub {
    fn make() -> Self {
        Stub::new()
    }

    fn counting(&mut self, h: &mut DomHost<Self>, log: Value) -> Owned<Value> {
        let log = self.retain(log);
        self.new_closure::<DomHost<Stub>>(h.realm, counting, vec![log])
    }

    fn collect_all(&mut self, h: &mut DomHost<Self>) {
        while h.collect(self, madcawl_js::Deadline::Work(u32::MAX)) == madcawl_js::Step::More {}
    }
}

include!("batch/tests.rs");
