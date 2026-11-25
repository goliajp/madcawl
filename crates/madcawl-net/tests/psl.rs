//! The public suffix list against its upstream test vectors, and the
//! cookie rules that depend on it.

use madcawl_net::Url;
use madcawl_net::cookie::{Api, CookieJar, JarLimits, SiteContext, site_of};
use madcawl_net::psl::registrable_domain;

const NOW: i64 = 1_800_000_000;

/// A domain in the ASCII form a parsed URL gives it.
fn ascii(domain: &str) -> String {
    Url::parse(&format!("http://{domain}/")).unwrap().host_str().unwrap().to_owned()
}

fn quoted(arg: &str) -> Option<&str> {
    arg.trim().strip_prefix('\'')?.strip_suffix('\'')
}

#[test]
fn upstream_test_vectors() {
    let text = include_str!("../psl/test_psl.txt");
    let mut checked = 0;
    for line in text.lines() {
        let Some(args) = line.strip_prefix("checkPublicSuffix(").and_then(|l| l.strip_suffix(");")) else { continue };
        let (input, expected) = args.split_once(',').unwrap();
        let Some(input) = quoted(input) else {
            assert_eq!(input, "null");
            continue;
        };
        let expected = quoted(expected).map(ascii);
        // a leading dot is an empty label, which the URL parser would keep
        let host = if input.starts_with('.') { input.to_ascii_lowercase() } else { ascii(input) };
        assert_eq!(registrable_domain(&host).map(str::to_owned), expected, "{line}");
        checked += 1;
    }
    assert_eq!(checked, 77, "every vector that is not commented out");
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn set(jar: &mut CookieJar, u: &str, line: &str) -> bool {
    jar.set(&url(u), line, Api::Http, NOW)
}

fn sent(jar: &mut CookieJar, u: &str) -> String {
    jar.header(&url(u), SiteContext::SameSite, NOW).unwrap_or_default()
}

#[test]
fn a_domain_attribute_naming_a_public_suffix_is_refused() {
    let mut jar = CookieJar::new(JarLimits::default());
    assert!(!set(&mut jar, "https://evil.github.io/", "a=1; Domain=github.io"));
    assert!(set(&mut jar, "https://a.b.example.co.uk/", "b=1; Domain=example.co.uk"));
    assert!(!set(&mut jar, "https://a.b.example.co.uk/", "c=1; Domain=co.uk"));
    assert!(!set(&mut jar, "https://a.b.example.co.uk/", "d=1; Domain=uk"));
    assert_eq!(sent(&mut jar, "https://other.example.co.uk/"), "b=1");
    assert_eq!(sent(&mut jar, "https://victim.co.uk/"), "");
    assert_eq!(sent(&mut jar, "https://victim.github.io/"), "");
}

#[test]
fn a_host_that_is_a_public_suffix_keeps_host_only_cookies() {
    let mut jar = CookieJar::new(JarLimits::default());
    assert!(set(&mut jar, "https://github.io/", "a=1; Domain=github.io"));
    assert!(set(&mut jar, "https://co.uk/", "b=1"));
    assert_eq!(sent(&mut jar, "https://github.io/"), "a=1");
    assert_eq!(sent(&mut jar, "https://evil.github.io/"), "", "host-only, not sent to children");
    assert_eq!(sent(&mut jar, "https://x.co.uk/"), "");
}

#[test]
fn localhost_and_ip_hosts() {
    let mut jar = CookieJar::new(JarLimits::default());
    assert!(set(&mut jar, "http://localhost/", "a=1; Domain=localhost"));
    assert!(!set(&mut jar, "http://app.localhost/", "b=1; Domain=localhost"));
    assert!(set(&mut jar, "http://app.localhost/", "c=1"));
    assert_eq!(sent(&mut jar, "http://localhost/"), "a=1");
    assert_eq!(sent(&mut jar, "http://app.localhost/"), "c=1");
    assert!(set(&mut jar, "http://10.0.0.1/", "d=1; Domain=10.0.0.1"));
    assert!(!set(&mut jar, "http://10.0.0.1/", "e=1; Domain=0.0.1"));
    assert!(set(&mut jar, "http://[::1]/", "f=1"));
    assert_eq!(sent(&mut jar, "http://10.0.0.1/"), "d=1");
    assert_eq!(sent(&mut jar, "http://[::1]/"), "f=1");
}

#[test]
fn an_idn_domain_attribute_is_compared_in_ascii_form() {
    let mut jar = CookieJar::new(JarLimits::default());
    assert!(set(&mut jar, "https://www.食狮.公司.cn/", "a=1; Domain=食狮.公司.cn"));
    assert!(!set(&mut jar, "https://www.食狮.公司.cn/", "b=1; Domain=公司.cn"));
    assert!(!set(&mut jar, "https://www.食狮.公司.cn/", "c=1; Domain=食狮.公司.cn/x"));
    assert_eq!(sent(&mut jar, "https://xn--85x722f.xn--55qx5d.cn/"), "a=1");
}

#[test]
fn sites_are_registrable_domains() {
    assert_eq!(site_of("a.b.example.co.uk"), "example.co.uk");
    assert_eq!(site_of("evil.github.io"), "evil.github.io");
    assert_eq!(site_of("github.io"), "github.io");
    assert_eq!(site_of("www.example.com."), "example.com");
    assert_eq!(site_of("192.168.0.1"), "192.168.0.1");
    assert_eq!(site_of("[::1]"), "[::1]");
}
