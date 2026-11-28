//! A pane page: one page a display end asked for, in a content process
//! of its own that draws it on a display channel straight to the
//! display end. The host opens it as it opens an agent session's page
//! (page numbers, audit lines, network registrations alike) and keeps
//! nothing of the pixels. A pane whose content process ends is gone:
//! starting it again with a new display channel is not done here.

use std::os::fd::{OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

use madcawl_agent::{Frame, kind, tag};
use madcawl_wire::host::PaneOpen;

use super::Proxy;
use crate::serve::{Shared, code, error_frame};

pub(crate) struct Pane<'a> {
    proxy: Proxy<'a>,
    pub page: u32,
}

impl<'a> Pane<'a> {
    /// Opens the page `o` asks for as session `id`'s, in a content process
    /// started for it; the pane, and the display end's side of its
    /// display channel. The refusal when it cannot open.
    pub fn open(id: u32, shared: &'a Shared, o: &PaneOpen) -> Result<(Pane<'a>, OwnedFd), Frame> {
        if shared.config.isolate.is_none() {
            return Err(error_frame(code::NOT_ALLOWED, "pane pages need a host started with --isolate"));
        }
        let (ours, theirs) = UnixStream::pair().map_err(|e| error_frame(code::IO, &format!("no display channel: {e}")))?;
        let mut proxy = Proxy::new(id, shared);
        proxy.display = Some(OwnedFd::from(theirs));
        let f = Frame::new(kind::OPEN)
            .str(tag::URL, &o.url)
            .num(tag::WIDTH, o.width.into())
            .num(tag::HEIGHT, o.height.into());
        let r = proxy.forward(&f);
        let page = match (r.kind, r.get_num(tag::PAGE)) {
            (kind::OPENED, Some(p)) if proxy.child.is_some() => p as u32,
            _ => {
                proxy.end();
                return Err(match r.kind {
                    kind::ERROR => r,
                    _ => error_frame(code::IO, "the pane's content process did not open the page"),
                });
            }
        };
        proxy.log("pane-open", "ok");
        Ok((Pane { proxy, page }, OwnedFd::from(ours)))
    }

    /// The content process's channel, while it runs.
    pub fn fd(&self) -> Option<RawFd> {
        self.proxy.child.as_ref().map(super::Content::fd)
    }

    pub fn pid(&self) -> i32 {
        self.proxy.child.as_ref().map_or(0, super::Content::pid)
    }

    /// Reads what the content process said while idle; false once it has
    /// ended, which ends the pane.
    pub fn spoke(&mut self) -> bool {
        let p = &mut self.proxy;
        let Some(mut c) = p.child.take() else { return false };
        let ended = c.idle_read(&mut |q| p.answer(q));
        match ended {
            None => {
                p.child = Some(c);
                true
            }
            Some(how) => {
                p.log("pane-lost", &how);
                eprintln!("madcawl-shell: pane p{} of session {}: the content process ended ({how})", self.page, p.id);
                false
            }
        }
    }

    /// Closes the page and ends its content process.
    pub fn end(mut self) {
        self.proxy.end();
    }
}
