//! What a child navigable loads (`frames.rs`): its iframe's attributes
//! read once, on insertion, and the document that comes of them put in
//! place of the initial `about:blank`, in the same window.

use alt_url::Url;
use madcawl_dom::HeldNode;
use madcawl_js::Engine;
use madcawl_str::atom;

use crate::event::Target;
use crate::event_loop::Task;
use crate::frames::{Load, blank, same_origin};
use crate::host::DomHost;
use crate::scripts::ScriptTask;

impl<E: Engine> DomHost<E> {
    /// The standard's "process the iframe attributes" on insertion.
    pub(crate) fn process_frame_attributes(&mut self, e: &mut E, w: u64) {
        let f = &self.frames.live[&w];
        let el = f.element.id();
        let sandboxed = self.dom.attribute(el, atom!("sandbox")).is_some_and(|s| {
            let s = crate::scripts::text(s);
            !s.split_ascii_whitespace().any(|t| t.eq_ignore_ascii_case("allow-same-origin"))
        });
        let parent_origin = self.origin_of(f.parent);
        if let Some(srcdoc) = self.dom.attribute(el, atom!("srcdoc")) {
            let mut b = Vec::new();
            srcdoc.write_wtf8(&mut b);
            let f = self.frames.live.get_mut(&w).expect("just made");
            (f.same_origin, f.load) = (!sandboxed, Load::Srcdoc(b));
            self.event_loop.tasks.push_back(Task::Script(ScriptTask::Frame(w)));
            return;
        }
        let src = self.dom.attribute(el, atom!("src")).map(crate::scripts::text).filter(|s| !s.is_empty());
        let doc = self.dom.owner_document(el);
        let url = src.and_then(|s| self.base_url(doc).join(&s).ok()).filter(|u| !(u.scheme() == "about" && u.path() == "blank"));
        let f = self.frames.live.get_mut(&w).expect("just made");
        match url {
            None => {
                f.same_origin = !sandboxed;
                self.fire_simple(e, Target::Node(el), atom!("load"), false);
            }
            Some(u) => {
                f.same_origin = !sandboxed && same_origin(&parent_origin, &u.origin());
                if f.same_origin {
                    f.load = Load::Fetching;
                    self.frames.requests.push((w, u.as_str().to_owned()));
                }
            }
        }
    }

    /// The documents of `src` to fetch since the last call: a request id
    /// and the URL, resolved and of the parent's origin.
    pub fn take_frame_requests(&mut self) -> Vec<(u64, String)> {
        std::mem::take(&mut self.frames.requests)
    }

    /// Request `id` ended: the URL it ended at after redirects and the
    /// document (WTF-8), or `None` when the fetch failed, which leaves
    /// `about:blank`. A document that ended at another origin is not
    /// loaded. Its iframe's `load` follows in a task.
    pub fn frame_fetched(&mut self, id: u64, response: Option<(&str, &[u8])>) {
        let parent = match self.frames.live.get(&id) {
            Some(f) if matches!(f.load, Load::Fetching) => f.parent,
            _ => return,
        };
        let parent_origin = self.origin_of(parent);
        let f = self.frames.live.get_mut(&id).expect("looked up");
        f.load = match response.and_then(|(u, b)| Some((Url::parse(u).ok()?, b))) {
            Some((u, b)) if same_origin(&parent_origin, &u.origin()) => Load::Fetched(u, b.to_vec()),
            Some(_) => {
                f.same_origin = false;
                Load::Nothing
            }
            None => Load::Fetched(blank(), b"<html><head></head><body></body></html>".to_vec()),
        };
        if matches!(f.load, Load::Nothing) {
            self.frame_settled(parent);
        } else {
            self.event_loop.tasks.push_back(Task::Script(ScriptTask::Frame(id)));
        }
    }

    /// The task of a navigation whose document is here: it replaces the
    /// navigable's document, in the same window, and `load` is fired at
    /// the iframe once the navigables of that document have loaded. Also
    /// the task that fires it then.
    pub(crate) fn frame_loaded(&mut self, e: &mut E, w: u64) {
        let Some(f) = self.frames.live.get_mut(&w) else { return };
        let (url, html) = match &f.load {
            Load::Srcdoc(_) | Load::Fetched(..) => match std::mem::replace(&mut f.load, Load::Parsed) {
                Load::Srcdoc(b) => (Url::parse("about:srcdoc").expect("a valid URL"), b),
                Load::Fetched(u, b) => (u, b),
                _ => unreachable!("matched above"),
            },
            Load::Parsed => return self.frame_complete(e, w),
            Load::Nothing | Load::Fetching => return,
        };
        if let Some(new) = self.dom.new_active_document(false) {
            self.dom.record_parsed_frames(true);
            let _complete = self.dom.parse_into(new.id(), &html);
            let mut iframes = Vec::new();
            self.dom.take_parsed_frames(&mut iframes);
            self.dom.record_parsed_frames(false);
            crate::impls::mark_started(self, new.id());
            self.replace_frame_document(e, w, new, url);
            self.check_frames(e);
            self.frames_connected(e, iframes);
        }
        if self.frames.live.contains_key(&w) {
            self.frame_complete(e, w);
        }
    }

    /// Navigable `w`'s document is in place: unless one below it is still
    /// loading, `load` fires at its iframe, which may let the document
    /// the iframe is in finish loading.
    fn frame_complete(&mut self, e: &mut E, w: u64) {
        if self.frame_delays(w) {
            return;
        }
        let f = self.frames.live.get_mut(&w).expect("a live frame");
        f.load = Load::Nothing;
        let (el, parent) = (f.element.id(), f.parent);
        self.fire_simple(e, Target::Node(el), atom!("load"), false);
        self.frame_settled(parent);
    }

    /// Whether a navigable whose iframe is in window `w`'s document is
    /// loading, which delays that document's `load` (the standard's
    /// "delay the load event"): at most `MAX_FRAMES` are.
    pub(crate) fn frame_delays(&self, w: u64) -> bool {
        self.frames.live.values().any(|f| f.parent == w && !matches!(f.load, Load::Nothing))
    }

    /// A navigable whose iframe is in window `w`'s document stopped
    /// loading (loaded, went, or ended at another origin): `w`'s own
    /// `load` follows once no other one delays it, the top-level
    /// window's as its scripts let it, a child's in a task.
    pub(crate) fn frame_settled(&mut self, w: u64) {
        if w == 0 {
            self.check_load();
        } else if self.frames.live.get(&w).is_some_and(|f| matches!(f.load, Load::Parsed)) && !self.frame_delays(w) {
            self.event_loop.tasks.push_back(Task::Script(ScriptTask::Frame(w)));
        }
    }

    fn replace_frame_document(&mut self, e: &mut E, w: u64, new: HeldNode, url: Url) {
        let f = self.frames.live.get_mut(&w).expect("a live frame");
        let doc = new.id();
        let old = std::mem::replace(&mut f.doc, new);
        f.url = url;
        let realm = f.realm;
        self.frames.by_doc.remove(&old.id().index());
        self.frames.by_doc.insert(doc.index(), w);
        let global = e.global(realm);
        let d = self.wrap(e, doc);
        crate::window::put(e, self, global.get(), "document", d.get());
        e.release(d);
        e.release(global);
        self.dom.empty_document(old.id());
        let f = self.frames.live.get_mut(&w).expect("a live frame");
        self.frames.former.insert(old.id().index(), w);
        let prev = f.former.replace(old);
        assert!(prev.is_none(), "a navigable loads one document");
    }
}
