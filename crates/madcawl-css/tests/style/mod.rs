//! Parsing a page and finding its elements, for the cascade's tests.

use madcawl_css::Stylesheet;
use madcawl_css::style::{
    Cascade, Change, ComputedStyle, Device, Display, ElementStates, Restyled, Styles,
    style_document,
};
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

/// The page's `<style>` sheets, parsed.
pub fn sheets(dom: &Dom) -> Vec<madcawl_css::Stylesheet> {
    madcawl_css::style::sheet_sources(dom)
        .into_iter()
        .filter_map(|s| match s {
            madcawl_css::style::SheetSource::Inline { text, .. } => {
                madcawl_css::Stylesheet::parse(text).ok()
            }
            madcawl_css::style::SheetSource::Link { .. } => None,
        })
        .collect()
}

/// A cascade of the user agent sheet and `sheets`.
pub fn cascade(sheets: &[madcawl_css::Stylesheet]) -> madcawl_css::style::Cascade<'_> {
    use madcawl_css::style::{Cascade, Origin, ua_sheet};
    let mut c = Cascade::new(Device::default());
    c.add(ua_sheet(), Origin::UserAgent);
    for s in sheets {
        c.add(s, Origin::Author);
    }
    c
}

/// How many elements have other values, custom properties or rendered
/// flags in `a` than in `b`.
pub fn differences(dom: &Dom, a: &Styles, b: &Styles) -> usize {
    dom.descendants(dom.document())
        .filter(|&n| dom.kind(n) == NodeKind::Element)
        .filter(|&n| {
            a.get(n) != b.get(n)
                || a.custom_properties(n) != b.custom_properties(n)
                || a.is_rendered(dom, n) != b.is_rendered(dom, n)
                || a.is_visible(dom, n) != b.is_visible(dom, n)
        })
        .count()
}

/// The element with id `id`.
pub fn by_id(dom: &Dom, id: &str) -> NodeId {
    let want = Atom::lookup(id.as_bytes()).unwrap_or_else(|| panic!("no #{id}"));
    dom.descendants(dom.document())
        .find(|&n| dom.kind(n) == NodeKind::Element && dom.element_id(n) == Some(want))
        .unwrap_or_else(|| panic!("no #{id}"))
}

/// A page's cascade, and the same one without `PathKey` that restyles
/// are checked against.
pub struct Pair<'a> {
    pub keyed: Cascade<'a>,
    pub plain: Cascade<'a>,
}

impl<'a> Pair<'a> {
    pub fn new(list: &'a [Stylesheet]) -> Self {
        let mut plain = cascade(list);
        plain.set_path_key(false);
        Pair {
            keyed: cascade(list),
            plain,
        }
    }

    pub fn compute_with(&self, dom: &Dom, st: &ElementStates) -> Styles {
        self.keyed.compute_with(dom, st)
    }
}

/// Restyle `s` after `changes` and check it against a full run without
/// `PathKey`; every `PathKey` hit of the restyle is checked too.
#[track_caller]
pub fn check(
    c: &Pair<'_>,
    dom: &Dom,
    s: &mut Styles,
    st: &ElementStates,
    ch: &[Change],
) -> Restyled {
    let r = c.keyed.restyle(dom, s, st, ch);
    assert_eq!(
        s.stats().mismatches,
        0,
        "a hit differs from the full path at {:?}",
        s.stats().first_mismatch
    );
    let full = c.plain.compute_with(dom, st);
    assert_eq!(
        differences(dom, s, &full),
        0,
        "restyled differs from a full run"
    );
    r
}

pub fn atom_of(name: &str) -> Atom {
    Atom::lookup(name.as_bytes()).expect("a name the page uses")
}

/// Set an attribute and say so.
pub fn set(dom: &mut Dom, el: NodeId, name: Atom, v: &str) -> Change {
    let old = dom.attribute(el, name).map(|s| {
        let mut b = Vec::new();
        s.write_wtf8(&mut b);
        String::from_utf8(b).expect("utf-8")
    });
    dom.set_attribute(el, name, v.as_bytes());
    Change::Attribute { el, name, old }
}
