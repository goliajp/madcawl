//! The HTML fragment serialization algorithm (HTML 13.3), written as
//! WTF-8: what `innerHTML` and `outerHTML` read, and what a document
//! is written back as.
//!
//! There are no shadow roots and no `is` values yet, so the steps for
//! them have nothing to do.

use madcawl_html::{AttrNamespace, Namespace, atom};
use madcawl_str::{Atom, DomString};

use crate::dom::{Dom, NodeId};
use crate::node::NodeKind;

impl Dom {
    /// Write the children of `node` (an element, document or fragment),
    /// a template's being its contents. `scripting` is whether
    /// scripting is enabled for the node, which decides whether the text
    /// in a `noscript` is written literally.
    pub fn write_html(&self, node: NodeId, scripting: bool, out: &mut Vec<u8>) {
        if self.serializes_as_void(node) {
            return;
        }
        // template contents belong to an inert document without a
        // browsing context, so scripting is disabled for everything in them
        let (node, scripting) = match self.contents(node) {
            Some(c) => (c, false),
            None => (node, scripting),
        };
        for c in self.children(node) {
            self.write_node(c, scripting, out);
        }
    }

    /// Write `node` itself and everything below it: the serialization
    /// of a fragment whose only child is `node`.
    pub fn write_outer_html(&self, node: NodeId, scripting: bool, out: &mut Vec<u8>) {
        match self.kind(node) {
            NodeKind::Document | NodeKind::Fragment => self.write_html(node, scripting, out),
            _ => self.write_node(node, scripting, out),
        }
    }

    fn write_node(&self, c: NodeId, scripting: bool, out: &mut Vec<u8>) {
        match self.kind(c) {
            NodeKind::Element => {
                let name = self.local_name(c).expect("an element");
                out.push(b'<');
                name.write_wtf8(out);
                for a in self.attributes(c) {
                    out.push(b' ');
                    match a.ns {
                        AttrNamespace::None => {}
                        AttrNamespace::Xml => out.extend_from_slice(b"xml:"),
                        AttrNamespace::Xmlns if a.local == atom!("xmlns") => {}
                        AttrNamespace::Xmlns => out.extend_from_slice(b"xmlns:"),
                        AttrNamespace::XLink => out.extend_from_slice(b"xlink:"),
                    }
                    a.local.write_wtf8(out);
                    out.extend_from_slice(b"=\"");
                    write_escaped(a.value, true, out);
                    out.push(b'"');
                }
                out.push(b'>');
                if self.serializes_as_void(c) {
                    return;
                }
                self.write_html(c, scripting, out);
                out.extend_from_slice(b"</");
                name.write_wtf8(out);
                out.push(b'>');
            }
            NodeKind::Text => {
                let data = self.char_data(c.index());
                if self
                    .parent(c)
                    .is_some_and(|p| self.holds_raw_text(p, scripting))
                {
                    data.write_wtf8(out);
                } else {
                    write_escaped(data, false, out);
                }
            }
            NodeKind::Comment => {
                out.extend_from_slice(b"<!--");
                self.char_data(c.index()).write_wtf8(out);
                out.extend_from_slice(b"-->");
            }
            NodeKind::ProcessingInstruction => {
                out.extend_from_slice(b"<?");
                self.pi_target(c)
                    .expect("a processing instruction")
                    .write_wtf8(out);
                out.push(b' ');
                self.char_data(c.index()).write_wtf8(out);
                out.extend_from_slice(b"?>");
            }
            NodeKind::Doctype => {
                out.extend_from_slice(b"<!DOCTYPE ");
                self.doctype(c).expect("a doctype")[0].write_wtf8(out);
                out.push(b'>');
            }
            NodeKind::Document | NodeKind::Fragment => {
                unreachable!("a document or fragment is never a child")
            }
        }
    }

    fn html_name(&self, n: NodeId) -> Option<Atom> {
        (self.namespace(n) == Some(Namespace::Html))
            .then(|| self.local_name(n))
            .flatten()
    }

    /// One of the void elements, or basefont, bgsound, frame, keygen or
    /// param.
    fn serializes_as_void(&self, n: NodeId) -> bool {
        self.html_name(n).is_some_and(|name| {
            [
                atom!("area"),
                atom!("base"),
                atom!("br"),
                atom!("col"),
                atom!("embed"),
                atom!("hr"),
                atom!("img"),
                atom!("input"),
                atom!("link"),
                atom!("meta"),
                atom!("source"),
                atom!("track"),
                atom!("wbr"),
                atom!("basefont"),
                atom!("bgsound"),
                atom!("frame"),
                atom!("keygen"),
                atom!("param"),
            ]
            .contains(&name)
        })
    }

    fn holds_raw_text(&self, parent: NodeId, scripting: bool) -> bool {
        self.html_name(parent).is_some_and(|name| {
            [
                atom!("style"),
                atom!("script"),
                atom!("xmp"),
                atom!("iframe"),
                atom!("noembed"),
                atom!("noframes"),
                atom!("plaintext"),
            ]
            .contains(&name)
                || scripting && name == atom!("noscript")
        })
    }
}

/// Escaping a string: `&`, U+00A0, `<` and `>`, and in attribute mode
/// `"` as well.
fn write_escaped(s: &DomString, attribute: bool, out: &mut Vec<u8>) {
    let start = out.len();
    s.write_wtf8(out);
    // in WTF-8, C2 A0 is only ever U+00A0, and the ASCII bytes are only
    // ever themselves
    let special = |b: u8| matches!(b, b'&' | b'<' | b'>' | 0xC2) || attribute && b == b'"';
    let Some(first) = out[start..].iter().position(|&b| special(b)) else {
        return;
    };
    // the escaped tail is appended after the raw one, which is then
    // drained, so no buffer is allocated per string
    let from = start + first;
    let end = out.len();
    let mut i = from;
    while i < end {
        let b = out[i];
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' if attribute => out.extend_from_slice(b"&quot;"),
            0xC2 if i + 1 < end && out[i + 1] == 0xA0 => {
                out.extend_from_slice(b"&nbsp;");
                i += 1;
            }
            _ => out.push(b),
        }
        i += 1;
    }
    out.drain(from..end);
}
