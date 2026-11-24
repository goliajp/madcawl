//! Parsing a page and finding its elements, for the cascade's tests.

use madcawl_css::style::{ComputedStyle, Device, Display, Styles, style_document};
use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::{Atom, Options, Step, Tokenizer, TreeBuilder};

pub struct Page {
    pub dom: Dom,
    pub styles: Styles,
}

pub fn parse(html: &str) -> Dom {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(html.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    b.into_sink()
}

pub fn page(html: &str) -> Page {
    page_on(html, Device::default())
}

pub fn page_on(html: &str, device: Device) -> Page {
    let dom = parse(html);
    let styles = style_document(&dom, device, &mut |_| None);
    Page { dom, styles }
}

impl Page {
    pub fn el(&self, id: &str) -> NodeId {
        let want = Atom::lookup(id.as_bytes()).unwrap_or_else(|| panic!("no #{id}"));
        self.dom
            .descendants(self.dom.document())
            .find(|&n| {
                self.dom.kind(n) == NodeKind::Element && self.dom.element_id(n) == Some(want)
            })
            .unwrap_or_else(|| panic!("no #{id}"))
    }

    pub fn style(&self, id: &str) -> &ComputedStyle {
        self.styles.get(self.el(id)).expect("a styled element")
    }

    pub fn display(&self, id: &str) -> Display {
        self.style(id).display
    }

    pub fn rendered(&self, id: &str) -> bool {
        self.styles.is_rendered(&self.dom, self.el(id))
    }

    pub fn visible(&self, id: &str) -> bool {
        self.styles.is_visible(&self.dom, self.el(id))
    }

    /// The first text node below `#id`.
    pub fn text_in(&self, id: &str) -> NodeId {
        self.dom
            .descendants(self.el(id))
            .find(|&n| self.dom.kind(n) == NodeKind::Text)
            .expect("text")
    }
}
