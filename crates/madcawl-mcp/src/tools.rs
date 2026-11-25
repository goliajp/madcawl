//! The tools offered to the agent and how a call becomes a request
//! frame and its reply becomes text. Descriptions stay short: they take
//! the agent's context in every conversation.

use std::path::Path;

use crate::client::Client;
use crate::frame::{Frame, kind, tag};
use crate::json::{Value, obj, s};

/// The budget a snapshot or Markdown read gets when the call gives none.
pub const DEFAULT_BUDGET: u64 = 6000;

const VIEWS: [(&str, u64, &str); 4] = [
    (
        "outline",
        0,
        "Landmarks, headings and counts of controls; read this first.",
    ),
    (
        "snapshot",
        1,
        "The page as an indented tree, a node a line: role \"name\" [ref] states.",
    ),
    (
        "read",
        2,
        "The page's text as Markdown; links and controls carry [ref]s.",
    ),
    (
        "find",
        3,
        "Elements matching text and/or role, each with its [ref] and a line of context.",
    ),
];

fn prop(ty: &str, desc: &str) -> Value {
    obj([("type", s(ty)), ("description", s(desc))])
}

fn tool(name: &str, desc: &str, props: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let props = Value::Obj(props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
    let required = Value::Arr(required.iter().map(|r| s(r)).collect());
    let schema = obj([
        ("type", s("object")),
        ("properties", props),
        ("required", required),
    ]);
    obj([
        ("name", s(name)),
        ("description", s(desc)),
        ("inputSchema", schema),
    ])
}

fn page_prop() -> (&'static str, Value) {
    ("page", prop("string", "Page id from open, e.g. p1"))
}

fn view_props(name: &str) -> Vec<(&'static str, Value)> {
    let budget = if matches!(name, "snapshot" | "read") {
        "Most estimated tokens to return (default 6000); the rest is folded into lines with a [ref] to expand"
    } else {
        "Most estimated tokens to return; the rest is folded into lines with a [ref] to expand"
    };
    vec![
        page_prop(),
        (
            "scope",
            prop(
                "string",
                "page (default), viewport, or main (the main content)",
            ),
        ),
        (
            "ref",
            prop(
                "string",
                "Only this element's subtree: a ref like e3k9 from an earlier reply",
            ),
        ),
        ("budget", prop("integer", budget)),
        (
            "include_hidden",
            prop("boolean", "Also content users cannot see, marked hidden"),
        ),
    ]
}

pub fn list() -> Value {
    let mut all = vec![
        tool(
            "open",
            "Open a local HTML file as a windowless page (absolute path or file:// URL; web URLs are refused). Returns its page id and outline.",
            vec![
                (
                    "path",
                    prop("string", "Absolute path or file:// URL of the file"),
                ),
                (
                    "width",
                    prop("integer", "Viewport width in CSS px (default 1280)"),
                ),
                (
                    "height",
                    prop("integer", "Viewport height in CSS px (default 800)"),
                ),
                (
                    "charset",
                    prop(
                        "string",
                        "The file's charset when it does not say, e.g. shift_jis",
                    ),
                ),
            ],
            &["path"],
        ),
        tool("close", "Close a page.", vec![page_prop()], &["page"]),
        tool("pages", "List the open pages.", vec![], &[]),
    ];
    for (name, _, desc) in VIEWS {
        let mut props = view_props(name);
        if name == "find" {
            props.push((
                "text",
                prop("string", "Text the element's name or content contains"),
            ));
            props.push((
                "role",
                prop("string", "ARIA role, e.g. link, heading, button"),
            ));
            props.push(("limit", prop("integer", "Most hits (default 20)")));
        }
        all.push(tool(name, desc, props, &["page"]));
    }
    all.push(tool(
        "diff",
        "What changed on the page since the reply numbered `since` (every reply gives its seq).",
        vec![
            page_prop(),
            ("since", prop("integer", "A seq from an earlier reply")),
        ],
        &["page", "since"],
    ));
    Value::Arr(all)
}

/// `p3`, `3` or 3 as a page number.
fn page_of(args: &Value) -> Result<u64, String> {
    let bad = || "`page` is required: an id from open, like p1".to_owned();
    match args.get("page") {
        Some(Value::Str(p)) => p.trim().trim_start_matches('p').parse().map_err(|_| bad()),
        Some(_) => args.u64_at("page").ok_or_else(bad),
        None => Err(bad()),
    }
}

fn open_frame(args: &Value, cwd: &Path) -> Result<Frame, String> {
    let path = args.str_at("path").filter(|p| !p.trim().is_empty());
    let path = path.ok_or("`path` is required")?.trim();
    let has_scheme = path
        .find(':')
        .is_some_and(|i| i > 1 && !path[..i].contains('/'));
    let path = if has_scheme || Path::new(path).is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path).to_string_lossy().into_owned()
    };
    let mut f = Frame::new(kind::OPEN).str(tag::URL, &path);
    for (key, t) in [("width", tag::WIDTH), ("height", tag::HEIGHT)] {
        if let Some(n) = args.u64_at(key) {
            f = f.num(t, n);
        }
    }
    if let Some(c) = args.str_at("charset") {
        f = f.str(tag::CHARSET, c);
    }
    Ok(f)
}

fn view_frame(name: &str, view: u64, args: &Value) -> Result<Frame, String> {
    let mut f = Frame::new(kind::READ)
        .num(tag::PAGE, page_of(args)?)
        .num(tag::VIEW, view);
    let scope = match (args.str_at("ref"), args.str_at("scope").unwrap_or("page")) {
        (Some(r), _) if !r.trim().is_empty() => {
            f = f.str(tag::REF, r.trim().trim_matches(['[', ']']));
            2
        }
        (_, "viewport") => 1,
        (_, "main") => 3,
        (_, "page" | "") => 0,
        (_, other) => return Err(format!("unknown scope {other:?}: page, viewport or main")),
    };
    f = f.num(tag::SCOPE, scope);
    let budget = args
        .u64_at("budget")
        .or_else(|| matches!(name, "snapshot" | "read").then_some(DEFAULT_BUDGET));
    if let Some(b) = budget {
        f = f.num(tag::BUDGET, b);
    }
    if args.bool_at("include_hidden") == Some(true) {
        f = f.num(tag::HIDDEN, 1);
    }
    if name == "find" {
        let text = args.str_at("text").filter(|t| !t.is_empty());
        let role = args.str_at("role").filter(|t| !t.is_empty());
        if text.is_none() && role.is_none() {
            return Err("find needs `text`, `role` or both".into());
        }
        if let Some(t) = text {
            f = f.str(tag::TEXT, t);
        }
        if let Some(r) = role {
            f = f.str(tag::ROLE, r);
        }
        f = f.num(tag::LIMIT, args.u64_at("limit").unwrap_or(20));
    }
    Ok(f)
}

fn request(name: &str, args: &Value, cwd: &Path) -> Result<Frame, String> {
    match name {
        "open" => open_frame(args, cwd),
        "close" => Ok(Frame::new(kind::CLOSE).num(tag::PAGE, page_of(args)?)),
        "pages" => Ok(Frame::new(kind::PAGES)),
        "diff" => {
            let since = args
                .u64_at("since")
                .ok_or("`since` is required: a seq from an earlier reply")?;
            Ok(Frame::new(kind::DIFF)
                .num(tag::PAGE, page_of(args)?)
                .num(tag::SINCE, since))
        }
        _ => match VIEWS.iter().find(|v| v.0 == name) {
            Some(&(n, view, _)) => view_frame(n, view, args),
            None => Err(format!("unknown tool {name:?}")),
        },
    }
}

/// The text a reply shows the agent, and whether it is an error.
fn render(page: Option<u64>, f: &Frame) -> (String, bool) {
    let text = f.get_str(tag::TEXT).unwrap_or("");
    let mut head = match (f.kind, page) {
        (kind::ERROR, _) => return (format!("error: {text}"), true),
        (kind::OPENED, _) => format!(
            "opened p{} {}",
            f.get_num(tag::PAGE).unwrap_or(0),
            f.get_str(tag::URL).unwrap_or("")
        ),
        (_, Some(p)) => format!("p{p}"),
        _ => String::new(),
    };
    if let Some(seq) = f.get_num(tag::SEQ) {
        head.push_str(&format!(" · seq {seq}"));
    }
    if let Some(t) = f.get_num(tag::TOKENS) {
        head.push_str(&format!(" · ~{t} tokens (estimated)"));
    }
    if let Some(n) = f.get_num(tag::FOLDED).filter(|&n| n > 0) {
        head.push_str(&format!(
            " · {n} folded; a fold's [ref] as `ref` expands it"
        ));
    }
    if head.is_empty() {
        return (text.to_owned(), false);
    }
    (format!("{head}\n{text}"), false)
}

/// Carries out one tool call.
pub fn call(client: &mut Client, name: &str, args: &Value, cwd: &Path) -> (String, bool) {
    let f = match request(name, args, cwd) {
        Ok(f) => f,
        Err(e) => return (format!("error: {e}"), true),
    };
    let page = page_of(args).ok().filter(|_| name != "open");
    match client.call(&f) {
        Ok(reply) => render(page, &reply),
        Err(e) => (format!("error: {e}"), true),
    }
}
