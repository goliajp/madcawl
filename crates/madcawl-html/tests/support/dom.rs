//! A DOM just big enough to print what the tree builder built, in
//! the html5lib test format.

use madcawl_html::{AttrNamespace, Attrs, Namespace, QuirksMode, TreeSink};

pub enum Kind {
    Document,
    Doctype(Vec<u8>, Vec<u8>, Vec<u8>),
    Element {
        ns: Namespace,
        name: Vec<u8>,
        attrs: Vec<(Vec<u8>, Vec<u8>)>,
        contents: Option<usize>,
    },
    Text(Vec<u8>),
    Comment(Vec<u8>),
    Pi(Vec<u8>, Vec<u8>),
    Fragment,
}

pub struct Node {
    kind: Kind,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// A DOM just big enough to print what the parser built.
pub struct Dom {
    pub nodes: Vec<Node>,
    /// Every `associate_form` call: element, form, whether the intended
    /// parent was in the form's tree (and so the two were associated).
    pub forms: Vec<(usize, usize, bool)>,
}

impl Dom {
    pub fn new() -> Self {
        Dom {
            nodes: vec![Node {
                kind: Kind::Document,
                parent: None,
                children: Vec::new(),
            }],
            forms: Vec::new(),
        }
    }

    fn root(&self, mut node: usize) -> usize {
        while let Some(p) = self.nodes[node].parent {
            node = p;
        }
        node
    }

    /// The element's tag name, for tests that look at what was recorded.
    #[allow(dead_code)]
    pub fn name(&self, node: usize) -> &[u8] {
        match &self.nodes[node].kind {
            Kind::Element { name, .. } => name,
            _ => b"",
        }
    }

    fn add(&mut self, kind: Kind) -> usize {
        self.nodes.push(Node {
            kind,
            parent: None,
            children: Vec::new(),
        });
        self.nodes.len() - 1
    }

    fn detach(&mut self, node: usize) {
        if let Some(p) = self.nodes[node].parent.take() {
            self.nodes[p].children.retain(|&c| c != node);
        }
    }

    fn insert(&mut self, parent: usize, child: usize, at: usize) {
        self.detach(child);
        self.nodes[parent].children.insert(at, child);
        self.nodes[child].parent = Some(parent);
    }

    fn index_of(&self, parent: usize, child: usize) -> usize {
        self.nodes[parent]
            .children
            .iter()
            .position(|&c| c == child)
            .expect("a child of its parent")
    }
}

fn attr_name(ns: AttrNamespace, local: &[u8]) -> Vec<u8> {
    let prefix: &[u8] = match ns {
        AttrNamespace::None => return local.to_vec(),
        AttrNamespace::XLink => b"xlink ",
        AttrNamespace::Xml => b"xml ",
        AttrNamespace::Xmlns => b"xmlns ",
    };
    [prefix, local].concat()
}

impl TreeSink for Dom {
    type Handle = usize;

    fn document(&self) -> usize {
        0
    }

    fn create_element(&mut self, ns: Namespace, local: &[u8], attrs: Attrs<'_>) -> usize {
        let contents =
            (ns == Namespace::Html && local == b"template").then(|| self.add(Kind::Fragment));
        let attrs = attrs
            .iter()
            .map(|a| (attr_name(a.ns, a.local), a.value.to_vec()))
            .collect();
        self.add(Kind::Element {
            ns,
            name: local.to_vec(),
            attrs,
            contents,
        })
    }

    fn create_comment(&mut self, data: &[u8]) -> usize {
        self.add(Kind::Comment(data.to_vec()))
    }

    fn create_processing_instruction(&mut self, target: &[u8], data: &[u8]) -> usize {
        self.add(Kind::Pi(target.to_vec(), data.to_vec()))
    }

    fn append_doctype(&mut self, name: &[u8], public_id: &[u8], system_id: &[u8]) {
        let d = self.add(Kind::Doctype(
            name.to_vec(),
            public_id.to_vec(),
            system_id.to_vec(),
        ));
        self.append(0, d);
    }

    fn append(&mut self, parent: usize, child: usize) {
        self.detach(child);
        let n = self.nodes[parent].children.len();
        self.insert(parent, child, n);
    }

    fn insert_before(&mut self, parent: usize, child: usize, before: usize) {
        self.detach(child);
        let i = self.index_of(parent, before);
        self.insert(parent, child, i);
    }

    fn insert_text(&mut self, parent: usize, before: Option<usize>, text: &[u8]) {
        let i = match before {
            Some(b) => self.index_of(parent, b),
            None => self.nodes[parent].children.len(),
        };
        if i > 0 {
            let prev = self.nodes[parent].children[i - 1];
            if let Kind::Text(t) = &mut self.nodes[prev].kind {
                t.extend_from_slice(text);
                return;
            }
        }
        let t = self.add(Kind::Text(text.to_vec()));
        self.insert(parent, t, i);
    }

    fn remove(&mut self, node: usize) {
        self.detach(node);
    }

    fn reparent_children(&mut self, from: usize, to: usize) {
        for c in std::mem::take(&mut self.nodes[from].children) {
            self.nodes[c].parent = None;
            self.append(to, c);
        }
    }

    fn parent(&self, node: usize) -> Option<usize> {
        self.nodes[node].parent
    }

    fn add_attrs_if_missing(&mut self, element: usize, attrs: Attrs<'_>) {
        let Kind::Element { attrs: have, .. } = &mut self.nodes[element].kind else {
            panic!("not an element");
        };
        for a in attrs.iter() {
            let name = attr_name(a.ns, a.local);
            if !have.iter().any(|(n, _)| *n == name) {
                have.push((name, a.value.to_vec()));
            }
        }
    }

    fn template_contents(&mut self, template: usize) -> usize {
        match &self.nodes[template].kind {
            Kind::Element {
                contents: Some(c), ..
            } => *c,
            _ => panic!("not a template"),
        }
    }

    fn set_quirks_mode(&mut self, _mode: QuirksMode) {}

    fn associate_form(&mut self, element: usize, form: usize, intended_parent: usize) {
        let same = self.root(intended_parent) == self.root(form);
        self.forms.push((element, form, same));
    }

    fn children_finished(&mut self, element: usize) {
        if self.is_html(element, b"option") {
            self.clone_into_selectedcontent(element);
        }
    }
}

/// A customizable `<select>` shows its selected option's content in its
/// `<selectedcontent>` element. The parser triggers the copy when an
/// option's children are complete; the copying is the DOM's business,
/// done here just well enough for the tests that check it.
impl Dom {
    fn is_html(&self, node: usize, name: &[u8]) -> bool {
        matches!(&self.nodes[node].kind, Kind::Element { ns: Namespace::Html, name: n, .. } if n == name)
    }

    fn has_attr(&self, node: usize, name: &[u8]) -> bool {
        matches!(&self.nodes[node].kind, Kind::Element { attrs, .. } if attrs.iter().any(|(n, _)| n == name))
    }

    fn descendants(&self, node: usize, out: &mut Vec<usize>) {
        for &c in &self.nodes[node].children {
            out.push(c);
            self.descendants(c, out);
        }
    }

    fn clone_into_selectedcontent(&mut self, option: usize) {
        let mut select = self.nodes[option].parent;
        while let Some(s) = select.filter(|&s| !self.is_html(s, b"select")) {
            select = self.nodes[s].parent;
        }
        let Some(select) = select.filter(|&s| !self.has_attr(s, b"multiple")) else {
            return;
        };
        let mut all = Vec::new();
        self.descendants(select, &mut all);
        let options: Vec<usize> = all
            .iter()
            .copied()
            .filter(|&n| self.is_html(n, b"option"))
            .collect();
        let selected = if self.has_attr(option, b"selected") {
            true
        } else {
            options.first() == Some(&option)
                && !options.iter().any(|&o| self.has_attr(o, b"selected"))
        };
        let target = all
            .iter()
            .copied()
            .find(|&n| self.is_html(n, b"selectedcontent"));
        let (true, Some(target)) = (selected, target) else {
            return;
        };
        for c in std::mem::take(&mut self.nodes[target].children) {
            self.nodes[c].parent = None;
        }
        for c in self.nodes[option].children.clone() {
            let copy = self.deep_clone(c);
            self.append(target, copy);
        }
    }

    fn deep_clone(&mut self, node: usize) -> usize {
        let kind = match &self.nodes[node].kind {
            Kind::Element {
                ns, name, attrs, ..
            } => Kind::Element {
                ns: *ns,
                name: name.clone(),
                attrs: attrs.clone(),
                contents: None,
            },
            Kind::Text(t) => Kind::Text(t.clone()),
            Kind::Comment(d) => Kind::Comment(d.clone()),
            Kind::Pi(t, d) => Kind::Pi(t.clone(), d.clone()),
            Kind::Document | Kind::Doctype(..) | Kind::Fragment => panic!("not inside an option"),
        };
        let copy = self.add(kind);
        for c in self.nodes[node].children.clone() {
            let cc = self.deep_clone(c);
            self.append(copy, cc);
        }
        copy
    }
}

fn indent(out: &mut Vec<u8>, depth: usize) {
    out.extend_from_slice(b"| ");
    for _ in 0..depth {
        out.extend_from_slice(b"  ");
    }
}

pub fn print(dom: &Dom, node: usize, depth: usize, out: &mut Vec<u8>) {
    for &c in &dom.nodes[node].children {
        indent(out, depth);
        match &dom.nodes[c].kind {
            Kind::Doctype(name, public, system) => {
                out.extend_from_slice(b"<!DOCTYPE ");
                out.extend_from_slice(name);
                if !public.is_empty() || !system.is_empty() {
                    out.extend_from_slice(b" \"");
                    out.extend_from_slice(public);
                    out.extend_from_slice(b"\" \"");
                    out.extend_from_slice(system);
                    out.push(b'"');
                }
                out.extend_from_slice(b">\n");
            }
            Kind::Element {
                ns,
                name,
                attrs,
                contents,
            } => {
                out.push(b'<');
                match ns {
                    Namespace::Html => {}
                    Namespace::Svg => out.extend_from_slice(b"svg "),
                    Namespace::MathMl => out.extend_from_slice(b"math "),
                }
                out.extend_from_slice(name);
                out.extend_from_slice(b">\n");
                let mut sorted: Vec<_> = attrs.iter().collect();
                sorted.sort();
                for (n, v) in sorted {
                    indent(out, depth + 1);
                    out.extend_from_slice(n);
                    out.extend_from_slice(b"=\"");
                    out.extend_from_slice(v);
                    out.extend_from_slice(b"\"\n");
                }
                if let Some(t) = contents {
                    indent(out, depth + 1);
                    out.extend_from_slice(b"content\n");
                    print(dom, *t, depth + 2, out);
                }
            }
            Kind::Text(t) => {
                out.push(b'"');
                out.extend_from_slice(t);
                out.extend_from_slice(b"\"\n");
            }
            Kind::Comment(d) => {
                out.extend_from_slice(b"<!-- ");
                out.extend_from_slice(d);
                out.extend_from_slice(b" -->\n");
            }
            Kind::Pi(t, d) => {
                out.extend_from_slice(b"<?");
                out.extend_from_slice(t);
                out.push(b' ');
                out.extend_from_slice(d);
                out.extend_from_slice(b"?>\n");
            }
            Kind::Document | Kind::Fragment => panic!("a document or fragment as a child"),
        }
        print(dom, c, depth + 1, out);
    }
}
