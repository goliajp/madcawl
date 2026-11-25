//! Whether a server's certificate chain is trusted for a host name. The
//! verdict comes from Security.framework's `SecTrust` with the SSL policy,
//! so user and enterprise roots, revocation and certificate transparency
//! policy match Safari's. Accepted verdicts are kept per (leaf hash, host)
//! for at most an hour and never past the leaf's expiry, up to a cap.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ffi::{c_char, c_void};
use std::sync::{Mutex, PoisonError};
use torajs_tls::client::ServerCertVerifier;
use torajs_tls::torajs_crypto::sha2::sha256;
use torajs_tls::torajs_x509::{Cert, ServerName, TrustAnchor, UnixTime, Verifier};
use torajs_tls::{AlertDescription, Rejection};

type CFTypeRef = *const c_void;

#[repr(C)]
struct Callbacks {
    _private: [u8; 0],
}

const UTF8: u32 = 0x0800_0100;
const VERDICT_CAP: usize = 4096;
const VERDICT_LIFETIME: u64 = 3600;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: Callbacks;
    fn CFRelease(cf: CFTypeRef);
    fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, len: isize) -> CFTypeRef;
    fn CFArrayCreate(alloc: CFTypeRef, v: *const CFTypeRef, n: isize, cb: *const Callbacks) -> CFTypeRef;
    fn CFStringCreateWithBytes(alloc: CFTypeRef, b: *const u8, n: isize, enc: u32, ext: u8) -> CFTypeRef;
    fn CFErrorCopyDescription(err: CFTypeRef) -> CFTypeRef;
    fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, len: isize, enc: u32) -> u8;
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCertificateCreateWithData(alloc: CFTypeRef, data: CFTypeRef) -> CFTypeRef;
    fn SecPolicyCreateSSL(server: u8, host: CFTypeRef) -> CFTypeRef;
    fn SecTrustCreateWithCertificates(certs: CFTypeRef, policies: CFTypeRef, out: *mut CFTypeRef) -> i32;
    fn SecTrustEvaluateWithError(trust: CFTypeRef, error: *mut CFTypeRef) -> bool;
}

/// Owns one CoreFoundation reference.
struct Cf(CFTypeRef);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a +1 reference this value owns
            unsafe { CFRelease(self.0) };
        }
    }
}

fn cf_string(s: &str) -> Cf {
    // SAFETY: valid UTF-8 bytes and length
    Cf(unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0) })
}

fn describe(err: CFTypeRef) -> String {
    if err.is_null() {
        return "not trusted".into();
    }
    let _err = Cf(err);
    // SAFETY: `err` is a CFError
    let desc = Cf(unsafe { CFErrorCopyDescription(err) });
    let mut buf = [0 as c_char; 512];
    // SAFETY: buffer and its length
    let ok = unsafe { CFStringGetCString(desc.0, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
    if ok == 0 {
        return "not trusted".into();
    }
    // SAFETY: CFStringGetCString wrote a nul-terminated string
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
}

/// Asks `SecTrust` whether `chain` (leaf first) is good for `host`.
pub fn system_trusts(chain: &[&[u8]], host: &str) -> Result<(), String> {
    let mut certs = Vec::with_capacity(chain.len());
    for der in chain {
        // SAFETY: bytes and length of a slice
        let data = Cf(unsafe { CFDataCreate(std::ptr::null(), der.as_ptr(), der.len() as isize) });
        // SAFETY: `data` is a CFData
        let cert = Cf(unsafe { SecCertificateCreateWithData(std::ptr::null(), data.0) });
        if cert.0.is_null() {
            return Err("a certificate Security.framework cannot read".into());
        }
        certs.push(cert);
    }
    let refs: Vec<CFTypeRef> = certs.iter().map(|c| c.0).collect();
    // SAFETY: an array of live CF objects, retained by the array
    let array = Cf(unsafe {
        CFArrayCreate(std::ptr::null(), refs.as_ptr(), refs.len() as isize, &kCFTypeArrayCallBacks)
    });
    let name = cf_string(host);
    // SAFETY: `name` is a CFString
    let policy = Cf(unsafe { SecPolicyCreateSSL(1, name.0) });
    let mut trust: CFTypeRef = std::ptr::null();
    // SAFETY: array of certificates, one policy, valid out pointer
    let status = unsafe { SecTrustCreateWithCertificates(array.0, policy.0, &mut trust) };
    if status != 0 {
        return Err(format!("SecTrustCreateWithCertificates: {status}"));
    }
    let trust = Cf(trust);
    let mut err: CFTypeRef = std::ptr::null();
    // SAFETY: a SecTrust and an out pointer for a +1 CFError
    if unsafe { SecTrustEvaluateWithError(trust.0, &mut err) } {
        Ok(())
    } else {
        Err(describe(err))
    }
}

/// Where trust comes from: the system, or only the given root
/// certificates (DER), as a local test CA needs.
#[derive(Clone, Debug)]
pub enum TrustSource {
    System,
    Anchors(Vec<Vec<u8>>),
}

type VerdictKey = ([u8; 32], String);

/// Accepted verdicts and when each stops counting, at most `cap`; the
/// oldest goes first.
pub struct Verdicts {
    expires: HashMap<VerdictKey, u64>,
    order: VecDeque<VerdictKey>,
    cap: usize,
}

impl Verdicts {
    pub fn new(cap: usize) -> Self {
        Verdicts { expires: HashMap::new(), order: VecDeque::new(), cap }
    }

    pub fn len(&self) -> usize {
        self.expires.len()
    }

    pub fn is_empty(&self) -> bool {
        self.expires.is_empty()
    }

    /// Whether `key` was accepted and has not expired at `now`; an expired
    /// entry is dropped.
    pub fn holds(&mut self, key: &VerdictKey, now: u64) -> bool {
        match self.expires.get(key) {
            Some(&until) if now < until => true,
            Some(_) => {
                self.expires.remove(key);
                self.order.retain(|k| k != key);
                false
            }
            None => false,
        }
    }

    /// Remembers an accepted verdict from `now` until the earlier of the
    /// leaf's `not_after` and an hour on.
    pub fn insert(&mut self, key: VerdictKey, not_after: u64, now: u64) {
        let until = not_after.min(now.saturating_add(VERDICT_LIFETIME));
        if self.expires.insert(key.clone(), until).is_none() {
            self.order.push_back(key);
            if self.order.len() > self.cap
                && let Some(old) = self.order.pop_front()
            {
                self.expires.remove(&old);
            }
        }
    }
}

thread_local! {
    /// Why the system refused the last chain verified on this thread. The
    /// TLS layer's rejection carries only an alert, and the handshake runs
    /// on the thread that verifies, so it picks the reason up here.
    static LAST_REASON: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The reason the last rejection on this thread gave, taken.
pub fn take_rejection_reason() -> Option<String> {
    LAST_REASON.with(|r| r.borrow_mut().take())
}

fn set_rejection_reason(reason: String) {
    LAST_REASON.with(|r| *r.borrow_mut() = Some(reason));
}

pub struct Trust {
    source: TrustSource,
    verdicts: Mutex<Verdicts>,
}

impl Trust {
    pub fn new(source: TrustSource) -> Self {
        Trust { source, verdicts: Mutex::new(Verdicts::new(VERDICT_CAP)) }
    }

    fn verdicts(&self) -> std::sync::MutexGuard<'_, Verdicts> {
        self.verdicts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn cached_verdicts(&self) -> usize {
        self.verdicts().len()
    }
}

impl ServerCertVerifier for Trust {
    fn verify_server_cert<'a>(
        &self,
        end_entity: &Cert<'a>,
        intermediates: &[&'a [u8]],
        server_name: &ServerName<'_>,
        now: UnixTime,
    ) -> Result<(), Rejection> {
        let host = server_name.to_string();
        match &self.source {
            TrustSource::Anchors(roots) => {
                let anchors: Vec<TrustAnchor<'_>> =
                    roots.iter().filter_map(|r| TrustAnchor::from_cert_der(r).ok()).collect();
                Verifier::new(&anchors).verify_server_cert(end_entity, intermediates, server_name, now)?;
                Ok(())
            }
            TrustSource::System => {
                let key = (sha256(end_entity.der()), host);
                if self.verdicts().holds(&key, now.as_secs()) {
                    return Ok(());
                }
                let mut chain = vec![end_entity.der()];
                chain.extend_from_slice(intermediates);
                match system_trusts(&chain, &key.1) {
                    Ok(()) => {
                        self.verdicts().insert(key, end_entity.not_after().as_secs(), now.as_secs());
                        Ok(())
                    }
                    Err(reason) => {
                        set_rejection_reason(reason);
                        Err(Rejection::with_alert(AlertDescription::BadCertificate)
                            .expect("bad_certificate is an error alert"))
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> VerdictKey {
        ([n; 32], "example.com".into())
    }

    #[test]
    fn verdicts_expire_at_the_leaf_expiry_or_after_an_hour() {
        let mut v = Verdicts::new(2);
        v.insert(key(1), 10_000_000, 1000);
        assert!(v.holds(&key(1), 1000 + 3599));
        assert!(!v.holds(&key(1), 1000 + 3600), "an hour after it was stored");
        assert!(v.is_empty(), "the expired entry is dropped");
        v.insert(key(2), 1500, 1000);
        assert!(v.holds(&key(2), 1499));
        assert!(!v.holds(&key(2), 1500), "the leaf's notAfter comes first");
        v.insert(key(3), u64::MAX, 0);
        v.insert(key(4), u64::MAX, 0);
        v.insert(key(5), u64::MAX, 0);
        assert_eq!(v.len(), 2);
        assert!(!v.holds(&key(3), 1), "the oldest went at the cap");
    }
}
