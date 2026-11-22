//! The results of the parser in the JSON form css-parsing-tests uses.

use madcawl_css::{
    AT_EOF, BlockItem, ComponentValue, INTEGER, Kind, ParsedRule, Span, SyntaxError, Token,
    parse_component_value_list,
};

use super::json::Json;

fn s(v: &str) -> Json {
    Json::Str(v.as_bytes().to_vec())
}

fn arr(items: Vec<Json>) -> Json {
    Json::Arr(items)
}

fn value(t: &Token, src: &str) -> Json {
    let mut out = String::new();
    t.write_value(src, &mut out);
    s(&out)
}

fn numeric(name: &str, t: &Token, src: &str) -> Vec<Json> {
    let kind = if t.is(INTEGER) { "integer" } else { "number" };
    vec![
        s(name),
        s(t.value.of(src)),
        Json::Num(t.number(src)),
        s(kind),
    ]
}

/// One token as one or two JSON values: a string or url the input cut
/// short is followed by an error.
fn token(t: &Token, src: &str, out: &mut Vec<Json>) {
    let j = match t.kind {
        Kind::Ident => arr(vec![s("ident"), value(t, src)]),
        Kind::Function => unreachable!("functions are blocks"),
        Kind::AtKeyword => arr(vec![s("at-keyword"), value(t, src)]),
        Kind::Hash { id } => arr(vec![
            s("hash"),
            value(t, src),
            s(if id { "id" } else { "unrestricted" }),
        ]),
        Kind::String => arr(vec![s("string"), value(t, src)]),
        Kind::BadString => arr(vec![s("error"), s("bad-string")]),
        Kind::Url => arr(vec![s("url"), value(t, src)]),
        Kind::BadUrl => arr(vec![s("error"), s("bad-url")]),
        Kind::Delim(c) => s(&(c as char).to_string()),
        Kind::Number => arr(numeric("number", t, src)),
        Kind::Percentage => arr(numeric("percentage", t, src)),
        Kind::Dimension => {
            let mut v = numeric("dimension", t, src);
            v.push(s(&t.unit_chars(src).collect::<String>()));
            arr(v)
        }
        Kind::UnicodeRange { start, end } => arr(vec![
            s("unicode-range"),
            Json::Num(start as f64),
            Json::Num(end as f64),
        ]),
        Kind::Whitespace => s(" "),
        Kind::Cdo => s("<!--"),
        Kind::Cdc => s("-->"),
        Kind::Colon => s(":"),
        Kind::Semicolon => s(";"),
        Kind::Comma => s(","),
        Kind::CloseSquare => arr(vec![s("error"), s("]")]),
        Kind::CloseParen => arr(vec![s("error"), s(")")]),
        Kind::CloseCurly => arr(vec![s("error"), s("}")]),
        Kind::IncludeMatch => s("~="),
        Kind::DashMatch => s("|="),
        Kind::PrefixMatch => s("^="),
        Kind::SuffixMatch => s("$="),
        Kind::SubstringMatch => s("*="),
        Kind::Column => s("||"),
        Kind::OpenSquare | Kind::OpenParen | Kind::OpenCurly | Kind::Eof => {
            unreachable!("{:?} is not a value", t.kind)
        }
    };
    out.push(j);
    if t.is(AT_EOF) {
        let what = if t.kind == Kind::Url {
            "eof-in-url"
        } else {
            "eof-in-string"
        };
        out.push(arr(vec![s("error"), s(what)]));
    }
}

pub fn component(v: &ComponentValue, src: &str, out: &mut Vec<Json>) {
    match v {
        ComponentValue::Token(t) => token(t, src, out),
        ComponentValue::Block { open, contents } => {
            let mut items = match open.kind {
                Kind::Function => vec![s("function"), value(open, src)],
                Kind::OpenCurly => vec![s("{}")],
                Kind::OpenSquare => vec![s("[]")],
                _ => vec![s("()")],
            };
            for c in contents {
                component(c, src, &mut items);
            }
            out.push(arr(items));
        }
    }
}

pub fn list(src: &str, span: Span) -> Json {
    let mut out = Vec::new();
    for v in parse_component_value_list(src, span) {
        component(&v, src, &mut out);
    }
    arr(out)
}

pub fn rule(r: &ParsedRule, src: &str) -> Json {
    match r {
        ParsedRule::At {
            name,
            prelude,
            block,
        } => arr(vec![
            s("at-rule"),
            value(name, src),
            list(src, *prelude),
            block.map_or(Json::Null, |b| list(src, b)),
        ]),
        ParsedRule::Qualified { prelude, block } => arr(vec![
            s("qualified rule"),
            list(src, *prelude),
            list(src, *block),
        ]),
    }
}

pub fn item(i: &BlockItem, src: &str) -> Json {
    match i {
        BlockItem::Declaration(d) => arr(vec![
            s("declaration"),
            value(&d.name, src),
            list(src, d.value),
            Json::Bool(d.important),
        ]),
        BlockItem::Rule(r) => rule(r, src),
        BlockItem::Invalid => error("invalid"),
    }
}

pub fn items(v: &[BlockItem], src: &str) -> Json {
    arr(v.iter().map(|i| item(i, src)).collect())
}

pub fn error(what: &str) -> Json {
    arr(vec![s("error"), s(what)])
}

pub fn syntax_error(e: SyntaxError) -> Json {
    error(match e {
        SyntaxError::Empty => "empty",
        SyntaxError::Invalid => "invalid",
        SyntaxError::ExtraInput => "extra-input",
    })
}
