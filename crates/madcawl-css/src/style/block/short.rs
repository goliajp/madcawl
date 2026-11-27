//! A shorthand's value written from its longhands' serialized values, as
//! CSSOM gives it for `getPropertyValue` and `cssText`: the shortest
//! form that sets the same longhands, or nothing when the shorthand
//! cannot say what they hold.

use super::super::shorthand::Short;
use super::entry::Entry;

/// The most longhands a shorthand sets: `border`'s.
pub(crate) const MAX_LONGHANDS: usize = 12;

const WIDE: &[&str] = &["initial", "inherit", "unset", "revert", "revert-layer"];

fn join(parts: &[&str], sep: &str, out: &mut String) {
    for (k, p) in parts.iter().enumerate() {
        if k > 0 {
            out.push_str(sep);
        }
        out.push_str(p);
    }
}

/// Four sides as one to four values.
fn sides(t: [&str; 4], out: &mut String) {
    let [a, b, c, d] = t;
    let n = if b != d {
        4
    } else if a != c {
        3
    } else if a != b {
        2
    } else {
        1
    };
    join(&t[..n], " ", out);
}

fn pair(a: &str, b: &str, out: &mut String) {
    let both = [a, b];
    join(&both[..if a == b { 1 } else { 2 }], " ", out);
}

/// Width, style and color, each left out where it is the initial one.
fn border_side(w: &str, s: &str, c: &str, out: &mut String) {
    let start = out.len();
    let parts = [
        (w, ["medium", "3px"]),
        (s, ["none", "none"]),
        (c, ["currentcolor", "currentcolor"]),
    ];
    for (v, init) in parts {
        if !init.contains(&v) {
            if out.len() > start {
                out.push(' ');
            }
            out.push_str(v);
        }
    }
    if out.len() == start {
        out.push_str("none");
    }
}

/// A grid line's value names a line, which the end of the pair takes
/// when it is left out.
fn custom_ident(v: &str) -> bool {
    let b = v.as_bytes();
    let number = b.first().is_some_and(u8::is_ascii_digit)
        || b.len() > 1 && b[0] == b'-' && b[1].is_ascii_digit();
    v != "auto" && !v.starts_with("span") && !number
}

fn default_end(start: &str) -> &str {
    if custom_ident(start) { start } else { "auto" }
}

fn grid_area(t: [&str; 4], out: &mut String) {
    let [rs, cs, re, ce] = t;
    let mut n = 4;
    if ce == default_end(cs) {
        n = 3;
        if re == default_end(rs) {
            n = 2;
            if cs == default_end(rs) {
                n = 1;
            }
        }
    }
    join(&t[..n], " / ", out);
}

fn white_space(c: &str, w: &str, out: &mut String) {
    out.push_str(match (c, w) {
        ("collapse", "wrap") => "normal",
        ("collapse", "nowrap") => "nowrap",
        ("preserve", "nowrap") => "pre",
        ("preserve", "wrap") => "pre-wrap",
        ("preserve-breaks", "wrap") => "pre-line",
        ("break-spaces", "wrap") => "break-spaces",
        _ => return join(&[c, w], " ", out),
    });
}

fn font(t: &[&str], out: &mut String) {
    let [style, weight, size, lh, family] = t else {
        return;
    };
    if *style != "normal" {
        out.push_str(style);
        out.push(' ');
    }
    if !matches!(*weight, "normal" | "400") {
        out.push_str(weight);
        out.push(' ');
    }
    out.push_str(size);
    if *lh != "normal" {
        out.push_str(" / ");
        out.push_str(lh);
    }
    out.push(' ');
    out.push_str(family);
}

/// `grid-template` from its rows, columns and areas; only the forms
/// without areas are written.
fn template(t: &[&str], out: &mut String) {
    let [rows, cols, areas] = t else { return };
    match (*rows, *cols, *areas) {
        ("none", "none", "none") => out.push_str("none"),
        (r, c, "none") => join(&[r, c], " / ", out),
        _ => {}
    }
}

/// The value of `sh` from its longhands, in the order `longhands` gives
/// them, appended to `out`; nothing when there is none.
pub(crate) fn write_short(sh: Short, ps: &[&Entry], out: &mut String) {
    if let Some((s, raw)) = &ps[0].pending {
        if *s == sh && ps.iter().all(|p| p.pending == ps[0].pending) {
            out.push_str(raw);
        }
        return;
    }
    if ps.iter().any(|p| p.pending.is_some()) {
        return;
    }
    let mut all = [""; MAX_LONGHANDS];
    let t = &mut all[..ps.len()];
    for (v, p) in t.iter_mut().zip(ps) {
        *v = &p.text;
    }
    let t = &*t;
    if t.iter().any(|v| WIDE.contains(v)) {
        if t.iter().all(|v| *v == t[0]) {
            out.push_str(t[0]);
        }
        return;
    }
    write_values(sh, t, out);
}

/// The value of `sh` from longhand values none of which is pending or a
/// CSS-wide keyword.
fn write_values(sh: Short, t: &[&str], out: &mut String) {
    match sh {
        Short::Box(_) => sides([t[0], t[1], t[2], t[3]], out),
        Short::Pair(..)
        | Short::BorderFlowPart(..)
        | Short::Gap
        | Short::Overflow
        | Short::PlaceContent
        | Short::PlaceItems
        | Short::PlaceSelf => pair(t[0], t[1], out),
        Short::BorderSide(_) | Short::BorderFlow(_, Some(_)) => border_side(t[0], t[1], t[2], out),
        Short::Border => {
            if t.chunks(4).all(|g| g.iter().all(|v| *v == g[0])) {
                border_side(t[0], t[4], t[8], out);
            }
        }
        Short::BorderFlow(_, None) => {
            if t.chunks(2).all(|g| g[0] == g[1]) {
                border_side(t[0], t[2], t[4], out);
            }
        }
        Short::Background | Short::TextWrap => out.push_str(t[0]),
        Short::Font => font(t, out),
        Short::Flex => join(t, " ", out),
        Short::FlexFlow => match (t[0], t[1]) {
            (d, "nowrap") => out.push_str(d),
            ("row", w) => out.push_str(w),
            (d, w) => join(&[d, w], " ", out),
        },
        Short::ListStyle => match (t[0], t[1]) {
            (ty, "outside") => out.push_str(ty),
            (ty, pos) => join(&[pos, ty], " ", out),
        },
        Short::WhiteSpace => white_space(t[0], t[1], out),
        Short::GridLines(_) => {
            if t[1] == default_end(t[0]) {
                out.push_str(t[0]);
            } else {
                join(t, " / ", out);
            }
        }
        Short::GridArea => grid_area([t[0], t[1], t[2], t[3]], out),
        Short::GridTemplate => template(t, out),
        Short::Grid => {
            if t[3..] == ["auto", "auto", "row"] {
                template(&t[..3], out);
            }
        }
    }
}
