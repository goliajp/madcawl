//! How far the lazy parsing of declaration blocks has gone.

use super::{Body, Rule, Stylesheet};

impl Stylesheet {
    /// How many declaration blocks the sheet has, and how many of them
    /// have been parsed.
    pub fn bodies(&self) -> (usize, usize) {
        let mut n = (0, 0);
        count(&self.rules, &mut n);
        n
    }
}

fn count(rules: &[Rule], n: &mut (usize, usize)) {
    fn body(b: &Body, n: &mut (usize, usize)) {
        n.0 += 1;
        n.1 += b.is_parsed() as usize;
    }
    for r in rules {
        match r {
            Rule::Style(s) => {
                body(&s.body, n);
                count(&s.rules, n);
            }
            Rule::FontFace(b) | Rule::Page { body: b, .. } => body(b, n),
            Rule::Media { rules, .. } | Rule::Supports { rules, .. } => count(rules, n),
            Rule::Layer { rules: Some(r), .. } => count(r, n),
            _ => {}
        }
    }
}
