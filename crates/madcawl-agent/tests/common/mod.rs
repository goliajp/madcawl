#![allow(dead_code)]

use madcawl_agent::{Agents, Geometry, Inputs, SessionId, View, ViewKind, ViewOpts};
use madcawl_css::style::{Device, Styles, style_document};
use madcawl_dom::{Dom, NodeId};
use madcawl_html::{DocumentParser, Options, Source, Step};
use madcawl_layout::Layout;

/// A parsed page with its styles and geometry, and the agents on it.
pub struct Page {
    pub dom: Dom,
    pub styles: Styles,
    pub geometry: Option<Geometry>,
    pub agents: Agents,
}

pub fn parse(html: &str) -> Dom {
    let mut p = DocumentParser::new(Dom::new, Options::default(), Source::default());
    p.feed(html.as_bytes());
    p.end();
    assert_eq!(p.run(), Step::Done);
    p.into_sink()
}

impl Page {
    pub fn new(html: &str) -> Page {
        let dom = parse(html);
        let styles = style_document(&dom, Device::default(), &mut |_| None);
        let mut p = Page {
            dom,
            styles,
            geometry: None,
            agents: Agents::new(),
        };
        p.relayout();
        p
    }

    /// Styles and lays the page out again after the DOM changed.
    pub fn relayout(&mut self) {
        self.styles = style_document(&self.dom, Device::default(), &mut |_| None);
        let mut layout = Layout::new(&self.dom, &self.styles, 1280.0, 800.0);
        self.geometry = Some(Geometry::from_layout(
            &mut layout,
            &self.styles,
            1280.0,
            800.0,
        ));
    }

    pub fn inputs(&self) -> Inputs<'_> {
        Inputs {
            styles: Some(&self.styles),
            geometry: self.geometry.as_ref(),
        }
    }

    pub fn attach(&mut self) -> SessionId {
        let inputs = Inputs {
            styles: Some(&self.styles),
            geometry: self.geometry.as_ref(),
        };
        self.agents
            .attach(&mut self.dom, inputs)
            .expect("room for a session")
    }

    pub fn view(&mut self, s: SessionId, kind: ViewKind, opts: ViewOpts) -> (u64, View) {
        let inputs = Inputs {
            styles: Some(&self.styles),
            geometry: self.geometry.as_ref(),
        };
        self.agents
            .view(s, &mut self.dom, inputs, &kind, opts)
            .expect("a session")
    }

    pub fn snapshot(&mut self, s: SessionId) -> String {
        self.view(s, ViewKind::Snapshot, ViewOpts::default()).1.text
    }

    pub fn diff(
        &mut self,
        s: SessionId,
        since: u64,
    ) -> Result<(u64, View), madcawl_agent::AgentError> {
        let inputs = Inputs {
            styles: Some(&self.styles),
            geometry: self.geometry.as_ref(),
        };
        self.agents.diff(s, &mut self.dom, inputs, since)
    }

    pub fn resolve(&mut self, s: SessionId, r: &str) -> Result<NodeId, madcawl_agent::AgentError> {
        let inputs = Inputs {
            styles: Some(&self.styles),
            geometry: self.geometry.as_ref(),
        };
        self.agents.resolve(s, &mut self.dom, inputs, r)
    }

    /// The element with the ID, by a walk.
    pub fn by_id(&self, id: &str) -> NodeId {
        let doc = self.dom.document();
        self.dom
            .descendants(doc)
            .find(|&n| {
                self.dom
                    .attribute(n, madcawl_str::atom!("id"))
                    .is_some_and(|v| v.eq_str(id))
            })
            .expect("an element with that id")
    }
}

/// The reference a snapshot line containing `needle` carries.
pub fn ref_on(text: &str, needle: &str) -> String {
    let line = text
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle} in\n{text}"));
    let a = line.find(" [e").expect("a reference") + 2;
    let b = a + line[a..].find(']').expect("closed");
    line[a..b].to_owned()
}
