//! What a response is called on disk: `Content-Disposition` (RFC 6266,
//! with RFC 8187's `filename*`), else the URL's last path segment, made
//! safe for one directory: no path, no leading dot, no control or
//! direction-changing characters, a bounded length, and a numbered suffix
//! on a collision.

use madcawl_net::Url;
use std::borrow::Cow;

const MAX_NAME: usize = 200;
const FALLBACK: &str = "download";

/// The parameters of a `Content-Disposition` value after its type,
/// split on `;` outside quoted strings.
fn params(v: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut parts = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (i, c) in v.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => {
                parts.push(&v[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&v[start..]);
    for p in parts.into_iter().skip(1) {
        let Some((k, val)) = p.split_once('=') else { continue };
        let val = val.trim();
        let val = match val.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            Some(q) => unquote(q),
            None => val.to_owned(),
        };
        out.push((k.trim().to_ascii_lowercase(), val));
    }
    out
}

fn unquote(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    let mut chars = q.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' { chars.next().unwrap_or('\\') } else { c });
    }
    out
}

/// The disposition type is `attachment`.
pub fn is_attachment(v: &[u8]) -> bool {
    let v = String::from_utf8_lossy(v);
    v.split(';').next().is_some_and(|t| t.trim().eq_ignore_ascii_case("attachment"))
}

/// RFC 8187 `charset'language'value`; UTF-8 and ISO-8859-1 are read.
fn ext_value(v: &str) -> Option<String> {
    let (charset, rest) = v.split_once('\'')?;
    let (_, value) = rest.split_once('\'')?;
    let bytes = alt_url::percent::decode(value.as_bytes());
    match charset.to_ascii_lowercase().as_str() {
        "utf-8" => Some(String::from_utf8_lossy(&bytes).into_owned()),
        "iso-8859-1" => Some(bytes.iter().map(|&b| char::from(b)).collect()),
        _ => None,
    }
}

/// The name a response asks for, before it is made safe.
pub fn suggested(disposition: Option<&[u8]>, url: &Url) -> String {
    if let Some(d) = disposition {
        let p = params(&String::from_utf8_lossy(d));
        let star = p.iter().find(|(k, _)| k == "filename*").and_then(|(_, v)| ext_value(v));
        if let Some(n) = star.or_else(|| p.into_iter().find(|(k, _)| k == "filename").map(|(_, v)| v)) {
            return n;
        }
    }
    let seg = url.path().rsplit('/').next().unwrap_or("");
    String::from_utf8_lossy(&alt_url::percent::decode(seg.as_bytes())).into_owned()
}

/// Marks that change the direction text is shown in, which would let a
/// name show another extension than the one it has.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

/// `name` as one file name: what follows the last path separator,
/// control characters, direction marks and the characters file systems
/// refuse replaced,
/// leading dots and spaces and trailing ones removed, at most
/// `MAX_NAME` bytes keeping a short extension.
pub fn sanitize(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String =
        base.chars().map(|c| if c.is_control() || is_bidi_control(c) || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*') { '_' } else { c }).collect();
    let trimmed = cleaned.trim_start_matches(['.', ' ']).trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return FALLBACK.to_owned();
    }
    if trimmed.len() <= MAX_NAME {
        return trimmed.to_owned();
    }
    let (stem, ext) = split_ext(trimmed);
    let ext = if ext.len() <= 16 { ext } else { "" };
    let mut cut = MAX_NAME - ext.len();
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{ext}", &stem[..cut])
}

/// The name and its extension with the dot, which may be empty.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    }
}

/// `name` with ` (k)` before its extension; `name` itself for 0.
pub fn numbered(name: &str, k: usize) -> Cow<'_, str> {
    if k == 0 {
        return Cow::Borrowed(name);
    }
    let (stem, ext) = split_ext(name);
    Cow::Owned(format!("{stem} ({k}){ext}"))
}

/// A navigation to this type shows a page; anything else is saved.
/// No type means the page is sniffed, and shown.
pub fn renders(content_type: Option<&[u8]>) -> bool {
    let Some(ct) = content_type else { return true };
    let ct = String::from_utf8_lossy(ct);
    let essence = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if essence.is_empty() || essence.starts_with("text/") || essence.ends_with("+xml") || essence.ends_with("+json") {
        return true;
    }
    matches!(
        essence.as_str(),
        "application/xhtml+xml"
            | "application/xml"
            | "application/json"
            | "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn names_from_the_disposition_or_the_url() {
        let u = url("http://h/files/report%20v2.pdf?x=1");
        assert_eq!(suggested(None, &u), "report v2.pdf");
        assert_eq!(suggested(Some(b"attachment"), &u), "report v2.pdf");
        assert_eq!(suggested(Some(b"attachment; filename=plain.txt"), &u), "plain.txt");
        assert_eq!(suggested(Some(br#"attachment; filename="a \"q\"; b.txt""#), &u), r#"a "q"; b.txt"#);
        let star = b"attachment; filename=\"fallback.txt\"; filename*=UTF-8''%E2%82%AC%20rates.txt";
        assert_eq!(suggested(Some(star), &u), "\u{20ac} rates.txt");
        assert_eq!(suggested(Some(b"attachment; filename*=iso-8859-1'en'%E9t%E9.txt"), &u), "\u{e9}t\u{e9}.txt");
        assert_eq!(suggested(None, &url("http://h/")), "");
        assert!(is_attachment(b"Attachment; filename=x"));
        assert!(!is_attachment(b"inline; filename=x"));
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("..\\..\\boot.ini"), "boot.ini");
        assert_eq!(sanitize(".bashrc"), "bashrc");
        assert_eq!(sanitize("..."), "download");
        assert_eq!(sanitize(""), "download");
        assert_eq!(sanitize("a\u{0}b\nc:d?.txt "), "a_b_c_d_.txt");
        assert_eq!(sanitize("invoice\u{202e}fdp.exe"), "invoice_fdp.exe");
        assert_eq!(sanitize("\u{feff}\u{200e}a\u{2066}b\u{2069}\u{200f}.txt"), "__a_b__.txt");
        let long = format!("{}.pdf", "\u{e9}".repeat(300));
        let s = sanitize(&long);
        assert!(s.len() <= MAX_NAME && s.ends_with(".pdf"), "{s}");
        assert_eq!(numbered("r.tar.gz", 2), "r.tar (2).gz");
        assert_eq!(numbered("README", 1), "README (1)");
    }

    #[test]
    fn what_a_navigation_shows() {
        for t in ["text/html; charset=utf-8", "text/plain", "image/svg+xml", "application/json", "IMAGE/PNG"] {
            assert!(renders(Some(t.as_bytes())), "{t}");
        }
        assert!(renders(None));
        for t in ["application/pdf", "application/octet-stream", "application/zip", "video/mp4"] {
            assert!(!renders(Some(t.as_bytes())), "{t}");
        }
    }
}
