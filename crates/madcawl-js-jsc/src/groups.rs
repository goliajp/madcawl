//! Trees of wrappers in the framework's heap.
//!
//! The framework's collector sees only its own heap, so the wrappers of
//! one detached tree are made reachable from one another there: each
//! belongs to a group (a `Set` holding every wrapper of the tree), and
//! the realm's `WeakMap` from wrapper to group gives each one an edge
//! back. Script holding any wrapper of the tree keeps all of them, with
//! their own properties; once it holds none, the collector frees them
//! together, their finalizers tell the host, and the host's trial
//! deletion frees the nodes. A wrapper of a node in the document is
//! protected instead, as long as its realm lives (its group would hang
//! off the global, which lives as long): reading it back is then a
//! pointer read, not a call into script.

use madcawl_js::Tree;

use crate::isolate::Jsc;
use crate::objects::WrapRec;
use crate::protos::Helper;
use crate::value::{FALSE, TRUE, UNDEFINED, Value};

fn record<'a>(word: u64) -> &'a mut WrapRec {
    // SAFETY: a word the host holds names a record not yet freed
    unsafe { &mut *(word as *mut WrapRec) }
}

impl Jsc {
    /// `false` when script was being stopped and nothing was placed.
    pub(crate) fn place(&mut self, tree: Tree<Value>, words: &[u64], refused: &mut Vec<usize>) -> bool {
        let Some(&first) = words.first() else { return true };
        let realm = record(first).realm;
        let settled = |&w: &u64| record(w).strong && !record(w).grouped;
        if self.realms[realm as usize].detached || (tree == Tree::Anchored && words.iter().all(settled)) {
            return true;
        }
        self.words_placed += words.len() as u64;
        let Some(live) = self.live_records(realm, words, refused) else { return false };
        let placed = match tree {
            Tree::Anchored => self.anchor(realm, words, &live),
            Tree::New => self.group(realm, UNDEFINED, words, &live),
            Tree::With(r) => self.group(realm, r, words, &live),
        };
        self.unprotect(live.array);
        placed
    }

    /// Which of `words` are alive, with each one's wrapper in an array
    /// that keeps them while they are placed; the dead go to `refused`.
    /// `None` when script was being stopped as their `WeakRef`s were read:
    /// nothing is placed then, and nothing refused.
    fn live_records(&mut self, realm: u32, words: &[u64], refused: &mut Vec<usize>) -> Option<Live> {
        let weak: Vec<Value> = words.iter().map(|&w| record(w)).filter(|r| !r.strong && !r.dead).map(|r| r.weak).collect();
        // the array the reads come back in holds them until the array of
        // live wrappers does
        let (read, held) = if weak.is_empty() {
            (Vec::new(), UNDEFINED)
        } else {
            let arr = self.array(self.realms[realm as usize].ctx, &weak);
            let r = self.call_helper(realm, Helper::DerefAll, &[arr]);
            self.unprotect(arr);
            match r {
                Ok(a) => {
                    self.protect(a);
                    (self.elements(realm, a, weak.len()), a)
                }
                Err(_) => {
                    self.after_stop();
                    return None;
                }
            }
        };
        let mut read = read.into_iter();
        let mut alive = vec![false; words.len()];
        let mut values = Vec::with_capacity(words.len());
        for (i, &w) in words.iter().enumerate() {
            let rec = record(w);
            let v = if rec.dead {
                UNDEFINED
            } else if rec.strong {
                rec.js
            } else {
                read.next().expect("one read per weak record")
            };
            if v == UNDEFINED {
                rec.orphan = true;
                refused.push(i);
            } else {
                alive[i] = true;
                values.push(v);
            }
        }
        let array = self.array(self.realms[realm as usize].ctx, &values);
        self.unprotect(held);
        Some(Live { alive, array })
    }

    fn anchor(&mut self, realm: u32, words: &[u64], live: &Live) -> bool {
        let mut grouped = Vec::new();
        for (i, &w) in words.iter().enumerate() {
            let rec = record(w);
            if !live.alive[i] {
                continue;
            }
            if !rec.strong {
                rec.strong = true;
                self.protect(rec.js);
            }
            // a strong wrapper is read without it
            self.unprotect(std::mem::replace(&mut rec.weak, Value(0)));
            if rec.grouped {
                grouped.push(w);
            }
        }
        if grouped.is_empty() {
            return true;
        }
        let js: Vec<Value> = grouped.iter().map(|&w| record(w).js).collect();
        let arr = self.array(self.realms[realm as usize].ctx, &js);
        let ok = self.call_helper(realm, Helper::Ungroup, &[arr]).is_ok();
        self.unprotect(arr);
        if !ok {
            // still in their old groups, which they would keep alive: the
            // host places them again
            self.after_stop();
            return false;
        }
        for w in grouped {
            record(w).grouped = false;
        }
        true
    }

    fn group(&mut self, realm: u32, root: Value, words: &[u64], live: &Live) -> bool {
        let ctx = self.realms[realm as usize].ctx;
        let need: Vec<Value> = live_records(words, live).map(|r| if r.weak == Value(0) { TRUE } else { FALSE }).collect();
        let need_arr = self.array(ctx, &need);
        let r = self.call_helper(realm, Helper::Group, &[root, live.array, need_arr]);
        self.unprotect(need_arr);
        let made = match r {
            Ok(a) => self.elements(realm, a, need.iter().filter(|&&n| n == TRUE).count()),
            Err(_) => {
                // they stay as they were, strong ones protected, until the
                // host places them again
                self.after_stop();
                return false;
            }
        };
        let mut made = made.into_iter();
        for rec in live_records(words, live) {
            if rec.weak == Value(0) {
                rec.weak = made.next().expect("a WeakRef for each that had none");
                self.protect(rec.weak);
            }
            rec.grouped = true;
            if std::mem::take(&mut rec.strong) {
                self.unprotect(rec.js);
            }
        }
        true
    }

    /// The first `n` elements of array `a`, uncounted: `a` holds them
    /// until the caller is done, as the value a helper returned.
    fn elements(&mut self, realm: u32, a: Value, n: usize) -> Vec<Value> {
        let ctx = self.realms[realm as usize].ctx;
        (0..n as u32)
            .map(|i| {
                // SAFETY: the array a helper returned, still on this stack
                Value::from_ref(unsafe { crate::ffi::JSObjectGetPropertyAtIndex(ctx, a.0 as _, i, std::ptr::null_mut()) })
            })
            .collect()
    }
}

/// The live records among `words`, in order.
fn live_records<'a>(words: &'a [u64], live: &'a Live) -> impl Iterator<Item = &'a mut WrapRec> + 'a {
    words.iter().zip(&live.alive).filter(|&(_, &a)| a).map(|(&w, _)| record(w))
}

struct Live {
    alive: Vec<bool>,
    /// The live wrappers, counted.
    array: Value,
}
