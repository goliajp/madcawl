//! Fitting the Markdown view into a budget: tables keep their header and
//! first rows, the rest folded into `… N more rows [ref]`; then the
//! blocks past the budget are left out, at a block boundary.

use super::markdown::Block;
use crate::text::estimate_tokens;

const KEEP_ROWS: usize = 3;

fn join(blocks: &[Block]) -> String {
    let mut out = String::new();
    let mut last_item = false;
    for b in blocks.iter().filter(|b| !b.text.is_empty()) {
        if !out.is_empty() {
            out.push_str(if last_item && b.item { "\n" } else { "\n\n" });
        }
        out.push_str(&b.text);
        last_item = b.item;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

pub(crate) fn fit(mut blocks: Vec<Block>, budget: Option<usize>) -> (String, usize) {
    let Some(budget) = budget else {
        return (join(&blocks), 0);
    };
    let cost = |bs: &[Block]| {
        bs.iter()
            .map(|b| estimate_tokens(&b.text) + 1)
            .sum::<usize>()
    };
    let mut folded = 0;
    if cost(&blocks) > budget {
        for b in &mut blocks {
            let Some(t) = &b.table else { continue };
            if t.body.len() <= KEEP_ROWS {
                continue;
            }
            let more = t.body.len() - KEEP_ROWS;
            let mut text = t.head.clone();
            for row in &t.body[..KEEP_ROWS] {
                text.push('\n');
                text.push_str(row);
            }
            text.push_str(&format!("\n… {more} more rows [{}]", t.reference));
            b.text = text;
            folded += more;
        }
    }
    if cost(&blocks) > budget {
        let mut used = 0;
        let mut cut = blocks.len();
        for (i, b) in blocks.iter().enumerate() {
            used += estimate_tokens(&b.text) + 1;
            if used + 12 > budget {
                cut = i;
                break;
            }
        }
        let rest = blocks.len() - cut;
        blocks.truncate(cut);
        folded += rest;
        blocks.push(Block {
            text: format!("… {rest} more blocks past the budget; read a smaller scope"),
            table: None,
            item: false,
        });
    }
    (join(&blocks), folded)
}
