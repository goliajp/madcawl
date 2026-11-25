//! Host name resolution through the system's dnssd (`DNSServiceGetAddrInfo`),
//! so VPN, split and encrypted DNS settings apply, and a bounded cache.

use crate::error::NetError;
use std::collections::HashMap;
use std::ffi::{CString, c_char, c_int, c_void};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

type ServiceRef = *mut c_void;
type Reply = unsafe extern "C" fn(
    ServiceRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const u8,
    u32,
    *mut c_void,
);

const FLAG_MORE_COMING: u32 = 0x1;
const FLAG_ADD: u32 = 0x2;
const FLAG_RETURN_INTERMEDIATES: u32 = 0x1000;
const PROTO_V4: u32 = 0x1;
const PROTO_V6: u32 = 0x2;
const ERR_NO_SUCH_RECORD: i32 = -65554;
const AF_INET: u8 = 2;
const AF_INET6: u8 = 30;
const POLLIN: i16 = 1;

// after the first family answers, how long to wait for the other; RFC 8305
// §3 waits 50 ms but then keeps adding late answers to a connection race,
// which a sequential connect cannot do, so this waits longer once
const SECOND_FAMILY_WAIT: Duration = Duration::from_millis(300);
const MAX_ADDRS: usize = 32;
const MAX_TTL: u32 = 300;

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn DNSServiceGetAddrInfo(
        sd: *mut ServiceRef,
        flags: u32,
        interface: u32,
        protocol: u32,
        host: *const c_char,
        reply: Reply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(sd: ServiceRef) -> c_int;
    fn DNSServiceProcessResult(sd: ServiceRef) -> i32;
    fn DNSServiceRefDeallocate(sd: ServiceRef);
    fn poll(fds: *mut PollFd, n: u32, timeout_ms: c_int) -> c_int;
}

#[derive(Default)]
struct Answers {
    addrs: Vec<IpAddr>,
    v4_done: bool,
    v6_done: bool,
    more_coming: bool,
    ttl: u32,
    error: Option<i32>,
}

fn sockaddr_ip(sa: *const u8) -> Option<IpAddr> {
    if sa.is_null() {
        return None;
    }
    // SAFETY: dnssd hands a sockaddr whose first byte is its length
    let len = unsafe { *sa } as usize;
    // SAFETY: the sockaddr is `len` bytes long
    let b = unsafe { std::slice::from_raw_parts(sa, len.max(2)) };
    match b[1] {
        AF_INET if b.len() >= 8 => Some(IpAddr::V4(Ipv4Addr::new(b[4], b[5], b[6], b[7]))),
        AF_INET6 if b.len() >= 24 => {
            let o: [u8; 16] = b[8..24].try_into().ok()?;
            Some(IpAddr::V6(Ipv6Addr::from(o)))
        }
        _ => None,
    }
}

fn family_of(sa: *const u8) -> Option<u8> {
    // SAFETY: as in sockaddr_ip, the family is the second byte
    (!sa.is_null()).then(|| unsafe { *sa.add(1) })
}

unsafe extern "C" fn on_reply(
    _sd: ServiceRef,
    flags: u32,
    _interface: u32,
    error: i32,
    _host: *const c_char,
    addr: *const u8,
    ttl: u32,
    context: *mut c_void,
) {
    // SAFETY: the context is the `Answers` that outlives the query
    let a = unsafe { &mut *context.cast::<Answers>() };
    a.more_coming = flags & FLAG_MORE_COMING != 0;
    let family = family_of(addr);
    match error {
        0 => {
            if flags & FLAG_ADD != 0
                && let Some(ip) = sockaddr_ip(addr)
            {
                if a.addrs.len() < MAX_ADDRS && !a.addrs.contains(&ip) {
                    a.addrs.push(ip);
                }
                a.ttl = if a.ttl == 0 { ttl } else { a.ttl.min(ttl) };
            }
        }
        ERR_NO_SUCH_RECORD => {}
        e => a.error = Some(e),
    }
    match family {
        Some(AF_INET) => a.v4_done = true,
        Some(AF_INET6) => a.v6_done = true,
        _ => {}
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub addrs: Vec<IpAddr>,
    pub ttl: u32,
    /// both families answered, so the result is worth caching
    pub complete: bool,
}

/// Resolves `host` to its addresses in the order answered, waiting at
/// most `timeout`.
pub fn resolve(host: &str, timeout: Duration) -> Result<Resolved, NetError> {
    let name = CString::new(host).map_err(|_| NetError::Dns("nul in host".into()))?;
    let mut answers = Answers::default();
    let mut sd: ServiceRef = std::ptr::null_mut();
    // SAFETY: valid out pointer, nul-terminated name, the callback matches
    // the C signature and the context lives until the ref is deallocated
    let err = unsafe {
        DNSServiceGetAddrInfo(
            &mut sd,
            FLAG_RETURN_INTERMEDIATES,
            0,
            PROTO_V4 | PROTO_V6,
            name.as_ptr(),
            on_reply,
            (&mut answers as *mut Answers).cast(),
        )
    };
    if err != 0 {
        return Err(NetError::Dns(format!("dnssd error {err}")));
    }
    let result = drive(sd, &mut answers, timeout);
    // SAFETY: `sd` came from a successful DNSServiceGetAddrInfo
    unsafe { DNSServiceRefDeallocate(sd) };
    result?;
    if answers.addrs.is_empty() {
        return Err(match answers.error {
            Some(e) => NetError::Dns(format!("{host}: dnssd error {e}")),
            None => NetError::Dns(format!("{host}: no addresses")),
        });
    }
    Ok(Resolved {
        complete: answers.v4_done && answers.v6_done,
        ttl: answers.ttl.min(MAX_TTL),
        addrs: answers.addrs,
    })
}

fn drive(sd: ServiceRef, a: &mut Answers, timeout: Duration) -> Result<(), NetError> {
    let start = Instant::now();
    let mut deadline = start + timeout;
    // SAFETY: `sd` is a live service ref
    let fd = unsafe { DNSServiceRefSockFD(sd) };
    loop {
        let done = a.v4_done && a.v6_done;
        if (done || a.error.is_some()) && !a.more_coming {
            return Ok(());
        }
        if !a.addrs.is_empty() && !a.more_coming {
            deadline = deadline.min(Instant::now() + SECOND_FAMILY_WAIT);
        }
        let now = Instant::now();
        if now >= deadline {
            return if a.addrs.is_empty() { Err(NetError::DnsTimeout) } else { Ok(()) };
        }
        let ms = (deadline - now).as_millis().clamp(1, i32::MAX as u128) as c_int;
        let mut p = PollFd { fd, events: POLLIN, revents: 0 };
        // SAFETY: one valid pollfd
        let n = unsafe { poll(&mut p, 1, ms) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(NetError::Dns(e.to_string()));
        }
        if n > 0 {
            // SAFETY: the socket is readable; this runs `on_reply`
            let err = unsafe { DNSServiceProcessResult(sd) };
            if err != 0 {
                return Err(NetError::Dns(format!("dnssd error {err}")));
            }
        }
    }
}

struct Entry {
    addrs: Vec<IpAddr>,
    expires: Instant,
    used: u64,
}

/// Resolved addresses by host name, at most `cap` hosts; when full the
/// least recently used entry goes.
pub struct DnsCache {
    map: Mutex<(HashMap<String, Entry>, u64)>,
    cap: usize,
}

impl DnsCache {
    pub fn new(cap: usize) -> Self {
        DnsCache { map: Mutex::new((HashMap::new(), 0)), cap: cap.max(1) }
    }

    pub fn len(&self) -> usize {
        self.map.lock().unwrap().0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The addresses of `host`: an IP literal as it is, otherwise from the
    /// cache while fresh, otherwise resolved and cached.
    pub fn lookup(&self, host: &str, timeout: Duration) -> Result<Vec<IpAddr>, NetError> {
        let literal = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
        if let Ok(ip) = literal.parse::<IpAddr>() {
            return Ok(vec![ip]);
        }
        {
            let mut g = self.map.lock().unwrap();
            let (map, clock) = &mut *g;
            *clock += 1;
            let now = *clock;
            if let Some(e) = map.get_mut(host) {
                if e.expires > Instant::now() {
                    e.used = now;
                    return Ok(e.addrs.clone());
                }
                map.remove(host);
            }
        }
        let r = resolve(host, timeout)?;
        if r.complete {
            self.insert(host, &r.addrs, r.ttl);
        }
        Ok(r.addrs)
    }

    fn insert(&self, host: &str, addrs: &[IpAddr], ttl: u32) {
        let mut g = self.map.lock().unwrap();
        let (map, clock) = &mut *g;
        *clock += 1;
        if map.len() >= self.cap && !map.contains_key(host) {
            let oldest = map.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                map.remove(&k);
            }
        }
        let expires = Instant::now() + Duration::from_secs(u64::from(ttl.max(1)));
        map.insert(host.to_owned(), Entry { addrs: addrs.to_vec(), expires, used: *clock });
    }
}
