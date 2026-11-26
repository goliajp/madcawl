//! Screenshots through the host and the MCP side: the viewport, an
//! element, the whole page, marks with references, the PNG as image
//! content or saved by the host in the session's downloads directory,
//! and requests that cannot be taken.

mod serve_fixture;

use madcawl_mcp::json::Value;
use serve_fixture::{Fixture, Mcp, path_arg};

/// A tool call's text and its image content's PNG bytes, if any.
fn shot(m: &mut Mcp, args: &str) -> (String, bool, Option<Vec<u8>>) {
    let line = format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"name":"screenshot","arguments":{args}}}}}"#
    );
    let v = m.0.handle(&line).unwrap();
    let r = v.get("result").unwrap();
    let Some(Value::Arr(c)) = r.get("content") else {
        panic!("{}", v.to_json())
    };
    let text = c[0].str_at("text").unwrap().to_owned();
    let png = c.get(1).map(|i| {
        assert_eq!(i.str_at("type"), Some("image"));
        assert_eq!(i.str_at("mimeType"), Some("image/png"));
        unbase64(i.str_at("data").unwrap())
    });
    (text, r.bool_at("isError").unwrap(), png)
}

fn unbase64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes().filter(|&c| c != b'=') {
        acc = acc << 6 | val(c) as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

fn png_size(png: &[u8]) -> (u32, u32) {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let be = |i: usize| u32::from_be_bytes(png[i..i + 4].try_into().unwrap());
    (be(16), be(20))
}

fn open(m: &mut Mcp, f: &Fixture) -> String {
    let t = m.ok("open", &path_arg(&f.page()));
    t.split_whitespace().nth(1).unwrap().to_owned()
}

#[test]
fn the_viewport_an_element_and_the_page_come_back_as_png_with_marks_listed() {
    let mut f = Fixture::new();
    f.config.downloads = Some(f.dir.join("dl"));
    f.start();
    let mut m = f.mcp();
    let p = open(&mut m, &f);
    let (text, err, png) = shot(&mut m, &format!(r#"{{"page":"{p}"}}"#));
    assert!(!err, "{text}");
    assert!(text.contains("<<page-content-"), "in an envelope: {text}");
    assert!(
        text.contains("screenshot of the viewport (0,0 1280x800 CSS px) at scale 1: 1280x800 PNG"),
        "{text}"
    );
    assert_eq!(png_size(&png.unwrap()), (1280, 800));
    // marks name the link with the reference a snapshot gives it
    let snap = m.ok("snapshot", &format!(r#"{{"page":"{p}"}}"#));
    let line = snap.lines().find(|l| l.contains("Other page")).unwrap();
    let r = &line[line.find("[e").unwrap() + 1..line.find(']').unwrap()];
    let (text, _, marked) = shot(
        &mut m,
        &format!(r#"{{"page":"{p}","marks":true,"scale":0.5}}"#),
    );
    assert!(text.contains("640x400 PNG"), "{text}");
    // the marks are in the picture: it differs from the same one without them
    let (_, _, plain) = shot(&mut m, &format!(r#"{{"page":"{p}","scale":0.5}}"#));
    assert_ne!(marked.unwrap(), plain.unwrap());
    assert!(text.contains("marks: 1 interactive elements"), "{text}");
    assert!(
        text.contains(&format!("[{r}] link \"Other page\" at ")),
        "{text}"
    );
    // one element: the picture is its box
    let (text, err, png) = shot(
        &mut m,
        &format!(r#"{{"page":"{p}","ref":"{r}","scale":2}}"#),
    );
    assert!(!err, "{text}");
    let (w, h) = png_size(&png.unwrap());
    assert!(w > 0 && h > 0 && h < 100, "{w}x{h}: {text}");
    // the whole page, saved by the host in the session's downloads directory, never outside it
    let args = format!(r#"{{"page":"{p}","scope":"page","file":"../../page"}}"#);
    let (text, err, png) = shot(&mut m, &args);
    assert!(!err && png.is_none(), "{text}");
    let line = text
        .lines()
        .find(|l| l.starts_with("saved to "))
        .expect("where it went");
    let out = std::path::Path::new(&line["saved to ".len()..]);
    assert!(
        out.starts_with(f.dir.join("dl")) && out.ends_with("page.png"),
        "{text}"
    );
    assert_eq!(png_size(&std::fs::read(out).unwrap()).0, 1280);
    assert!(
        f.audit().contains("screenshot to page.png"),
        "the file name audited"
    );
    // a second save under the same name does not replace the first
    let (text, err, _) = shot(&mut m, &args);
    assert!(err && text.contains("already there"), "{text}");
}

#[test]
fn saving_without_a_downloads_directory_is_refused() {
    let f = Fixture::new();
    f.start();
    let mut m = f.mcp();
    let p = open(&mut m, &f);
    let (text, err, png) = shot(&mut m, &format!(r#"{{"page":"{p}","file":"x.png"}}"#));
    assert!(err && png.is_none(), "{text}");
    assert!(text.contains("downloads directory"), "{text}");
    assert!(!f.dir.join("x.png").exists());
}

#[test]
fn bad_screenshot_requests_are_errors() {
    let f = Fixture::new();
    f.start();
    let mut m = f.mcp();
    let p = open(&mut m, &f);
    let (text, err, png) = shot(&mut m, &format!(r#"{{"page":"{p}","ref":"e9999"}}"#));
    assert!(err && png.is_none(), "{text}");
    let (text, err, _) = shot(&mut m, &format!(r#"{{"page":"{p}","scale":5}}"#));
    assert!(err && text.contains("scale"), "{text}");
    let (text, err, _) = shot(
        &mut m,
        &format!(r#"{{"page":"{p}","ref":"e1","scope":"page"}}"#),
    );
    assert!(err && text.contains("leave out `scope`"), "{text}");
    let (text, err, _) = shot(&mut m, r#"{"page":"p999"}"#);
    assert!(err && text.contains("no such page"), "{text}");
}
