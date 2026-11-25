//! Opening a page for a session: the request checked against the
//! session's and the host's page counts and the allowed directories; a
//! web URL loaded through the host's network service, when it has one,
//! and waited on like an action.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;

use madcawl_agent::act::Until;
use madcawl_agent::{Frame, kind, tag};
use madcawl_html::Encoding;
use madcawl_net::Url;

use super::Conn;
use super::act::env;
use crate::serve::page::{OPEN_WAIT, Page, Request};
use crate::serve::{code, error_frame, target};

const MAX_VIEWPORT: u64 = 16_384;

fn is_web(u: &str) -> bool {
    matches!(target::scheme(u.trim()).map(str::to_ascii_lowercase).as_deref(), Some("http" | "https"))
}

impl Conn<'_> {
    pub(super) fn open(&mut self, f: &Frame) -> Frame {
        let r = self.open_page(f);
        let origin = match &r {
            Ok((p, _)) => p.url.clone(),
            Err(_) => f.get_str(tag::URL).unwrap_or("-").to_owned(),
        };
        let reply = match r {
            Ok((p, note)) => {
                let outline = Frame::new(kind::READ).num(tag::VIEW, 0);
                let mut view = p.handle(&outline).unwrap_or_else(|| error_frame(code::UNKNOWN, "no outline"));
                if let (Some(n), Some((_, t))) = (note, view.fields.iter_mut().find(|(t, _)| *t == tag::TEXT)) {
                    let mut v = n.into_bytes();
                    v.push(b'\n');
                    v.extend_from_slice(t);
                    *t = v;
                }
                let mut opened = Frame::new(kind::OPENED).num(tag::PAGE, p.id as u64).str(tag::URL, &p.url);
                opened.fields.extend(view.fields);
                let url = p.url.clone();
                self.wrap(opened, &url)
            }
            Err(e) => e,
        };
        self.log(&origin, "open", ("", ""), &reply);
        reply
    }

    /// The page opened, and a line on its load when it did not simply
    /// finish.
    fn open_page(&mut self, f: &Frame) -> Result<(&mut Page, Option<String>), Frame> {
        let config = &self.shared.config;
        let u = f
            .get_str(tag::URL)
            .ok_or_else(|| error_frame(code::BAD_REQUEST, "missing the path or URL to open"))?;
        if self.pages.len() >= config.max_pages {
            let msg = format!("this session has {} pages open already; close one", self.pages.len());
            return Err(error_frame(code::TOO_MANY_PAGES, &msg));
        }
        let web = is_web(u);
        if web && self.shared.net.is_none() {
            let msg = "only local files can be opened on this host: start it with --net to open http and https URLs";
            return Err(error_frame(code::REFUSED, msg));
        }
        let file = match web {
            true => None,
            false => Some(target::resolve(u, &config.allow).map_err(|e| error_frame(e.code(), &e.message()))?),
        };
        let dim = |t, d| f.get_num(t).unwrap_or(d).clamp(1, MAX_VIEWPORT) as f32;
        let size = (dim(tag::WIDTH, 1280), dim(tag::HEIGHT, 800));
        let charset = match f.get_str(tag::CHARSET) {
            Some(c) => Some(
                Encoding::for_label(c.as_bytes())
                    .ok_or_else(|| error_frame(code::BAD_REQUEST, &format!("unknown charset {c:?}")))?,
            ),
            None => None,
        };
        let total = &self.shared.pages;
        if total.fetch_add(1, Ordering::SeqCst) >= config.max_total_pages {
            total.fetch_sub(1, Ordering::SeqCst);
            let msg = format!("the host has {} pages open across its sessions already", config.max_total_pages);
            return Err(error_frame(code::TOO_MANY_PAGES, &msg));
        }
        let loaded = self.load(u, file, size, charset);
        let (page, note) = loaded.inspect_err(|_| {
            total.fetch_sub(1, Ordering::SeqCst);
        })?;
        self.shared.owners.lock().unwrap_or_else(|e| e.into_inner()).insert(page.id, self.id);
        self.pages.push(page);
        Ok((self.pages.last_mut().expect("pushed"), note))
    }

    /// A new page with `file`, or the web URL `u`, loaded into it.
    fn load(
        &self,
        u: &str,
        file: Option<(PathBuf, String)>,
        size: (f32, f32),
        charset: Option<Encoding>,
    ) -> Result<(Page, Option<String>), Frame> {
        let id = self.shared.next_page.fetch_add(1, Ordering::SeqCst);
        let mut page = Page::blank(id, size, self.shared.net.as_ref(), self.id);
        let Some(u) = (match file {
            Some((path, url)) => {
                let r = page.load_file(&path, url, charset, &self.shared.config.allow);
                r.map_err(|e| error_frame(code::IO, &format!("cannot open the page: {e}")))?;
                None
            }
            None => Some(u),
        }) else {
            return Ok((page, None));
        };
        let env = env(self.shared, self.id);
        let url = Url::parse(u.trim()).map_err(|e| error_frame(code::BAD_REQUEST, &format!("not a URL: {e:?}")))?;
        page.goto(Request::get(url, None), &env).map_err(|e| error_frame(code::NAVIGATION, &e))?;
        let until = Until { timeout_ms: OPEN_WAIT.as_millis() as u64, ..Until::default() };
        let w = page.wait(&until, Instant::now(), &env);
        if let Some(why) = w.notes.iter().find(|n| n.starts_with("the load failed")) {
            let r = error_frame(code::NAVIGATION, why);
            page.close();
            return Err(r);
        }
        let mut lines: Vec<String> = w.notes.into_iter().filter(|n| !n.starts_with("navigated")).collect();
        if !w.stable {
            lines.push(format!("still loading: {}", w.missing.join("; ")));
        }
        Ok((page, (!lines.is_empty()).then(|| lines.join("\n"))))
    }
}
