//! A diff as written: what entered the view (`+`), what left it (`-`),
//! and what changed its name, state or value (`~`), one line a node, each
//! with its reference. A node's entries in the ring collapse to one: one
//! added and removed in between is not written.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId};

use crate::record::Record;
use crate::refs::Ref;
use crate::ring::Entry;
use crate::sync::Delta;
use crate::tree::{Inputs, SemTree};
use crate::view::{Reader, ViewOpts, node_line};

pub(crate) fn write<'e>(
    t: &SemTree,
    dom: &Dom,
    inputs: Inputs,
    since: u64,
    seq: u64,
    entries: impl Iterator<Item = &'e Entry>,
) -> String {
    // by (slot, generation): (first delta, last delta), in first-seen order
    let mut order: Vec<(u32, u32)> = Vec::new();
    let mut seen: HashMap<(u32, u32), (Delta, Delta)> = HashMap::new();
    for e in entries {
        let k = (e.slot, e.generation);
        match seen.get_mut(&k) {
            Some(v) => v.1 = e.delta,
            None => {
                seen.insert(k, (e.delta, e.delta));
                order.push(k);
            }
        }
    }
    let r = Reader::new(t, dom, inputs, ViewOpts::default());
    let mut lines = Vec::new();
    for k in order {
        let (first, last) = seen[&k];
        let gone = |out: &mut Vec<String>| {
            let rf = Ref {
                index: k.0,
                slot_gen: k.1,
                page_gen: t.page_gen(),
            };
            out.push(format!("- [{}]", rf.text()));
        };
        let current = NodeId::from_index(k.0)
            .and_then(|n| t.record(n).copied().map(|rec| (n, rec)))
            .filter(|(_, rec)| rec.generation == k.1);
        match (first, last, current) {
            (Delta::Added, Delta::Removed, _) => {}
            (_, Delta::Removed, _) | (_, _, None) => gone(&mut lines),
            (first, _, Some((n, rec))) => {
                let sign = if first == Delta::Added { '+' } else { '~' };
                if rec.shown() {
                    lines.push(format!("{sign} {}", line(&r, n, &rec)));
                } else if first != Delta::Added {
                    gone(&mut lines);
                }
            }
        }
    }
    let mut out = format!("diff {since}..{seq}: {} changes\n", lines.len());
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out
}

fn line(r: &Reader, n: NodeId, rec: &Record) -> String {
    if rec.role == crate::role::Role::Text {
        let mut s = String::from("text ");
        crate::refs::push_quoted(&mut s, r.name(rec));
        s.push(' ');
        r.push_ref(&mut s, n, rec);
        return s;
    }
    node_line(r, n, rec)
}
