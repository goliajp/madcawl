//! One live host per isolate: the engine's notices and cycle candidates
//! go to the host that settles.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js_stub::Stub;

#[test]
#[should_panic(expected = "one live host per isolate")]
fn a_second_live_host_on_one_isolate_is_refused() {
    let mut e = Stub::new();
    let _first = DomHost::new(&mut e, Dom::new());
    let _second = DomHost::new(&mut e, Dom::new());
}

#[test]
fn a_host_may_follow_one_that_closed() {
    let mut e = Stub::new();
    let first = DomHost::new(&mut e, Dom::new());
    first.close(&mut e);
    let second = DomHost::new(&mut e, Dom::new());
    second.close(&mut e);
}
