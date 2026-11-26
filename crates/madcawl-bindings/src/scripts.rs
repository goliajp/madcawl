//! Script elements: HTML's "prepare the script element" for classic
//! scripts, their sources fetched by the embedder, and the four places a
//! script with a source waits until it runs: the parser's pending
//! parsing-blocking script, the scripts that run when parsing is done
//! (`defer`), those that run in order as soon as possible (inserted by
//! script with `async` false) and those that run as soon as possible.
//!
//! A source is asked for by an id: the embedder takes the requests
//! ([`DomHost::take_script_requests`]), fetches each URL as written,
//! resolved against the document's, and hands back the text, or `None`
//! when the fetch failed ([`DomHost::script_fetched`]); that queues a
//! task. A host with a [`Loader`] fetches at once instead.
//!
//! Module scripts and import maps are not run, and nothing marks a
//! script element as parser-made until its end tag is reached.

use std::collections::{HashMap, VecDeque};

use madcawl_dom::{HeldNode, NodeId, NodeKind};
use madcawl_js::{Engine, Root};
use madcawl_str::{DomString, atom};

use crate::event_loop::Task;
use crate::host::DomHost;
use crate::parse::{Loader, Parser};

/// The host's own tasks.
pub(crate) enum ScriptTask {
    /// The source of the script with this id arrived (or failed).
    Fetched(u64),
    /// Fire `DOMContentLoaded` at the document.
    ContentLoaded,
    /// Fire `load` at the window.
    Load,
}

/// Where a script with a source waits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Wait {
    Blocking,
    Defer,
    InOrder,
    Soon,
}

/// How far the document has loaded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    /// No document is being loaded.
    Idle,
    Parsing,
    /// Parsing is done; the `defer` scripts run.
    Deferring,
    ContentLoadedQueued,
    /// `DOMContentLoaded` fired; the other scripts are awaited.
    ContentLoaded,
    LoadQueued,
    Complete,
}

pub(crate) struct Pending<E: Engine> {
    pub el: HeldNode,
    /// The element's wrapper, on an engine that keeps wrappers by reach:
    /// its listeners live on it, and the element may have left every
    /// tree before its script runs.
    pub wrapper: Option<Root<E>>,
    pub url: DomString,
    pub wait: Wait,
    /// `None` until the fetch ends, then the source or `None`.
    pub result: Option<Option<DomString>>,
}

/// A script element's flags from the standard. An element with no
/// entry has the defaults: not started, not parser-made, force async.
#[derive(Clone, Copy)]
pub(crate) struct Flags {
    pub started: bool,
    pub parser: bool,
    pub force_async: bool,
}

const DEFAULT: Flags = Flags { started: false, parser: false, force_async: true };

pub(crate) struct Scripts<E: Engine> {
    /// The parser, from `parse_document` until it is done.
    pub parser: Option<Box<Parser>>,
    pub loader: Option<Loader>,
    /// By node index; an entry goes when its node is freed.
    pub flags: HashMap<u32, Flags>,
    pub pending: HashMap<u64, Pending<E>>,
    pub blocking: Option<u64>,
    pub defer: VecDeque<u64>,
    pub in_order: VecDeque<u64>,
    /// How many of `pending` wait as soon as possible.
    pub soon: usize,
    pub requests: Vec<(u64, DomString)>,
    next_id: u64,
    pub current: Option<NodeId>,
    pub phase: Phase,
}

impl<E: Engine> Scripts<E> {
    pub(crate) fn new() -> Self {
        Scripts {
            parser: None,
            loader: None,
            flags: HashMap::new(),
            pending: HashMap::new(),
            blocking: None,
            defer: VecDeque::new(),
            in_order: VecDeque::new(),
            soon: 0,
            requests: Vec::new(),
            next_id: 1,
            current: None,
            phase: Phase::Idle,
        }
    }

    pub(crate) fn flags(&self, el: NodeId) -> Flags {
        self.flags.get(&el.index()).copied().unwrap_or(DEFAULT)
    }

    pub(crate) fn is_ready(&self, id: u64) -> bool {
        self.pending.get(&id).is_some_and(|p| p.result.is_some())
    }
}

/// Whether `n` is an HTML `script` element.
pub(crate) fn is_script(dom: &madcawl_dom::Dom, n: NodeId) -> bool {
    dom.namespace(n) == Some(madcawl_html::Namespace::Html) && dom.local_name(n) == Some(atom!("script"))
}

/// A JavaScript MIME type essence, ASCII case-insensitive.
fn is_js_type(t: &str) -> bool {
    const TYPES: [&str; 16] = [
        "application/ecmascript", "application/javascript", "application/x-ecmascript", "application/x-javascript",
        "text/ecmascript", "text/javascript", "text/javascript1.0", "text/javascript1.1", "text/javascript1.2",
        "text/javascript1.3", "text/javascript1.4", "text/javascript1.5", "text/jscript", "text/livescript",
        "text/x-ecmascript", "text/x-javascript",
    ];
    TYPES.iter().any(|x| x.eq_ignore_ascii_case(t))
}

pub(crate) fn text(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    String::from_utf8_lossy(&b).into_owned()
}

impl<E: Engine> DomHost<E> {
    fn script_attr(&self, el: NodeId, name: madcawl_str::Atom) -> Option<String> {
        self.dom.attribute(el, name).map(text)
    }

    /// Whether the element's `type` and `language` make it a classic
    /// script.
    fn is_classic(&self, el: NodeId) -> bool {
        match self.script_attr(el, atom!("type")) {
            Some(t) if !t.is_empty() => is_js_type(t.trim_matches([' ', '\t', '\n', '\x0c', '\r'])),
            Some(_) => true,
            None => match self.script_attr(el, atom!("language")) {
                Some(l) if !l.is_empty() => is_js_type(&format!("text/{l}")),
                _ => true,
            },
        }
    }

    pub(crate) fn child_text(&self, el: NodeId) -> DomString {
        let mut b = Vec::new();
        for c in self.dom.children(el).filter(|&c| self.dom.kind(c) == NodeKind::Text) {
            self.dom.text(c).expect("a text node").write_wtf8(&mut b);
        }
        DomString::from_wtf8(&b)
    }

    /// The standard's "prepare the script element"; `parser` when the
    /// parser reached the element's end tag.
    pub(crate) fn prepare_script(&mut self, e: &mut E, el: NodeId, parser: bool) {
        let mut f = self.scripts.flags(el);
        if f.started {
            return;
        }
        let parser_made = parser || f.parser;
        let has_async = self.dom.attribute(el, atom!("async")).is_some();
        f.parser = false;
        if parser_made && !has_async {
            f.force_async = true;
        }
        let source = self.child_text(el);
        let src = self.script_attr(el, atom!("src"));
        let runs = (src.is_some() || !source.is_empty()) && self.dom.is_connected(el) && self.is_classic(el);
        if runs {
            if parser_made {
                (f.parser, f.force_async) = (true, false);
            }
            f.started = true;
        }
        self.scripts.flags.insert(el.index(), f);
        if !runs {
            return;
        }
        let Some(src) = src else {
            let url = DomString::from_wtf8(b"inline");
            return self.execute_script(e, el, Some(source), &url, false);
        };
        let wait = match self.dom.attribute(el, atom!("defer")).is_some() {
            _ if has_async => Wait::Soon,
            true if parser_made => Wait::Defer,
            _ if parser_made => Wait::Blocking,
            _ if !f.force_async => Wait::InOrder,
            _ => Wait::Soon,
        };
        let url = DomString::from_wtf8(src.as_bytes());
        // an empty src fails without a fetch, as a task
        let fetch = !src.is_empty();
        self.add_pending(e, el, url, if fetch { wait } else { Wait::Soon }, fetch);
    }

    fn add_pending(&mut self, e: &mut E, el: NodeId, url: DomString, wait: Wait, fetch: bool) {
        let id = self.scripts.next_id;
        self.scripts.next_id += 1;
        let wrapper = (E::PLACES_WRAPPERS && self.dom.wrapper(el) != 0).then(|| Root::new(self.wrap(e, el)));
        let held = self.dom.hold(el);
        let s = &mut self.scripts;
        match wait {
            Wait::Blocking => s.blocking = Some(id),
            Wait::Defer => s.defer.push_back(id),
            Wait::InOrder => s.in_order.push_back(id),
            Wait::Soon => s.soon += 1,
        }
        s.pending.insert(id, Pending { el: held, wrapper, url: url.clone(), wait, result: None });
        if !fetch {
            return self.script_fetched(id, None);
        }
        match self.scripts.loader.as_mut() {
            Some(l) => {
                let got = l(&text(&url)).map(|b| DomString::from_wtf8(String::from_utf8_lossy(&b).as_bytes()));
                self.script_fetched(id, got);
            }
            None => self.scripts.requests.push((id, url)),
        }
    }

    /// Fetch external scripts with `loader`, at once, instead of handing
    /// out requests.
    pub fn set_loader(&mut self, loader: Loader) {
        self.scripts.loader = Some(loader);
    }

    /// The script sources to fetch since the last call: an id, and the
    /// `src` value as written.
    pub fn take_script_requests(&mut self) -> Vec<(u64, DomString)> {
        std::mem::take(&mut self.scripts.requests)
    }

    /// The source of request `id` arrived as `text`, or the fetch failed
    /// (`None`): a task runs what that lets run. A request no element
    /// waits on any more is ignored.
    pub fn script_fetched(&mut self, id: u64, text: Option<DomString>) {
        let Some(p) = self.scripts.pending.get_mut(&id) else { return };
        if p.result.is_some() {
            return;
        }
        p.result = Some(text);
        self.event_loop.tasks.push_back(Task::Script(ScriptTask::Fetched(id)));
    }

    /// Nanoseconds until the window has something to run: 0 with a task
    /// or a source request waiting, else the next timer's, if any.
    pub fn next_work_in(&self) -> Option<u64> {
        if !self.scripts.requests.is_empty() || !self.event_loop.tasks.is_empty() {
            return Some(0);
        }
        self.event_loop.next_due_in()
    }

    /// How many script sources are awaited.
    pub fn scripts_pending(&self) -> usize {
        self.scripts.pending.values().filter(|p| p.result.is_none()).count()
    }

    /// Whether the document has loaded: its `load` event fired.
    pub fn is_loaded(&self) -> bool {
        self.scripts.phase == crate::scripts::Phase::Complete
    }
}
