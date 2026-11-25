use super::*;

const NOW: i64 = 1_800_000_000;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn names(jar: &mut CookieJar, u: &str, ctx: SiteContext, api: Api) -> Vec<String> {
    jar.matching(&url(u), ctx, api, NOW).into_iter().map(|c| c.name).collect()
}

#[test]
fn domain_and_path_matching() {
    let mut jar = CookieJar::new(JarLimits::default());
    let u = url("https://www.example.com/app/page");
    assert!(jar.set(&u, "host=1", Api::Http, NOW));
    assert!(jar.set(&u, "wide=1; Domain=example.com; Path=/", Api::Http, NOW));
    assert!(!jar.set(&u, "other=1; Domain=example.org", Api::Http, NOW));
    assert!(!jar.set(&u, "tld=1; Domain=com", Api::Http, NOW));
    let same = SiteContext::SameSite;
    assert_eq!(names(&mut jar, "https://www.example.com/app/x", same, Api::Http), ["host", "wide"]);
    assert_eq!(names(&mut jar, "https://www.example.com/other", same, Api::Http), ["wide"]);
    assert_eq!(names(&mut jar, "https://api.example.com/app/x", same, Api::Http), ["wide"]);
    assert_eq!(names(&mut jar, "https://notexample.com/", same, Api::Http), Vec::<String>::new());
    assert!(!path_matches("/app", "/application"));
    assert!(path_matches("/app", "/app/x"));
}

#[test]
fn ip_and_single_label_hosts_get_host_only_cookies() {
    let mut jar = CookieJar::new(JarLimits::default());
    let u = url("http://127.0.0.1:8080/");
    assert!(jar.set(&u, "a=1", Api::Http, NOW));
    assert!(jar.set(&u, "b=1; Domain=127.0.0.1", Api::Http, NOW));
    assert!(!jar.set(&u, "c=1; Domain=0.0.1", Api::Http, NOW));
    let l = url("http://localhost/");
    assert!(!jar.set(&l, "d=1; Domain=localhost.", Api::Http, NOW));
    assert!(jar.set(&l, "e=1", Api::Http, NOW));
    assert_eq!(names(&mut jar, "http://127.0.0.1:9/", SiteContext::SameSite, Api::Http), ["a", "b"]);
}

#[test]
fn secure_http_only_and_prefixes() {
    let mut jar = CookieJar::new(JarLimits::default());
    let https = url("https://example.com/");
    let http = url("http://example.com/");
    assert!(!jar.set(&http, "s=1; Secure", Api::Http, NOW));
    assert!(jar.set(&https, "s=1; Secure", Api::Http, NOW));
    assert!(jar.set(&https, "h=1; HttpOnly", Api::Http, NOW));
    assert!(!jar.set(&https, "h=2", Api::Script, NOW), "script cannot overwrite an HttpOnly cookie");
    assert!(!jar.set(&https, "j=1; HttpOnly", Api::Script, NOW));
    assert!(!jar.set(&http, "s=2", Api::Http, NOW), "insecure origin cannot shadow a secure cookie");
    assert_eq!(names(&mut jar, "http://example.com/", SiteContext::SameSite, Api::Http), ["h"]);
    assert_eq!(names(&mut jar, "https://example.com/", SiteContext::SameSite, Api::Script), ["s"]);
    assert!(!jar.set(&https, "__Secure-a=1", Api::Http, NOW));
    assert!(jar.set(&https, "__Secure-a=1; Secure", Api::Http, NOW));
    assert!(!jar.set(&https, "__Host-a=1; Secure; Path=/x", Api::Http, NOW));
    assert!(!jar.set(&https, "__Host-a=1; Secure; Path=/; Domain=example.com", Api::Http, NOW));
    assert!(jar.set(&https, "__Host-b=1; Secure; Path=/", Api::Http, NOW));
}

#[test]
fn same_site_rules() {
    let mut jar = CookieJar::new(JarLimits::default());
    let u = url("https://example.com/");
    for line in ["lax=1; SameSite=Lax", "strict=1; SameSite=Strict", "none=1; SameSite=None; Secure", "dflt=1"] {
        assert!(jar.set(&u, line, Api::Http, NOW));
    }
    assert!(!jar.set(&u, "insecure_none=1; SameSite=None", Api::Http, NOW));
    let mut got = |ctx| {
        let mut n = names(&mut jar, "https://example.com/", ctx, Api::Http);
        n.sort();
        n
    };
    assert_eq!(got(SiteContext::SameSite), ["dflt", "lax", "none", "strict"]);
    assert_eq!(got(SiteContext::CrossSiteNavigation { safe: true }), ["dflt", "lax", "none"]);
    assert_eq!(got(SiteContext::CrossSiteNavigation { safe: false }), ["none"]);
    assert_eq!(got(SiteContext::CrossSite), ["none"]);
}

#[test]
fn expiry_replacement_and_deletion() {
    let mut jar = CookieJar::new(JarLimits::default());
    let u = url("https://example.com/");
    assert!(jar.set(&u, "a=1; Max-Age=10", Api::Http, NOW));
    assert!(jar.set(&u, "b=1; Expires=Thu, 01 Jan 1970 00:00:00 GMT", Api::Http, NOW));
    assert_eq!(jar.len(), 1);
    assert_eq!(jar.matching(&u, SiteContext::SameSite, Api::Http, NOW + 11).len(), 0);
    assert!(jar.set(&u, "a=2", Api::Http, NOW));
    assert_eq!(jar.len(), 1);
    assert_eq!(jar.header(&u, SiteContext::SameSite, NOW).as_deref(), Some("a=2"));
    assert!(jar.set(&u, "a=; Max-Age=0", Api::Http, NOW));
    assert!(jar.is_empty());
    assert!(jar.set(&u, "c=1; Max-Age=99999999999", Api::Http, NOW));
    let c = &jar.matching(&u, SiteContext::SameSite, Api::Http, NOW)[0];
    assert_eq!(c.expires, Some(NOW + MAX_LIFETIME));
}

#[test]
fn the_jar_stays_within_its_caps() {
    let mut jar = CookieJar::new(JarLimits { per_domain: 4, total: 10 });
    let u = url("https://a.example.com/");
    for i in 0..20 {
        assert!(jar.set(&u, &format!("c{i}=1"), Api::Http, NOW));
    }
    assert_eq!(jar.len(), 4);
    assert_eq!(names(&mut jar, "https://a.example.com/", SiteContext::SameSite, Api::Http), ["c16", "c17", "c18", "c19"]);
    for d in 0..10 {
        let u = url(&format!("https://h{d}.example.org/"));
        assert!(jar.set(&u, "x=1", Api::Http, NOW));
    }
    assert!(jar.len() <= 10);
    assert_eq!(jar.len(), 10);
}

#[test]
fn sites_without_a_suffix_list() {
    assert_eq!(site_of("a.b.example.com"), "example.com");
    assert_eq!(site_of("example.com"), "example.com");
    assert_eq!(site_of("localhost"), "localhost");
    assert_eq!(site_of("127.0.0.1"), "127.0.0.1");
}
