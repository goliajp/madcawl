//! The Fetch Standard's rules an XMLHttpRequest checks before it asks
//! for a request and after a response comes: methods and header names
//! and values, the forbidden request headers (the network service drops
//! them too), CORS-safelisted request headers, the CORS check, which
//! response headers a page may see, and a response's MIME type.

/// A method or header name: one or more token characters.
pub(crate) fn is_token(b: &[u8]) -> bool {
    !b.is_empty() && b.iter().all(|&c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
}

fn is_http_space(c: u8) -> bool {
    matches!(c, b'\t' | b'\n' | b'\r' | b' ')
}

/// A header value with leading and trailing HTTP whitespace removed.
pub(crate) fn normalize(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|&c| !is_http_space(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|&c| !is_http_space(c)).map_or(start, |i| i + 1);
    &b[start..end]
}

/// A normalized value is valid with no NUL, CR or LF in it.
pub(crate) fn is_value(b: &[u8]) -> bool {
    !b.iter().any(|&c| matches!(c, 0 | b'\r' | b'\n'))
}

pub(crate) fn is_forbidden_method(m: &str) -> bool {
    ["CONNECT", "TRACE", "TRACK"].iter().any(|f| f.eq_ignore_ascii_case(m))
}

/// DELETE, GET, HEAD, OPTIONS, POST and PUT upper-cased; any other
/// method as written.
pub(crate) fn normalize_method(m: &str) -> String {
    const NORMALIZED: [&str; 6] = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"];
    NORMALIZED.iter().find(|n| n.eq_ignore_ascii_case(m)).map_or_else(|| m.to_owned(), |n| (*n).to_owned())
}

/// The Fetch Standard's forbidden request headers.
pub(crate) fn is_forbidden_header(name: &str, value: &[u8]) -> bool {
    const NAMES: [&str; 21] = [
        "accept-charset", "accept-encoding", "access-control-request-headers", "access-control-request-method", "connection",
        "content-length", "cookie", "cookie2", "date", "dnt", "expect", "host", "keep-alive", "origin", "referer", "set-cookie",
        "te", "trailer", "transfer-encoding", "upgrade", "via",
    ];
    let lower = name.to_ascii_lowercase();
    if NAMES.contains(&lower.as_str()) || lower.starts_with("proxy-") || lower.starts_with("sec-") {
        return true;
    }
    matches!(lower.as_str(), "x-http-method" | "x-http-method-override" | "x-method-override")
        && value.split(|&c| c == b',').any(|m| is_forbidden_method(&String::from_utf8_lossy(normalize(m))))
}

/// A request header a cross-origin request may carry without a preflight.
pub(crate) fn is_safelisted_request_header(name: &str, value: &[u8]) -> bool {
    if value.len() > 128 {
        return false;
    }
    let unsafe_byte = |c: &u8| (*c < 0x20 && *c != b'\t') || b"\"():<>?@[\\]{}\x7f".contains(c);
    match name.to_ascii_lowercase().as_str() {
        "accept" => !value.iter().any(unsafe_byte),
        "accept-language" | "content-language" => value.iter().all(|&c| c.is_ascii_alphanumeric() || b" *,-.;=".contains(&c)),
        "content-type" => {
            let (essence, _) = mime(&String::from_utf8_lossy(value));
            !value.iter().any(unsafe_byte) && matches!(essence.as_str(), "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain")
        }
        _ => false,
    }
}

/// The essence of a MIME type, lowercased, and its charset parameter
/// when it has one; the essence is empty when there is no type.
pub(crate) fn mime(s: &str) -> (String, Option<String>) {
    let mut parts = s.split(';');
    let essence = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let charset = parts.find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case("charset").then(|| v.trim().trim_matches('"').to_owned())
    });
    (essence, charset)
}

/// The value of header `name` in `headers`, the values of all with that
/// name joined by `, `.
pub(crate) fn header(headers: &[(String, Vec<u8>)], name: &str) -> Option<Vec<u8>> {
    let mut out: Option<Vec<u8>> = None;
    for (_, v) in headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case(name)) {
        match out.as_mut() {
            Some(o) => {
                o.extend_from_slice(b", ");
                o.extend_from_slice(v);
            }
            None => out = Some(v.clone()),
        }
    }
    out
}

/// Whether a CORS response lets `origin` read it: its
/// `Access-Control-Allow-Origin` is `origin`, or `*` when no credentials
/// went, and with credentials its `Access-Control-Allow-Credentials` is
/// `true`.
pub(crate) fn cors_allows(headers: &[(String, Vec<u8>)], origin: &str, credentials: bool) -> bool {
    let Some(allow) = header(headers, "access-control-allow-origin") else { return false };
    if allow == b"*" {
        return !credentials;
    }
    allow == origin.as_bytes() && (!credentials || header(headers, "access-control-allow-credentials").as_deref() == Some(b"true"))
}

/// The headers of a response a page may see: never `Set-Cookie`, and of a
/// CORS response only the safelisted ones and those it exposes.
pub(crate) fn visible(headers: Vec<(String, Vec<u8>)>, cors: bool, credentials: bool) -> Vec<(String, Vec<u8>)> {
    const SAFELISTED: [&str; 7] = ["cache-control", "content-language", "content-length", "content-type", "expires", "last-modified", "pragma"];
    let exposed: Vec<String> = header(&headers, "access-control-expose-headers")
        .map(|v| v.split(|&c| c == b',').map(|n| String::from_utf8_lossy(normalize(n)).to_ascii_lowercase()).collect())
        .unwrap_or_default();
    let any = !credentials && exposed.iter().any(|n| n == "*");
    headers
        .into_iter()
        .filter(|(n, _)| {
            let n = n.to_ascii_lowercase();
            n != "set-cookie" && n != "set-cookie2" && (!cors || any || SAFELISTED.contains(&n.as_str()) || exposed.contains(&n))
        })
        .collect()
}

/// `getAllResponseHeaders`: names lowercased and sorted, the values of a
/// name joined, a `name: value` line each, each ended by CRLF.
pub(crate) fn all_headers(headers: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut names: Vec<String> = headers.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
    names.sort();
    names.dedup();
    let mut out = Vec::new();
    for n in names {
        out.extend_from_slice(n.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(&header(headers, &n).unwrap_or_default());
        out.extend_from_slice(b"\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(list: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
        list.iter().map(|(n, v)| ((*n).to_owned(), v.as_bytes().to_vec())).collect()
    }

    #[test]
    fn names_values_and_methods() {
        assert!(is_token(b"X-Custom_1") && !is_token(b"") && !is_token(b"a b") && !is_token(b"a:b"));
        assert_eq!(normalize(b" \t a b \r\n"), b"a b");
        assert_eq!(normalize(b" \t "), b"");
        assert!(is_value(b"a b") && !is_value(b"a\0b") && !is_value(b"a\nb"));
        assert_eq!(normalize_method("post"), "POST");
        assert_eq!(normalize_method("patch"), "patch");
        assert!(is_forbidden_method("trace") && !is_forbidden_method("PATCH"));
    }

    #[test]
    fn forbidden_and_safelisted_request_headers() {
        for n in ["Cookie", "content-length", "Sec-Fetch-Site", "Proxy-Authorization", "Origin", "Referer"] {
            assert!(is_forbidden_header(n, b"x"), "{n}");
        }
        assert!(is_forbidden_header("X-HTTP-Method-Override", b"GET, trace"));
        assert!(!is_forbidden_header("X-HTTP-Method-Override", b"PATCH"));
        assert!(!is_forbidden_header("X-Requested-With", b"XMLHttpRequest"));
        assert!(is_safelisted_request_header("Content-Type", b"text/plain;charset=UTF-8"));
        assert!(!is_safelisted_request_header("Content-Type", b"application/json"));
        assert!(is_safelisted_request_header("Accept", b"*/*"));
        assert!(!is_safelisted_request_header("Accept-Language", b"en(x)"));
        assert!(!is_safelisted_request_header("X-Custom", b"1"));
    }

    #[test]
    fn the_cors_check_and_what_a_page_sees() {
        let r = h(&[("Access-Control-Allow-Origin", "https://a.test"), ("Access-Control-Allow-Credentials", "true")]);
        assert!(cors_allows(&r, "https://a.test", true));
        assert!(!cors_allows(&r, "https://b.test", false));
        let star = h(&[("Access-Control-Allow-Origin", "*")]);
        assert!(cors_allows(&star, "https://a.test", false) && !cors_allows(&star, "https://a.test", true));
        assert!(!cors_allows(&h(&[]), "https://a.test", false));
        let all = h(&[("Content-Type", "text/plain"), ("X-A", "1"), ("X-B", "2"), ("Set-Cookie", "s=1"), ("Access-Control-Expose-Headers", "x-a")]);
        let names = |v: Vec<(String, Vec<u8>)>| v.into_iter().map(|(n, _)| n).collect::<Vec<_>>().join(",");
        assert_eq!(names(visible(all.clone(), false, false)), "Content-Type,X-A,X-B,Access-Control-Expose-Headers");
        assert_eq!(names(visible(all, true, false)), "Content-Type,X-A");
    }

    #[test]
    fn all_headers_are_sorted_lowercased_and_joined() {
        let r = h(&[("X-B", "2"), ("content-type", "text/plain"), ("X-b", "3"), ("A", "")]);
        assert_eq!(all_headers(&r), b"a: \r\ncontent-type: text/plain\r\nx-b: 2, 3\r\n");
        assert_eq!(header(&r, "x-B").as_deref(), Some(b"2, 3".as_slice()));
        assert_eq!(mime("Text/HTML; Charset=\"Shift_JIS\""), ("text/html".to_owned(), Some("Shift_JIS".to_owned())));
        assert_eq!(mime(""), (String::new(), None));
    }
}
