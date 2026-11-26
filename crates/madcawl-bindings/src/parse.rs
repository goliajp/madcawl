//! Parsing a document with its scripts run: the tree builder stops
//! after each script element's end tag, the script runs, and
//! `document.write` puts text into the input at the insertion point and
//! tokenizes it there, scripts inside it included.
//!
//! The DOM is the tree builder's sink while the parser runs and the
//! host's while a script does; it moves between the two (the nodes stay
//! where they are: only the arena's owner changes). Every call into the
//! engine happens while the host has it.

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};
use madcawl_js::{Engine, ScriptOrigin};
use madcawl_str::{DomString, atom};

use crate::host::DomHost;

/// Fetches a script's source by its `src` value; `None` when there is
/// none to be had.
pub type Loader = Box<dyn FnMut(&str) -> Option<Vec<u8>>>;

pub(crate) struct Parser {
    tok: Tokenizer,
    b: TreeBuilder<Dom>,
    /// `defer` and `async` scripts, run in order once parsing is done.
    later: Vec<(DomString, DomString)>,
}

/// What a script element asks for.
enum Script {
    Skip,
    Now(DomString, DomString),
    Later(DomString, DomString),
}

impl<E: Engine> DomHost<E> {
    /// Fetch external scripts with `loader`.
    pub fn set_loader(&mut self, loader: Loader) {
        self.loader = Some(loader);
    }

    /// Parse `html` (WTF-8) into this host's DOM, which is replaced,
    /// running scripts as the parser reaches them, then the deferred
    /// ones.
    pub fn parse_document(&mut self, e: &mut E, html: &[u8]) {
        let dom = std::mem::replace(&mut self.dom, Dom::new());
        let mut b = TreeBuilder::new(dom, Options { scripting: true, ..Options::default() });
        b.pause_for_scripts(true);
        let mut tok = Tokenizer::new();
        tok.feed(html);
        tok.end();
        self.parser = Some(Box::new(Parser { tok, b, later: Vec::new() }));
        self.pump(e, false);
        let p = self.parser.take().expect("the parser");
        self.dom = p.b.into_sink();
        for (src, url) in p.later {
            self.run_script(e, &src, &url);
        }
        self.settle(e);
    }

    fn swap_dom(&mut self) {
        let p = self.parser.as_mut().expect("the parser");
        std::mem::swap(&mut self.dom, p.b.sink_mut());
    }

    /// Tokenize until the input runs out (or, `inserted`, until the
    /// insertion point), running each script the parser stops for.
    fn pump(&mut self, e: &mut E, inserted: bool) {
        loop {
            let p = self.parser.as_mut().expect("the parser");
            let step = if inserted { p.tok.run_inserted(&mut p.b) } else { p.tok.run(&mut p.b) };
            if step != Step::Paused {
                return;
            }
            if let Some(s) = p.b.take_script() {
                self.parser_script(e, s);
            }
        }
    }

    fn parser_script(&mut self, e: &mut E, script: NodeId) {
        self.swap_dom();
        if !e.in_script() {
            e.run_microtasks(self);
        }
        self.parser.as_mut().expect("the parser").tok.begin_script();
        match self.script_of(script) {
            Script::Now(src, url) => self.run_script(e, &src, &url),
            Script::Later(src, url) => self.parser.as_mut().expect("the parser").later.push((src, url)),
            Script::Skip => {}
        }
        self.parser.as_mut().expect("the parser").tok.end_script();
        self.swap_dom();
    }

    fn script_of(&mut self, script: NodeId) -> Script {
        let dom = &self.dom;
        let attr = |name| dom.attribute(script, name).map(text);
        let classic = match attr(atom!("type")) {
            None => true,
            Some(t) => {
                let t = t.trim().to_ascii_lowercase();
                t.is_empty() || matches!(t.as_str(), "text/javascript" | "application/javascript" | "text/ecmascript")
            }
        };
        if !classic {
            return Script::Skip;
        }
        let Some(src) = attr(atom!("src")) else {
            let mut b = Vec::new();
            for c in dom.children(script).filter(|&c| dom.kind(c) == NodeKind::Text) {
                dom.text(c).expect("a text node").write_wtf8(&mut b);
            }
            return Script::Now(DomString::from_wtf8(&b), DomString::from_wtf8(b"inline"));
        };
        let later = attr(atom!("defer")).is_some() || attr(atom!("async")).is_some();
        let Some(bytes) = self.loader.as_mut().and_then(|l| l(&src)) else { return Script::Skip };
        let (s, u) = (DomString::from_wtf8(&bytes), DomString::from_wtf8(src.as_bytes()));
        if later { Script::Later(s, u) } else { Script::Now(s, u) }
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

    /// `document.write(text)`: see the module's description.
    pub(crate) fn document_write(&mut self, e: &mut E, text: &DomString) {
        let Some(p) = self.parser.as_mut() else { return };
        if !p.tok.has_insertion_point() {
            return;
        }
        let mut b = Vec::new();
        text.write_wtf8(&mut b);
        p.tok.insert(&b);
        self.swap_dom();
        self.pump(e, true);
        self.swap_dom();
    }
}

fn text(s: &DomString) -> String {
    let mut b = Vec::new();
    s.write_wtf8(&mut b);
    String::from_utf8_lossy(&b).into_owned()
}
