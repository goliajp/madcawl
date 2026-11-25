//! The jar's cookies that outlive it, as bytes for a profile's file.
//!
//! Layout: the magic `MCCJ`, a format version (u16), two reserved bytes,
//! the body's length (u32) and its FNV-1a 64 checksum (u64), all
//! little-endian; then the body, a sequence of records, each a kind (u8),
//! a length (u32) and that many bytes. A reader skips record kinds it does
//! not know, so a later writer may add kinds without a new version.

use super::{Cookie, CookieJar, SameSite};
use std::collections::HashMap;
use super::parse::{MAX_ATTR, MAX_PAIR};

const MAGIC: &[u8; 4] = b"MCCJ";
const VERSION: u16 = 1;
const HEADER: usize = 20;
const KIND_COOKIE: u8 = 1;
/// more than a full jar at its default caps can take; a longer file is
/// refused before it is read
pub const MAX_FILE: usize = 32 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    Truncated,
    BadMagic,
    Version(u16),
    Checksum,
    Malformed,
    TooLarge,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Truncated => f.write_str("the file is cut short"),
            StoreError::BadMagic => f.write_str("not a cookie store"),
            StoreError::Version(v) => write!(f, "unknown format version {v}"),
            StoreError::Checksum => f.write_str("checksum mismatch"),
            StoreError::Malformed => f.write_str("a record is malformed"),
            StoreError::TooLarge => f.write_str("the file is over its size cap"),
        }
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn same_site_byte(s: SameSite) -> u8 {
    match s {
        SameSite::Default => 0,
        SameSite::None => 1,
        SameSite::Lax => 2,
        SameSite::Strict => 3,
    }
}

fn encode_cookie(c: &Cookie, out: &mut Vec<u8>) {
    out.push(KIND_COOKIE);
    let len_at = out.len();
    out.extend_from_slice(&[0; 4]);
    for s in [&c.name, &c.value, &c.domain, &c.path] {
        put_str(out, s);
    }
    out.push(u8::from(c.host_only) | u8::from(c.secure) << 1 | u8::from(c.http_only) << 2);
    out.push(same_site_byte(c.same_site));
    out.extend_from_slice(&c.expires.unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&c.created.to_le_bytes());
    out.extend_from_slice(&c.accessed.to_le_bytes());
    let len = (out.len() - len_at - 4) as u32;
    out[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], StoreError> {
        if self.0.len() < n {
            return Err(StoreError::Malformed);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, StoreError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn u64(&mut self) -> Result<u64, StoreError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")))
    }

    fn str(&mut self, max: usize) -> Result<String, StoreError> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(StoreError::Malformed);
        }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| StoreError::Malformed)
    }
}

fn decode_cookie(payload: &[u8]) -> Result<Cookie, StoreError> {
    let mut r = Reader(payload);
    let name = r.str(MAX_PAIR)?;
    let value = r.str(MAX_PAIR)?;
    let domain = r.str(MAX_ATTR)?;
    let path = r.str(MAX_ATTR)?;
    let flags = r.take(1)?[0];
    let same_site = match r.take(1)?[0] {
        1 => SameSite::None,
        2 => SameSite::Lax,
        3 => SameSite::Strict,
        _ => SameSite::Default,
    };
    let expires = r.u64()? as i64;
    let (created, accessed) = (r.u64()?, r.u64()?);
    if domain.is_empty() || name.len() + value.len() > MAX_PAIR {
        return Err(StoreError::Malformed);
    }
    Ok(Cookie {
        name,
        value,
        domain,
        host_only: flags & 1 != 0,
        path,
        expires: Some(expires),
        secure: flags & 2 != 0,
        http_only: flags & 4 != 0,
        same_site,
        created,
        accessed,
    })
}

/// The cookies of a store file, checked whole before any is used.
fn decode(bytes: &[u8]) -> Result<Vec<Cookie>, StoreError> {
    if bytes.len() > MAX_FILE {
        return Err(StoreError::TooLarge);
    }
    if bytes.len() < HEADER {
        return Err(StoreError::Truncated);
    }
    if &bytes[..4] != MAGIC {
        return Err(StoreError::BadMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != VERSION {
        return Err(StoreError::Version(version));
    }
    let len = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes")) as usize;
    let sum = u64::from_le_bytes(bytes[12..20].try_into().expect("eight bytes"));
    let body = &bytes[HEADER..];
    if body.len() < len {
        return Err(StoreError::Truncated);
    }
    if body.len() > len {
        return Err(StoreError::Malformed);
    }
    if fnv1a(body) != sum {
        return Err(StoreError::Checksum);
    }
    let mut r = Reader(body);
    let mut out = Vec::new();
    while !r.0.is_empty() {
        let kind = r.take(1)?[0];
        let n = r.u32()? as usize;
        let payload = r.take(n)?;
        if kind == KIND_COOKIE {
            out.push(decode_cookie(payload)?);
        }
    }
    Ok(out)
}

impl CookieJar {
    /// Calls `f` whenever a cookie that outlives the jar is stored,
    /// replaced, deleted or evicted; `f` runs with the jar locked.
    pub fn set_on_change(&mut self, f: impl Fn() + Send + 'static) {
        self.on_change = Some(Box::new(f));
    }

    pub(super) fn changed(&self, persistent: bool) {
        if persistent && let Some(f) = &self.on_change {
            f();
        }
    }

    /// Writes the store file's bytes: every cookie with an expiry that
    /// has not passed at `now`. Session cookies are left out.
    pub fn encode_persistent(&self, now: i64, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&[0; 2 + 4 + 8]);
        let keep = |c: &&Cookie| c.expires.is_some_and(|e| e >= now);
        for c in self.by_domain.values().flatten().filter(keep) {
            encode_cookie(c, out);
        }
        let body_len = (out.len() - HEADER) as u32;
        let sum = fnv1a(&out[HEADER..]);
        out[8..12].copy_from_slice(&body_len.to_le_bytes());
        out[12..20].copy_from_slice(&sum.to_le_bytes());
    }

    /// Adds the cookies of a store file, dropping those expired at `now`
    /// and staying within the jar's caps; the number added. A file that
    /// fails its checks adds nothing. The file's clock values are only
    /// ordered, never adopted: the loaded cookies are numbered after the
    /// jar's clock, by when they were made and when they were last used.
    pub fn load_persistent(&mut self, bytes: &[u8], now: i64) -> Result<usize, StoreError> {
        let mut cookies = decode(bytes)?;
        cookies.retain(|c| c.expires.is_some_and(|e| e >= now));
        let mut per_domain: HashMap<&str, usize> = HashMap::new();
        let mut keep = vec![false; cookies.len()];
        let mut newest: Vec<usize> = (0..cookies.len()).collect();
        newest.sort_by_key(|&i| std::cmp::Reverse(cookies[i].accessed));
        let mut kept = 0;
        for &i in &newest {
            let n = per_domain.entry(cookies[i].domain.as_str()).or_default();
            if *n < self.limits.per_domain && kept < self.limits.total {
                (*n, kept, keep[i]) = (*n + 1, kept + 1, true);
            }
        }
        let mut it = keep.iter();
        cookies.retain(|_| *it.next().expect("one flag per cookie"));
        self.renumber(&mut cookies);
        let before = self.total;
        for c in cookies {
            self.insert_loaded(c, now);
        }
        Ok(self.total.saturating_sub(before))
    }

    /// Gives `cookies` the clock values after the jar's, keeping their
    /// order by `created` and by `accessed`, and sorts them by `accessed`.
    fn renumber(&mut self, cookies: &mut [Cookie]) {
        let base = self.clock;
        cookies.sort_by_key(|c| c.created);
        for (k, c) in cookies.iter_mut().enumerate() {
            c.created = base + 1 + k as u64;
        }
        cookies.sort_by_key(|c| c.accessed);
        for (k, c) in cookies.iter_mut().enumerate() {
            c.accessed = base + 1 + k as u64;
        }
        self.clock = base + cookies.len() as u64;
    }

    /// Stores a loaded cookie in place of the one it matches; room is
    /// made only when the jar or its domain is at its cap.
    fn insert_loaded(&mut self, c: Cookie, now: i64) {
        let list = self.by_domain.entry(c.domain.clone()).or_default();
        if let Some(i) = list.iter().position(|o| o.name == c.name && o.path == c.path && o.host_only == c.host_only) {
            list.remove(i);
            self.total -= 1;
        }
        if list.len() >= self.limits.per_domain || self.total >= self.limits.total {
            self.make_room(&c.domain, now);
        }
        self.by_domain.entry(c.domain.clone()).or_default().push(c);
        self.total += 1;
    }
}

#[cfg(test)]
mod tests;
