//! The URL Standard's setters on a parsed URL, done with the one URL
//! parser madcawl has: each setter checks what the standard's state
//! override checks, writes the URL again with the new component in place,
//! percent-encoded with the component's own set, and parses that. `None`
//! where the standard leaves the URL as it was.

use alt_url::Url;
use alt_url::percent::{self, EncodeSet};

pub(crate) fn is_special(scheme: &str) -> bool {
    matches!(scheme, "ftp" | "file" | "http" | "https" | "ws" | "wss")
}

/// The input with ASCII tab and newline removed, as the parser does.
fn clean(v: &str) -> String {
    v.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect()
}

/// The parts of a URL, serialized, to write it again with one changed.
struct Parts<'a> {
    scheme: &'a str,
    user: &'a str,
    pass: &'a str,
    host: Option<&'a str>,
    port: Option<u16>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

impl<'a> Parts<'a> {
    fn of(u: &'a Url) -> Self {
        Parts {
            scheme: u.scheme(),
            user: u.username(),
            pass: u.password().unwrap_or(""),
            host: u.host_str(),
            port: u.port(),
            path: u.path(),
            query: u.query(),
            fragment: u.fragment(),
        }
    }

    /// The standard's URL serializer over these parts, parsed again.
    fn parse(&self) -> Option<Url> {
        let mut s = format!("{}:", self.scheme);
        match self.host {
            Some(host) => {
                s.push_str("//");
                if !self.user.is_empty() || !self.pass.is_empty() {
                    s.push_str(self.user);
                    if !self.pass.is_empty() {
                        s.push(':');
                        s.push_str(self.pass);
                    }
                    s.push('@');
                }
                s.push_str(host);
                if let Some(p) = self.port {
                    s.push_str(&format!(":{p}"));
                }
            }
            None if self.path.starts_with("//") => s.push_str("/."),
            None => {}
        }
        s.push_str(self.path);
        if let Some(q) = self.query {
            s.push('?');
            s.push_str(q);
        }
        if let Some(f) = self.fragment {
            s.push('#');
            s.push_str(f);
        }
        Url::parse(&s).ok()
    }
}

fn encoded(v: &str, set: EncodeSet) -> String {
    let mut out = String::with_capacity(v.len());
    percent::encode(&mut out, v, set);
    out
}

pub(crate) fn protocol(u: &Url, v: &str) -> Option<Url> {
    let v = clean(v);
    let scheme = v.split(':').next().unwrap_or("");
    let ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let scheme = scheme.to_ascii_lowercase();
    let p = Parts::of(u);
    if !ok || is_special(&scheme) != is_special(p.scheme) {
        return None;
    }
    if scheme == "file" && (!p.user.is_empty() || !p.pass.is_empty() || p.port.is_some()) {
        return None;
    }
    if p.scheme == "file" && p.host == Some("") {
        return None;
    }
    Parts { scheme: &scheme, ..p }.parse()
}

/// Whether the URL can have a username, password or port.
fn has_credentials_room(u: &Url) -> bool {
    !matches!(u.host_str(), None | Some("")) && u.scheme() != "file"
}

pub(crate) fn username(u: &Url, v: &str) -> Option<Url> {
    if !has_credentials_room(u) {
        return None;
    }
    let user = encoded(v, EncodeSet::USERINFO);
    Parts { user: &user, ..Parts::of(u) }.parse()
}

pub(crate) fn password(u: &Url, v: &str) -> Option<Url> {
    if !has_credentials_room(u) {
        return None;
    }
    let pass = encoded(v, EncodeSet::USERINFO);
    Parts { pass: &pass, ..Parts::of(u) }.parse()
}

/// The host and the port after it, up to where the host state stops; a
/// `file` URL's host state takes no port.
fn split_host(u: &Url, v: &str) -> (String, Option<String>) {
    let special = is_special(u.scheme());
    let end = v.find(|c| matches!(c, '/' | '?' | '#') || (special && c == '\\')).unwrap_or(v.len());
    let v = &v[..end];
    if u.scheme() == "file" {
        return (v.to_owned(), None);
    }
    let mut brackets = false;
    for (i, c) in v.char_indices() {
        match c {
            '[' => brackets = true,
            ']' => brackets = false,
            ':' if !brackets => return (v[..i].to_owned(), Some(v[i + 1..].to_owned())),
            _ => {}
        }
    }
    (v.to_owned(), None)
}

fn with_host(u: &Url, host: &str) -> Option<Url> {
    let p = Parts::of(u);
    if u.cannot_be_a_base() || host.contains('@') {
        return None;
    }
    if host.is_empty() && (is_special(p.scheme) || !p.user.is_empty() || !p.pass.is_empty() || p.port.is_some()) {
        return None;
    }
    let host = if p.scheme == "file" && host.eq_ignore_ascii_case("localhost") { "" } else { host };
    Parts { host: Some(host), ..p }.parse()
}

/// The host setter: a port after the host is set too when it is one,
/// the host staying set when it is not.
pub(crate) fn host(u: &Url, v: &str) -> Option<Url> {
    let (h, port_part) = split_host(u, &clean(v));
    let u = with_host(u, &h)?;
    Some(match port_part {
        Some(p) if !p.is_empty() => port(&u, &p).unwrap_or(u),
        _ => u,
    })
}

pub(crate) fn hostname(u: &Url, v: &str) -> Option<Url> {
    let (h, port_part) = split_host(u, &clean(v));
    if port_part.is_some() {
        return None;
    }
    with_host(u, &h)
}

pub(crate) fn port(u: &Url, v: &str) -> Option<Url> {
    if !has_credentials_room(u) {
        return None;
    }
    let v = clean(v);
    let p = Parts::of(u);
    if v.is_empty() {
        return Parts { port: None, ..p }.parse();
    }
    let digits = &v[..v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len())];
    if digits.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for d in digits.bytes() {
        n = n * 10 + u32::from(d - b'0');
        if n > u32::from(u16::MAX) {
            return None;
        }
    }
    Parts { port: Some(n as u16), ..p }.parse()
}

pub(crate) fn pathname(u: &Url, v: &str) -> Option<Url> {
    if u.cannot_be_a_base() {
        return None;
    }
    let p = Parts::of(u);
    let special = is_special(p.scheme);
    let v = encoded(&clean(v), EncodeSet::PATH);
    let path = match () {
        _ if v.is_empty() && !special && p.host.is_some() => String::new(),
        _ if v.starts_with('/') || (special && v.starts_with('\\')) => v,
        _ => format!("/{v}"),
    };
    Parts { path: &path, ..p }.parse()
}

pub(crate) fn search(u: &Url, v: &str) -> Option<Url> {
    let p = Parts::of(u);
    let v = clean(v);
    let v = v.strip_prefix('?').unwrap_or(&v);
    if v.is_empty() {
        return Parts { query: None, ..p }.parse();
    }
    let set = if is_special(p.scheme) { EncodeSet::SPECIAL_QUERY } else { EncodeSet::QUERY };
    let q = encoded(v, set);
    Parts { query: Some(&q), ..p }.parse()
}

/// The query of a URL set to `q`, already form-urlencoded.
pub(crate) fn with_query(u: &Url, q: Option<&str>) -> Option<Url> {
    Parts { query: q, ..Parts::of(u) }.parse()
}

pub(crate) fn hash(u: &Url, v: &str) -> Option<Url> {
    let p = Parts::of(u);
    let v = clean(v);
    let v = v.strip_prefix('#').unwrap_or(&v);
    if v.is_empty() {
        return Parts { fragment: None, ..p }.parse();
    }
    let f = encoded(v, EncodeSet::FRAGMENT);
    Parts { fragment: Some(&f), ..p }.parse()
}
