//! Fetch's main flow for network requests: one HTTP/1.1 exchange per hop
//! over the pool, cookies read and stored at each hop, redirects followed
//! with the method rewriting the Fetch standard asks for, and the body
//! decoded.

mod exchange;

use crate::cookie::{Api, CookieJar, JarLimits, SiteContext, site_of};
use crate::dns::DnsCache;
use crate::error::NetError;
use crate::pool::{Pool, PoolLimits};
use crate::trust::{Trust, TrustSource};
use crate::{decode, http1};
use alt_url::Url;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Config {
    pub dns_timeout: Duration,
    pub connect_timeout: Duration,
    /// how long one read or write may block
    pub io_timeout: Duration,
    pub max_redirects: usize,
    /// the body as sent, and after content decoding
    pub max_body: usize,
    pub max_decoded: usize,
    pub dns_cache: usize,
    pub pool: PoolLimits,
    pub jar: JarLimits,
    pub trust: TrustSource,
    pub user_agent: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            dns_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(15),
            io_timeout: Duration::from_secs(30),
            max_redirects: 20,
            max_body: 64 << 20,
            max_decoded: 128 << 20,
            dns_cache: 256,
            pool: PoolLimits::default(),
            jar: JarLimits::default(),
            trust: TrustSource::System,
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) madcawl/0.0.1".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Redirect {
    #[default]
    Follow,
    /// hand the redirect response back as it is
    Manual,
    Error,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub url: Url,
    /// headers from the requester; the forbidden ones are dropped
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Option<Vec<u8>>,
    /// a top-level navigation
    pub navigation: bool,
    /// the document that made the request; `None` for the user
    pub initiator: Option<Url>,
    pub credentials: bool,
    pub redirect: Redirect,
}

impl Request {
    pub fn get(url: Url) -> Self {
        Request {
            method: "GET".into(),
            url,
            headers: Vec::new(),
            body: None,
            navigation: true,
            initiator: None,
            credentials: true,
            redirect: Redirect::Follow,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    /// every URL the request went to, the final one last
    pub url_list: Vec<Url>,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn url(&self) -> &Url {
        self.url_list.last().expect("at least the request url")
    }

    pub fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_slice())
    }
}

/// Request headers a requester may not set (Fetch's forbidden request
/// header names, and the ones this stack writes itself).
pub fn is_forbidden(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "accept-charset", "accept-encoding", "access-control-request-headers",
        "access-control-request-method", "connection", "content-length", "cookie", "cookie2",
        "date", "dnt", "expect", "host", "keep-alive", "origin", "referer", "set-cookie", "te",
        "trailer", "transfer-encoding", "upgrade", "via",
    ];
    let lower = name.to_ascii_lowercase();
    NAMES.contains(&lower.as_str()) || lower.starts_with("proxy-") || lower.starts_with("sec-")
}

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The network state one network process owns: the DNS cache, the
/// connection pool, the cookie jar and the trust verdicts, all bounded.
pub struct Client {
    pub config: Config,
    dns: DnsCache,
    pool: Pool,
    jar: Mutex<CookieJar>,
    trust: Arc<Trust>,
}

fn host_of(url: &Url) -> &str {
    url.host_str().unwrap_or("")
}

fn site_context(req: &Request, url: &Url, method: &str) -> SiteContext {
    let Some(init) = &req.initiator else { return SiteContext::SameSite };
    if init.scheme() == url.scheme() && site_of(host_of(init)) == site_of(host_of(url)) {
        SiteContext::SameSite
    } else if req.navigation {
        SiteContext::CrossSiteNavigation { safe: matches!(method, "GET" | "HEAD") }
    } else {
        SiteContext::CrossSite
    }
}

impl Client {
    pub fn new(config: Config) -> Self {
        Client {
            dns: DnsCache::new(config.dns_cache),
            pool: Pool::new(config.pool),
            jar: Mutex::new(CookieJar::new(config.jar)),
            trust: Arc::new(Trust::new(config.trust.clone())),
            config,
        }
    }

    pub fn jar(&self) -> &Mutex<CookieJar> {
        &self.jar
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub fn dns(&self) -> &DnsCache {
        &self.dns
    }

    pub fn fetch(&self, req: &Request) -> Result<Response, NetError> {
        let mut url = req.url.clone();
        let mut method = req.method.to_ascii_uppercase();
        let mut body = req.body.clone();
        let mut headers: Vec<(String, Vec<u8>)> =
            req.headers.iter().filter(|(n, _)| !is_forbidden(n)).cloned().collect();
        let mut url_list = vec![url.clone()];
        loop {
            if !matches!(url.scheme(), "http" | "https") {
                return Err(NetError::Scheme(url.scheme().to_owned()));
            }
            let ctx = site_context(req, &url, &method);
            let cookie = if req.credentials { self.jar.lock().unwrap().header(&url, ctx, now_secs()) } else { None };
            let origin = (!matches!(method.as_str(), "GET" | "HEAD"))
                .then(|| req.initiator.as_ref().map_or("null".into(), |i| i.origin().ascii_serialization()));
            let extra = exchange::Extra { cookie: cookie.as_deref(), origin: origin.as_deref() };
            let raw = self.exchange(&method, &url, &headers, extra, body.as_deref())?;
            if req.credentials {
                let mut jar = self.jar.lock().unwrap();
                let now = now_secs();
                for (_, v) in raw.headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case("set-cookie")) {
                    jar.set(&url, &String::from_utf8_lossy(v), Api::Http, now);
                }
            }
            let location = raw.header("location").map(|l| String::from_utf8_lossy(l).into_owned());
            if let (true, Some(loc)) = (matches!(raw.status, 301 | 302 | 303 | 307 | 308), location) {
                match req.redirect {
                    Redirect::Manual => return Ok(finish_raw(raw, url_list)),
                    Redirect::Error => return Err(NetError::Refused("redirect with redirect mode error".into())),
                    Redirect::Follow => {}
                }
                if url_list.len() > self.config.max_redirects {
                    return Err(NetError::TooManyRedirects);
                }
                let next = redirect_url(&url, &loc)?;
                rewrite_for_redirect(raw.status, &mut method, &mut body, &mut headers);
                if next.origin().ascii_serialization() != url.origin().ascii_serialization() {
                    headers.retain(|(n, _)| !n.eq_ignore_ascii_case("authorization"));
                }
                url = next;
                url_list.push(url.clone());
                continue;
            }
            return self.finish(raw, &method, url_list);
        }
    }

    fn finish(&self, raw: http1::RawResponse, method: &str, url_list: Vec<Url>) -> Result<Response, NetError> {
        let has_body = method != "HEAD" && !matches!(raw.status, 204 | 304);
        let mut r = finish_raw(raw, url_list);
        if has_body && let Some(enc) = r.header("content-encoding") {
            let codings = decode::codings(enc)?;
            r.body = decode::decode(std::mem::take(&mut r.body), &codings, self.config.max_decoded)?;
        }
        Ok(r)
    }
}

/// The redirect target, keeping the fragment when the target has none.
fn redirect_url(url: &Url, location: &str) -> Result<Url, NetError> {
    let next = url.join(location).map_err(|e| NetError::BadUrl(format!("{location}: {e:?}")))?;
    match (next.fragment(), url.fragment()) {
        (None, Some(f)) => Ok(next.join(&format!("#{f}")).unwrap_or(next)),
        _ => Ok(next),
    }
}

/// 301 and 302 turn POST into GET, 303 turns anything but GET and HEAD
/// into GET; the body and its headers go with the old method.
fn rewrite_for_redirect(status: u16, method: &mut String, body: &mut Option<Vec<u8>>, headers: &mut Vec<(String, Vec<u8>)>) {
    let rewrite = (matches!(status, 301 | 302) && method == "POST") || (status == 303 && !matches!(method.as_str(), "GET" | "HEAD"));
    if rewrite {
        *method = "GET".into();
        *body = None;
        headers.retain(|(n, _)| {
            let n = n.to_ascii_lowercase();
            !matches!(n.as_str(), "content-encoding" | "content-language" | "content-location" | "content-type")
        });
    }
}

fn finish_raw(raw: http1::RawResponse, url_list: Vec<Url>) -> Response {
    Response { status: raw.status, reason: raw.reason, url_list, headers: raw.headers, body: raw.body }
}
