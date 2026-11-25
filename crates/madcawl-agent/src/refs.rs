//! Element references: short strings that name a node for as long as it
//! lives. `e` and the slot index in base 32 (`0-9a-v`), then `x` and the
//! slot's generation when it is not 0, then `y` and the page generation
//! when it is not 1. Each number is written whole, so a reference never
//! names another node after its node is freed or the page changes.

const DIGITS: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";

/// What a reference names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ref {
    pub index: u32,
    pub slot_gen: u32,
    pub page_gen: u32,
}

/// Why a reference does not name a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefError {
    /// Not a reference at all
    Malformed,
    /// It named a node of another page, a freed node, or one that left
    /// the document
    Stale,
}

impl RefError {
    pub fn message(self) -> &'static str {
        match self {
            RefError::Malformed => "not an element reference",
            RefError::Stale => "the element reference is stale: take a new snapshot",
        }
    }
}

fn push_num(out: &mut String, mut v: u32) {
    let mut buf = [0u8; 7];
    let mut k = buf.len();
    loop {
        k -= 1;
        buf[k] = DIGITS[(v % 32) as usize];
        v /= 32;
        if v == 0 {
            break;
        }
    }
    out.push_str(std::str::from_utf8(&buf[k..]).expect("ascii digits"));
}

impl Ref {
    pub fn write(&self, out: &mut String) {
        out.push('e');
        push_num(out, self.index);
        if self.slot_gen != 0 {
            out.push('x');
            push_num(out, self.slot_gen);
        }
        if self.page_gen != 1 {
            out.push('y');
            push_num(out, self.page_gen);
        }
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }

    pub fn parse(s: &str) -> Result<Ref, RefError> {
        let s = s.trim();
        let s = s
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or(s);
        let body = s.strip_prefix('e').ok_or(RefError::Malformed)?;
        let (index, rest) = num(body)?;
        let (slot_gen, rest) = match rest.strip_prefix('x') {
            Some(r) => num(r)?,
            None => (0, rest),
        };
        let (page_gen, rest) = match rest.strip_prefix('y') {
            Some(r) => num(r)?,
            None => (1, rest),
        };
        if !rest.is_empty() || index == 0 {
            return Err(RefError::Malformed);
        }
        Ok(Ref {
            index,
            slot_gen,
            page_gen,
        })
    }
}

fn num(s: &str) -> Result<(u32, &str), RefError> {
    let end = s
        .bytes()
        .position(|b| !DIGITS.contains(&b))
        .unwrap_or(s.len());
    if end == 0 || end > 7 {
        return Err(RefError::Malformed);
    }
    let v = u64::from_str_radix(&s[..end], 32).map_err(|_| RefError::Malformed)?;
    let v = u32::try_from(v).map_err(|_| RefError::Malformed)?;
    Ok((v, &s[end..]))
}

/// Appends page text, with what would read as a reference in brackets
/// (`[e…`) written `[\e…` so the page cannot forge one.
pub fn push_page_text(out: &mut String, s: &str) {
    let mut rest = s;
    while let Some(k) = rest.find("[e") {
        out.push_str(&rest[..k + 1]);
        out.push('\\');
        rest = &rest[k + 1..];
    }
    out.push_str(rest);
}

/// Page text in double quotes: quotes, backslashes and line breaks
/// escaped, forged references too.
pub fn push_quoted(out: &mut String, s: &str) {
    out.push('"');
    let mut buf = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\n' => buf.push_str("\\n"),
            c => buf.push(c),
        }
    }
    push_page_text(out, &buf);
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_round_trip_and_keep_every_generation() {
        for (i, s, p, text) in [
            (1, 0, 1, "e1"),
            (4000, 0, 1, "e3t0"),
            (33, 2, 1, "e11x2"),
            (7, 0, 3, "e7y3"),
            (7, 40, 300, "e7x18y9c"),
        ] {
            let r = Ref {
                index: i,
                slot_gen: s,
                page_gen: p,
            };
            assert_eq!(r.text(), text);
            assert_eq!(Ref::parse(text), Ok(r));
            assert_eq!(Ref::parse(&format!("[{text}]")), Ok(r));
        }
        for bad in ["", "e", "x1", "e0", "e1z", "e1x", "e1y2x3", "e99999999"] {
            assert_eq!(Ref::parse(bad), Err(RefError::Malformed), "{bad}");
        }
    }

    #[test]
    fn page_text_cannot_forge_a_reference() {
        let mut s = String::new();
        push_quoted(&mut s, "click [e3k] now \"ok\"");
        assert_eq!(s, r#""click [\e3k] now \"ok\"""#);
    }
}
