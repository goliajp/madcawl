//! The boxes of `::before` and `::after`: one box with the pseudo-element's computed values as the first or
//! last child of its element's, holding the text its `content` makes.

use madcawl_css::style::{ComputedStyle, Content, ContentItem};
use madcawl_dom::{Dom, NodeId};

use super::build::Builder;
use super::kind::kind_of;
use super::{Kind, NONE};

// the text of a `content` list: strings, the element's attributes, and quotes (not nested); counters and
// images make nothing
fn content_text(dom: &Dom, el: NodeId, c: &Content) -> Vec<u16> {
    let mut out = Vec::new();
    let Content::Items(items) = c else {
        return out;
    };
    for it in items.iter() {
        match it {
            ContentItem::Text(t) => out.extend(t.encode_utf16()),
            ContentItem::Attr(name) => {
                let value = dom.attributes(el).find(|a| {
                    a.ns == madcawl_html::AttrNamespace::None
                        && a.local.to_dom_string().as_ascii_str() == Some(&**name)
                });
                if let Some(a) = value {
                    out.extend(a.value.units());
                }
            }
            // the quotes WebKit gives a document without a language
            ContentItem::OpenQuote | ContentItem::CloseQuote => out.push(0x22),
            ContentItem::Counter(_)
            | ContentItem::NoOpenQuote
            | ContentItem::NoCloseQuote
            | ContentItem::Image => {}
        }
    }
    out
}

impl<'a> Builder<'a> {
    /// The computed values a box takes for the shape of the tree: a pseudo-element's own.
    pub(super) fn style_of(&self, b: u32) -> Option<&'a ComputedStyle> {
        let bx = self.t.boxes[b as usize];
        match bx.pseudo {
            0 => self.styles.get(bx.node),
            k => self.styles.pseudo(bx.node, k == 2),
        }
    }

    /// The box of `el`'s `::before` (`after` false) or `::after`, if it has one, appended to `parent`.
    pub(super) fn generate(&mut self, el: NodeId, parent: u32, after: bool) {
        if parent == NONE {
            return;
        }
        let Some(s) = self.styles.pseudo(el, after) else {
            return;
        };
        let pseudo = 1 + u8::from(after);
        let pb = self.push(kind_of(self.dom, el, s), el);
        self.t.boxes[pb as usize].pseudo = pseudo;
        self.append(parent, pb);
        let text = content_text(self.dom, el, &s.rare.content);
        if text.is_empty() {
            return;
        }
        let i = self.t.generated.len() as u32;
        self.t.generated.push(text.into_boxed_slice());
        let tb = self.push(Kind::Generated(i), el);
        self.t.boxes[tb as usize].pseudo = pseudo;
        self.append(pb, tb);
    }
}
