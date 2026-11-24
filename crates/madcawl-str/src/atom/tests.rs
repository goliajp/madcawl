use super::generated::NAMES;
use super::*;
use crate::abi;

#[test]
fn every_static_name_finds_its_own_atom() {
    for (i, name) in NAMES.iter().enumerate() {
        let a = Atom::lookup_static(name.as_bytes()).expect(name);
        assert_eq!(a.index() as usize, i, "{name}");
        assert!(a.is_static());
        assert_eq!(a.as_static().unwrap().as_ascii_str(), Some(*name));
    }
}

#[test]
fn the_names_are_sorted_and_distinct() {
    assert!(NAMES.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()));
}

#[test]
fn names_that_are_not_static_find_nothing() {
    for name in [
        &b""[..],
        b"DIV",
        b"divx",
        b"di",
        b"my-widget",
        "dív".as_bytes(),
        &[0xFF; 40],
    ] {
        assert_eq!(Atom::lookup_static(name), None, "{name:?}");
    }
}

#[test]
fn the_parser_case_fixups_have_both_spellings() {
    for (lower, mixed) in [
        ("foreignobject", "foreignObject"),
        ("viewbox", "viewBox"),
        ("definitionurl", "definitionURL"),
    ] {
        let a = Atom::lookup_static(lower.as_bytes()).unwrap();
        let b = Atom::lookup_static(mixed.as_bytes()).unwrap();
        assert_ne!(a, b);
    }
}

#[test]
fn the_macro_agrees_with_lookup() {
    assert_eq!(Some(atom!("div")), Atom::lookup_static(b"div"));
    assert_eq!(
        Some(atom!("annotation-xml")),
        Atom::lookup_static(b"annotation-xml")
    );
    assert_eq!(
        atom!("xlink:href").as_static().unwrap().as_ascii_str(),
        Some("xlink:href")
    );
}

#[test]
fn feeding_in_pieces_gives_the_same_atom() {
    let mut h = NameHash::new();
    for piece in [&b"block"[..], b"q", b"uote"] {
        h.feed(piece);
    }
    assert_eq!(h.finish(b"blockquote"), Some(atom!("blockquote")));
    let mut h = NameHash::new();
    h.feed(b"blockquotes");
    assert_eq!(h.finish(b"blockquotes"), None);
}

#[test]
fn a_static_block_is_static_in_the_engine_layout() {
    let s = atom!("table").as_static().unwrap();
    let h = s.block().header();
    assert_eq!(abi::refcount(h), 0);
    assert_eq!(abi::tag(h), abi::TAG_STR);
    assert_eq!(
        abi::flags(h),
        abi::FLAG_STATIC_LITERAL | abi::STR_FLAG_IS_LATIN1 | abi::STR_FLAG_IS_ASCII
    );
    assert_eq!(s.block().0.as_ptr() as usize % 8, 0);
    // SAFETY: the word at offset 12 follows the length
    let pool = unsafe {
        s.block()
            .0
            .as_ptr()
            .add(abi::STR_PAD_OFF)
            .cast::<u32>()
            .read()
    };
    assert_eq!(pool, 0);
    let d: DomString = s.into();
    let d2 = d.clone();
    drop(d);
    assert!(d2.eq_str("table"), "a static block is never counted");
}

#[test]
fn a_static_atom_reads_on_another_thread() {
    fn is_sync<T: Send + Sync>(_: &T) {}
    let s = atom!("textarea").as_static().unwrap();
    is_sync(&s);
    let main = s.block().0.as_ptr() as usize;
    let (text, units, block) = std::thread::spawn(move || {
        let t = atom!("textarea").as_static().unwrap();
        (
            s.as_ascii_str().unwrap(),
            t.units().collect::<Vec<_>>(),
            t.block().0.as_ptr() as usize,
        )
    })
    .join()
    .unwrap();
    assert_eq!(text, "textarea");
    assert_eq!(units, "textarea".encode_utf16().collect::<Vec<_>>());
    assert_eq!(block, main, "one block for the whole process");
}

#[test]
fn a_static_name_is_held_without_the_table() {
    let held = HeldAtoms::new();
    assert_eq!(held.hold(b"div"), atom!("div"));
    assert!(held.is_empty());
}

#[test]
fn holders_share_a_dynamic_atom_until_the_last_one_goes() {
    let name = b"x-shared-by-two";
    let a = HeldAtoms::new();
    let b = HeldAtoms::new();
    let x = a.hold(name);
    assert!(!x.is_static());
    assert_eq!(a.hold(name), x, "the same holder twice");
    assert_eq!(b.hold(name), x);
    assert_eq!(a.len(), 1);
    assert_eq!(Atom::lookup(name), Some(x));
    drop(a);
    assert_eq!(Atom::lookup(name), Some(x), "b still has it");
    let mut v = Vec::new();
    x.write_wtf8(&mut v);
    assert_eq!(v, name);
    assert!(x.to_dom_string().eq_str("x-shared-by-two"));
    drop(b);
    assert_eq!(Atom::lookup(name), None, "freed with its last holder");
    assert_eq!(format!("{x:?}"), format!("Atom({} released)", x.index()));
}

#[test]
fn a_dynamic_name_keeps_text_beyond_latin1() {
    let held = HeldAtoms::new();
    let name = "x-漢字😀".as_bytes();
    let x = held.hold(name);
    let s = x.to_dom_string();
    assert!(!s.is_latin1());
    let mut v = Vec::new();
    x.write_wtf8(&mut v);
    assert_eq!(v, name);
}

#[test]
fn threads_interning_the_same_names_agree() {
    let names: Vec<Vec<u8>> = (0..64)
        .map(|i| format!("x-threaded-{i}").into_bytes())
        .collect();
    let runs: Vec<Vec<u32>> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                s.spawn(|| {
                    let held = HeldAtoms::new();
                    let atoms = names.iter().map(|n| held.hold(n).index()).collect();
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    for (n, &a) in names.iter().zip(&atoms) {
                        assert_eq!(held.hold(n).index(), a);
                    }
                    atoms
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    assert!(
        runs.windows(2).all(|w| w[0] == w[1]),
        "one table for every thread"
    );
    for n in &names {
        assert_eq!(Atom::lookup(n), None);
    }
}

#[test]
fn keeping_an_atom_outlives_the_holder_it_came_from() {
    let parser = HeldAtoms::new();
    let x = parser.hold(b"x-kept-by-a-document");
    let document = HeldAtoms::new();
    document.keep(x);
    document.keep(atom!("div"));
    assert_eq!(document.len(), 1);
    drop(parser);
    assert_eq!(Atom::lookup(b"x-kept-by-a-document"), Some(x));
    drop(document);
    assert_eq!(Atom::lookup(b"x-kept-by-a-document"), None);
}

#[test]
fn a_name_as_long_as_an_empty_recent_slot_says_is_held() {
    let held = HeldAtoms::new();
    held.hold(b"x-first");
    for len in [254, 255, 256] {
        let name = vec![b'y'; len];
        let x = held.hold(&name);
        assert_eq!(held.hold(&name), x, "{len}");
    }
}
