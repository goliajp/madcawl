//! A small JSON reader and writer for JSON-RPC lines: whole values, no
//! streaming, nesting bounded so a hostile line cannot exhaust the
//! stack.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// A number as its JSON text, so an id of any size goes back as it
    /// came
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

const MAX_DEPTH: usize = 64;

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::str)
    }

    /// A non-negative whole number, or a string of one.
    pub fn u64_at(&self, key: &str) -> Option<u64> {
        match self.get(key)? {
            Value::Num(n) => n.parse().ok(),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn bool_at(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => out.push_str(n),
            Value::Str(s) => write_str(out, s),
            Value::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Value::Obj(kv) => {
                out.push('{');
                for (i, (k, v)) in kv.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// An object from key/value pairs, in order.
pub fn obj<const N: usize>(kv: [(&str, Value); N]) -> Value {
    Value::Obj(kv.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

pub fn num(n: i64) -> Value {
    Value::Num(n.to_string())
}

pub fn s(v: &str) -> Value {
    Value::Str(v.to_owned())
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// One complete value; whitespace may follow it, nothing else.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        at: 0,
    };
    let v = p.value(0)?;
    p.ws();
    if p.at != p.b.len() {
        return Err(p.err("trailing bytes"));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("{what} at byte {}", self.at)
    }

    fn ws(&mut self) {
        while matches!(self.b.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        let hit = self.b[self.at..].starts_with(lit);
        if hit {
            self.at += lit.len();
        }
        hit
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err(self.err("nested too deep"));
        }
        self.ws();
        match self.b.get(self.at) {
            Some(b'{') => self.seq(depth, b'}', true),
            Some(b'[') => self.seq(depth, b']', false),
            Some(b'"') => self.string().map(Value::Str),
            Some(b't') if self.eat(b"true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat(b"false") => Ok(Value::Bool(false)),
            Some(b'n') if self.eat(b"null") => Ok(Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.err("unexpected byte")),
        }
    }

    /// An object (`keyed`) or an array, after its opening byte.
    fn seq(&mut self, depth: usize, close: u8, keyed: bool) -> Result<Value, String> {
        self.at += 1;
        let (mut kv, mut items) = (Vec::new(), Vec::new());
        self.ws();
        if !self.eat(&[close]) {
            loop {
                self.ws();
                if keyed {
                    if self.b.get(self.at) != Some(&b'"') {
                        return Err(self.err("expected a key"));
                    }
                    let k = self.string()?;
                    self.ws();
                    if !self.eat(b":") {
                        return Err(self.err("expected ':'"));
                    }
                    kv.push((k, self.value(depth + 1)?));
                } else {
                    items.push(self.value(depth + 1)?);
                }
                self.ws();
                if self.eat(b",") {
                    continue;
                }
                if self.eat(&[close]) {
                    break;
                }
                return Err(self.err("expected ',' or a close"));
            }
        }
        Ok(if keyed {
            Value::Obj(kv)
        } else {
            Value::Arr(items)
        })
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        while matches!(
            self.b.get(self.at),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.at += 1;
        }
        // the text is kept and written back as it came, so it must be
        // json's number grammar, not just anything f64 parses
        std::str::from_utf8(&self.b[start..self.at])
            .ok()
            .filter(|s| super::json_number::is_number(s.as_bytes()) && s.parse::<f64>().is_ok_and(f64::is_finite))
            .map(|s| Value::Num(s.to_owned()))
            .ok_or_else(|| self.err("bad number"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let h = self
            .b
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.err("short escape"))?;
        let n = std::str::from_utf8(h)
            .ok()
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .ok_or_else(|| self.err("bad escape"))?;
        self.at += 4;
        Ok(n)
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let run = self.b[self.at..]
                .iter()
                .position(|&c| c == b'"' || c == b'\\')
                .ok_or_else(|| self.err("unterminated string"))?;
            out.extend_from_slice(&self.b[self.at..self.at + run]);
            self.at += run + 1;
            if self.b[self.at - 1] == b'"' {
                break;
            }
            let c = *self
                .b
                .get(self.at)
                .ok_or_else(|| self.err("unterminated escape"))?;
            self.at += 1;
            let ch = match c {
                b'"' => '"',
                b'\\' => '\\',
                b'/' => '/',
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let hi = self.hex4()?;
                    let back = self.at;
                    let cp = if (0xD800..0xDC00).contains(&hi) && self.eat(b"\\u") {
                        match self.hex4()? {
                            lo @ 0xDC00..0xE000 => 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00),
                            // not a low half: this one stands alone, the
                            // next escape is read on its own
                            _ => {
                                self.at = back;
                                0xFFFD
                            }
                        }
                    } else {
                        hi
                    };
                    char::from_u32(cp).unwrap_or('\u{FFFD}')
                }
                _ => return Err(self.err("bad escape")),
            };
            out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
        }
        String::from_utf8(out).map_err(|_| self.err("invalid UTF-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_and_write_back() {
        let v =
            parse(r#"{"a":[1,2.5,"x\"y\n\u4e2d\ud83d\ude00"],"b":true,"c":null,"d":{}}"#).unwrap();
        assert_eq!(v.get("b"), Some(&Value::Bool(true)));
        let text = v.to_json();
        assert!(text.starts_with(r#"{"a":[1,2.5,"#), "{text}");
        assert_eq!(parse(&text).unwrap(), v);
        assert_eq!(parse(r#"{"n":"7"}"#).unwrap().u64_at("n"), Some(7));
        let big = r#"{"id":9007199254740993,"f":-1.5e3}"#;
        assert_eq!(parse(big).unwrap().to_json(), big);
        let lone = parse(r#""\ud800\u0041\ud800x\udc00""#).unwrap();
        assert_eq!(lone, Value::Str("\u{FFFD}A\u{FFFD}x\u{FFFD}".into()));
    }

    #[test]
    fn malformed_and_deep_input_are_errors() {
        for bad in ["1.", "01", "-.5", "+1", "1e", ".5"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        for bad in [
            "", "{", "{\"a\":}", "[1,]", "\"open", "tru", "1 2", "\"\\x\"", "{1:2}",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert!(parse(&"[".repeat(10_000)).is_err());
    }
}
