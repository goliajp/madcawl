//! A block written out as `cssText` gives it.

use super::super::known_names::{ShortInfo, shorts};
use super::entry::{Entry, Key};
use super::{Declarations, key_name};

impl Declarations {
    /// The block serialized, as `cssText` gives it: each declaration in
    /// order, a longhand written as the first shorthand with the most
    /// longhands that all of them, not yet written, with one priority,
    /// can be written as.
    pub fn write_css(&self, out: &mut String) {
        let mut done = vec![false; self.entries.len()];
        let table = shorts();
        let mut value = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            if done[i] {
                continue;
            }
            let written = match &e.key {
                Key::Known(w) => table.of(w).iter().any(|&k| {
                    self.try_short(&table.list[usize::from(k)], e, &mut done, &mut value, out)
                }),
                Key::Other(_) => false,
            };
            if !written {
                declaration(key_name(&e.key), &e.text, e.important, out);
                done[i] = true;
            }
        }
    }

    /// Write shorthand `s` for longhand `e` when every longhand of it is
    /// there, not yet written, with `e`'s priority, and it has a value.
    fn try_short(
        &self,
        s: &ShortInfo,
        e: &Entry,
        done: &mut [bool],
        value: &mut String,
        out: &mut String,
    ) -> bool {
        if !s.in_css_text {
            return false;
        }
        let Some(p) = self.parts(s) else { return false };
        let at = &p.0[..p.1];
        if at.iter().any(|&k| done[usize::from(k)]) || self.one_priority(&p) != Some(e.important) {
            return false;
        }
        value.clear();
        self.write_parts(s, &p, value);
        if value.is_empty() {
            return false;
        }
        declaration(s.name, value, e.important, out);
        at.iter().for_each(|&k| done[usize::from(k)] = true);
        true
    }
}

fn declaration(name: &str, value: &str, important: bool, out: &mut String) {
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(name);
    out.push_str(": ");
    out.push_str(value);
    if important {
        out.push_str(" !important");
    }
    out.push(';');
}
