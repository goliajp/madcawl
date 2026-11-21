//! Just enough JSON to read the html5lib test files. Strings come out
//! as WTF-8, since the tests carry lone surrogates.

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
    Arr(Vec<Json>),
    Obj(Vec<(Vec<u8>, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(kv) => kv.iter().find(|(k, _)| k == key.as_bytes()).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => &[],
        }
    }
    pub fn str(&self) -> Option<&[u8]> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Json {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, p.s.len(), "trailing data");
    v
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, b: u8) {
        self.ws();
        assert_eq!(self.s[self.i], b, "expected {:?} at {}", b as char, self.i);
        self.i += 1;
    }
    fn value(&mut self) -> Json {
        self.ws();
        match self.s[self.i] {
            b'{' => {
                self.i += 1;
                let mut kv = Vec::new();
                self.ws();
                if self.s[self.i] == b'}' {
                    self.i += 1;
                    return Json::Obj(kv);
                }
                loop {
                    self.ws();
                    let k = self.string();
                    self.eat(b':');
                    kv.push((k, self.value()));
                    self.ws();
                    let b = self.s[self.i];
                    self.i += 1;
                    if b == b'}' {
                        return Json::Obj(kv);
                    }
                    assert_eq!(b, b',');
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.s[self.i] == b']' {
                    self.i += 1;
                    return Json::Arr(a);
                }
                loop {
                    a.push(self.value());
                    self.ws();
                    let b = self.s[self.i];
                    self.i += 1;
                    if b == b']' {
                        return Json::Arr(a);
                    }
                    assert_eq!(b, b',');
                }
            }
            b'"' => Json::Str(self.string()),
            b't' => {
                self.i += 4;
                Json::Bool(true)
            }
            b'f' => {
                self.i += 5;
                Json::Bool(false)
            }
            b'n' => {
                self.i += 4;
                Json::Null
            }
            _ => {
                let start = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                Json::Num(
                    std::str::from_utf8(&self.s[start..self.i])
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
        }
    }
    fn string(&mut self) -> Vec<u8> {
        assert_eq!(self.s[self.i], b'"');
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let b = self.s[self.i];
            self.i += 1;
            match b {
                b'"' => return out,
                b'\\' => {
                    let e = self.s[self.i];
                    self.i += 1;
                    match e {
                        b'u' => {
                            let mut c = self.hex4();
                            if (0xD800..0xDC00).contains(&c) && self.s[self.i..].starts_with(b"\\u")
                            {
                                let save = self.i;
                                self.i += 2;
                                let lo = self.hex4();
                                if (0xDC00..0xE000).contains(&lo) {
                                    c = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
                                } else {
                                    self.i = save;
                                }
                            }
                            push_wtf8(&mut out, c);
                        }
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        _ => out.push(e),
                    }
                }
                _ => out.push(b),
            }
        }
    }
    fn hex4(&mut self) -> u32 {
        let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap();
        self.i += 4;
        u32::from_str_radix(h, 16).unwrap()
    }
}

pub fn push_wtf8(out: &mut Vec<u8>, c: u32) {
    match c {
        0..=0x7F => out.push(c as u8),
        0x80..=0x7FF => out.extend_from_slice(&[0xC0 | (c >> 6) as u8, 0x80 | (c & 0x3F) as u8]),
        0x800..=0xFFFF => out.extend_from_slice(&[
            0xE0 | (c >> 12) as u8,
            0x80 | ((c >> 6) & 0x3F) as u8,
            0x80 | (c & 0x3F) as u8,
        ]),
        _ => out.extend_from_slice(&[
            0xF0 | (c >> 18) as u8,
            0x80 | ((c >> 12) & 0x3F) as u8,
            0x80 | ((c >> 6) & 0x3F) as u8,
            0x80 | (c & 0x3F) as u8,
        ]),
    }
}

/// Undo one more level of `\uXXXX`, for the tests marked `doubleEscaped`.
pub fn unescape(s: &[u8]) -> Vec<u8> {
    let text = format!("\"{}\"", String::from_utf8_lossy(s).replace('"', "\\\""));
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    p.string()
}
