//! A tree written in the html5lib tree construction test format, the
//! `#document` section of a `.dat` case.

use madcawl_html::{AttrNamespace, Namespace};

use crate::dom::{Dom, NodeId};
use crate::node::NodeKind;

fn indent(out: &mut Vec<u8>, depth: usize) {
    out.extend_from_slice(b"| ");
    for _ in 0..depth {
        out.extend_from_slice(b"  ");
    }
}

impl Dom {
    /// Write the children of `root` and everything below them, one
    /// node a line.
    pub fn write_dat(&self, root: NodeId, out: &mut Vec<u8>) {
        self.write_children(root, 0, out);
    }

    fn write_children(&self, node: NodeId, depth: usize, out: &mut Vec<u8>) {
        for c in self.children(node) {
            indent(out, depth);
            match self.kind(c) {
                NodeKind::Doctype => self.write_doctype(c, out),
                NodeKind::Element => self.write_element(c, depth, out),
                NodeKind::Text => {
                    out.push(b'"');
                    self.char_data(c.index()).write_wtf8(out);
                    out.extend_from_slice(b"\"\n");
                }
                NodeKind::Comment => {
                    out.extend_from_slice(b"<!-- ");
                    self.char_data(c.index()).write_wtf8(out);
                    out.extend_from_slice(b" -->\n");
                }
                NodeKind::ProcessingInstruction => {
                    out.extend_from_slice(b"<?");
                    self.pi_target(c)
                        .expect("a processing instruction")
                        .write_wtf8(out);
                    out.push(b' ');
                    self.char_data(c.index()).write_wtf8(out);
                    out.extend_from_slice(b"?>\n");
                }
                NodeKind::Document | NodeKind::Fragment => {
                    unreachable!("a document or fragment is never a child")
                }
            }
            self.write_children(c, depth + 1, out);
        }
    }

    fn write_doctype(&self, d: NodeId, out: &mut Vec<u8>) {
        let [name, public, system] = self.doctype(d).expect("a doctype");
        out.extend_from_slice(b"<!DOCTYPE ");
        name.write_wtf8(out);
        if !public.is_empty() || !system.is_empty() {
            out.extend_from_slice(b" \"");
            public.write_wtf8(out);
            out.extend_from_slice(b"\" \"");
            system.write_wtf8(out);
            out.push(b'"');
        }
        out.extend_from_slice(b">\n");
    }

    fn write_element(&self, el: NodeId, depth: usize, out: &mut Vec<u8>) {
        out.push(b'<');
        match self.namespace(el).expect("an element") {
            Namespace::Html => {}
            Namespace::Svg => out.extend_from_slice(b"svg "),
            Namespace::MathMl => out.extend_from_slice(b"math "),
        }
        self.local_name(el).expect("an element").write_wtf8(out);
        out.extend_from_slice(b">\n");
        let mut attrs: Vec<(Vec<u8>, Vec<u8>)> = self
            .attributes(el)
            .map(|a| {
                let mut name = match a.ns {
                    AttrNamespace::None => Vec::new(),
                    AttrNamespace::XLink => b"xlink ".to_vec(),
                    AttrNamespace::Xml => b"xml ".to_vec(),
                    AttrNamespace::Xmlns => b"xmlns ".to_vec(),
                };
                a.local.write_wtf8(&mut name);
                let mut value = Vec::new();
                a.value.write_wtf8(&mut value);
                (name, value)
            })
            .collect();
        attrs.sort();
        for (n, v) in attrs {
            indent(out, depth + 1);
            out.extend_from_slice(&n);
            out.extend_from_slice(b"=\"");
            out.extend_from_slice(&v);
            out.extend_from_slice(b"\"\n");
        }
        if let Some(t) = self.contents(el) {
            indent(out, depth + 1);
            out.extend_from_slice(b"content\n");
            self.write_children(t, depth + 2, out);
        }
    }
}
