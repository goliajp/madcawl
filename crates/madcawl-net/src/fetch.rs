//! Fetch's main flow for network requests: one HTTP/1.1 exchange per hop
//! over the pool, cookies read and stored at each hop, redirects followed
//! with the method rewriting the Fetch standard asks for, and the body
//! decoded as it arrives.

mod exchange;
mod forbidden;
mod hop;

pub use forbidden::{is_forbidden, is_forbidden_method, normalize_method};
pub use hop::Sink;

use hop::Hop;

use crate::cookie::{CookieJar, JarLimits, SiteContext, site_of};
use crate::dns::DnsCache;
use crate::error::NetError;
use crate::pool::{Pool, PoolLimits};
use crate::sock::Cancel;
use crate::trust::{Trust, TrustSource};
use alt_url::Url;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Config {
    pub dns_timeout: Duration,
    pub connect_timeout: Duration,
    /// how long one read or write may block
    pub io_timeout: Duration,
    /// how long a whole request may take, redirects included
    pub request_timeout: Duration,
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
            request_timeout: Duration::from_secs(300),
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
    /// when not empty, the only origins (as `Url::origin` writes them)
    /// a redirect may carry the body to
    pub body_origins: Vec<String>,
    /// ends the request early, its socket operations included
    pub cancel: Option<Arc<Cancel>>,
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
            body_origins: Vec::new(),
            cancel: None,
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

    /// The cookie jar; a lock poisoned by a panic elsewhere is taken over,
    /// since every change to the jar leaves it consistent.
    pub fn lock_jar(&self) -> MutexGuard<'_, CookieJar> {
        self.jar.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The whole response, its body decoded and held in memory up to the
    /// configured caps.
    pub fn fetch(&self, req: &Request) -> Result<Response, NetError> {
        struct Collect(Option<Response>);
        impl Sink for Collect {
            fn head(&mut self, r: &Response) -> Result<(), NetError> {
                self.0 = Some(r.clone());
                Ok(())
            }
            fn body(&mut self, chunk: &[u8]) -> Result<(), NetError> {
                if let Some(r) = &mut self.0 {
                    r.body.extend_from_slice(chunk);
                }
                Ok(())
            }
        }
        let mut c = Collect(None);
        self.fetch_into(req, &mut c)?;
        Ok(c.0.expect("the head comes before a successful end"))
    }

    /// Fetches `req`, handing the final response's head and then its
    /// decoded body to `sink` as they arrive; the body is never held whole.
    pub fn fetch_into(&self, req: &Request, sink: &mut dyn Sink) -> Result<(), NetError> {
        if is_forbidden_method(&req.method) {
            return Err(NetError::Refused(format!("forbidden method {}", req.method)));
        }
        let deadline = Instant::now() + self.config.request_timeout;
        let mut url = req.url.clone();
        let mut method = normalize_method(&req.method);
        let mut body = req.body.as_deref();
        let mut headers: Vec<(String, Vec<u8>)> =
            req.headers.iter().filter(|(n, v)| !is_forbidden(n, v)).cloned().collect();
        let mut url_list = vec![url.clone()];
        loop {
            if !matches!(url.scheme(), "http" | "https") {
                return Err(NetError::Scheme(url.scheme().to_owned()));
            }
            if req.cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                return Err(NetError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(NetError::Deadline);
            }
            let ctx = site_context(req, &url, &method);
            let cookie = if req.credentials { self.lock_jar().header(&url, ctx, now_secs()) } else { None };
            let origin = (!matches!(method.as_str(), "GET" | "HEAD"))
                .then(|| req.initiator.as_ref().map_or("null".into(), |i| i.origin().ascii_serialization()));
            let extra = exchange::Extra { cookie: cookie.as_deref(), origin: origin.as_deref(), body, deadline, cancel: req.cancel.as_ref() };
            let mut hop = Hop { client: self, req, url: &url, url_list: &url_list, method: &method, sink: &mut *sink, decoder: None, delivered: false };
            let raw = self.exchange(&method, &url, &headers, extra, &mut hop)?;
            if hop.delivered {
                let Hop { decoder, sink, .. } = hop;
                return match decoder {
                    Some(mut d) => d.finish(&mut |b| sink.body(b)),
                    None => Ok(()),
                };
            }
            if req.redirect == Redirect::Error {
                return Err(NetError::Refused("redirect with redirect mode error".into()));
            }
            if url_list.len() > self.config.max_redirects {
                return Err(NetError::TooManyRedirects);
            }
            let loc = String::from_utf8_lossy(raw.head.header("location").unwrap_or_default()).into_owned();
            let next = redirect_url(&url, &loc)?;
            rewrite_for_redirect(raw.head.status, &mut method, &mut body, &mut headers);
            let to = next.origin().ascii_serialization();
            if body.is_some() && !req.body_origins.is_empty() && !req.body_origins.contains(&to) {
                return Err(NetError::Refused(format!("a redirect would send the request body on to {to}, an origin not approved for it")));
            }
            if next.origin().ascii_serialization() != url.origin().ascii_serialization() {
                headers.retain(|(n, _)| !n.eq_ignore_ascii_case("authorization"));
            }
            url = next;
            url_list.push(url.clone());
        }
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
fn rewrite_for_redirect(status: u16, method: &mut String, body: &mut Option<&[u8]>, headers: &mut Vec<(String, Vec<u8>)>) {
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
