//! What a requester may not ask for: Fetch's forbidden methods and
//! forbidden request headers, and its method normalization.

/// CONNECT, TRACE and TRACK, in any case. TRACE would echo the request
/// back, `Cookie` included, to whoever made it.
pub fn is_forbidden_method(method: &str) -> bool {
    ["CONNECT", "TRACE", "TRACK"].iter().any(|m| m.eq_ignore_ascii_case(method))
}

/// DELETE, GET, HEAD, OPTIONS, POST and PUT in upper case; any other
/// method as it was given.
pub fn normalize_method(method: &str) -> String {
    const NORMALIZED: &[&str] = &["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"];
    match NORMALIZED.iter().find(|m| m.eq_ignore_ascii_case(method)) {
        Some(m) => (*m).to_owned(),
        None => method.to_owned(),
    }
}

/// Request headers a requester may not set: Fetch's forbidden request
/// headers, and the ones this stack writes itself.
pub fn is_forbidden(name: &str, value: &[u8]) -> bool {
    const NAMES: &[&str] = &[
        "accept-charset", "accept-encoding", "access-control-request-headers",
        "access-control-request-method", "connection", "content-length", "cookie", "cookie2",
        "date", "dnt", "expect", "host", "keep-alive", "origin", "referer", "set-cookie", "te",
        "trailer", "transfer-encoding", "upgrade", "via",
    ];
    const OVERRIDES: &[&str] = &["x-http-method", "x-http-method-override", "x-method-override"];
    let lower = name.to_ascii_lowercase();
    if NAMES.contains(&lower.as_str()) || lower.starts_with("proxy-") || lower.starts_with("sec-") {
        return true;
    }
    // a method override naming a forbidden method would smuggle it past
    // the method check through a server that honours the override
    OVERRIDES.contains(&lower.as_str())
        && value
            .split(|&b| b == b',')
            .any(|m| is_forbidden_method(&String::from_utf8_lossy(m.trim_ascii())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods() {
        for m in ["CONNECT", "trace", "Track"] {
            assert!(is_forbidden_method(m), "{m}");
        }
        assert!(!is_forbidden_method("TRACES"));
        assert_eq!(normalize_method("post"), "POST");
        assert_eq!(normalize_method("delete"), "DELETE");
        assert_eq!(normalize_method("patch"), "patch");
        assert_eq!(normalize_method("Foo"), "Foo");
    }

    #[test]
    fn method_override_headers() {
        assert!(is_forbidden("X-HTTP-Method-Override", b"trace"));
        assert!(is_forbidden("x-method-override", b"GET, \tConnect "));
        assert!(is_forbidden("X-HTTP-Method", b"TRACK"));
        assert!(!is_forbidden("X-HTTP-Method-Override", b"PATCH"));
        assert!(!is_forbidden("X-Other", b"TRACE"));
        assert!(is_forbidden("Sec-Fetch-Mode", b"cors"));
    }
}
