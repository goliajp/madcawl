//! The `screenshot` tool: the request it makes, and the reply's PNG as
//! the tool's image content, or saved by the host as a file in the
//! session's downloads directory.

use crate::frame::{Frame, kind, tag};
use crate::json::{Value, obj, s};

fn p(ty: &str, d: &str) -> Value {
    obj([("type", s(ty)), ("description", s(d))])
}

pub(crate) fn tool(page: (&'static str, Value)) -> Value {
    crate::tools::tool(
        "screenshot",
        "A PNG of the page as painted: the viewport (default), the whole page, or one element by ref. With marks, each interactive element is outlined and labelled with its ref, drawn by the engine over the page, which cannot cover them; the page can still paint look-alike boxes, so take refs only from the list of marked elements in the text.",
        vec![
            page,
            (
                "scope",
                p("string", "viewport (default) or page; not with ref"),
            ),
            ("ref", p("string", "Only this element, e.g. e3k9")),
            (
                "scale",
                p(
                    "number",
                    "Device px per CSS px, 0.1 to 2 (default 1); below 1 shrinks",
                ),
            ),
            (
                "marks",
                p("boolean", "Label interactive elements with their refs"),
            ),
            (
                "file",
                p(
                    "string",
                    "Have the host save the PNG under this file name in its downloads directory instead of returning the image",
                ),
            ),
        ],
        &["page"],
    )
}

pub(crate) fn frame(page: u64, args: &Value) -> Result<Frame, String> {
    let mut f = Frame::new(kind::SCREENSHOT).num(tag::PAGE, page);
    let scope = match (
        args.str_at("ref").map(str::trim).filter(|r| !r.is_empty()),
        args.str_at("scope").unwrap_or(""),
    ) {
        (Some(_), "page" | "viewport") => {
            return Err("`ref` takes one element; leave out `scope`".into());
        }
        (Some(r), "") => {
            f = f.str(tag::REF, r.trim_matches(['[', ']']));
            2
        }
        (_, "viewport" | "") => 1,
        (_, "page") => 0,
        (_, other) => return Err(format!("unknown scope {other:?}: viewport or page")),
    };
    f = f.num(tag::SCOPE, scope);
    if let Some(v) = args.get("scale") {
        let k = match v {
            Value::Num(n) => n.parse::<f64>().ok(),
            Value::Str(n) => n.trim().parse::<f64>().ok(),
            _ => None,
        };
        let k = k
            .filter(|k| (0.1..=2.0).contains(k))
            .ok_or("`scale` must be a number from 0.1 to 2")?;
        f = f.num(tag::SCALE, (k * 100.0).round() as u64);
    }
    if args.bool_at("marks") == Some(true) {
        f = f.num(tag::MARKS, 1);
    }
    if let Some(n) = args.str_at("file").map(str::trim).filter(|n| !n.is_empty()) {
        f = f.str(tag::FILE, n);
    }
    Ok(f)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `b` in standard base64 with padding.
pub fn base64(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The PNG a reply carries, to go out as image content; none when the
/// host saved it.
pub(crate) fn picture(reply: &Frame) -> Option<Vec<u8>> {
    reply.field(tag::DATA).map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn base64_matches_the_standard_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ];
        for (a, b) in cases {
            assert_eq!(base64(a.as_bytes()), b);
        }
        assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
    }

    #[test]
    fn requests_carry_scope_scale_and_marks() {
        let a = json::parse(r#"{"ref":"[e3]","scale":0.5,"marks":true}"#).unwrap();
        let f = frame(2, &a).unwrap();
        assert_eq!(f.kind, kind::SCREENSHOT);
        assert_eq!(f.get_num(tag::SCOPE), Some(2));
        assert_eq!(f.get_str(tag::REF), Some("e3"));
        assert_eq!(f.get_num(tag::SCALE), Some(50));
        assert_eq!(f.get_num(tag::MARKS), Some(1));
        let page = frame(2, &json::parse(r#"{"scope":"page"}"#).unwrap()).unwrap();
        assert_eq!(
            (page.get_num(tag::SCOPE), page.get_num(tag::SCALE)),
            (Some(0), None)
        );
        assert!(frame(2, &json::parse(r#"{"scale":3}"#).unwrap()).is_err());
        assert!(frame(2, &json::parse(r#"{"scope":"all"}"#).unwrap()).is_err());
        let both = frame(2, &json::parse(r#"{"ref":"e3","scope":"page"}"#).unwrap());
        assert!(both.unwrap_err().contains("leave out `scope`"));
        let saved = frame(2, &json::parse(r#"{"file":" a.png "}"#).unwrap()).unwrap();
        assert_eq!(saved.get_str(tag::FILE), Some("a.png"));
    }
}
