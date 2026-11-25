//! The public suffix list (publicsuffix.org), built in: the rules of
//! both its ICANN and PRIVATE sections, wildcards and exceptions, looked
//! up by binary search over a generated table without allocating.
//!
//! Hosts are given in their ASCII form, lowercase, without a trailing
//! dot, as a parsed URL holds them: a label that is not ASCII is compared
//! in its Punycode form. A host with an empty label has no public suffix.

mod table;

pub use table::{COMMIT, VERSION};

use table::{EXCEPTION, RULE, RULES, WILDCARD};

fn flags(key: &str) -> u8 {
    RULES.binary_search_by(|(k, _)| (*k).cmp(key)).map_or(0, |i| RULES[i].1)
}

/// The public suffix of `host`, as a tail of it; a host the list says
/// nothing about has its last label as its public suffix.
pub fn public_suffix(host: &str) -> Option<&str> {
    if host.is_empty() || host.split('.').any(str::is_empty) {
        return None;
    }
    let mut longest = host.rfind('.').map_or(0, |i| i + 1);
    let mut parent = 0;
    let mut end = host.len();
    loop {
        let start = host[..end].rfind('.').map_or(0, |i| i + 1);
        let f = flags(&host[start..]);
        if f & EXCEPTION != 0 {
            // an exception wins over every other rule and drops its leftmost label
            let dot = host[start..].find('.').map_or(host.len(), |i| start + i + 1);
            return Some(&host[dot..]);
        }
        if f & RULE != 0 || parent & WILDCARD != 0 {
            longest = start;
        }
        if start == 0 {
            return Some(&host[longest..]);
        }
        parent = f;
        end = start - 1;
    }
}

/// Whether `domain` is itself a public suffix.
pub fn is_public_suffix(domain: &str) -> bool {
    public_suffix(domain).is_some_and(|s| s.len() == domain.len())
}

/// The public suffix and one label more (eTLD+1); `None` when `host` is a
/// public suffix itself.
pub fn registrable_domain(host: &str) -> Option<&str> {
    let suffix = public_suffix(host)?;
    let rest = host.len().checked_sub(suffix.len() + 1)?;
    let start = host[..rest].rfind('.').map_or(0, |i| i + 1);
    Some(&host[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_wildcards_and_exceptions() {
        assert_eq!(public_suffix("www.example.co.uk"), Some("co.uk"));
        assert_eq!(registrable_domain("a.b.example.co.uk"), Some("example.co.uk"));
        assert_eq!(registrable_domain("co.uk"), None);
        assert_eq!(public_suffix("evil.github.io"), Some("github.io"));
        assert!(is_public_suffix("github.io") && is_public_suffix("com") && is_public_suffix("localhost"));
        assert!(!is_public_suffix("example.com"));
        assert_eq!(public_suffix("a.b.c.kobe.jp"), Some("c.kobe.jp"));
        assert_eq!(public_suffix("www.city.kobe.jp"), Some("kobe.jp"));
        assert_eq!(public_suffix("a..com"), None);
        assert_eq!(public_suffix(""), None);
    }

    #[test]
    fn the_table_is_sorted_and_carries_its_version() {
        assert!(RULES.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(RULES.iter().all(|(k, f)| k.is_ascii() && *f != 0 && *f < 8));
        assert_eq!(COMMIT.len(), 40);
        assert!(!VERSION.is_empty());
    }
}
