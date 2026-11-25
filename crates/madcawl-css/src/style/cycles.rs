//! The custom properties of one element that are in a dependency cycle.
//!
//! Each declared custom property depends on every custom property a
//! `var()` in its value names, fallbacks included. Every property in a
//! strongly connected part of that graph with a cycle in it is
//! guaranteed-invalid, whatever the order the properties are resolved
//! in (Tarjan's algorithm, without recursion: a chain may be as long as
//! the element has properties).

use crate::tokenizer::{Kind, Tokenizer};

use super::props::Decl;

/// The names the `var()` functions in `src` refer to, nested ones too.
fn references(src: &str, mut each: impl FnMut(&str)) {
    let mut t = Tokenizer::new(src);
    let mut after_var = false;
    let mut name = String::new();
    loop {
        let tok = t.next_token();
        match tok.kind {
            Kind::Eof => return,
            Kind::Function => after_var = tok.value_is(src, "var"),
            Kind::Whitespace => {}
            Kind::Ident if after_var => {
                name.clear();
                tok.write_value(src, &mut name);
                each(&name);
                after_var = false;
            }
            _ => after_var = false,
        }
    }
}

/// By property of `declared` (sorted by name, one of each), whether it
/// is in a cycle.
pub(crate) fn cyclic(declared: &[(&str, &Decl)]) -> Vec<bool> {
    let mut edges: Vec<Vec<u32>> = vec![Vec::new(); declared.len()];
    for (i, (_, d)) in declared.iter().enumerate() {
        if let (true, Some(raw)) = (d.substitutes, &d.raw) {
            references(raw, |r| {
                if let Ok(j) = declared.binary_search_by(|(m, _)| (*m).cmp(r)) {
                    edges[i].push(j as u32);
                }
            });
        }
    }
    if edges.iter().all(Vec::is_empty) {
        return vec![false; declared.len()];
    }
    in_cycles(&edges)
}

fn in_cycles(edges: &[Vec<u32>]) -> Vec<bool> {
    let n = edges.len();
    let mut out = vec![false; n];
    const NONE: u32 = u32::MAX;
    // by property: the order it was reached in, and the lowest such
    // number it reaches while its part is open
    let (mut order, mut low) = (vec![NONE; n], vec![0u32; n]);
    let (mut open, mut on) = (Vec::new(), vec![false; n]);
    let mut next = 0u32;
    for root in 0..n {
        if order[root] != NONE {
            continue;
        }
        // the path being walked: a property and the next of its edges
        let mut path: Vec<(usize, usize)> = vec![(root, 0)];
        (order[root], low[root], next) = (next, next, next + 1);
        open.push(root);
        on[root] = true;
        while let Some(&mut (v, ref mut e)) = path.last_mut() {
            if let Some(&w) = edges[v].get(*e) {
                *e += 1;
                let w = w as usize;
                if order[w] == NONE {
                    (order[w], low[w], next) = (next, next, next + 1);
                    open.push(w);
                    on[w] = true;
                    path.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(order[w]);
                }
                continue;
            }
            path.pop();
            if let Some(&(u, _)) = path.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == order[v] {
                let at = open.iter().rposition(|&x| x == v).expect("open");
                let part = open.split_off(at);
                let cycle = part.len() > 1 || edges[v].contains(&(v as u32));
                for x in part {
                    on[x] = false;
                    out[x] = cycle;
                }
            }
        }
    }
    out
}
