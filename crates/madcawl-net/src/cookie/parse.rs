//! Reading a `Set-Cookie` value and the cookie date format (RFC 6265bis
//! §5.1.1, §5.6).

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SameSite {
    /// no attribute: treated as `Lax`
    #[default]
    Default,
    None,
    Lax,
    Strict,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetCookie {
    pub name: String,
    pub value: String,
    /// unix seconds from `Expires`
    pub expires: Option<i64>,
    /// seconds from `Max-Age`, which wins over `Expires`
    pub max_age: Option<i64>,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
}

/// The most bytes of a name and value together, and of one attribute value.
pub const MAX_PAIR: usize = 4096;
pub const MAX_ATTR: usize = 1024;

fn has_ctl(s: &str) -> bool {
    s.bytes().any(|b| (b < 0x20 && b != b'\t') || b == 0x7f)
}

/// Parses one `Set-Cookie` field value; `None` when the cookie is to be
/// ignored.
pub fn parse(line: &str) -> Option<SetCookie> {
    let (pair, attrs) = line.split_once(';').unwrap_or((line, ""));
    let (name, value) = match pair.split_once('=') {
        Some((n, v)) => (n.trim_matches([' ', '\t']), v.trim_matches([' ', '\t'])),
        None => ("", pair.trim_matches([' ', '\t'])),
    };
    if (name.is_empty() && value.is_empty()) || name.len() + value.len() > MAX_PAIR {
        return None;
    }
    if has_ctl(name) || has_ctl(value) {
        return None;
    }
    let mut c = SetCookie { name: name.into(), value: value.into(), ..SetCookie::default() };
    for attr in attrs.split(';') {
        let (k, v) = attr.split_once('=').unwrap_or((attr, ""));
        let (k, v) = (k.trim_matches([' ', '\t']), v.trim_matches([' ', '\t']));
        if v.len() > MAX_ATTR {
            continue;
        }
        match k.to_ascii_lowercase().as_str() {
            "expires" => {
                if let Some(t) = parse_date(v) {
                    c.expires = Some(t);
                }
            }
            "max-age" => {
                let digits = v.strip_prefix('-').unwrap_or(v);
                if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    let n = digits.parse::<i64>().unwrap_or(i64::MAX);
                    c.max_age = Some(if v.starts_with('-') { -1 } else { n });
                }
            }
            "domain" if !v.is_empty() => {
                c.domain = Some(v.strip_prefix('.').unwrap_or(v).to_ascii_lowercase());
            }
            "path" => c.path = v.starts_with('/').then(|| v.to_owned()),
            "secure" => c.secure = true,
            "httponly" => c.http_only = true,
            "samesite" => {
                c.same_site = match v.to_ascii_lowercase().as_str() {
                    "none" => SameSite::None,
                    "lax" => SameSite::Lax,
                    "strict" => SameSite::Strict,
                    _ => SameSite::Default,
                }
            }
            _ => {}
        }
    }
    Some(c)
}

fn is_delim(b: u8) -> bool {
    matches!(b, 0x09 | 0x20..=0x2f | 0x3b..=0x40 | 0x5b..=0x60 | 0x7b..=0x7e)
}

fn leading_digits(t: &str, min: usize, max: usize) -> Option<u32> {
    let n = t.bytes().take_while(u8::is_ascii_digit).count();
    (min..=max).contains(&n).then(|| t[..n].parse().ok()).flatten()
}

fn time(t: &str) -> Option<(u32, u32, u32)> {
    let mut parts = t.splitn(3, ':');
    let h = leading_digits(parts.next()?, 1, 2)?;
    let m = leading_digits(parts.next()?, 1, 2)?;
    let s = leading_digits(parts.next()?, 1, 2)?;
    Some((h, m, s))
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// The cookie date algorithm; unix seconds, or `None` for a date it
/// refuses.
pub fn parse_date(s: &str) -> Option<i64> {
    let (mut hms, mut day, mut month, mut year) = (None, None, None, None);
    for tok in s.split(|c: char| c.is_ascii() && is_delim(c as u8)).filter(|t| !t.is_empty()) {
        if hms.is_none()
            && let Some(t) = time(tok)
        {
            hms = Some(t);
        } else if day.is_none()
            && let Some(d) = leading_digits(tok, 1, 2)
        {
            day = Some(d);
        } else if month.is_none()
            && let Some(m) = tok.get(..3).and_then(|p| MONTHS.iter().position(|m| p.eq_ignore_ascii_case(m)))
        {
            month = Some(m as u32 + 1);
        } else if year.is_none()
            && let Some(y) = leading_digits(tok, 2, 4)
        {
            year = Some(y);
        }
    }
    let (h, mi, sec) = hms?;
    let (day, month, mut year) = (day?, month?, year?);
    if (70..=99).contains(&year) {
        year += 1900;
    } else if year <= 69 {
        year += 2000;
    }
    if !(1..=31).contains(&day) || year < 1601 || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    Some(days_from_civil(i64::from(year), month, day) * 86400 + i64::from(h * 3600 + mi * 60 + sec))
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_in_the_usual_and_odd_forms() {
        assert_eq!(parse_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_date("Wed, 21 Oct 2015 07:28:00 GMT"), Some(1_445_412_480));
        assert_eq!(parse_date("Wednesday, 21-Oct-15 07:28:00 GMT"), Some(1_445_412_480));
        assert_eq!(parse_date("21 Oct 2015 7:28:0"), Some(1_445_412_480));
        assert_eq!(parse_date("Oct 2015 07:28:00"), None);
        assert_eq!(parse_date("32 Oct 2015 07:28:00"), None);
    }

    #[test]
    fn attributes_are_read_and_bad_pairs_dropped() {
        let c = parse("sid=abc; Path=/app; Domain=.Example.COM; Secure; HttpOnly; SameSite=Strict; Max-Age=60").unwrap();
        assert_eq!((c.name.as_str(), c.value.as_str()), ("sid", "abc"));
        assert_eq!(c.domain.as_deref(), Some("example.com"));
        assert_eq!(c.path.as_deref(), Some("/app"));
        assert!(c.secure && c.http_only);
        assert_eq!(c.same_site, SameSite::Strict);
        assert_eq!(c.max_age, Some(60));
        assert_eq!(parse("noequals").unwrap().value, "noequals");
        assert!(parse("=").is_none());
        assert!(parse("a=b\u{1}").is_none());
        assert_eq!(parse("a=b; Path=rel").unwrap().path, None);
        assert_eq!(parse("a=b; Max-Age=-5").unwrap().max_age, Some(-1));
    }
}
