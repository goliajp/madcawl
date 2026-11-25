//! Opening a page for a session: the request checked against the
//! session's and the host's page counts and the allowed directories.

use std::sync::atomic::Ordering;

use madcawl_agent::{Frame, kind, tag};
use madcawl_html::Encoding;

use super::Conn;
use crate::serve::page::Page;
use crate::serve::{code, error_frame, target};

const MAX_VIEWPORT: u64 = 16_384;

impl Conn<'_> {
    pub(super) fn open(&mut self, f: &Frame) -> Frame {
        let r = self.open_page(f);
        let origin = match &r {
            Ok(p) => p.url.clone(),
            Err(_) => f.get_str(tag::URL).unwrap_or("-").to_owned(),
        };
        let reply = match r {
            Ok(p) => {
                let outline = Frame::new(kind::READ).num(tag::VIEW, 0);
                let view = p
                    .handle(&outline)
                    .unwrap_or_else(|| error_frame(code::UNKNOWN, "no outline"));
                let mut opened = Frame::new(kind::OPENED)
                    .num(tag::PAGE, p.id as u64)
                    .str(tag::URL, &p.url);
                opened.fields.extend(view.fields);
                let url = p.url.clone();
                self.wrap(opened, &url)
            }
            Err(e) => e,
        };
        self.log(&origin, "open", ("", ""), &reply);
        reply
    }

    fn open_page(&mut self, f: &Frame) -> Result<&mut Page, Frame> {
        let config = &self.shared.config;
        let u = f
            .get_str(tag::URL)
            .ok_or_else(|| error_frame(code::BAD_REQUEST, "missing the path or URL to open"))?;
        if self.pages.len() >= config.max_pages {
            let msg = format!(
                "this session has {} pages open already; close one",
                self.pages.len()
            );
            return Err(error_frame(code::TOO_MANY_PAGES, &msg));
        }
        let (path, url) =
            target::resolve(u, &config.allow).map_err(|e| error_frame(e.code(), &e.message()))?;
        let dim = |t, d| f.get_num(t).unwrap_or(d).clamp(1, MAX_VIEWPORT) as f32;
        let size = (dim(tag::WIDTH, 1280), dim(tag::HEIGHT, 800));
        let charset = match f.get_str(tag::CHARSET) {
            Some(c) => Some(Encoding::for_label(c.as_bytes()).ok_or_else(|| {
                error_frame(code::BAD_REQUEST, &format!("unknown charset {c:?}"))
            })?),
            None => None,
        };
        let total = &self.shared.pages;
        if total.fetch_add(1, Ordering::SeqCst) >= config.max_total_pages {
            total.fetch_sub(1, Ordering::SeqCst);
            let msg = format!(
                "the host has {} pages open across its sessions already",
                config.max_total_pages
            );
            return Err(error_frame(code::TOO_MANY_PAGES, &msg));
        }
        let id = self.next_page;
        let page = Page::open(id, &path, url, size, charset, &config.allow).map_err(|e| {
            total.fetch_sub(1, Ordering::SeqCst);
            error_frame(code::IO, &format!("cannot open the page: {e}"))
        })?;
        self.next_page += 1;
        self.pages.push(page);
        Ok(self.pages.last_mut().expect("pushed"))
    }
}
