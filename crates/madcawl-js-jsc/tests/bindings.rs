//! The binding test batch of `madcawl-bindings` on JavaScriptCore: the
//! same tests the stub engine passes.

#[path = "../../madcawl-bindings/tests/batch/mod.rs"]
mod batch;

use madcawl_bindings::DomHost;
use madcawl_js::{Engine, Owned, ScriptOrigin};
use madcawl_js_jsc::{Jsc, Value};
use madcawl_str::DomString;

type E = Jsc;

const COUNTING: &str = "(function (log) {
  return function () {
    log.calls = (log.calls | 0) + 1;
    if (log.fail) throw new TypeError('the listener fails');
  };
})";

impl batch::Driver for Jsc {
    fn make() -> Self {
        Jsc::new()
    }

    fn counting(&mut self, h: &mut DomHost<Self>, log: Value) -> Owned<Value> {
        let url = DomString::from_wtf8(b"batch");
        let origin = ScriptOrigin { url: &url, line: 1, column: 0 };
        let make = self.evaluate(h, h.realm, &DomString::from_wtf8(COUNTING.as_bytes()), &origin);
        assert!(!Jsc::is_thrown(make.get()));
        let f = self.call(h, make.get(), Jsc::undefined(), &[log]);
        assert!(!Jsc::is_thrown(f.get()));
        self.release(make);
        f
    }

    fn collect_all(&mut self, h: &mut DomHost<Self>) {
        self.collect_now();
        while h.collect(self, madcawl_js::Deadline::Work(u32::MAX)) == madcawl_js::Step::More {}
    }
}

include!("../../madcawl-bindings/tests/batch/tests.rs");
