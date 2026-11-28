//! What each hop of a request carries by its initiator, credentials mode
//! and mode: cookies only to the initiator's origin in same-origin
//! credentials mode, `Origin` and no request needing a preflight to
//! another origin in CORS mode, nothing to another origin in same-origin
//! mode, and an opaque initiator's requests cross-site with `Origin: null`.
//! Two fixture servers on 127.0.0.1 are two origins of one site.

// the fixture's test CA is not needed here: plain HTTP only
#[allow(unused_imports)]
mod common;

use common::{Fixture, Req, reply, serve};
use madcawl_net::fetch::{Config, Credentials, Mode, Request};
use madcawl_net::{Client, NetError, Url};
use std::sync::Arc;
use std::time::Duration;

fn client() -> Client {
    Client::new(Config { io_timeout: Duration::from_secs(5), ..Config::default() })
}

/// A site whose `/go?<url>` redirects to `<url>`, whose `/login` sets
/// cookies, and whose other paths echo the cookies and `Origin` they got.
fn site() -> Fixture {
    serve(Arc::new(|r: &Req| {
        let echo = format!("cookie={};origin={}", r.header("cookie").unwrap_or_default(), r.header("origin").unwrap_or_default());
        Some(match r.target.split_once('?') {
            Some(("/go", to)) => reply(302, &[("Location", to)], b""),
            _ if r.target == "/login" => reply(200, &[("Set-Cookie", "s=1; SameSite=Strict"), ("Set-Cookie", "l=1; SameSite=Lax")], b""),
            _ if r.target == "/set" => reply(200, &[("Set-Cookie", "late=1")], echo.as_bytes()),
            _ => reply(200, &[], echo.as_bytes()),
        })
    }))
}

fn req(url: &str, initiator: &str, credentials: Credentials, mode: Mode) -> Request {
    let mut r = Request::get(Url::parse(url).unwrap());
    (r.navigation, r.initiator, r.credentials, r.mode) = (false, Some(Url::parse(initiator).unwrap()), credentials, mode);
    r
}

fn body(c: &Client, r: &Request) -> Result<String, NetError> {
    c.fetch(r).map(|r| String::from_utf8(r.body).unwrap())
}

fn two_sites(c: &Client) -> (Fixture, Fixture) {
    let (a, b) = (site(), site());
    c.fetch(&Request::get(Url::parse(&a.url("/login")).unwrap())).unwrap();
    (a, b)
}

#[test]
fn same_origin_credentials_stop_at_another_origin() {
    let c = client();
    let (a, b) = two_sites(&c);
    let page = a.url("/page");
    let same = req(&a.url("/echo"), &page, Credentials::SameOrigin, Mode::Cors);
    assert_eq!(body(&c, &same).unwrap(), "cookie=s=1; l=1;origin=");
    let hop = format!("{}?{}", a.url("/go"), b.url("/set"));
    let away = req(&hop, &page, Credentials::SameOrigin, Mode::Cors);
    let origin = format!("http://{}", a.addr);
    assert_eq!(body(&c, &away).unwrap(), format!("cookie=;origin={origin}"), "no cookies on the hop to another origin");
    assert_eq!(body(&c, &req(&a.url("/echo"), &page, Credentials::Include, Mode::NoCors)).unwrap(), "cookie=s=1; l=1;origin=", "nor stored from it");
    let include = req(&b.url("/echo"), &page, Credentials::Include, Mode::Cors);
    assert_eq!(body(&c, &include).unwrap(), format!("cookie=s=1; l=1;origin={origin}"), "cookies are per host, so include sends them");
}

#[test]
fn cors_mode_sends_origin_and_refuses_what_needs_a_preflight() {
    let c = client();
    let (a, b) = two_sites(&c);
    let page = a.url("/page");
    let origin = format!("http://{}", a.addr);
    let get = req(&b.url("/echo"), &page, Credentials::Omit, Mode::Cors);
    assert_eq!(body(&c, &get).unwrap(), format!("cookie=;origin={origin}"));
    assert_eq!(body(&c, &req(&a.url("/echo"), &page, Credentials::Omit, Mode::Cors)).unwrap(), "cookie=;origin=", "not to its own origin");
    assert_eq!(body(&c, &req(&b.url("/echo"), &page, Credentials::Omit, Mode::NoCors)).unwrap(), "cookie=;origin=");
    let mut custom = req(&b.url("/custom"), &page, Credentials::Omit, Mode::Cors);
    custom.headers.push(("X-Custom".into(), b"1".to_vec()));
    assert!(matches!(c.fetch(&custom), Err(NetError::Refused(_))));
    let mut json = req(&b.url("/json"), &page, Credentials::Omit, Mode::Cors);
    (json.method, json.body) = ("POST".into(), Some(b"{}".to_vec()));
    json.headers.push(("Content-Type".into(), b"application/json".to_vec()));
    assert!(matches!(c.fetch(&json), Err(NetError::Refused(_))));
    let mut put = req(&b.url("/put"), &page, Credentials::Omit, Mode::Cors);
    put.method = "PUT".into();
    assert!(matches!(c.fetch(&put), Err(NetError::Refused(_))));
    custom.url = Url::parse(&format!("{}?{}", a.url("/go"), b.url("/custom"))).unwrap();
    assert!(matches!(c.fetch(&custom), Err(NetError::Refused(_))), "nor after a redirect to another origin");
    assert!(b.requests().iter().all(|r| !matches!(r.target.as_str(), "/custom" | "/json" | "/put")), "none of them reached the other origin");
    let third = site();
    let tainted = req(&format!("{}?{}", b.url("/go"), third.url("/echo")), &page, Credentials::Omit, Mode::Cors);
    assert_eq!(body(&c, &tainted).unwrap(), "cookie=;origin=null", "a redirect between two other origins taints the origin");
}

#[test]
fn same_origin_mode_refuses_another_origin() {
    let c = client();
    let (a, b) = two_sites(&c);
    let page = a.url("/page");
    assert!(body(&c, &req(&a.url("/w.js"), &page, Credentials::SameOrigin, Mode::SameOrigin)).is_ok());
    let away = req(&format!("{}?{}", a.url("/go"), b.url("/w.js")), &page, Credentials::SameOrigin, Mode::SameOrigin);
    assert!(matches!(c.fetch(&away), Err(NetError::Refused(_))));
    assert!(b.requests().is_empty());
}

#[test]
fn an_opaque_initiator_is_cross_site_with_a_null_origin() {
    let c = &client();
    let (a, _b) = two_sites(c);
    let mut post = req(&a.url("/echo"), "about:blank", Credentials::Include, Mode::Cors);
    (post.method, post.body) = ("POST".into(), Some(b"x".to_vec()));
    assert_eq!(body(c, &post).unwrap(), "cookie=;origin=null", "no SameSite cookie, and Origin: null");
    post.initiator = Some(Url::parse(&a.url("/page")).unwrap());
    assert_eq!(body(c, &post).unwrap(), format!("cookie=s=1; l=1;origin=http://{}", a.addr));
}
