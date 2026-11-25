//! An input's value as the value sanitization algorithm of its type
//! leaves it, and the directionality `dirname` reports.

use crate::node::{ancestors, attr, is_space, keyword};
use madcawl_dom::{Dom, NodeId};
use madcawl_str::atom;

fn strip_newlines(s: &str) -> String {
    s.chars().filter(|&c| c != '\n' && c != '\r').collect()
}

/// A valid floating-point number (HTML 2.3.4.3).
fn is_float(s: &str) -> bool {
    let b = s.strip_prefix('-').unwrap_or(s).as_bytes();
    let digits = |b: &[u8]| b.iter().take_while(|c| c.is_ascii_digit()).count();
    let int = digits(b);
    let mut i = int;
    let mut frac = 0;
    if b.get(i) == Some(&b'.') {
        frac = digits(&b[i + 1..]);
        if frac == 0 {
            return false;
        }
        i += 1 + frac;
    }
    if int + frac == 0 {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let e = digits(&b[i..]);
        if e == 0 {
            return false;
        }
        i += e;
    }
    i == b.len()
}

fn float_attr(dom: &Dom, n: NodeId, a: madcawl_str::Atom) -> Option<f64> {
    attr(dom, n, a).filter(|v| is_float(v)).and_then(|v| v.parse().ok())
}

/// A range's value: a valid number clamped to its range, else the
/// midpoint (HTML 4.10.5.1.13); step mismatch is not corrected.
fn range(dom: &Dom, n: NodeId, v: &str) -> String {
    let min = float_attr(dom, n, atom!("min")).unwrap_or(0.0);
    let max = float_attr(dom, n, atom!("max")).unwrap_or(100.0).max(min);
    let x = if is_float(v) { v.parse::<f64>().unwrap_or(min).clamp(min, max) } else { min + (max - min) / 2.0 };
    format!("{x}")
}

fn color(v: &str) -> String {
    let ok = v.len() == 7 && v.starts_with('#') && v[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if ok { v.to_ascii_lowercase() } else { "#000000".to_owned() }
}

/// `raw` as the input's type sanitizes it. Date and time types keep
/// their value unchecked.
pub(crate) fn sanitize(dom: &Dom, n: NodeId, ty: &str, raw: String) -> String {
    match ty {
        "text" | "search" | "tel" | "password" => strip_newlines(&raw),
        "url" => strip_newlines(&raw).trim_matches(is_space).to_owned(),
        "email" if dom.attribute(n, atom!("multiple")).is_some() => {
            let parts: Vec<String> = raw.split(',').map(|p| p.trim_matches(is_space).to_owned()).collect();
            parts.join(",")
        }
        "email" => strip_newlines(&raw).trim_matches(is_space).to_owned(),
        "number" if !is_float(&raw) => String::new(),
        "range" => range(dom, n, &raw),
        "color" => color(&raw),
        _ => raw,
    }
}

fn is_rtl_char(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08ff | 0xfb1d..=0xfdff | 0xfe70..=0xfefc | 0x10800..=0x10fff | 0x1e800..=0x1efff)
}

/// `ltr` or `rtl` from the nearest `dir` attribute; `auto` looks at the
/// first strong character of the value (letters of right-to-left scripts
/// by block, every other letter as left-to-right).
pub(crate) fn directionality(dom: &Dom, n: NodeId, value: &str) -> &'static str {
    for e in std::iter::once(n).chain(ancestors(dom, n)) {
        match keyword(dom, e, atom!("dir")).as_deref() {
            Some("rtl") => return "rtl",
            Some("ltr") => return "ltr",
            Some("auto") => {
                let first = value.chars().find(|&c| c.is_alphabetic());
                return if first.is_some_and(is_rtl_char) { "rtl" } else { "ltr" };
            }
            _ => {}
        }
    }
    "ltr"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_and_colors() {
        for ok in ["0", "-1", "1.5", ".5", "1e3", "2E-2", "-0.25e+1"] {
            assert!(is_float(ok), "{ok}");
        }
        for bad in ["", "-", "1.", "+1", "1e", " 1", "0x10", "1,5", "Infinity"] {
            assert!(!is_float(bad), "{bad}");
        }
        assert_eq!(color("#AbCdEf"), "#abcdef");
        assert_eq!(color("red"), "#000000");
    }
}
