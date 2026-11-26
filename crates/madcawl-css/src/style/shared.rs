//! Parsed stylesheets shared by the documents of one process. Pages of
//! one site link the same large stylesheets, and parsing them again for
//! every page is most of the cost of styling a page that has few
//! elements.
//!
//! An entry is found by its scope, a partition (an agent session's) and
//! the site of the top-level document, and by the text it was parsed
//! from. Only pages of one scope share a sheet, so how long a page takes
//! to style tells nothing about what another session or another site
//! loaded. A 64-bit hash, seeded per cache, picks the slot; the scope and
//! the whole text are compared before a sheet is reused, the text outside
//! the lock. A slot holds one entry; another with the same hash is parsed
//! and not kept. The sheets are `Arc`s and a parsed sheet is never
//! changed (the declarations parsed on first use are a function of the
//! text), so a page cannot change what another page sees; something that
//! edits a stylesheet works on its own copy.
//!
//! The cache is bounded by weight and evicts the least recently used
//! sheets first. The weight is an estimate made from the text's length
//! before parsing, not a measure of what the sheet holds. A sheet heavier
//! than a quarter of the bound is parsed and not kept, so one sheet
//! cannot empty the cache. Evicting drops only the cache's reference: a
//! page styling with the sheet keeps it until it is done, outside the
//! bound.
//!
//! A linked sheet can also be found by what its loader knows of it
//! before reading it (`known`), so a page that links a held sheet does
//! not read and decode its bytes again.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::sheet::{SourceTooLarge, Stylesheet};

mod known;

/// The estimated memory of a parsed sheet for each byte of its text, the
/// text included, its declarations all parsed (measured 3.5 over a corpus
/// of 62 real sheets, 4.7 at most), and of an entry on its own
const WEIGHT_PER_BYTE: usize = 5;
const WEIGHT_PER_ENTRY: usize = 512;

pub struct SheetCache {
    inner: Mutex<Inner>,
    max_weight: usize,
    seed: u64,
    parses: AtomicU64,
    hits: AtomicU64,
    unread: AtomicU64,
}

/// Which pages may share a sheet: those of one partition whose top-level
/// documents are of one site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope<'a> {
    pub partition: u64,
    pub site: &'a str,
}

struct Entry {
    partition: u64,
    site: Box<str>,
    sheet: Arc<Stylesheet>,
    weight: usize,
    used: u64,
    /// The keys the entry is known by in `Inner::known`, oldest first
    known: Vec<Box<[u8]>>,
}

#[derive(Default)]
struct Inner {
    by_hash: HashMap<u64, Entry>,
    /// The hash of every entry, by when it was last used
    by_use: BTreeMap<u64, u64>,
    known: known::Known,
    weight: usize,
    clock: u64,
}

/// How many sheets a cache parsed and reused, and what it holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetCacheStats {
    pub parses: u64,
    /// Sheets reused after comparing their text
    pub hits: u64,
    /// Sheets reused by what their loader knew of them, unread
    pub unread: u64,
    pub entries: usize,
    pub weight: usize,
    /// loader names held (`get_known`), a few for each entry at most
    pub names: usize,
}

/// A multiply-rotate hash over eight bytes at a time; collisions are
/// told apart by comparing the scope and the text.
fn hash(seed: u64, scope: Scope, text: &str) -> u64 {
    let mut h = seed ^ (text.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut mix = |b: &[u8]| {
        let (words, rest) = b.as_chunks::<8>();
        let mut last = [0u8; 8];
        last[..rest.len()].copy_from_slice(rest);
        for w in words.iter().chain([&last]) {
            h = (h.rotate_left(5) ^ u64::from_le_bytes(*w)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    };
    mix(&scope.partition.to_le_bytes());
    mix(scope.site.as_bytes());
    mix(text.as_bytes());
    h
}

fn weight(text: &str) -> usize {
    text.len().saturating_mul(WEIGHT_PER_BYTE) + WEIGHT_PER_ENTRY
}

impl SheetCache {
    /// A cache that holds at most `max_weight` (bytes, as `weight` estimates them).
    pub fn new(max_weight: usize) -> SheetCache {
        SheetCache {
            inner: Mutex::new(Inner::default()),
            max_weight,
            seed: std::collections::hash_map::RandomState::new().hash_one(0u64),
            parses: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            unread: AtomicU64::new(0),
        }
    }

    /// The sheet parsed from `text` for a page of `scope`: the one held
    /// when the cache has it, else parsed now and kept when it fits.
    pub fn get(&self, scope: Scope, text: String) -> Result<Arc<Stylesheet>, SourceTooLarge> {
        self.get_as(scope, Cow::Owned(text), None)
    }

    /// The same, the text copied only when it is parsed; when `id` is
    /// given, the sheet is known by it from then on while held (`get_known`).
    pub fn get_as(
        &self,
        scope: Scope,
        text: Cow<'_, str>,
        id: Option<&[u8]>,
    ) -> Result<Arc<Stylesheet>, SourceTooLarge> {
        let h = hash(self.seed, scope, &text);
        let held = self.lock().candidate(h, scope);
        if let Some(s) = held.filter(|s| s.source() == text) {
            let mut i = self.lock();
            i.touch(h, &s);
            if let Some(id) = id {
                i.know(known::key(scope, id), h, &s);
            }
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(s);
        }
        let w = weight(&text);
        // parsed outside the lock: another page may parse the same text
        // meanwhile, and the first one in is kept
        let sheet = Arc::new(Stylesheet::parse(text.into_owned())?);
        self.parses.fetch_add(1, Ordering::Relaxed);
        if w <= self.max_weight / 4 {
            let mut i = self.lock();
            i.keep(h, scope, &sheet, w, self.max_weight);
            if let Some(id) = id {
                i.know(known::key(scope, id), h, &sheet);
            }
        }
        Ok(sheet)
    }

    /// The sheet held for pages of `scope` that its loader knows by `id`,
    /// a name the loader vouches stands for one text only (as a file's
    /// identity does while the file is unchanged), without the text.
    pub fn get_known(&self, scope: Scope, id: &[u8]) -> Option<Arc<Stylesheet>> {
        let mut i = self.lock();
        let (h, s) = i.known(&known::key(scope, id))?;
        i.touch(h, &s);
        self.unread.fetch_add(1, Ordering::Relaxed);
        Some(s)
    }

    /// Drops every sheet of `partition`, as when its session ends.
    pub fn forget(&self, partition: u64) {
        let mut i = self.lock();
        let gone: Vec<u64> = i
            .by_hash
            .iter()
            .filter(|(_, e)| e.partition == partition)
            .map(|(&h, _)| h)
            .collect();
        for h in gone {
            i.remove(h);
        }
    }

    pub fn stats(&self) -> SheetCacheStats {
        let i = self.lock();
        SheetCacheStats {
            parses: self.parses.load(Ordering::Relaxed),
            hits: self.hits.load(Ordering::Relaxed),
            unread: self.unread.load(Ordering::Relaxed),
            entries: i.by_hash.len(),
            weight: i.weight,
            names: i.known.len(),
        }
    }

    /// the weight the cache holds at most
    pub fn max_weight(&self) -> usize {
        self.max_weight
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // the map is consistent between statements, so a panic elsewhere
        // while it was held leaves nothing half done
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Inner {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// The sheet in slot `h` when it is of `scope`; its text is still to compare.
    fn candidate(&self, h: u64, scope: Scope) -> Option<Arc<Stylesheet>> {
        let e = self.by_hash.get(&h)?;
        (e.partition == scope.partition && *e.site == *scope.site).then(|| e.sheet.clone())
    }

    /// Marks the entry of `sheet` in slot `h` used now, when it is still there.
    fn touch(&mut self, h: u64, sheet: &Arc<Stylesheet>) {
        let now = self.tick();
        let Some(e) = self
            .by_hash
            .get_mut(&h)
            .filter(|e| Arc::ptr_eq(&e.sheet, sheet))
        else {
            return;
        };
        self.by_use.remove(&e.used);
        e.used = now;
        self.by_use.insert(now, h);
    }

    fn remove(&mut self, h: u64) {
        let e = self.by_hash.remove(&h).expect("every use has its entry");
        self.by_use.remove(&e.used);
        self.known.forget(&e.known);
        self.weight -= e.weight;
    }

    fn keep(&mut self, h: u64, scope: Scope, sheet: &Arc<Stylesheet>, weight: usize, max: usize) {
        if self.by_hash.contains_key(&h) {
            return;
        }
        while self.weight + weight > max {
            let Some((_, &old)) = self.by_use.first_key_value() else {
                break;
            };
            self.remove(old);
        }
        let used = self.tick();
        self.by_use.insert(used, h);
        let site = scope.site.into();
        let partition = scope.partition;
        let sheet = sheet.clone();
        self.by_hash.insert(
            h,
            Entry {
                partition,
                site,
                sheet,
                weight,
                used,
                known: Vec::new(),
            },
        );
        self.weight += weight;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: Scope = Scope {
        partition: 1,
        site: "https://example.com",
    };

    #[test]
    fn the_same_text_parses_once() {
        let c = SheetCache::new(1 << 20);
        let a = c.get(HERE, "p { color: red }".into()).unwrap();
        let b = c.get(HERE, "p { color: red }".into()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let d = c.get(HERE, "p { color: blue }".into()).unwrap();
        assert!(!Arc::ptr_eq(&a, &d));
        let s = c.stats();
        assert_eq!((s.parses, s.hits, s.entries), (2, 1, 2));
    }

    #[test]
    fn another_session_or_site_does_not_share() {
        let c = SheetCache::new(1 << 20);
        let text = "p { color: red }";
        let a = c.get(HERE, text.into()).unwrap();
        let other_session = Scope {
            partition: 2,
            ..HERE
        };
        let other_site = Scope {
            site: "https://example.org",
            ..HERE
        };
        for scope in [other_session, other_site] {
            let b = c.get(scope, text.into()).unwrap();
            assert!(!Arc::ptr_eq(&a, &b));
        }
        assert_eq!(c.stats().hits, 0);
        assert!(Arc::ptr_eq(&a, &c.get(HERE, text.into()).unwrap()));
    }

    #[test]
    fn forget_drops_a_partition() {
        let c = SheetCache::new(1 << 20);
        let other = Scope {
            partition: 2,
            ..HERE
        };
        c.get(HERE, "a {}".into()).unwrap();
        c.get(other, "a {}".into()).unwrap();
        c.forget(1);
        let s = c.stats();
        assert_eq!((s.entries, s.weight), (1, weight("a {}")));
        c.get(HERE, "a {}".into()).unwrap();
        c.get(other, "a {}".into()).unwrap();
        assert_eq!((c.stats().parses, c.stats().hits), (3, 1));
    }

    #[test]
    fn a_colliding_text_is_parsed_and_not_shared() {
        let c = SheetCache::new(1 << 20);
        let a = c.get(HERE, "a {}".into()).unwrap();
        // the first text's entry moved to the slot of the second, as if their hashes were equal
        {
            let mut i = c.lock();
            let (ha, hb) = (hash(c.seed, HERE, "a {}"), hash(c.seed, HERE, "b {}"));
            let e = i.by_hash.remove(&ha).unwrap();
            i.by_use.insert(e.used, hb);
            i.by_hash.insert(hb, e);
        }
        let b = c.get(HERE, "b {}".into()).unwrap();
        assert_eq!(b.source(), "b {}");
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(c.stats().entries, 1);
    }

    #[test]
    fn weight_stays_under_the_bound() {
        let text = |i: usize| format!(".c{i} {{ margin: {i}px }}");
        let max = 16 * weight(&text(100));
        let c = SheetCache::new(max);
        for i in 0..1000 {
            c.get(HERE, text(i)).unwrap();
            assert!(c.stats().weight <= max);
        }
        let s = c.stats();
        assert!(s.entries >= 15 && s.entries <= 16, "{s:?}");
        // the most recent are kept, the oldest gone
        c.get(HERE, text(999)).unwrap();
        c.get(HERE, text(0)).unwrap();
        assert_eq!(c.stats().hits, 1);
    }

    #[test]
    fn use_keeps_an_entry() {
        let text = |i: usize| format!(".c{i} {{ margin: {i}px }}");
        let c = SheetCache::new(16 * weight(&text(10)));
        c.get(HERE, text(0)).unwrap();
        for i in 1..40 {
            c.get(HERE, text(0)).unwrap();
            c.get(HERE, text(i)).unwrap();
        }
        // the first sheet, used between every two others, is never evicted
        assert_eq!(c.stats().hits, 39);
        assert_eq!(c.stats().parses, 40);
    }

    #[test]
    fn a_heavy_sheet_is_not_kept() {
        let c = SheetCache::new(4 * weight("a {}") - 1);
        c.get(HERE, "a {}".into()).unwrap();
        assert_eq!(c.stats().entries, 0);
    }

    #[test]
    fn the_hash_sees_every_byte_and_the_seed() {
        let base = "x".repeat(37);
        let h = |t: &str| hash(7, HERE, t);
        for i in 0..base.len() {
            let mut t = base.clone().into_bytes();
            t[i] = b'y';
            assert_ne!(h(&base), h(std::str::from_utf8(&t).unwrap()));
        }
        assert_ne!(h("ab"), h("ab\0"));
        assert_ne!(h("ab"), hash(8, HERE, "ab"));
        assert_ne!(SheetCache::new(1).seed, SheetCache::new(1).seed);
    }
}
