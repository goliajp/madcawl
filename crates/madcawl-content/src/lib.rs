//! The engine the content process runs documents' scripts on, picked
//! here and nowhere else: the system engine of `madcawl-js-jsc`, until
//! torajs-engine replaces it. A document gets an isolate of its own, as
//! one live host per isolate requires, and gives it up with its DOM once
//! loaded.

use std::time::Duration;

use madcawl_bindings::{CookieStore, Credentials, DomHost, FetchResponse, Mode};
use madcawl_dom::Dom;
use madcawl_js::Control;
use madcawl_js_jsc::Jsc;
use madcawl_shell::serve::cookies::Cookies;
use madcawl_shell::serve::script_fetch::{self, ScriptFetch, ScriptResponse};
use madcawl_shell::serve::scripts::ScriptPage;
use madcawl_str::DomString;

pub struct JscPage {
    e: Jsc,
    /// `None` once the DOM is taken.
    h: Option<DomHost<Jsc>>,
}

/// The page's cookie jar as `document.cookie` reaches it.
struct Jar(Box<dyn Cookies>);

impl CookieStore for Jar {
    fn cookies(&mut self, url: &str) -> String {
        self.0.get(url)
    }

    fn set_cookie(&mut self, url: &str, line: &str) {
        self.0.set(url, line);
    }

    fn idle(&mut self) {
        self.0.idle();
    }
}

/// A document of WTF-8 `html` with its scripts run on that engine,
/// parsed as far as its scripts let it, each stopped once it has run
/// for `limit` at a stretch.
pub fn open(html: &[u8], url: &str, cookies: Option<Box<dyn Cookies>>, limit: Option<Duration>) -> Box<dyn ScriptPage> {
    let mut e = Jsc::new();
    e.set_time_limit(limit).expect("an isolate with the watchdog");
    let mut h = DomHost::new(&mut e, Dom::new());
    // a URL the network service fetched always parses
    let parsed = h.set_url(url);
    debug_assert!(parsed, "a document URL that does not parse");
    if let Some(c) = cookies {
        h.set_cookie_store(Box::new(Jar(c)));
    }
    h.install_window(&mut e);
    h.parse_document(&mut e, html);
    h.cookies_idle();
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

    fn frame_requests(&mut self) -> Vec<(u64, String)> {
        let (_, h) = self.host();
        h.take_frame_requests()
    }

    fn frame_fetched(&mut self, id: u64, response: Option<(&str, &str)>) {
        let (_, h) = self.host();
        h.frame_fetched(id, response.map(|(url, text)| (url, text.as_bytes())));
    }

    fn fetches(&mut self) -> Vec<ScriptFetch> {
        let (_, h) = self.host();
        h.take_fetches()
            .into_iter()
            .map(|r| {
                let credentials = match r.credentials {
                    Credentials::Omit => script_fetch::Credentials::Omit,
                    Credentials::SameOrigin => script_fetch::Credentials::SameOrigin,
                    Credentials::Include => script_fetch::Credentials::Include,
                };
                let mode = match r.mode {
                    Mode::Cors => script_fetch::Mode::Cors,
                    Mode::SameOrigin => script_fetch::Mode::SameOrigin,
                };
                let (id, what, method, url, headers, body, initiator) = (r.id, r.destination, r.method, r.url, r.headers, r.body, r.initiator);
                ScriptFetch { id, what, method, url, headers, body, initiator, credentials, mode }
            })
            .collect()
    }

    fn fetch_done(&mut self, id: u64, response: Option<ScriptResponse>) {
        let (_, h) = self.host();
        let r = response.map(|r| FetchResponse { status: r.status, status_text: r.status_text, url: r.url, headers: r.headers, body: r.body });
        h.fetch_done(id, r);
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
        h.cookies_idle();
        h.is_loaded()
    }

    fn due(&self) -> Option<Duration> {
        let h = self.h.as_ref().expect("a host until the DOM is taken");
        h.next_work_in().map(Duration::from_nanos)
    }

    fn take_notes(&mut self) -> Vec<String> {
        let (_, h) = self.host();
        h.take_notes().iter().map(ToString::to_string).collect()
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
