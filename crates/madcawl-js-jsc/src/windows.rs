//! The engine's window methods on JavaScriptCore: the WindowProxy and
//! the cross-origin stand-in (`weak.rs`), wrappers moving between
//! realms (`wrap.rs`).

use madcawl_js::{ClassDef, Host, Owned, Windows};

use crate::isolate::{Jsc, Realm};
use crate::value::Value;

impl Windows for Jsc {
    fn define_interfaces<H: Host<Jsc>>(
        &mut self,
        _host: &mut H,
        realm: Realm,
        obj: Value,
        classes: &'static [&'static ClassDef<Jsc, H>],
    ) -> bool {
        self.lazy_interfaces(realm, obj, classes)
    }

    fn new_window_proxy(&mut self, realm: Realm) -> Owned<Value> {
        self.window_proxy(realm)
    }

    fn set_window_proxy_target(&mut self, proxy: Value, realm: Realm) {
        self.retarget(proxy, realm);
    }

    fn window_proxy_target(&self, proxy: Value) -> Option<Realm> {
        self.proxies.get(&proxy.0).map(|&(_, r)| r)
    }

    fn new_cross_origin_object(&mut self, realm: Realm, members: Value, deny: Value) -> Owned<Value> {
        self.cross_origin(realm, members, deny)
    }

    fn move_wrapper(&mut self, word: u64, realm: Realm) {
        self.rehome(word, realm);
    }
}
