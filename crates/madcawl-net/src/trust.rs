//! Whether a server's certificate chain is trusted for a host name. The
//! verdict comes from Security.framework's `SecTrust` with the SSL policy,
//! so user and enterprise roots, revocation and certificate transparency
//! policy match Safari's. Accepted verdicts are kept per (leaf hash, host)
//! for the life of the process, up to a cap.

#![allow(non_snake_case)]

use std::collections::{HashSet, VecDeque};
use std::ffi::{c_char, c_void};
use std::sync::Mutex;
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

pub struct Trust {
    source: TrustSource,
    verdicts: Mutex<(HashSet<VerdictKey>, VecDeque<VerdictKey>)>,
}

impl Trust {
    pub fn new(source: TrustSource) -> Self {
        Trust { source, verdicts: Mutex::new((HashSet::new(), VecDeque::new())) }
    }

    pub fn cached_verdicts(&self) -> usize {
        self.verdicts.lock().unwrap().0.len()
    }

    fn remember(&self, key: VerdictKey) {
        let mut g = self.verdicts.lock().unwrap();
        let (set, order) = &mut *g;
        if set.insert(key.clone()) {
            order.push_back(key);
            if order.len() > VERDICT_CAP
                && let Some(old) = order.pop_front()
            {
                set.remove(&old);
            }
        }
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
                if self.verdicts.lock().unwrap().0.contains(&key) {
                    return Ok(());
                }
                let mut chain = vec![end_entity.der()];
                chain.extend_from_slice(intermediates);
                match system_trusts(&chain, &key.1) {
                    Ok(()) => {
                        self.remember(key);
                        Ok(())
                    }
                    Err(_) => Err(Rejection::with_alert(AlertDescription::BadCertificate)
                        .expect("bad_certificate is an error alert")),
                }
            }
        }
    }
}
