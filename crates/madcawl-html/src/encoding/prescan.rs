//! "Prescan a byte stream to determine its encoding" (13.2.3.2) and
//! "the algorithm for extracting a character encoding from a meta
//! element".

use super::Encoding;

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

/// The encoding the first bytes of a document name in a `meta`, or
/// `None`. The caller passes at most the first 1024 bytes. Running off
/// the end of `bytes` anywhere is `None`.
pub fn prescan(bytes: &[u8]) -> Option<Encoding> {
    if bytes.starts_with(&[0x3C, 0x00, 0x3F, 0x00, 0x78, 0x00]) {
        return Some(Encoding::Utf16le);
    }
    if bytes.starts_with(&[0x00, 0x3C, 0x00, 0x3F, 0x00, 0x78]) {
        return Some(Encoding::Utf16be);
    }
    let mut s = Scan { bytes, pos: 0 };
    loop {
        let rest = s.bytes.get(s.pos..)?;
        if rest.is_empty() {
            return None;
        }
        if rest.starts_with(b"<!--") {
            // the two dashes may be the ones that opened the comment
            let end = find(&rest[2..], b"-->")?;
            s.pos += 2 + end + 2;
        } else if rest.len() > 5
            && rest[..5].eq_ignore_ascii_case(b"<meta")
            && (is_space(rest[5]) || rest[5] == b'/')
        {
            s.pos += 5;
            if let Some(found) = s.meta()? {
                return Some(found);
            }
        } else if rest[0] == b'<' && tag_start(&rest[1..]) {
            let from = s.pos;
            s.pos = from + rest.iter().position(|&b| is_space(b) || b == b'>')?;
            while s.attribute()?.is_some() {}
        } else if rest.starts_with(b"<!") || rest.starts_with(b"</") || rest.starts_with(b"<?") {
            s.pos += rest.iter().position(|&b| b == b'>')?;
        }
        s.pos += 1;
    }
}

/// `<` is followed by an optional `/` and an ASCII letter.
fn tag_start(after: &[u8]) -> bool {
    let after = after.strip_prefix(b"/").unwrap_or(after);
    after.first().is_some_and(u8::is_ascii_alphabetic)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

struct Scan<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Scan<'_> {
    fn at(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// The attributes of a `<meta`: `Some(None)` when they name no
    /// encoding and the scan goes on, `None` when it runs off the end.
    fn meta(&mut self) -> Option<Option<Encoding>> {
        let mut seen: Vec<Vec<u8>> = Vec::new();
        let mut got_pragma = false;
        let mut need_pragma = None;
        // `None` not seen, `Some(None)` a label that names no encoding
        let mut charset: Option<Option<Encoding>> = None;
        while let Some((name, value)) = self.attribute()? {
            if seen.contains(&name) {
                continue;
            }
            match &name[..] {
                b"http-equiv" if value == b"content-type" => got_pragma = true,
                b"content" => {
                    if let Some(e) = extract_from_content(&value)
                        && charset.is_none()
                    {
                        charset = Some(Some(e));
                        need_pragma = Some(true);
                    }
                }
                b"charset" => {
                    charset = Some(Encoding::for_label(&value));
                    need_pragma = Some(false);
                }
                _ => {}
            }
            seen.push(name);
        }
        let need_pragma = match need_pragma {
            None => return Some(None),
            Some(n) => n,
        };
        if need_pragma && !got_pragma {
            return Some(None);
        }
        Some(charset.flatten().map(|e| match e {
            Encoding::Utf16be | Encoding::Utf16le => Encoding::Utf8,
            Encoding::XUserDefined => Encoding::Windows1252,
            e => e,
        }))
    }

    /// "Get an attribute": `Some(None)` when there is none before `>`,
    /// `None` when the bytes run out. Names and values come lowercased.
    fn attribute(&mut self) -> Option<Option<(Vec<u8>, Vec<u8>)>> {
        while matches!(self.at()?, b if is_space(b) || b == b'/') {
            self.pos += 1;
        }
        if self.at()? == b'>' {
            return Some(None);
        }
        let mut name = Vec::new();
        loop {
            match self.at()? {
                b'=' if !name.is_empty() => {
                    self.pos += 1;
                    return self.value(name);
                }
                b if is_space(b) => break,
                b'/' | b'>' => return Some(Some((name, Vec::new()))),
                b => name.push(b.to_ascii_lowercase()),
            }
            self.pos += 1;
        }
        while is_space(self.at()?) {
            self.pos += 1;
        }
        if self.at()? != b'=' {
            return Some(Some((name, Vec::new())));
        }
        self.pos += 1;
        self.value(name)
    }

    fn value(&mut self, name: Vec<u8>) -> Option<Option<(Vec<u8>, Vec<u8>)>> {
        let mut value = Vec::new();
        while is_space(self.at()?) {
            self.pos += 1;
        }
        match self.at()? {
            q @ (b'"' | b'\'') => loop {
                self.pos += 1;
                let b = self.at()?;
                if b == q {
                    self.pos += 1;
                    return Some(Some((name, value)));
                }
                value.push(b.to_ascii_lowercase());
            },
            b'>' => return Some(Some((name, value))),
            b => {
                value.push(b.to_ascii_lowercase());
                self.pos += 1;
            }
        }
        loop {
            match self.at()? {
                b if is_space(b) || b == b'>' => return Some(Some((name, value))),
                b => value.push(b.to_ascii_lowercase()),
            }
            self.pos += 1;
        }
    }
}

/// The encoding a `meta` element's `content` value names after
/// `charset=`, or `None`.
pub fn extract_from_content(s: &[u8]) -> Option<Encoding> {
    let mut pos = 0;
    loop {
        let at = s[pos..]
            .windows(7)
            .position(|w| w.eq_ignore_ascii_case(b"charset"))?;
        pos += at + 7;
        while s.get(pos).is_some_and(|&b| is_space(b)) {
            pos += 1;
        }
        if s.get(pos) != Some(&b'=') {
            continue;
        }
        pos += 1;
        while s.get(pos).is_some_and(|&b| is_space(b)) {
            pos += 1;
        }
        let rest = &s[pos..];
        return match rest.first()? {
            &q @ (b'"' | b'\'') => {
                let end = rest[1..].iter().position(|&b| b == q)?;
                Encoding::for_label(&rest[1..1 + end])
            }
            _ => {
                let end = rest
                    .iter()
                    .position(|&b| is_space(b) || b == b';')
                    .unwrap_or(rest.len());
                Encoding::for_label(&rest[..end])
            }
        };
    }
}
