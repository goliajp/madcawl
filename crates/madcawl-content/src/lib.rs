//! The engine the content process runs documents' scripts on, picked
//! here and nowhere else: the system engine of `madcawl-js-jsc`, until
//! torajs-engine replaces it. A document gets an isolate of its own, as
//! one live host per isolate requires, and gives it up with its DOM once
//! loaded.

use madcawl_bindings::DomHost;
use madcawl_dom::Dom;
use madcawl_js_jsc::Jsc;
use madcawl_shell::serve::scripts::ScriptPage;
use madcawl_str::DomString;

pub struct JscPage {
    e: Jsc,
    /// `None` once the DOM is taken.
    h: Option<DomHost<Jsc>>,
}

/// A document of WTF-8 `html` with its scripts run on that engine,
/// parsed as far as its scripts let it.
pub fn open(html: &[u8], _url: &str) -> Box<dyn ScriptPage> {
    let mut e = Jsc::new();
    let mut h = DomHost::new(&mut e, Dom::new());
    h.install_window(&mut e);
    h.parse_document(&mut e, html);
    Box::new(JscPage { e, h: Some(h) })
}

impl JscPage {
    fn host(&mut self) -> (&mut Jsc, &mut DomHost<Jsc>) {
        (&mut self.e, self.h.as_mut().expect("a host until the DOM is taken"))
    }
}

impl ScriptPage for JscPage {
    fn requests(&mut self) -> Vec<(u64, String)> {
        let (_, h) = self.host();
        h.take_script_requests().into_iter().map(|(id, src)| (id, text(&src))).collect()
    }

    fn fetched(&mut self, id: u64, source: Option<&str>) {
        let (_, h) = self.host();
        h.script_fetched(id, source.map(|s| DomString::from_wtf8(s.as_bytes())));
    }

    fn run(&mut self, max: usize) -> bool {
        let (e, h) = self.host();
        let mut ran = 0;
        while ran < max {
            let n = h.run_tasks(e, max - ran);
            if n == 0 {
                break;
            }
            ran += n;
        }
        h.is_loaded()
    }

    fn into_dom(mut self: Box<Self>) -> Dom {
        let h = self.h.take().expect("a host until the DOM is taken");
        h.into_dom(&mut self.e)
    }
}

impl Drop for JscPage {
    fn drop(&mut self) {
        if let Some(h) = self.h.take() {
            h.close(&mut self.e);
        }
    }
}

fn text(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    String::from_utf8_lossy(&b).into_owned()
}
