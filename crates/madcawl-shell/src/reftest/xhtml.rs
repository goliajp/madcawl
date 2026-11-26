//! What an XML parser would change in a WPT XHTML test before it reaches
//! the HTML parser, as far as these suites need: CDATA markers and
//! character references inside `<style>` (raw text to HTML, so the CSS
//! would see them; text inside a CDATA section is left as written), and
//! self-closing tags of elements that are not void (`<div/>`, which HTML
//! reads as an open tag). Nothing else of XML is applied.

const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// `text` with the changes above made.
pub fn as_html(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut i) = (0, 0);
    while let Some(lt) = b[i..].iter().position(|&c| c == b'<').map(|k| i + k) {
        if text[lt..].starts_with("<!--") {
            i = text[lt..].find("-->").map_or(text.len(), |k| lt + k + 3);
            continue;
        }
        let Some((name_end, gt, closed)) = open_tag(b, lt) else {
            i = lt + 1;
            continue;
        };
        let name = &text[lt + 1..name_end];
        if closed && !VOID.iter().any(|v| v.eq_ignore_ascii_case(name)) {
            out.push_str(&text[copied..lt]);
            out.push_str(text[lt..gt - 1].trim_end());
            out.push_str("></");
            out.push_str(name);
            out.push('>');
            (copied, i) = (gt + 1, gt + 1);
        } else if !closed && name.eq_ignore_ascii_case("style") {
            out.push_str(&text[copied..=gt]);
            let end = find_ci(&text[gt + 1..], "</style").map_or(text.len(), |k| gt + 1 + k);
            out.push_str(&unwrap_style(&text[gt + 1..end]));
            (copied, i) = (end, end);
        } else {
            i = gt + 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// The start tag at `lt`: where its name ends, where its `>` is, and
/// whether it closes itself; quoted attribute values are passed over.
fn open_tag(b: &[u8], lt: usize) -> Option<(usize, usize, bool)> {
    if !b.get(lt + 1)?.is_ascii_alphabetic() {
        return None;
    }
    let mut j = lt + 1;
    while j < b.len() && !(b[j].is_ascii_whitespace() || b[j] == b'/' || b[j] == b'>') {
        j += 1;
    }
    let name_end = j;
    let mut quote = None;
    while j < b.len() {
        match (quote, b[j]) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, q @ (b'"' | b'\'')) => quote = Some(q),
            (None, b'>') => return Some((name_end, j, b[j - 1] == b'/')),
            _ => {}
        }
        j += 1;
    }
    None
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// A style sheet's text with character references decoded and CDATA
/// markers dropped, the text inside a CDATA section kept as it is.
fn unwrap_style(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("<![CDATA[") {
        decode_into(&mut out, &rest[..at]);
        let inner = &rest[at + 9..];
        let end = inner.find("]]>").unwrap_or(inner.len());
        out.push_str(&inner[..end]);
        rest = inner.get(end + 3..).unwrap_or("");
    }
    decode_into(&mut out, rest);
    out
}

/// `text` onto `out` with its character references decoded.
fn decode_into(out: &mut String, text: &str) {
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // the `;` is ASCII, so the slices around it fall on char boundaries
        let semi = rest.as_bytes()[..rest.len().min(12)]
            .iter()
            .position(|&c| c == b';');
        let Some(end) = semi else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        match reference(&rest[1..end]) {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
}

fn reference(name: &str) -> Option<char> {
    let n = match name {
        "lt" => return Some('<'),
        "gt" => return Some('>'),
        "amp" => return Some('&'),
        "quot" => return Some('"'),
        "apos" => return Some('\''),
        _ => name.strip_prefix('#')?,
    };
    let v = match n.strip_prefix(['x', 'X']) {
        Some(h) => u32::from_str_radix(h, 16).ok()?,
        None => n.parse().ok()?,
    };
    char::from_u32(v)
}

#[cfg(test)]
mod tests {
    use super::as_html;

    #[test]
    fn style_text_and_self_closing_tags_read_as_xml_would_give_them() {
        let x = "<style type=\"text/css\"><![CDATA[ body &gt; div { color: red } ]]></style><div/><br/><span class=\"a\" />";
        assert_eq!(
            as_html(x),
            "<style type=\"text/css\"> body &gt; div { color: red } </style><div></div><br/><span class=\"a\"></span>",
            "text inside a CDATA section is not decoded"
        );
        assert_eq!(
            as_html("<style>p &gt; a { } <![CDATA[ i &lt; b ]]> q &amp; r</style>"),
            "<style>p > a { }  i &lt; b  q & r</style>"
        );
        assert_eq!(
            as_html("<STYLE>a::before{content:\"&#x41;&#66;&bogus;\"}</STYLE>"),
            "<STYLE>a::before{content:\"AB&bogus;\"}</STYLE>"
        );
        assert_eq!(
            as_html("<p>a &gt; b</p>"),
            "<p>a &gt; b</p>",
            "outside style nothing is decoded"
        );
        assert_eq!(as_html("<style>a & b"), "<style>a & b");
    }

    #[test]
    fn a_reference_scan_stops_short_of_multibyte_text() {
        assert_eq!(
            as_html("<style>a::before{content:\"&ééééé;\"} &é</style>"),
            "<style>a::before{content:\"&ééééé;\"} &é</style>"
        );
        assert_eq!(
            as_html("<style>&日本語;&#x41;</style>"),
            "<style>&日本語;A</style>"
        );
        // the twelfth byte after `&` falls inside a two-byte char
        assert_eq!(
            as_html("<style>&éééééé x</style>"),
            "<style>&éééééé x</style>"
        );
    }

    #[test]
    fn quoted_values_and_empty_style_elements_are_kept_whole() {
        assert_eq!(
            as_html("<a title=\"x/>y\" href='q>'/>t<img alt=\"/>\"/>"),
            "<a title=\"x/>y\" href='q>'></a>t<img alt=\"/>\"/>"
        );
        assert_eq!(
            as_html("<style type=\"text/css\"/><p>&amp;</p><style>&amp;</style>"),
            "<style type=\"text/css\"></style><p>&amp;</p><style>&</style>"
        );
        assert_eq!(as_html("<!-- <div/> --><b/>"), "<!-- <div/> --><b></b>");
    }
}
