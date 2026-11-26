//! What a content process tells its host about its pages, and a page
//! opened again in a new content process after the last one ended.

use std::fmt::Write as _;
use std::sync::atomic::Ordering;

use madcawl_agent::{Frame, kind, tag};

use super::Conn;
use super::open::is_web;
use crate::serve::link::internal;
use crate::serve::{code, error_frame, target};

/// A page's line in the answer to `STATE`.
#[derive(Clone, Debug, PartialEq)]
pub struct PageState {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    /// How far the viewport is scrolled, in CSS px
    pub y: u32,
    pub url: String,
}

impl PageState {
    /// The lines of a `STATE` answer, `id width height y url` each.
    pub fn parse(text: &str) -> Vec<PageState> {
        text.lines()
            .filter_map(|l| {
                let mut f = l.splitn(5, ' ');
                let mut n = || f.next()?.parse::<u32>().ok();
                let (id, width, height, y) = (n()?, n()?, n()?, n()?);
                Some(PageState { id, width, height, y, url: f.next()?.to_owned() })
            })
            .collect()
    }

    /// The request that opens the page again.
    pub fn restore_frame(&self) -> Frame {
        Frame::new(internal::RESTORE)
            .num(tag::PAGE, u64::from(self.id))
            .str(tag::URL, &self.url)
            .num(tag::WIDTH, u64::from(self.width))
            .num(tag::HEIGHT, u64::from(self.height))
            .num(tag::OFFSET, u64::from(self.y))
    }
}

impl Conn<'_> {
    /// The pages, a line each.
    pub(super) fn state(&self) -> Frame {
        let mut text = String::new();
        for p in &self.pages {
            let y = p.doc.geometry.as_ref().map_or(0.0, |g| g.viewport.y).max(0.0) as u32;
            let _ = writeln!(text, "{} {} {} {y} {}", p.id, p.width as u32, p.height as u32, p.url);
        }
        Frame::new(kind::RESULT).str(tag::TEXT, &text)
    }

    /// Opens a page again under the number it had, at the same size, and
    /// scrolls it to where it was.
    pub(super) fn restore(&mut self, f: &Frame) -> Frame {
        let reply = self.restore_page(f).unwrap_or_else(|e| e);
        let url = f.get_str(tag::URL).unwrap_or("-");
        self.log(url, "restore", ("", ""), &reply);
        reply
    }

    fn restore_page(&mut self, f: &Frame) -> Result<Frame, Frame> {
        let bad = || error_frame(code::BAD_REQUEST, "a restore names a page, its URL and its size");
        let (Some(id), Some(u)) = (f.get_num(tag::PAGE), f.get_str(tag::URL)) else { return Err(bad()) };
        let size = (f.get_num(tag::WIDTH).ok_or_else(bad)? as f32, f.get_num(tag::HEIGHT).ok_or_else(bad)? as f32);
        let file = match is_web(u) {
            true => None,
            false => Some(target::resolve(u, &self.shared.config.allow).map_err(|e| error_frame(e.code(), &e.message()))?),
        };
        let (mut page, _) = self.load(id as u32, u, file, size, None)?;
        let y = f.get_num(tag::OFFSET).unwrap_or(0);
        if y > 0 {
            page.geometry().scroll_to(y as f32);
        }
        self.shared.pages.fetch_add(1, Ordering::SeqCst);
        self.shared.owners.lock().unwrap_or_else(|e| e.into_inner()).insert(page.id, self.id);
        let text = format!("restored p{} at y={}", page.id, page.doc.geometry.as_ref().map_or(0.0, |g| g.viewport.y) as i64);
        self.pages.push(page);
        Ok(Frame::new(kind::RESULT).str(tag::TEXT, &text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_lines_round_trip() {
        let p = PageState { id: 9, width: 1280, height: 800, y: 1600, url: "file:///a b.html".into() };
        let line = format!("{} {} {} {} {}\nbad line\n", p.id, p.width, p.height, p.y, p.url);
        assert_eq!(PageState::parse(&line), std::slice::from_ref(&p));
        let f = p.restore_frame();
        assert_eq!((f.kind, f.get_num(tag::OFFSET), f.get_str(tag::URL)), (internal::RESTORE, Some(1600), Some("file:///a b.html")));
    }
}
