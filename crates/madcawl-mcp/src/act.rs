//! The `act`, `wait` and `requests` tools: one tool for every action, so
//! the action list costs one definition of context, and the wait that
//! follows each action built in.

use crate::frame::{Frame, kind, tag};
use crate::json::{Value, obj, s};

const ACTIONS: [&str; 14] = [
    "navigate", "back", "forward", "reload", "scroll", "click", "hover", "type", "press", "select", "check",
    "upload", "fill_form", "submit",
];

fn p(ty: &str, d: &str) -> Value {
    obj([("type", s(ty)), ("description", s(d))])
}

fn until_props() -> Vec<(&'static str, Value)> {
    vec![
        ("timeout_ms", p("integer", "Most ms to wait for the page to be stable (default 10000)")),
        ("until_ref", p("string", "Also wait until this ref names an element")),
        ("until_text", p("string", "Also wait until this text is on the page")),
    ]
}

pub(crate) fn tools(page: (&'static str, Value)) -> Vec<Value> {
    let mut act = vec![
        page.clone(),
        ("action", p("string", &ACTIONS.join(", "))),
        ("ref", p("string", "Target element, e.g. e3k9 (submitter for fill_form)")),
        (
            "value",
            p("string", "type: text; press: key (Enter, Tab, Backspace); navigate: URL; select: option; upload: file path; scroll: up or down"),
        ),
        ("amount", p("integer", "scroll: viewports (default 1)")),
        ("checked", p("boolean", "check: tick (default true) or clear")),
        ("fields", p("object", "fill_form: {ref: value}; checkboxes take true/false")),
        ("submit", p("boolean", "fill_form: submit the form after")),
    ];
    act.extend(until_props());
    let mut wait = vec![page.clone(), ("since", p("integer", "Diff from this seq (default: from now)"))];
    wait.extend(until_props());
    vec![
        crate::tools::tool(
            "act",
            "Act on the page by ref, wait until it is stable, and return what changed (or the new page's outline).",
            act,
            &["page", "action"],
        ),
        crate::tools::tool("wait", "Wait until the page is stable; returns what is still missing.", wait, &["page"]),
        crate::tools::tool(
            "requests",
            "The page's requests (method, URL, status, size, timings) and load errors.",
            vec![page, ("limit", p("integer", "Most recent requests (default 50)"))],
            &["page"],
        ),
    ]
}

fn until(mut f: Frame, args: &Value) -> Result<Frame, String> {
    if args.get("timeout_ms").is_some() {
        let t = args.u64_at("timeout_ms").ok_or("`timeout_ms` must be a non-negative integer")?;
        f = f.num(tag::TIMEOUT, t);
    }
    for (key, t) in [("until_ref", tag::UNTIL_REF), ("until_text", tag::UNTIL_TEXT)] {
        if let Some(v) = args.str_at(key).filter(|v| !v.trim().is_empty()) {
            f = f.str(t, v.trim().trim_matches(['[', ']']));
        }
    }
    Ok(f)
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Num(n) => Some(n.clone()),
        _ => None,
    }
}

pub(crate) fn act_frame(page: u64, args: &Value) -> Result<Frame, String> {
    let action = args.str_at("action").map(str::trim).unwrap_or("");
    if !ACTIONS.contains(&action) {
        return Err(format!("`action` must be one of {}", ACTIONS.join(", ")));
    }
    let mut f = Frame::new(kind::ACT).num(tag::PAGE, page).str(tag::ACTION, action);
    if let Some(r) = args.str_at("ref").filter(|r| !r.trim().is_empty()) {
        f = f.str(tag::REF, r.trim().trim_matches(['[', ']']));
    }
    if let Some(v) = args.get("value").and_then(scalar) {
        f = f.str(tag::VALUE, &v);
    }
    if let Some(n) = args.u64_at("amount") {
        f = f.num(tag::AMOUNT, n);
    }
    if action == "check" {
        f = f.num(tag::ON, u64::from(args.bool_at("checked") != Some(false)));
    }
    if action == "fill_form" {
        let Some(Value::Obj(fields)) = args.get("fields") else {
            return Err("fill_form needs `fields`: an object of ref to value".into());
        };
        for (r, v) in fields {
            let v = scalar(v).ok_or("a field's value must be a string, number or boolean")?;
            f = f.str(tag::FIELD, &format!("{}\0{v}", r.trim().trim_matches(['[', ']'])));
        }
        f = f.num(tag::ON, u64::from(args.bool_at("submit") == Some(true)));
    }
    until(f, args)
}

pub(crate) fn wait_frame(page: u64, args: &Value) -> Result<Frame, String> {
    let mut f = Frame::new(kind::WAIT).num(tag::PAGE, page);
    if let Some(n) = args.u64_at("since") {
        f = f.num(tag::SINCE, n);
    }
    until(f, args)
}

pub(crate) fn requests_frame(page: u64, args: &Value) -> Frame {
    let f = Frame::new(kind::REQUESTS).num(tag::PAGE, page);
    match args.u64_at("limit") {
        Some(n) => f.num(tag::LIMIT, n),
        None => f,
    }
}

/// The head line's part for an action's reply: stable or not, and the
/// time each part took.
pub(crate) fn timing(f: &Frame) -> String {
    let Some(stable) = f.get_num(tag::STABLE) else { return String::new() };
    let ms = |t| f.get_num(t).map_or(0.0, |u| u as f64 / 1e3);
    format!(
        " · {} · dispatch {:.3} ms, wait {:.3} ms, diff {:.3} ms",
        if stable == 1 { "stable" } else { "NOT stable" },
        ms(tag::DISPATCH_US),
        ms(tag::WAIT_US),
        ms(tag::DIFF_US)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn act_calls_become_frames() {
        let a = json::parse(r#"{"action":"fill_form","fields":{"[e3]":"alice","e4":true},"submit":true,"ref":"e9","timeout_ms":500}"#).unwrap();
        let f = act_frame(2, &a).unwrap();
        assert_eq!(f.kind, kind::ACT);
        assert_eq!(f.get_str(tag::ACTION), Some("fill_form"));
        let fields: Vec<&[u8]> = f.fields.iter().filter(|(t, _)| *t == tag::FIELD).map(|(_, v)| v.as_slice()).collect();
        assert_eq!(fields, [b"e3\0alice".as_slice(), b"e4\0true".as_slice()]);
        assert_eq!((f.get_num(tag::ON), f.get_num(tag::TIMEOUT), f.get_str(tag::REF)), (Some(1), Some(500), Some("e9")));
        let c = act_frame(1, &json::parse(r#"{"action":"check","ref":"e1","checked":false}"#).unwrap()).unwrap();
        assert_eq!(c.get_num(tag::ON), Some(0));
        assert!(act_frame(1, &json::parse(r#"{"action":"fly"}"#).unwrap()).is_err());
        assert!(act_frame(1, &json::parse(r#"{"action":"fill_form"}"#).unwrap()).is_err());
        let w = wait_frame(1, &json::parse(r#"{"until_text":"Saved","since":4}"#).unwrap()).unwrap();
        assert_eq!((w.get_str(tag::UNTIL_TEXT), w.get_num(tag::SINCE)), (Some("Saved"), Some(4)));
    }
}
