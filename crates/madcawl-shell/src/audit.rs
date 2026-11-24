//! Two checks, shaped differently, that nothing the cascade hides made
//! it into a read.
//!
//! By node: every node the read took something from, and every
//! reference written in the text, against the set of element and text
//! nodes the cascade does not leave visible or that are under
//! `opacity: 0`, worked out here from `Styles` again rather than taken
//! from the reader's `Shown`. By text: pieces of hidden text that
//! occur nowhere in what is shown (text or attributes) are searched for
//! in the written text. The text check runs on shown text as well, which
//! must be found: a search that cannot find anything would pass too.

use madcawl_css::style::Styles;
use madcawl_dom::{Dom, NodeId, NodeKind};

use crate::read::{Read, parse_reference, svg_title};

/// How many pieces of text each side of the text check samples.
const SAMPLES: usize = 300;
/// Shorter pieces are too likely to occur by chance.
const MIN_CHARS: usize = 12;
const MAX_CHARS: usize = 48;

#[derive(Debug, Default)]
pub struct Report {
    pub hidden_elements: usize,
    pub hidden_texts: usize,
    pub used: usize,
    pub used_hidden: Vec<u32>,
    pub refs: usize,
    pub refs_hidden: Vec<u32>,
    /// Hidden pieces of text, and how many were shown elsewhere
    pub hidden_pieces: usize,
    pub hidden_sampled: usize,
    pub not_distinct: usize,
    pub hidden_tested: usize,
    pub hidden_found: Vec<String>,
    pub shown_tested: usize,
    pub shown_found: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// Nothing leaked, but nothing was shown either: the checks could
    /// not have caught anything
    Empty,
    Fail,
}

impl Report {
    pub fn verdict(&self) -> Verdict {
        let leaked = !self.used_hidden.is_empty()
            || !self.refs_hidden.is_empty()
            || !self.hidden_found.is_empty();
        if leaked || self.shown_found != self.shown_tested {
            Verdict::Fail
        } else if self.used == 0 || self.refs == 0 || self.shown_tested == 0 {
            Verdict::Empty
        } else {
            Verdict::Pass
        }
    }
}

/// By node index: not shown. The `<title>`'s text is read as the
/// page's title, so it is left out.
pub fn hidden_set(dom: &Dom, styles: &Styles, title: Option<NodeId>) -> Vec<bool> {
    // by node index: the element or an ancestor is fully transparent;
    // tree order reaches a parent before its children
    let doc = dom.document();
    let len = dom
        .descendants(doc)
        .map(|n| n.index() as usize + 1)
        .max()
        .unwrap_or(0);
    let (mut clear, mut hidden) = (vec![false; len], vec![false; len]);
    for n in dom.descendants(doc) {
        let k = dom.kind(n);
        let i = n.index() as usize;
        let above = dom
            .parent(n)
            .is_some_and(|p| clear.get(p.index() as usize) == Some(&true));
        clear[i] =
            above || (k == NodeKind::Element && styles.get(n).is_some_and(|s| s.opacity() == 0.0));
        let in_title = title.is_some_and(|t| n == t || dom.parent(n) == Some(t));
        if matches!(k, NodeKind::Element | NodeKind::Text) && !in_title {
            hidden[i] = !styles.is_visible(dom, n) || clear[i];
        }
    }
    hidden
}

/// The node indices of the references in `text`: the one after an
/// item's kind at the start of a line, and those that end links,
/// `)[e…]`. The header and the lines of preformatted text are skipped.
pub fn references(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let body = text.split_once("\n---\n").map_or(text, |(_, b)| b);
    let mut push = |r: &str| {
        if let Some(n) = r.split_once(']').and_then(|(r, _)| parse_reference(r)) {
            out.push(n);
        }
    };
    for line in body.lines() {
        let line = line.trim_start_matches(' ');
        if line.starts_with("> ") {
            continue;
        }
        if let Some((_, rest)) = line.split_once(' ')
            && let Some(r) = rest.strip_prefix('[')
        {
            push(r);
        }
        for (i, _) in line.match_indices(")[e") {
            push(&line[i + 2..]);
        }
    }
    out
}

/// The longest run of a text with nothing the text form escapes or
/// folds, clipped; `None` when it is too short.
fn piece(t: &str) -> Option<String> {
    let mut best = "";
    let mut start = 0;
    let mut prev = '\0';
    for (i, c) in t.char_indices() {
        let breaks = matches!(c, '"' | '\\' | '|' | '[' | ']' | '(' | ')')
            || (c.is_whitespace() && c != ' ')
            || (c == ' ' && prev == ' ');
        if breaks {
            if t[start..i].trim().len() > best.len() {
                best = t[start..i].trim();
            }
            start = i + c.len_utf8();
        }
        prev = c;
    }
    if t[start..].trim().len() > best.len() {
        best = t[start..].trim();
    }
    let s: String = best.chars().take(MAX_CHARS).collect();
    (s.chars().count() >= MIN_CHARS).then_some(s)
}

fn text_of(dom: &Dom, n: NodeId) -> String {
    let mut v = Vec::new();
    if let Some(t) = dom.text(n) {
        t.write_wtf8(&mut v);
    }
    String::from_utf8_lossy(&v).into_owned()
}

/// About `n` items spread evenly over `all`.
fn spread<T>(all: &[T], n: usize) -> impl Iterator<Item = &T> {
    let step = all.len().div_ceil(n.max(1)).max(1);
    all.iter().step_by(step)
}

pub fn audit(dom: &Dom, styles: &Styles, read: &Read, text: &str) -> Report {
    let hidden = hidden_set(dom, styles, read.title_node);
    let is_hidden = |i: u32| hidden.get(i as usize).copied().unwrap_or(false);
    let mut r = Report::default();
    for n in dom.descendants(dom.document()) {
        if is_hidden(n.index()) {
            match dom.kind(n) {
                NodeKind::Element => r.hidden_elements += 1,
                _ => r.hidden_texts += 1,
            }
        }
    }
    r.used = read.used.len();
    r.used_hidden = read
        .used
        .iter()
        .map(|n| n.index())
        .filter(|&i| is_hidden(i))
        .collect();
    let refs = references(text);
    r.refs = refs.len();
    r.refs_hidden = refs.into_iter().filter(|&i| is_hidden(i)).collect();
    by_text(dom, &hidden, text, &mut r);
    r
}

fn by_text(dom: &Dom, hidden: &[bool], text: &str, r: &mut Report) {
    let shown = |n: NodeId| !hidden.get(n.index() as usize).copied().unwrap_or(true);
    let mut seen = String::new();
    let (mut hid, mut vis) = (Vec::new(), Vec::new());
    for n in dom.descendants(dom.document()) {
        let i = n.index() as usize;
        match dom.kind(n) {
            NodeKind::Text => {
                let t = text_of(dom, n);
                let p = piece(&t);
                if hidden.get(i).copied().unwrap_or(false) {
                    hid.extend(p);
                } else if shown(n) {
                    vis.extend(p);
                    seen.push_str(&t);
                    seen.push('\u{1}');
                }
            }
            NodeKind::Element if shown(n) => {
                for a in dom.attributes(n) {
                    let mut v = Vec::new();
                    a.value.write_wtf8(&mut v);
                    seen.push_str(&String::from_utf8_lossy(&v));
                    seen.push('\u{1}');
                }
                // a shown svg's <title> is never rendered but names it, as
                // an attribute would
                if let Some(t) = svg_title(dom, n) {
                    for c in dom.children(t) {
                        seen.push_str(&text_of(dom, c));
                    }
                    seen.push('\u{1}');
                }
            }
            _ => {}
        }
    }
    let seen = collapse(&seen);
    r.hidden_pieces = hid.len();
    // the containment test is linear in the shown text, so it runs on a
    // sample of the hidden pieces, not on all of them
    let sampled: Vec<&String> = spread(&hid, 4 * SAMPLES).collect();
    let distinct: Vec<&String> = sampled
        .iter()
        .copied()
        .filter(|p| !seen.contains(collapse(p).as_str()))
        .collect();
    r.hidden_sampled = sampled.len();
    r.not_distinct = sampled.len() - distinct.len();
    for p in spread(&distinct, SAMPLES) {
        r.hidden_tested += 1;
        if text.contains(p.as_str()) {
            r.hidden_found.push((*p).clone());
        }
    }
    for p in spread(&vis, SAMPLES) {
        r.shown_tested += 1;
        if text.contains(p.as_str()) {
            r.shown_found += 1;
        }
    }
}

/// Runs of white space to one space, as the text form writes text.
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c') {
            space = true;
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}
