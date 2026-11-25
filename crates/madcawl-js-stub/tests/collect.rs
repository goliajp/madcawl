//! The collector against a small host: host objects with the engine
//! header at offset 0, host-to-host edges, member handles and a
//! wrapper word, kept by hand by these rules.

use madcawl_str::abi;
use madcawl_js::{
    Bound, ClassDef, ClassId, Ctx, Deadline, Engine, Host, HostOps, HostRef, Key, Member, Root, Step,
    Tracer, Unlinker,
};
use madcawl_js_stub::{Realm, Stub, Value};
use madcawl_str::DomString;

#[repr(C)]
struct Obj {
    header: u64,
    wrapper: u64,
    edges: Vec<usize>,
    members: Vec<Member<Stub>>,
    skip: bool,
    freed: bool,
}

#[derive(Default)]
struct Objs {
    // boxed: a host object's address never changes
    #[allow(clippy::vec_box)]
    all: Vec<Box<Obj>>,
    /// Report every member edge twice: a broken traverse.
    overreport: bool,
    unlinked: u32,
}

fn obj(h: HostRef) -> &'static mut Obj {
    // SAFETY: every HostRef in these tests is a boxed Obj kept in Objs
    unsafe { &mut *(h.addr() as *mut Obj) }
}

fn inc(addr: usize) {
    let o = obj(HostRef::new(addr).unwrap());
    o.header = abi::with_refcount(o.header, abi::refcount(o.header) + 1);
}

fn dec(addr: usize) {
    let o = obj(HostRef::new(addr).unwrap());
    o.header = abi::with_refcount(o.header, abi::refcount(o.header) - 1);
}

fn traverse(host: &Objs, h: HostRef, t: &mut <Stub as Engine>::Tracer) {
    let o = obj(h);
    for &e in &o.edges {
        t.host(HostRef::new(e).unwrap());
    }
    for m in &o.members {
        m.trace(t);
        if host.overreport {
            m.trace(t);
        }
    }
    if o.wrapper & 1 != 0 {
        t.value(Stub::wrapper_value(o.wrapper & !1));
    }
}

fn unlink(host: &mut Objs, h: HostRef, u: &mut <Stub as Engine>::Unlinker) {
    host.unlinked += 1;
    let o = obj(h);
    for e in o.edges.drain(..) {
        dec(e);
    }
    for m in &mut o.members {
        m.unlink(u);
    }
    if o.wrapper & 1 != 0 {
        o.wrapper &= !1;
        let w = Stub::wrapper_value(o.wrapper);
        // SAFETY: the preserved edge's count, given up
        u.release(unsafe { madcawl_js::Owned::from_raw(w) });
    }
}

static OPS: HostOps<Stub, Objs> = HostOps {
    traverse,
    unlink,
    can_skip: |_, h| obj(h).skip,
    wrapper_needs_preserve: |_, h| obj(h).wrapper |= 1,
    wrapper_died: |_, h| {
        obj(h).wrapper = 0;
        dec(h.addr());
    },
};

static CLASS: ClassDef<Stub, Objs> = ClassDef {
    name: "Obj",
    id: ClassId(1),
    last: ClassId(1),
    parent: None,
    attributes: &[],
    operations: &[],
    constants: &[],
    interceptors: None,
    ops: Some(&OPS),
};

impl Host<Stub> for Objs {
    fn class_of(&self, _: HostRef) -> &'static ClassDef<Stub, Objs> {
        &CLASS
    }

    fn load_imported_module(&mut self, _: &mut Stub, _: &madcawl_js::ModuleRequest<'_>, _: ()) {}
}

impl Objs {
    fn new_obj(&mut self) -> HostRef {
        let mut b = Box::new(Obj {
            header: abi::with_refcount(0, 0),
            wrapper: 0,
            edges: Vec::new(),
            members: Vec::new(),
            skip: false,
            freed: false,
        });
        let h = HostRef::new(std::ptr::from_mut(&mut *b) as usize).unwrap();
        self.all.push(b);
        h
    }

    fn edge(&mut self, from: HostRef, to: HostRef) {
        obj(from).edges.push(to.addr());
        inc(to.addr());
    }

    fn wrap(&mut self, e: &mut Stub, realm: Realm, h: HostRef) -> madcawl_js::Owned<Value> {
        let o = obj(h);
        if o.wrapper != 0 {
            if let Some(w) = e.wrapper_upgrade(o.wrapper & !1) {
                return w;
            }
            if o.wrapper & 1 != 0 {
                // SAFETY: the preserved edge's count, given up
                e.release(unsafe { madcawl_js::Owned::from_raw(Stub::wrapper_value(o.wrapper & !1)) });
            }
            o.wrapper = 0;
            dec(h.addr());
        }
        let w = e.new_wrapper(realm, &CLASS, h);
        o.wrapper = Stub::wrapper_word(w.get());
        inc(h.addr());
        w
    }

    /// Deliver notices, free what reached 0, suspect what dropped:
    /// what bindings do after each call into the engine.
    fn settle(&mut self, e: &mut Stub) {
        loop {
            let delivered = e.drain_host(self);
            let mut freed = 0;
            for b in &mut self.all {
                if !b.freed && abi::refcount(b.header) == 0 {
                    b.freed = true;
                    freed += 1;
                    let h = HostRef::new(std::ptr::from_mut(&mut **b) as usize).unwrap();
                    for m in &mut b.members {
                        m.clear(e);
                    }
                    for x in b.edges.drain(..) {
                        dec(x);
                    }
                    e.forget(h);
                }
            }
            if delivered == 0 && freed == 0 {
                break;
            }
        }
    }

    fn live(&self) -> usize {
        self.all.iter().filter(|b| !b.freed).count()
    }
}

fn run(e: &mut Stub, host: &mut Objs) -> u32 {
    let mut steps = 0;
    while e.collect_cycles(host, Deadline::Work(1)) == Step::More {
        steps += 1;
        host.settle(e);
    }
    host.settle(e);
    steps
}

fn closure_capturing(e: &mut Stub, realm: Realm, v: madcawl_js::Owned<Value>) -> madcawl_js::Owned<Value> {
    e.new_closure::<Objs>(realm, |_, _, _, _, _| Stub::thrown(), vec![v])
}

#[test]
fn a_cycle_of_host_objects_alone_is_freed() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let (a, b) = (host.new_obj(), host.new_obj());
    host.edge(a, b);
    host.edge(b, a);
    e.suspect(a);
    run(&mut e, &mut host);
    assert_eq!(host.live(), 0);
    assert_eq!(e.last_collection().garbage, 2);
}

#[test]
fn a_cycle_through_a_listener_and_a_wrapper_is_freed() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let a = host.new_obj();
    let w = host.wrap(&mut e, realm, a);
    // the listener closes over the wrapper; the object holds the listener
    let w2 = e.retain(w.get());
    let f = closure_capturing(&mut e, realm, w2);
    obj(a).members.push(Member::new(f));
    let base = e.live_cells();
    e.release(w);
    host.settle(&mut e);
    assert_eq!(host.live(), 1, "held by its wrapper, held by the listener");
    run(&mut e, &mut host);
    assert_eq!(host.live(), 0);
    assert_eq!(e.live_cells(), base - 2, "the wrapper and the closure");
    assert_eq!(e.last_collection().survivors, 0);
}

#[test]
fn a_root_handle_keeps_the_cycle_and_its_release_frees_it() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let a = host.new_obj();
    let w = host.wrap(&mut e, realm, a);
    let w2 = e.retain(w.get());
    let f = closure_capturing(&mut e, realm, w2);
    let root = Root::new(e.retain(f.get()));
    obj(a).members.push(Member::new(f));
    e.release(w);
    e.suspect(a);
    run(&mut e, &mut host);
    assert_eq!(host.live(), 1, "a timer still holds the callback");
    root.release(&mut e);
    run(&mut e, &mut host);
    assert_eq!(host.live(), 0);
}

#[test]
fn an_own_property_preserves_the_wrapper_and_the_edge_is_reported() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let keep = host.new_obj();
    obj(keep).skip = true;
    inc(keep.addr());
    let a = host.new_obj();
    host.edge(keep, a);
    host.edge(a, keep);
    let w = host.wrap(&mut e, realm, a);
    let s = e.string(DomString::from_wtf8(b"kept"));
    assert!(e.set(&mut host, w.get(), Key::Str(&DomString::from_wtf8(b"x")), s.get()));
    e.release(s);
    assert_eq!(obj(a).wrapper & 1, 1, "preserved");
    let word = obj(a).wrapper;
    e.release(w);
    // the wrapper lives on with its property, held by the object
    host.settle(&mut e);
    e.suspect(a);
    run(&mut e, &mut host);
    assert_eq!(e.last_collection().skipped, 1);
    let again = host.wrap(&mut e, realm, a);
    assert_eq!(Stub::wrapper_word(again.get()), word & !1, "the same wrapper");
    let x = e.get(&mut host, again.get(), Key::Str(&DomString::from_wtf8(b"x")));
    assert_eq!(e.as_string(x.get()).unwrap(), DomString::from_wtf8(b"kept"));
    e.release(x);
    e.release(again);
    // once the object leaves the kept part, object and wrapper form a
    // cycle through the preserved edge, and go together
    obj(keep).edges.clear();
    dec(a.addr());
    obj(a).edges.clear();
    dec(keep.addr());
    e.suspect(a);
    run(&mut e, &mut host);
    assert_eq!(e.last_collection().garbage, 2);
    assert!(obj(a).freed);
    assert_eq!(e.live_cells(), 1, "the realm's global");
}

#[test]
fn a_wrapper_without_state_is_dropped_and_made_again() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let a = host.new_obj();
    inc(a.addr());
    let w = host.wrap(&mut e, realm, a);
    let old = obj(a).wrapper;
    e.release(w);
    assert_eq!(obj(a).wrapper, old, "the notice waits for the host");
    // read again before the host was told: the dead wrapper is refused,
    // and the host makes a new one
    assert!(e.wrapper_upgrade(old).is_none());
    obj(a).wrapper = 0;
    dec(a.addr());
    let back = host.wrap(&mut e, realm, a);
    let new = obj(a).wrapper;
    assert_ne!(new, old);
    host.settle(&mut e);
    assert_eq!(obj(a).wrapper, new, "no notice for the refused wrapper");
    assert_eq!(abi::refcount(obj(a).header), 2, "the host's count and the new wrapper's");
    assert_eq!(e.live_cells(), 2, "the global and the new wrapper");
    e.release(back);
    host.settle(&mut e);
    assert_eq!(obj(a).wrapper, 0);
    assert_eq!(abi::refcount(obj(a).header), 1);
    assert_eq!(e.live_cells(), 1);
}

#[test]
#[should_panic(expected = "a count added to a dead object")]
fn a_count_added_to_a_dead_wrapper_is_caught() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let a = host.new_obj();
    let w = host.wrap(&mut e, realm, a);
    let word = obj(a).wrapper;
    e.release(w);
    // what the old protocol did: retain the wrapper the word names
    let _ = e.retain(Stub::wrapper_value(word));
}

#[test]
fn work_done_between_batches_cannot_free_a_live_object() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let objs: Vec<HostRef> = (0..8).map(|_| host.new_obj()).collect();
    for i in 0..8 {
        host.edge(objs[i], objs[(i + 1) % 8]);
    }
    e.suspect(objs[0]);
    assert_eq!(e.collect_cycles(&mut host, Deadline::Work(3)), Step::More);
    // script takes a wrapper of one of them before the graph is done
    let w = host.wrap(&mut e, realm, objs[5]);
    run(&mut e, &mut host);
    assert_eq!(host.live(), 8);
    assert_eq!(e.last_collection().garbage, 0);
    e.release(w);
    host.settle(&mut e);
    e.suspect(objs[5]);
    run(&mut e, &mut host);
    assert_eq!(host.live(), 0);
    assert!(e.last_collection().batches > 1);
}

#[test]
fn an_edge_reported_and_not_counted_leaves_a_survivor() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let x = host.new_obj();
    let y = e.new_object(realm);
    let w = host.wrap(&mut e, realm, x);
    assert!(e.set(&mut host, y.get(), Key::Index(0), w.get()));
    e.release(w);
    let root = Root::new(e.retain(y.get()));
    obj(x).members.push(Member::new(y));
    host.overreport = true;
    e.suspect(x);
    run(&mut e, &mut host);
    assert_eq!(e.last_collection().survivors, 1);
    root.release(&mut e);
}

#[test]
fn more_edges_than_counts_keep_the_object_alive() {
    // a graph built in batches cannot tell a broken traverse from a node
    // moved between batches, so it keeps the object; the one-step check
    // over the garbage set is where an overreport stops the collection
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let x = host.new_obj();
    inc(x.addr());
    let y = e.new_object(realm);
    obj(x).members.push(Member::new(y));
    host.overreport = true;
    e.suspect(x);
    run(&mut e, &mut host);
    assert_eq!(e.last_collection().garbage, 0);
}

/// A host function through the C calling convention: its first
/// argument back, with a count.
unsafe extern "C" fn identity(ctx: *mut Ctx, _this: u64, argv: *const u64, argc: usize) -> u64 {
    // SAFETY: the engine calls it with a context for Objs and its
    // arguments
    let ((e, _host), args) = unsafe { (Ctx::parts::<Stub, Objs>(ctx), Stub::values(argv, argc)) };
    assert!(e.in_script(), "inside a host function");
    Stub::to_bits(e.retain(args[0]).into_raw())
}

#[test]
fn values_borrowed_into_a_call_and_returned_with_a_count() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let arg = e.new_object(realm);
    // SAFETY: identity reads its context as Stub and Objs
    let f = e.new_function::<Objs>(realm, "id", 1, unsafe { Bound::host_fn(identity) }, 0);
    let before = e.count_of(arg.get());
    let r = e.call(&mut host, f.get(), Stub::undefined(), &[arg.get()]);
    assert_eq!(r.get(), arg.get());
    assert_eq!(e.count_of(arg.get()), before + 1, "only the result's count");
    e.release(r);
    assert_eq!(e.count_of(arg.get()), before);
    let weak = e.weak(arg.get());
    let got = e.weak_get(&weak).unwrap();
    e.release(got);
    e.release(arg);
    assert!(e.weak_get(&weak).is_none(), "cleared when its target went");
    e.weak_drop(weak);
    e.release(f);
}

#[test]
fn a_cell_freed_between_batches_is_not_read() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let realm = e.new_realm();
    let x = host.new_obj();
    inc(x.addr());
    let y = e.new_object(realm);
    obj(x).members.push(Member::new(y));
    e.suspect(x);
    // the first batch visits x and queues its member y
    assert_eq!(e.collect_cycles(&mut host, Deadline::Work(0)), Step::More);
    // script drops y before the next batch
    obj(x).members[0].clear(&mut e);
    run(&mut e, &mut host);
    assert_eq!(e.last_collection().garbage, 0);
    assert_eq!(host.live(), 1);
}

#[test]
fn a_node_moved_between_batches_is_alive_not_an_error() {
    let (mut e, mut host) = (Stub::new(), Objs::default());
    let (z, w, x) = (host.new_obj(), host.new_obj(), host.new_obj());
    inc(z.addr());
    inc(w.addr());
    host.edge(z, x);
    e.suspect(w);
    e.suspect(z);
    // z is visited first and queues x with its count of 1
    assert_eq!(e.collect_cycles(&mut host, Deadline::Work(0)), Step::More);
    // x moves from z to w: its count stays 1, and w reports it later
    obj(z).edges.clear();
    obj(w).edges.push(x.addr());
    run(&mut e, &mut host);
    assert_eq!(e.last_collection().garbage, 0);
    assert_eq!(host.live(), 3);
}
