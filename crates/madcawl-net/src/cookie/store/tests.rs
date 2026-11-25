use super::*;
use crate::cookie::{Api, JarLimits, SiteContext};
use alt_url::Url;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const NOW: i64 = 1_800_000_000;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn filled() -> CookieJar {
    let mut jar = CookieJar::new(JarLimits::default());
    let u = url("https://www.example.com/app/x");
    assert!(jar.set(&u, "sid=abc; Max-Age=3600; HttpOnly; Secure; SameSite=Strict", Api::Http, NOW));
    assert!(jar.set(&u, "wide=1; Domain=example.com; Path=/; Max-Age=60", Api::Http, NOW));
    assert!(jar.set(&u, "session=1", Api::Http, NOW));
    jar
}

fn header(jar: &mut CookieJar, u: &str, now: i64) -> Option<String> {
    jar.header(&url(u), SiteContext::SameSite, now)
}

#[test]
fn persistent_cookies_round_trip_and_session_cookies_do_not() {
    let mut out = Vec::new();
    filled().encode_persistent(NOW, &mut out);
    let mut jar = CookieJar::new(JarLimits::default());
    assert_eq!(jar.load_persistent(&out, NOW), Ok(2));
    assert_eq!(header(&mut jar, "https://www.example.com/app/x", NOW).as_deref(), Some("sid=abc; wide=1"));
    let c = jar.matching(&url("https://www.example.com/app/x"), SiteContext::SameSite, Api::Http, NOW);
    assert!(c[0].http_only && c[0].secure && c[0].host_only && c[0].same_site == SameSite::Strict);
    assert_eq!(header(&mut jar, "http://www.example.com/app/x", NOW).as_deref(), Some("wide=1"));
}

#[test]
fn expired_cookies_are_dropped_on_load() {
    let mut out = Vec::new();
    filled().encode_persistent(NOW, &mut out);
    let mut jar = CookieJar::new(JarLimits::default());
    assert_eq!(jar.load_persistent(&out, NOW + 120), Ok(1));
    assert_eq!(header(&mut jar, "https://www.example.com/app/x", NOW + 120).as_deref(), Some("sid=abc"));
}

#[test]
fn damaged_files_are_refused_whole() {
    let mut out = Vec::new();
    filled().encode_persistent(NOW, &mut out);
    let mut jar = CookieJar::new(JarLimits::default());
    assert_eq!(jar.load_persistent(&out[..out.len() - 3], NOW), Err(StoreError::Truncated));
    assert_eq!(jar.load_persistent(&out[..10], NOW), Err(StoreError::Truncated));
    let mut flipped = out.clone();
    *flipped.last_mut().unwrap() ^= 1;
    assert_eq!(jar.load_persistent(&flipped, NOW), Err(StoreError::Checksum));
    let mut magic = out.clone();
    magic[0] = b'X';
    assert_eq!(jar.load_persistent(&magic, NOW), Err(StoreError::BadMagic));
    let mut version = out.clone();
    version[4] = 9;
    assert_eq!(jar.load_persistent(&version, NOW), Err(StoreError::Version(9)));
    assert!(jar.is_empty());
}

/// `body` under a header with its length and checksum.
fn file_of(body: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&fnv1a(body).to_le_bytes());
    out.extend_from_slice(body);
    out
}

#[test]
fn unknown_record_kinds_are_skipped() {
    let mut out = Vec::new();
    filled().encode_persistent(NOW, &mut out);
    let mut body = vec![200, 3, 0, 0, 0, 1, 2, 3];
    body.extend_from_slice(&out[HEADER..]);
    body.extend_from_slice(&[201, 0, 0, 0, 0]);
    let mut jar = CookieJar::new(JarLimits::default());
    assert_eq!(jar.load_persistent(&file_of(&body), NOW), Ok(2));
    let short = file_of(&[200, 9, 0, 0, 0, 1]);
    assert_eq!(jar.load_persistent(&short, NOW), Err(StoreError::Malformed));
}

#[test]
fn a_load_stays_within_the_jar_caps() {
    let mut big = CookieJar::new(JarLimits { per_domain: 50, total: 500 });
    for d in 0..10 {
        let u = url(&format!("https://h{d}.example.org/"));
        for i in 0..20 {
            assert!(big.set(&u, &format!("c{i}=1; Max-Age=600"), Api::Http, NOW));
        }
    }
    let mut out = Vec::new();
    big.encode_persistent(NOW, &mut out);
    let mut small = CookieJar::new(JarLimits { per_domain: 4, total: 10 });
    assert_eq!(small.load_persistent(&out, NOW), Ok(10));
    assert_eq!(small.len(), 10);
    assert!(small.by_domain.values().all(|l| l.len() <= 4));
}

#[test]
fn only_changes_to_persistent_cookies_are_reported() {
    let mut jar = CookieJar::new(JarLimits { per_domain: 2, total: 10 });
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    jar.set_on_change(move || {
        seen.fetch_add(1, Ordering::SeqCst);
    });
    let u = url("https://example.com/");
    let count = || n.load(Ordering::SeqCst);
    assert!(jar.set(&u, "s=1", Api::Http, NOW));
    assert_eq!(count(), 0, "a session cookie");
    assert!(jar.set(&u, "p=1; Max-Age=60", Api::Http, NOW));
    assert_eq!(count(), 1);
    assert!(jar.set(&u, "p=1", Api::Http, NOW));
    assert_eq!(count(), 2, "a persistent cookie replaced by a session one");
    assert!(jar.set(&u, "q=1", Api::Http, NOW));
    assert_eq!(count(), 3, "an eviction");
    assert!(jar.set(&u, "q=; Max-Age=0", Api::Http, NOW));
    assert_eq!(count(), 4, "a deletion");
    assert!(!jar.set(&u, "bad\u{1}=1; Max-Age=5", Api::Http, NOW));
    assert_eq!(count(), 4);
}

fn stored(domain: &str, name: &str, created: u64, accessed: u64) -> Cookie {
    Cookie {
        name: name.into(),
        value: "1".into(),
        domain: domain.into(),
        host_only: true,
        path: "/".into(),
        expires: Some(NOW + 600),
        secure: false,
        http_only: false,
        same_site: SameSite::Lax,
        created,
        accessed,
    }
}

fn file_with(cookies: &[Cookie]) -> Vec<u8> {
    let mut body = Vec::new();
    for c in cookies {
        encode_cookie(c, &mut body);
    }
    file_of(&body)
}

#[test]
fn the_files_clock_values_are_renumbered_not_adopted() {
    let file = file_with(&[stored("example.com", "max", u64::MAX, u64::MAX), stored("example.com", "old", 3, 5)]);
    let mut jar = CookieJar::new(JarLimits { per_domain: 2, total: 10 });
    assert_eq!(jar.load_persistent(&file, NOW), Ok(2));
    assert_eq!(jar.clock, 2);
    let u = url("http://example.com/");
    // the least recently used goes first, whatever the file claimed
    assert!(jar.set(&u, "new=1; Max-Age=60", Api::Http, NOW));
    assert_eq!(header(&mut jar, "http://example.com/", NOW).as_deref(), Some("max=1; new=1"));
    assert!(jar.set(&u, "newer=1; Max-Age=60", Api::Http, NOW));
    let mut left: Vec<String> = jar.by_domain["example.com"].iter().map(|c| c.name.clone()).collect();
    left.sort();
    assert_eq!(left, ["new", "newer"]);
}

#[test]
fn a_huge_file_keeps_only_the_newest_and_loads_fast() {
    let n = 100_000u64;
    let cookies: Vec<Cookie> = (0..n).map(|i| stored(&format!("h{i}.example.org"), "c", i, (i * 7919) % n)).collect();
    let file = file_with(&cookies);
    assert!(file.len() <= MAX_FILE);
    let mut jar = CookieJar::new(JarLimits::default());
    let t = std::time::Instant::now();
    assert_eq!(jar.load_persistent(&file, NOW), Ok(3000));
    assert!(t.elapsed() < std::time::Duration::from_secs(20), "{:?}", t.elapsed());
    // the newest by use are the ones whose file value was at least n - 3000
    let kept = |i: u64| (i * 7919) % n >= n - 3000;
    assert!((0..n).all(|i| jar.by_domain.contains_key(&format!("h{i}.example.org")) == kept(i)));
}
