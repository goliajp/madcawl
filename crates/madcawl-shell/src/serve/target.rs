//! What an `open` names: an absolute path or a `file:` URL, to a local
//! file under one of the allowed directories (none allowed, nothing
//! opens). Every other scheme is refused; pages come from local files
//! only.

use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum TargetError {
    /// A URL of another scheme than `file`, by its scheme
    Scheme(String),
    /// A `file:` URL naming another host
    Host,
    Relative,
    /// Not under any allowed directory
    NotAllowed,
    Missing(String),
}

impl TargetError {
    pub fn message(&self) -> String {
        match self {
            TargetError::Scheme(s) => format!(
                "only local files can be opened: `{s}:` URLs are refused; give an absolute path or a file:// URL"
            ),
            TargetError::Host => "a file:// URL must name no host (or localhost)".into(),
            TargetError::Relative => "the path must be absolute".into(),
            TargetError::NotAllowed => {
                "the file is outside the directories this host may open".into()
            }
            TargetError::Missing(e) => format!("cannot open the file: {e}"),
        }
    }

    pub fn code(&self) -> u64 {
        match self {
            TargetError::Scheme(_) | TargetError::Host => super::code::REFUSED,
            TargetError::NotAllowed => super::code::NOT_ALLOWED,
            TargetError::Relative => super::code::BAD_REQUEST,
            TargetError::Missing(_) => super::code::IO,
        }
    }
}

/// The scheme `s` starts with, if it starts with one (`x:` with `x` of
/// letters, digits, `+`, `-`, `.`, the first a letter).
pub fn scheme(s: &str) -> Option<&str> {
    let end = s.find(':')?;
    let name = &s[..end];
    let mut b = name.bytes();
    let first = b.next()?;
    let ok = first.is_ascii_alphabetic()
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'));
    ok.then_some(name)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The canonical path `target` names, and the `file:` URL it is known
/// by.
pub fn resolve(target: &str, allow: &[PathBuf]) -> Result<(PathBuf, String), TargetError> {
    let target = target.trim();
    let path = match scheme(target) {
        Some(s) if s.eq_ignore_ascii_case("file") => {
            let rest = &target[s.len() + 1..];
            let rest = rest.split(['?', '#']).next().unwrap_or("");
            let rest = match rest.strip_prefix("//") {
                Some(r) => {
                    let slash = r.find('/').unwrap_or(r.len());
                    if !matches!(&r[..slash], "" | "localhost") {
                        return Err(TargetError::Host);
                    }
                    &r[slash..]
                }
                None => rest,
            };
            percent_decode(rest).ok_or(TargetError::Missing("a bad %-escape".into()))?
        }
        Some(s) if s.len() > 1 => return Err(TargetError::Scheme(s.to_ascii_lowercase())),
        _ => target.to_owned(),
    };
    let path = Path::new(&path);
    if !path.is_absolute() {
        return Err(TargetError::Relative);
    }
    let path = path
        .canonicalize()
        .map_err(|e| TargetError::Missing(e.to_string()))?;
    if !allow.iter().any(|a| path.starts_with(a)) {
        return Err(TargetError::NotAllowed);
    }
    let url = file_url(&path);
    Ok((path, url))
}

/// The `file:` URL of an absolute path, with control characters, spaces,
/// `%`, `?` and `#` percent-encoded, so a file's name cannot break a
/// line it is written in.
pub fn file_url(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    let mut out = String::from("file://");
    let bytes = path.as_os_str().as_bytes();
    let mut run = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b < 0x21 || b == 0x7f || matches!(b, b'%' | b'?' | b'#') || b >= 0x80 {
            out.push_str(&String::from_utf8_lossy(&bytes[run..i]));
            out.push_str(&format!("%{b:02X}"));
            run = i + 1;
        }
    }
    out.push_str(&String::from_utf8_lossy(&bytes[run..]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_schemes_are_refused_and_file_urls_read() {
        for u in [
            "https://example.com/",
            "http://x",
            "data:text/html,hi",
            "javascript:1",
        ] {
            assert!(
                matches!(resolve(u, &[]), Err(TargetError::Scheme(_))),
                "{u}"
            );
        }
        assert_eq!(resolve("file://host/x", &[]), Err(TargetError::Host));
        assert_eq!(resolve("a/b.html", &[]), Err(TargetError::Relative));
        let dir = std::env::temp_dir().canonicalize().unwrap();
        let f = dir.join(format!("madcawl target {}.html", std::process::id()));
        std::fs::write(&f, "<p>x").unwrap();
        let url = format!("file://{}", f.display()).replace(' ', "%20");
        let here = [dir.clone()];
        let (p, u) = resolve(&url, &here).unwrap();
        assert_eq!(p, f);
        assert_eq!(u, url);
        assert_eq!(resolve(f.to_str().unwrap(), &here).unwrap().0, f);
        assert_eq!(
            resolve(f.to_str().unwrap(), &[]),
            Err(TargetError::NotAllowed)
        );
        let elsewhere = vec![PathBuf::from("/nonexistent-root")];
        assert_eq!(
            resolve(f.to_str().unwrap(), &elsewhere),
            Err(TargetError::NotAllowed)
        );
        assert_eq!(resolve(f.to_str().unwrap(), &[dir]).unwrap().0, f);
        std::fs::remove_file(&f).unwrap();
        let odd = Path::new("/a b/x\n<</page-content-1>>%?#é.html");
        assert_eq!(
            file_url(odd),
            "file:///a%20b/x%0A<</page-content-1>>%25%3F%23%C3%A9.html"
        );
    }
}
