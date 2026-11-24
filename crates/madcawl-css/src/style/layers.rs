//! The names an `@layer` rule's prelude gives, and the order of the
//! layers they make.

use crate::Span;
use crate::tokenizer::{Kind, Tokenizer};

use super::rules::{Origin, Rules};

/// The names of `@layer a, b.c`, each dotted name as one string;
/// `None` when the prelude is not a list of them.
pub(crate) fn layer_names(src: &str, span: Span) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // a name part comes next: first, after a dot or after a comma
    let mut part = true;
    let mut spaced = false;
    let mut t = Tokenizer::in_span(src, span);
    loop {
        let tok = t.next_token();
        match tok.kind {
            Kind::Whitespace => {
                spaced = true;
                continue;
            }
            Kind::Ident if part && (cur.is_empty() || !spaced) => {
                tok.write_value(src, &mut cur);
                part = false;
            }
            Kind::Delim(b'.') if !part && !spaced => {
                cur.push('.');
                part = true;
            }
            Kind::Comma if !part => {
                out.push(std::mem::take(&mut cur));
                part = true;
            }
            Kind::Eof if !part || out.is_empty() && cur.is_empty() => break,
            _ => return None,
        }
        spaced = false;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Some(out)
}

impl Rules<'_> {
    /// The rank of the author origin's unlayered rules.
    pub fn author_rank(&self) -> u32 {
        self.layer_ranks()[self.roots[Origin::Author as usize] as usize]
    }

    /// Each layer's rank among its origin's: a layer's sublayers come
    /// before the rules directly in it, and the unlayered rules last.
    pub fn layer_ranks(&self) -> Vec<u32> {
        let mut rank = vec![0; self.layers.len()];
        for &root in &self.roots {
            let mut next = 0;
            // post-order with an explicit stack: (layer, children done)
            let mut stack = vec![(root, false)];
            while let Some((l, done)) = stack.pop() {
                if done {
                    rank[l as usize] = next;
                    next += 1;
                    continue;
                }
                stack.push((l, true));
                for &c in self.layers[l as usize].children.iter().rev() {
                    stack.push((c, false));
                }
            }
        }
        rank
    }
}
