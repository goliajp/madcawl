use super::*;

#[test]
fn ascii_is_stored_a_byte_a_unit_and_reads_back_as_str() {
    let s = DomString::from_wtf8(b"hello");
    assert!(s.is_latin1() && s.is_ascii());
    assert_eq!(s.len(), 5);
    assert_eq!(s.as_ascii_str(), Some("hello"));
    assert!(s.eq_str("hello"));
}

#[test]
fn latin1_text_stays_one_byte_a_unit() {
    let s = DomString::from_wtf8("café".as_bytes());
    assert!(s.is_latin1() && !s.is_ascii());
    assert_eq!(s.len(), 4);
    assert_eq!(s.as_latin1(), Some(&[b'c', b'a', b'f', 0xE9][..]));
    assert!(s.eq_str("café"));
}

#[test]
fn wider_text_is_utf16_and_counts_surrogate_pairs_as_two() {
    let s = DomString::from_wtf8("漢字😀".as_bytes());
    assert!(!s.is_latin1());
    assert_eq!(s.len(), 4, "two BMP units and one pair");
    let units: Vec<u16> = s.units().collect();
    assert_eq!(units, "漢字😀".encode_utf16().collect::<Vec<_>>());
    let mut back = Vec::new();
    s.write_wtf8(&mut back);
    assert_eq!(back, "漢字😀".as_bytes());
}

#[test]
fn a_lone_surrogate_survives_the_round_trip() {
    let wtf8 = [b'a', 0xED, 0xA0, 0x80, b'b'];
    let s = DomString::from_wtf8(&wtf8);
    assert_eq!(s.units().collect::<Vec<_>>(), vec![0x61, 0xD800, 0x62]);
    let mut back = Vec::new();
    s.write_wtf8(&mut back);
    assert_eq!(back, wtf8);
}

#[test]
fn equality_and_hash_ignore_the_storage() {
    use std::hash::{BuildHasher, RandomState};
    let a = DomString::from_wtf8(b"same");
    let b = DomString::from_wtf8(b"same");
    assert_eq!(a, b);
    let h = RandomState::new();
    assert_eq!(h.hash_one(&a), h.hash_one(&b));
    assert_ne!(a, DomString::from_wtf8(b"other"));
}

#[test]
fn clones_share_one_block_and_the_empty_string_is_never_freed() {
    let a = DomString::from_wtf8("π".as_bytes());
    let b = a.clone();
    assert_eq!(a.block, b.block);
    drop(a);
    assert!(b.eq_str("π"), "the clone keeps the block alive");
    let e = DomString::empty();
    let e2 = e.clone();
    drop(e);
    drop(e2);
    assert!(DomString::from_wtf8(b"").is_empty());
}

#[test]
fn the_header_is_the_engine_layout() {
    let s = DomString::from_wtf8(b"x");
    let h = s.header();
    assert_eq!(abi::refcount(h), 1);
    assert_eq!(abi::tag(h), abi::TAG_STR);
    assert_eq!(
        abi::flags(h),
        abi::STR_FLAG_IS_LATIN1 | abi::STR_FLAG_IS_ASCII
    );
}

/// A count at its maximum aborts the process on the next clone rather
/// than wrapping to zero. The abort is watched from outside: the test
/// runs itself again as a child that does the clone.
#[test]
fn a_count_at_its_limit_aborts_instead_of_wrapping() {
    const CHILD: &str = "MADCAWL_STR_COUNT_LIMIT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let s = DomString::from_wtf8("counted, not static".as_bytes());
        assert_eq!(s.flags() & abi::FLAG_STATIC, 0);
        // SAFETY: a counted block's first word is its count
        unsafe { s.block.as_ptr().cast::<u32>().write(u32::MAX) };
        let _again = s.clone();
        unreachable!("the clone above returned");
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::a_count_at_its_limit_aborts_instead_of_wrapping",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        out.status.signal(),
        Some(6),
        "the child should die of SIGABRT: {:?}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}
