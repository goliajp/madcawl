//! Parsing a document with its scripts run: the tree builder stops
//! after each script element's end tag and the element is prepared
//! (`scripts.rs`); an inline script runs there, and a script with a
//! source and neither `async` nor `defer` becomes the pending
//! parsing-blocking script, which the parser waits for, across tasks
//! when its source has not arrived. `document.write` puts text into the
//! input at the insertion point and tokenizes it there, scripts inside
//! it included.
//!
//! The DOM is the tree builder's sink while the tokenizer runs and the
//! host's otherwise (the nodes stay where they are: only the arena's
//! owner changes). Every call into the engine happens while the host
//! has it. Nothing is freed while the parser lives: it holds nodes no
//! count records.

use madcawl_dom::Dom;
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_str::DomString;

use crate::host::DomHost;
use crate::scripts::Phase;

/// Fetches a script's source by its `src` value; `None` when there is
/// none to be had.
pub type Loader = Box<dyn FnMut(&str) -> Option<Vec<u8>>>;

/// `document.write` calls nested in one another past this depth (a
/// script that writes a script that writes, ...) are ignored, as WebKit
/// ignores them, until the outermost returns.
const MAX_WRITE_DEPTH: u32 = 21;

pub(crate) struct Parser {
    pub tok: Tokenizer,
    b: TreeBuilder<Dom>,
    /// The parser's script nesting level.
    pub nesting: u32,
    /// `document.write` calls in progress, nested.
    writes: u32,
    /// The depth limit was hit: writes are ignored until `writes` is 0.
    writes_capped: bool,
}

impl<E: Engine> DomHost<E> {
    /// Parse `html` (WTF-8) into this host's DOM, running scripts as the parser reaches them. Returns when parsing
    /// is done, or when it waits for a script's source; the rest (the
    /// parser going on, the `defer` scripts, `DOMContentLoaded` and the
    /// window's `load`) happens in tasks ([`DomHost::run_tasks`]).
    pub fn parse_document(&mut self, e: &mut E, html: &[u8]) {
        let dom = std::mem::replace(&mut self.dom, Dom::new());
        let mut b = TreeBuilder::new(dom, Options { scripting: true, ..Options::default() });
        b.pause_for_scripts(true);
        std::mem::swap(&mut self.dom, b.sink_mut());
        let mut tok = Tokenizer::new();
        tok.feed(html);
        tok.end();
        self.scripts.parser = Some(Box::new(Parser { tok, b, nesting: 0, writes: 0, writes_capped: false }));
        self.scripts.phase = Phase::Parsing;
        self.resume_parser(e);
    }

    /// Run the parser until the input is done, or until it waits for the
    /// source of its pending parsing-blocking script.
    pub(crate) fn resume_parser(&mut self, e: &mut E) {
        loop {
            if let Some(id) = self.scripts.blocking {
                if !self.scripts.is_ready(id) {
                    return self.settle(e);
                }
                self.run_blocking(e, id);
                continue;
            }
            if self.tokenize(e, false) != Step::Paused {
                break;
            }
        }
        self.scripts.parser = None;
        self.settle(e);
        self.run_defers(e);
    }

    /// Tokenize until the input runs out (or, `inserted`, until the
    /// insertion point), preparing the script the parser stops for.
    fn tokenize(&mut self, e: &mut E, inserted: bool) -> Step {
        let p = self.scripts.parser.as_mut().expect("the parser");
        std::mem::swap(&mut self.dom, p.b.sink_mut());
        let step = if inserted { p.tok.run_inserted(&mut p.b) } else { p.tok.run(&mut p.b) };
        std::mem::swap(&mut self.dom, p.b.sink_mut());
        if step == Step::Paused
            && let Some(s) = p.b.take_script()
        {
            self.parser_script(e, s);
        }
        step
    }

    fn parser_script(&mut self, e: &mut E, script: madcawl_dom::NodeId) {
        if !e.in_script() {
            e.run_microtasks(self);
        }
        let p = self.scripts.parser.as_mut().expect("the parser");
        p.tok.begin_script();
        p.nesting += 1;
        self.prepare_script(e, script, true);
        let p = self.scripts.parser.as_mut().expect("the parser");
        p.nesting -= 1;
        p.tok.end_script();
    }

    /// Run a classic script; what it throws is reported.
    pub fn run_script(&mut self, e: &mut E, source: &DomString, url: &DomString) {
        let r = e.evaluate(self, self.realm, source, &ScriptOrigin { url, line: 1, column: 0 });
        if e.in_script() {
            // a script document.write's parser ran: no checkpoint until
            // the outer script is done
            if E::is_thrown(r.get()) {
                let _sentinel = r.into_raw();
                if let Some(x) = e.take_exception() {
                    madcawl_js::Host::report_exception(self, e, x);
                }
            } else {
                e.release(r);
            }
            self.settle(e);
            return;
        }
        self.finish_task(e, r);
    }

    /// `document.write(text)`: see the module's description. While the
    /// parser has a pending parsing-blocking script the text waits in
    /// the input.
    pub(crate) fn document_write(&mut self, e: &mut E, text: &DomString) {
        let Some(p) = self.scripts.parser.as_mut() else { return };
        if !p.tok.has_insertion_point() || p.writes_capped {
            return;
        }
        if p.writes == MAX_WRITE_DEPTH {
            p.writes_capped = true;
            return;
        }
        let mut b = Vec::new();
        text.write_wtf8(&mut b);
        p.tok.insert(&b);
        if self.scripts.blocking.is_some() {
            return;
        }
        p.writes += 1;
        while self.tokenize(e, true) == Step::Paused && self.scripts.blocking.is_none() {}
        let p = self.scripts.parser.as_mut().expect("the parser");
        p.writes -= 1;
        p.writes_capped &= p.writes != 0;
    }

    /// The script element whose script is running, for
    /// `document.currentScript`.
    pub fn current_script(&self) -> Option<madcawl_dom::NodeId> {
        self.scripts.current
    }
}
