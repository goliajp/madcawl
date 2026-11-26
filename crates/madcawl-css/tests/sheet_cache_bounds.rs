//! a `SheetCache` stays within its weight, evicts the least recently
//! used sheets first, and eviction drops only the cache's own reference

use std::sync::Arc;

use madcawl_css::style::{Scope, SheetCache};

const HERE: Scope = Scope {
    partition: 1,
    site: "https://example.com",
};

fn text(i: usize, len: usize) -> String {
    let mut t = format!(".c{i:05} {{ margin: {i}px }}");
    while t.len() < len {
        t.push_str(" /* pad */");
    }
    t.truncate(len);
    t
}

/// the weight the cache gives a sheet of `t`
fn weight_of(t: &str) -> usize {
    let c = SheetCache::new(1 << 30);
    c.get(HERE, t.to_owned()).unwrap();
    c.stats().weight
}

#[test]
fn a_held_sheet_outlives_its_eviction() {
    let w = weight_of(&text(0, 400));
    let c = SheetCache::new(8 * w);
    let held = c.get(HERE, text(0, 400)).unwrap();
    for i in 1..20 {
        c.get(HERE, text(i, 400)).unwrap();
    }
    // gone from the cache, still whole for whoever has it
    assert_eq!(Arc::strong_count(&held), 1);
    assert_eq!(held.source(), text(0, 400));
    let again = c.get(HERE, text(0, 400)).unwrap();
    assert!(!Arc::ptr_eq(&held, &again));
    assert_eq!(c.stats().hits, 0);
}

#[test]
fn eviction_takes_the_least_recently_used_until_a_sheet_fits() {
    let small = weight_of(&text(0, 400));
    let big_text = text(99, 1000);
    let big = weight_of(&big_text);
    let c = SheetCache::new(10 * small);
    for i in 0..10 {
        c.get(HERE, text(i, 400)).unwrap();
    }
    assert_eq!(c.stats().entries, 10);
    // the first is used again, so the second is now the oldest
    c.get(HERE, text(0, 400)).unwrap();
    c.get(HERE, big_text.clone()).unwrap();
    let gone = (1..=10)
        .find(|k| 10 * small - k * small + big <= 10 * small)
        .unwrap();
    let s = c.stats();
    assert_eq!(s.entries, 10 - gone + 1, "{s:?}");
    assert_eq!(s.weight, (10 - gone) * small + big);
    let hits = s.hits;
    for i in std::iter::once(0).chain(gone + 1..10) {
        c.get(HERE, text(i, 400)).unwrap();
    }
    c.get(HERE, big_text).unwrap();
    assert_eq!(c.stats().hits - hits, (10 - gone) as u64 + 1);
    let parses = c.stats().parses;
    c.get(HERE, text(1, 400)).unwrap();
    assert_eq!(c.stats().parses, parses + 1, "the oldest was evicted");
}

#[test]
fn many_sessions_sites_and_names_stay_bounded() {
    let cap = 64 * weight_of(&text(0, 600));
    let c = SheetCache::new(cap);
    let sites = ["https://a.example", "https://b.example", "file:"];
    for step in 0..4000usize {
        let scope = Scope {
            partition: (step % 37) as u64,
            site: sites[step % sites.len()],
        };
        let t = text(step % 500, 200 + step % 900);
        // a file touched without a change: a new name for the same text
        let name = format!("file-{}-{step}", step % 500);
        c.get_as(scope, t.as_str().into(), Some(name.as_bytes()))
            .unwrap();
        let s = c.stats();
        assert!(s.weight <= cap, "{step}: {s:?}");
        assert!(s.names <= 4 * s.entries, "{step}: {s:?}");
    }
    for p in 0..37 {
        c.forget(p);
    }
    let s = c.stats();
    assert_eq!((s.entries, s.weight, s.names), (0, 0, 0));
}
