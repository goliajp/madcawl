//! The `extract` tool: records instead of text, by the view request
//! with the extraction's own fields.

use crate::frame::{Frame, tag};
use crate::json::{Value, obj, s};
use crate::tools::DEFAULT_BUDGET;

const WHAT: [&str; 5] = ["table", "list", "links", "images", "forms"];

pub(crate) fn tool(mut props: Vec<(&'static str, Value)>) -> Value {
    let p = |ty: &str, d: &str| obj([("type", s(ty)), ("description", s(d))]);
    for (k, v) in &mut props {
        if *k == "ref" {
            *v = p(
                "string",
                "table: the table (no ref lists the tables); list: one item, the rest are its kind; else the subtree",
            );
        }
    }
    props.push(("what", p("string", "table, list, links, images or forms")));
    props.push((
        "format",
        p("string", "json (default, one object a line) or markdown"),
    ));
    props.push((
        "offset",
        p(
            "integer",
            "Skip this many records (the reply says where more start)",
        ),
    ));
    crate::tools::tool(
        "extract",
        "Structured records from the page: table rows keyed by header, repeated items, links, images, form fields; each with its [ref].",
        props,
        &["page", "what"],
    )
}

/// The view request `f` turned into an extraction.
pub(crate) fn frame(mut f: Frame, args: &Value) -> Result<Frame, String> {
    let what = args.str_at("what").map(str::trim).unwrap_or("");
    if !WHAT.contains(&what) {
        return Err(format!("`what` must be one of {}", WHAT.join(", ")));
    }
    f = f.str(tag::WHAT, what);
    match args.str_at("format").map(str::trim) {
        None | Some("" | "json") => {}
        Some("markdown") => f = f.str(tag::FORMAT, "markdown"),
        Some(other) => return Err(format!("unknown format {other:?}: json or markdown")),
    }
    if args.get("offset").is_some() {
        let o = args
            .u64_at("offset")
            .ok_or("`offset` must be a non-negative integer")?;
        f = f.num(tag::OFFSET, o);
    }
    if f.get_num(tag::BUDGET).is_none() {
        f = f.num(tag::BUDGET, DEFAULT_BUDGET);
    }
    Ok(f)
}

/// The reply without its count of records left out, which its text
/// gives as an offset.
pub(crate) fn without_folded(mut reply: Frame) -> Frame {
    reply.fields.retain(|(t, _)| *t != tag::FOLDED);
    reply
}

#[cfg(test)]
mod tests {
    use crate::frame::{kind, tag};
    use crate::json;

    fn request(args: &str) -> Result<crate::frame::Frame, String> {
        crate::tools::request(
            "extract",
            &json::parse(args).unwrap(),
            std::path::Path::new("/"),
        )
    }

    #[test]
    fn an_extract_call_becomes_a_read_of_view_4() {
        let f = request(
            r#"{"page":"p2","what":"table","ref":"[e3k]","format":"markdown","offset":40}"#,
        )
        .unwrap();
        assert_eq!(f.kind, kind::READ);
        assert_eq!(f.get_num(tag::PAGE), Some(2));
        assert_eq!(f.get_num(tag::VIEW), Some(4));
        assert_eq!(f.get_num(tag::SCOPE), Some(2));
        assert_eq!(f.get_str(tag::REF), Some("e3k"));
        assert_eq!(f.get_str(tag::WHAT), Some("table"));
        assert_eq!(f.get_str(tag::FORMAT), Some("markdown"));
        assert_eq!(f.get_num(tag::OFFSET), Some(40));
        assert_eq!(f.get_num(tag::BUDGET), Some(crate::tools::DEFAULT_BUDGET));
    }

    #[test]
    fn what_and_format_are_checked() {
        assert!(request(r#"{"page":"p1"}"#).unwrap_err().contains("`what`"));
        assert!(request(r#"{"page":"p1","what":"tables"}"#).is_err());
        assert!(request(r#"{"page":"p1","what":"links","format":"csv"}"#).is_err());
        for bad in ["-1", "1.5", "\"abc\"", "null", "true", "[]"] {
            let args = format!(r#"{{"page":"p1","what":"links","offset":{bad}}}"#);
            let e = request(&args).unwrap_err();
            assert!(e.contains("`offset`"), "{bad}: {e}");
        }
        let f = request(r#"{"page":"p1","what":"links","offset":7}"#).unwrap();
        assert_eq!(f.get_num(tag::OFFSET), Some(7));
        let f = request(r#"{"page":"p1","what":"links","scope":"main","budget":50}"#).unwrap();
        assert_eq!(
            (f.get_num(tag::SCOPE), f.get_num(tag::BUDGET)),
            (Some(3), Some(50))
        );
    }
}
