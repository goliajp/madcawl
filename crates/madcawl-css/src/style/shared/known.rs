//! Held sheets by what their loader knows of them before reading: a
//! loader's name for a linked stylesheet (a file's identity, say) found
//! with its scope. A name leads to one held entry, and leaves with it.

use std::collections::HashMap;
use std::sync::Arc;

use crate::sheet::Stylesheet;

use super::{Inner, Scope};

/// The names one entry keeps at most: a file that keeps being touched
/// without a change gets a new name each time, and the oldest go
const PER_ENTRY: usize = 4;

#[derive(Default)]
pub(super) struct Known {
    /// The slot of the entry a key leads to
    slots: HashMap<Box<[u8]>, u64>,
}

/// The key of `id` for pages of `scope`.
pub(super) fn key(scope: Scope, id: &[u8]) -> Box<[u8]> {
    let mut k = Vec::with_capacity(12 + scope.site.len() + id.len());
    k.extend_from_slice(&scope.partition.to_le_bytes());
    k.extend_from_slice(&(scope.site.len() as u32).to_le_bytes());
    k.extend_from_slice(scope.site.as_bytes());
    k.extend_from_slice(id);
    k.into()
}

impl Known {
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Drops the keys of an entry that is gone.
    pub(super) fn forget(&mut self, keys: &[Box<[u8]>]) {
        for k in keys {
            self.slots.remove(k);
        }
    }
}

impl Inner {
    /// The slot and sheet `key` leads to.
    pub(super) fn known(&self, key: &[u8]) -> Option<(u64, Arc<Stylesheet>)> {
        let h = *self.known.slots.get(key)?;
        let e = self.by_hash.get(&h).expect("a known key leads to an entry");
        Some((h, e.sheet.clone()))
    }

    /// Makes `key` lead to the entry in slot `h`, when that entry holds `sheet`.
    pub(super) fn know(&mut self, key: Box<[u8]>, h: u64, sheet: &Arc<Stylesheet>) {
        let Some(e) = self
            .by_hash
            .get(&h)
            .filter(|e| Arc::ptr_eq(&e.sheet, sheet))
        else {
            return;
        };
        if e.known.contains(&key) {
            return;
        }
        // the key led to another text before: the loader's name now stands for this one
        if let Some(old) = self.known.slots.remove(&key) {
            let o = self
                .by_hash
                .get_mut(&old)
                .expect("a known key leads to an entry");
            o.known.retain(|k| *k != key);
        }
        let e = self.by_hash.get_mut(&h).expect("checked above");
        if e.known.len() == PER_ENTRY {
            let oldest = e.known.remove(0);
            self.known.slots.remove(&oldest);
        }
        e.known.push(key.clone());
        self.known.slots.insert(key, h);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SheetCache, weight};
    use super::*;

    const HERE: Scope = Scope {
        partition: 1,
        site: "https://example.com",
    };

    #[test]
    fn a_known_name_finds_the_sheet_unread() {
        let c = SheetCache::new(1 << 20);
        assert!(c.get_known(HERE, b"f1").is_none());
        let a = c
            .get_as(HERE, "p { color: red }".into(), Some(b"f1"))
            .unwrap();
        let b = c.get_known(HERE, b"f1").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(c.get_known(HERE, b"f2").is_none());
        // a name learnt on a hit by text too
        let d = c
            .get_as(HERE, "p { color: red }".into(), Some(b"f2"))
            .unwrap();
        assert!(Arc::ptr_eq(&a, &d));
        assert!(Arc::ptr_eq(&a, &c.get_known(HERE, b"f2").unwrap()));
        let s = c.stats();
        assert_eq!((s.parses, s.hits, s.unread), (1, 1, 2));
    }

    #[test]
    fn a_name_is_of_its_scope() {
        let c = SheetCache::new(1 << 20);
        c.get_as(HERE, "a {}".into(), Some(b"f")).unwrap();
        for scope in [
            Scope {
                partition: 2,
                ..HERE
            },
            Scope {
                site: "https://example.org",
                ..HERE
            },
        ] {
            assert!(c.get_known(scope, b"f").is_none());
        }
        // a site that ends where another's name begins is another key
        let short = Scope {
            site: "https://example.co",
            ..HERE
        };
        assert!(c.get_known(short, b"mf").is_none());
    }

    #[test]
    fn a_name_leaves_with_its_entry() {
        let text = |i: usize| format!(".c{i} {{ margin: {i}px }}");
        let c = SheetCache::new(4 * weight(&text(10)));
        c.get_as(HERE, text(0).into(), Some(b"first")).unwrap();
        for i in 1..10 {
            c.get(HERE, text(i)).unwrap();
        }
        assert!(c.get_known(HERE, b"first").is_none(), "evicted");
        assert!(c.lock().known.slots.is_empty());
        c.get_as(HERE, text(20).into(), Some(b"x")).unwrap();
        c.forget(1);
        assert!(c.get_known(HERE, b"x").is_none());
        assert!(c.lock().known.slots.is_empty());
    }

    #[test]
    fn a_sheet_not_kept_is_not_known() {
        let c = SheetCache::new(4 * weight("a {}") - 1);
        c.get_as(HERE, "a {}".into(), Some(b"f")).unwrap();
        assert!(c.get_known(HERE, b"f").is_none());
    }

    #[test]
    fn a_name_given_to_another_text_moves() {
        let c = SheetCache::new(1 << 20);
        c.get_as(HERE, "a {}".into(), Some(b"f")).unwrap();
        let b = c.get_as(HERE, "b {}".into(), Some(b"f")).unwrap();
        assert!(Arc::ptr_eq(&b, &c.get_known(HERE, b"f").unwrap()));
        let i = c.lock();
        let held: usize = i.by_hash.values().map(|e| e.known.len()).sum();
        assert_eq!((held, i.known.slots.len()), (1, 1));
    }

    #[test]
    fn an_entry_keeps_its_newest_names() {
        let c = SheetCache::new(1 << 20);
        for n in 0..10u8 {
            c.get_as(HERE, "a {}".into(), Some(&[n])).unwrap();
        }
        for n in 0..10u8 {
            assert_eq!(
                c.get_known(HERE, &[n]).is_some(),
                n >= 10 - PER_ENTRY as u8,
                "{n}"
            );
        }
        assert_eq!(c.lock().known.slots.len(), PER_ENTRY);
    }
}
