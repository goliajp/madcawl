//! The session's page numbers: given out to its content process up to
//! the session's and the host's limits, and taken back when the process
//! no longer holds the page. The process's word on its pages counts
//! only for pages this session owns.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use madcawl_agent::{Frame, kind, tag};

use super::Proxy;
use crate::serve::conn::PageState;
use crate::serve::link::internal;
use crate::serve::{code, error_frame};

impl Proxy<'_> {
    /// A page number, counted against the session's and the host's page
    /// limits and owned by this session from now on.
    pub(super) fn new_page(&mut self) -> Frame {
        let s = self.shared;
        if self.pages.len() + self.reserved.len() >= s.config.max_pages {
            return error_frame(code::TOO_MANY_PAGES, &format!("this session has {} pages open already; close one", s.config.max_pages));
        }
        if s.pages.fetch_add(1, Ordering::SeqCst) >= s.config.max_total_pages {
            s.pages.fetch_sub(1, Ordering::SeqCst);
            let msg = format!("the host has {} pages open across its sessions already", s.config.max_total_pages);
            return error_frame(code::TOO_MANY_PAGES, &msg);
        }
        let id = s.next_page.fetch_add(1, Ordering::SeqCst);
        s.owners.lock().unwrap_or_else(|e| e.into_inner()).insert(id, self.id);
        self.reserved.push(id);
        Frame::new(kind::RESULT).num(tag::PAGE, u64::from(id))
    }

    /// Gives back page `page` when it is this session's; another
    /// session's page, or one given back already, stays as it is.
    pub(super) fn release(&self, page: u32) {
        let mut owners = self.shared.owners.lock().unwrap_or_else(|e| e.into_inner());
        if owners.get(&page) == Some(&self.id) {
            owners.remove(&page);
            self.shared.pages.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Learns where the pages are now; pages gone since (closed, expired,
    /// failed to open) are given back.
    pub(super) fn refresh(&mut self) {
        if self.child.is_none() {
            return;
        }
        let pages = match self.ask(&Frame::new(internal::STATE), self.shared.config.hang) {
            Ok(r) => PageState::parse(r.get_str(tag::TEXT).unwrap_or("")),
            Err(fault) => {
                let how = self.ended(fault);
                let note = self.recover(&how);
                eprintln!("madcawl-shell: session {}: the content process ended ({how}); {note}", self.id);
                return;
            }
        };
        self.adopt(pages);
    }

    /// Takes the content process's word for which pages it holds, as far
    /// as they are this session's; pages gone since are given back.
    pub(super) fn adopt(&mut self, mut pages: Vec<PageState>) {
        let mut seen = HashSet::new();
        {
            let owners = self.shared.owners.lock().unwrap_or_else(|e| e.into_inner());
            pages.retain(|p| owners.get(&p.id) == Some(&self.id) && seen.insert(p.id));
        }
        let had: Vec<u32> = self.pages.iter().map(|p| p.id).chain(self.reserved.drain(..)).collect();
        for id in had.into_iter().filter(|id| !pages.iter().any(|p| p.id == *id)) {
            self.release(id);
        }
        self.pages = pages;
    }
}
