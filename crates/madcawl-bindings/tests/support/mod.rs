//! Doing through the engine interface what a page's script would.

// each test file uses a part of this
#![allow(dead_code)]

use madcawl_bindings::DomHost;
use madcawl_dom::{Dom, NodeId};
use madcawl_js::{Deadline, Engine, Key, Owned, Step};
use madcawl_js_stub::{CollectStats, Stub, Value};
use madcawl_str::DomString;

pub type V = Owned<Value>;

pub struct Js {
    pub e: Stub,
    pub h: DomHost<Stub>,
}

pub fn undefined() -> V {
    // SAFETY: undefined carries no count
    unsafe { Owned::from_raw(Stub::undefined()) }
}

/// A listener body: adds one to `calls` on the object it closes over
/// first, and throws when that object's `fail` is truthy.
fn counting(e: &mut Stub, h: &mut DomHost<Stub>, caps: &[Value], _: Value, _: &[Value]) -> V {
    let log = caps[0];
    let calls = DomString::from_wtf8(b"calls");
    let n = e.get(h, log, Key::Str(&calls));
    let k = e.as_number(n.get()).unwrap_or(0.0);
    e.release(n);
    assert!(e.set(h, log, Key::Str(&calls), Stub::number(k + 1.0)));
    let fail = e.get(h, log, Key::Str(&DomString::from_wtf8(b"fail")));
    let failing = e.to_boolean(fail.get());
    e.release(fail);
    if failing {
        return e.throw_error(madcawl_js::ErrorKind::Type, "the listener fails");
    }
    undefined()
}

impl Js {
    pub fn new() -> Js {
        Js::with(Dom::new())
    }

    pub fn with(dom: Dom) -> Js {
        let mut e = Stub::new();
        let h = DomHost::new(&mut e, dom);
        Js { e, h }
    }

    pub fn wrap(&mut self, n: NodeId) -> V {
        self.h.wrap(&mut self.e, n)
    }

    pub fn doc(&mut self) -> V {
        let d = self.h.dom.document();
        self.wrap(d)
    }

    pub fn node(&self, v: Value) -> NodeId {
        let obj = self.e.host_of(v, &madcawl_bindings::NodeClass::<Stub>::DEF).expect("a node");
        self.h.node(obj)
    }

    pub fn str(&mut self, s: &str) -> V {
        self.e.string(DomString::from_wtf8(s.as_bytes()))
    }

    pub fn get(&mut self, obj: Value, name: &str) -> V {
        let r = self.e.get(&mut self.h, obj, Key::Str(&DomString::from_wtf8(name.as_bytes())));
        assert!(!Stub::is_thrown(r.get()), "reading {name} threw");
        r
    }

    pub fn set(&mut self, obj: Value, name: &str, v: Value) {
        assert!(self.e.set(&mut self.h, obj, Key::Str(&DomString::from_wtf8(name.as_bytes())), v));
    }

    /// `obj.name(...args)`; thrown results come back as the sentinel.
    pub fn call(&mut self, obj: Value, name: &str, args: &[Value]) -> V {
        let f = self.get(obj, name);
        let r = self.e.call(&mut self.h, f.get(), obj, args);
        self.e.release(f);
        r
    }

    /// `obj.name(...args)` that must not throw, its result dropped.
    pub fn run(&mut self, obj: Value, name: &str, args: &[Value]) {
        let r = self.call(obj, name, args);
        assert!(!Stub::is_thrown(r.get()), "{name} threw {}", self.exception());
        self.e.release(r);
    }

    pub fn text(&mut self, v: Value) -> String {
        let s = self.e.as_string(v).expect("a string");
        let mut b = Vec::new();
        s.write_wtf8(&mut b);
        String::from_utf8(b).unwrap()
    }

    /// The text of `obj.name`, or "null".
    pub fn read(&mut self, obj: Value, name: &str) -> String {
        let v = self.get(obj, name);
        let out = match self.e.as_number(v.get()) {
            Some(n) => n.to_string(),
            None if v.get() == Stub::null() => "null".to_owned(),
            None if self.e.as_boolean(v.get()).is_some() => self.e.to_boolean(v.get()).to_string(),
            None => self.text(v.get()),
        };
        self.e.release(v);
        out
    }

    /// Whether `obj.name` is the same object as `other`.
    pub fn is(&mut self, obj: Value, name: &str, other: Value) -> bool {
        let v = self.get(obj, name);
        let same = v.get() == other;
        self.e.release(v);
        same
    }

    /// The name of the pending exception, taken.
    pub fn exception(&mut self) -> String {
        let Some(x) = self.e.take_exception() else { return "none".to_owned() };
        let name = self.read(x.get(), "name");
        self.e.release(x);
        name
    }

    /// A script function that closes over `captures`; see `counting`.
    pub fn closure(&mut self, captures: Vec<V>) -> V {
        self.e.new_closure::<DomHost<Stub>>(self.h.realm, counting, captures)
    }

    pub fn object(&mut self) -> V {
        self.e.new_object(self.h.realm)
    }

    pub fn retain(&mut self, v: Value) -> V {
        self.e.retain(v)
    }

    pub fn drop(&mut self, v: V) {
        self.e.release(v);
        self.h.settle(&mut self.e);
    }

    /// Collect until done, one unit of work at a time.
    pub fn collect(&mut self) -> CollectStats {
        while self.h.collect(&mut self.e, Deadline::Work(1)) == Step::More {}
        self.e.last_collection()
    }

    /// Whether the weak handle's target is gone.
    pub fn gone(&mut self, w: &u32) -> bool {
        match self.e.weak_get(w) {
            Some(v) => {
                self.e.release(v);
                false
            }
            None => true,
        }
    }

    pub fn audit(&self) {
        let a = self.h.dom.audit();
        assert_eq!(a.mismatches(), 0, "{a:?}");
    }
}
