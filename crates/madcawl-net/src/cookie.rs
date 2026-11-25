//! A bounded cookie jar after RFC 6265bis: domain and path matching,
//! `Secure`, `HttpOnly`, `SameSite` (absent means `Lax`), expiry, the
//! `__Secure-` and `__Host-` prefixes. A `Domain` attribute naming a
//! public suffix (the built-in list) is refused unless it is the host
//! itself, which then gets a host-only cookie; IP hosts only get
//! host-only cookies.

mod domain;
mod parse;

pub use parse::{SameSite, SetCookie, parse, parse_date};

use alt_url::{Host, Url};
use std::collections::HashMap;

const MAX_LIFETIME: i64 = 400 * 86400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    /// unix seconds; `None` lives until the jar is dropped
    pub expires: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
    created: u64,
    accessed: u64,
}

/// Who reads or writes: the network stack, or a page's script, which
/// never sees or sets `HttpOnly` cookies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    Http,
    Script,
}

/// How the request relates to the site that made it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteContext {
    SameSite,
    /// a cross-site top-level navigation; `safe` for GET and HEAD
    CrossSiteNavigation { safe: bool },
    CrossSite,
}

#[derive(Clone, Copy, Debug)]
pub struct JarLimits {
    pub per_domain: usize,
    pub total: usize,
}

impl Default for JarLimits {
    fn default() -> Self {
        JarLimits { per_domain: 180, total: 3000 }
    }
}

pub struct CookieJar {
    by_domain: HashMap<String, Vec<Cookie>>,
    total: usize,
    clock: u64,
    limits: JarLimits,
}

fn host_of(url: &Url) -> Option<(String, bool)> {
    match url.host()? {
        Host::Domain(d) => Some((d.trim_end_matches('.').to_ascii_lowercase(), false)),
        Host::Ipv4(a) => Some((a.to_string(), true)),
        Host::Ipv6(a) => Some((format!("[{a}]"), true)),
    }
}

fn is_secure(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "wss")
}

/// The site of a host: its registrable domain, or the host itself when
/// it is an IP address or a public suffix.
pub fn site_of(host: &str) -> &str {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.starts_with('[') || host.parse::<std::net::Ipv4Addr>().is_ok() {
        return host;
    }
    crate::psl::registrable_domain(host).unwrap_or(host)
}

fn default_path(url: &Url) -> String {
    let p = url.path();
    match p.rfind('/') {
        Some(i) if p.starts_with('/') && i > 0 => p[..i].to_owned(),
        _ => "/".to_owned(),
    }
}

fn path_matches(cookie: &str, request: &str) -> bool {
    request == cookie
        || (request.starts_with(cookie)
            && (cookie.ends_with('/') || request.as_bytes().get(cookie.len()) == Some(&b'/')))
}

fn domain_matches(host: &str, domain: &str, ip: bool) -> bool {
    host == domain || (!ip && host.len() > domain.len() && host.ends_with(domain) && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

struct Admitted {
    domain: String,
    host_only: bool,
    path: String,
    expires: Option<i64>,
}

/// Where a parsed cookie from `url` would be stored, or `None` when the
/// storage rules refuse it.
fn admit(sc: &SetCookie, url: &Url, api: Api, now: i64) -> Option<Admitted> {
    let (host, ip) = host_of(url)?;
    let secure_origin = is_secure(url);
    if (sc.secure && !secure_origin) || (sc.http_only && api == Api::Script) {
        return None;
    }
    if sc.same_site == SameSite::None && !sc.secure {
        return None;
    }
    let (domain, host_only) = match sc.domain.as_deref() {
        None => (host, true),
        Some(d) => domain::admit(d, &host, ip)?,
    };
    let path = sc.path.clone().unwrap_or_else(|| default_path(url));
    let lower = sc.name.to_ascii_lowercase();
    if lower.starts_with("__secure-") && !(sc.secure && secure_origin) {
        return None;
    }
    if lower.starts_with("__host-") && !(sc.secure && secure_origin && sc.domain.is_none() && path == "/") {
        return None;
    }
    let expires = match (sc.max_age, sc.expires) {
        (Some(m), _) if m <= 0 => Some(now - 1),
        (Some(m), _) => Some(now.saturating_add(m.min(MAX_LIFETIME))),
        (None, Some(e)) => Some(e.min(now + MAX_LIFETIME)),
        (None, None) => None,
    };
    Some(Admitted { domain, host_only, path, expires })
}

impl CookieJar {
    pub fn new(limits: JarLimits) -> Self {
        CookieJar { by_domain: HashMap::new(), total: 0, clock: 0, limits }
    }

    pub fn len(&self) -> usize {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// Stores the cookie of one `Set-Cookie` value received for `url`;
    /// false when it is ignored.
    pub fn set(&mut self, url: &Url, line: &str, api: Api, now: i64) -> bool {
        let Some(sc) = parse(line) else { return false };
        let Some(Admitted { domain, host_only, path, expires }) = admit(&sc, url, api, now) else {
            return false;
        };
        let secure_origin = is_secure(url);
        self.clock += 1;
        let clock = self.clock;
        let mut created = clock;
        if let Some(list) = self.by_domain.get_mut(&domain) {
            let shadows_secure = |c: &Cookie| {
                c.secure && c.name == sc.name && (path_matches(&c.path, &path) || path_matches(&path, &c.path))
            };
            if !secure_origin && list.iter().any(shadows_secure) {
                return false;
            }
            if let Some(i) = list.iter().position(|c| c.name == sc.name && c.path == path && c.host_only == host_only) {
                if list[i].http_only && api == Api::Script {
                    return false;
                }
                created = list.remove(i).created;
                self.total -= 1;
                if list.is_empty() {
                    self.by_domain.remove(&domain);
                }
            }
        }
        if expires.is_some_and(|e| e < now) {
            return true;
        }
        let cookie = Cookie {
            name: sc.name,
            value: sc.value,
            domain: domain.clone(),
            host_only,
            path,
            expires,
            secure: sc.secure,
            http_only: sc.http_only,
            same_site: sc.same_site,
            created,
            accessed: clock,
        };
        self.make_room(&domain, now);
        self.by_domain.entry(domain).or_default().push(cookie);
        self.total += 1;
        true
    }

    fn make_room(&mut self, domain: &str, now: i64) {
        for list in self.by_domain.values_mut() {
            let before = list.len();
            list.retain(|c| c.expires.is_none_or(|e| e >= now));
            self.total -= before - list.len();
        }
        if let Some(list) = self.by_domain.get_mut(domain)
            && list.len() >= self.limits.per_domain
            && let Some(i) = list.iter().enumerate().min_by_key(|(_, c)| c.accessed).map(|(i, _)| i)
        {
            list.remove(i);
            self.total -= 1;
        }
        while self.total >= self.limits.total {
            let oldest = self
                .by_domain
                .iter()
                .flat_map(|(d, l)| l.iter().enumerate().map(move |(i, c)| (c.accessed, d.clone(), i)))
                .min();
            let Some((_, d, i)) = oldest else { break };
            self.by_domain.get_mut(&d).expect("domain just seen").remove(i);
            self.total -= 1;
        }
        self.by_domain.retain(|_, l| !l.is_empty());
    }

    /// The cookies to send to `url`, longest path first.
    pub fn matching(&mut self, url: &Url, ctx: SiteContext, api: Api, now: i64) -> Vec<Cookie> {
        let Some((host, ip)) = host_of(url) else { return Vec::new() };
        let secure = is_secure(url);
        let path = if url.path().starts_with('/') { url.path().to_owned() } else { "/".to_owned() };
        let mut candidates = vec![host.clone()];
        if !ip {
            let mut rest = host.as_str();
            while let Some((_, parent)) = rest.split_once('.') {
                candidates.push(parent.to_owned());
                rest = parent;
            }
        }
        self.clock += 1;
        let clock = self.clock;
        let mut out = Vec::new();
        for d in candidates {
            let Some(list) = self.by_domain.get_mut(&d) else { continue };
            for c in list.iter_mut() {
                let host_ok = if c.host_only { c.domain == host } else { domain_matches(&host, &c.domain, ip) };
                let same_site_ok = match (ctx, c.same_site) {
                    (SiteContext::SameSite, _) | (_, SameSite::None) => true,
                    (SiteContext::CrossSiteNavigation { safe }, SameSite::Lax | SameSite::Default) => safe,
                    _ => false,
                };
                if host_ok
                    && path_matches(&c.path, &path)
                    && (!c.secure || secure)
                    && (!c.http_only || api == Api::Http)
                    && c.expires.is_none_or(|e| e >= now)
                    && same_site_ok
                {
                    c.accessed = clock;
                    out.push(c.clone());
                }
            }
        }
        out.sort_by(|a, b| b.path.len().cmp(&a.path.len()).then(a.created.cmp(&b.created)));
        out
    }

    /// The `Cookie` header value for a request to `url`, if any cookie goes.
    pub fn header(&mut self, url: &Url, ctx: SiteContext, now: i64) -> Option<String> {
        let list = self.matching(url, ctx, Api::Http, now);
        (!list.is_empty()).then(|| serialize(&list))
    }
}

/// `name=value` pairs joined with `; `, as both the `Cookie` header and
/// `document.cookie` show them.
pub fn serialize(list: &[Cookie]) -> String {
    let parts: Vec<String> = list
        .iter()
        .map(|c| if c.name.is_empty() { c.value.clone() } else { format!("{}={}", c.name, c.value) })
        .collect();
    parts.join("; ")
}

#[cfg(test)]
mod tests;
