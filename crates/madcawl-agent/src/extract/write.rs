//! Records as text: a first line saying what they are, how many there
//! are and where the next ones start, then JSON objects a line or a
//! Markdown table. Page text is escaped so it cannot forge a reference
//! (`[e…` is written `[\e…`). Records go in whole until the budget is
//! spent; the first always goes in.

use std::borrow::Cow;

use super::{ExtractOpts, Format, Records, Row, Value};
use crate::refs::{push_page_text, push_quoted};
use crate::text::estimate_tokens;
use crate::view::{View, finish};

/// `s` as a JSON string, page text escaped.
pub(crate) fn push_json(out: &mut String, s: &str) {
    let mut esc = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => esc.push_str("\\\""),
            '\\' => esc.push_str("\\\\"),
            '\n' => esc.push_str("\\n"),
            '\r' => esc.push_str("\\r"),
            '\t' => esc.push_str("\\t"),
            c if (c as u32) < 0x20 => esc.push_str(&format!("\\u{:04x}", c as u32)),
            c => esc.push(c),
        }
    }
    out.push('"');
    // a JSON string reads `[\\e` as `[\e`, the same escape a view writes
    let mut rest = esc.as_str();
    while let Some(k) = rest.find("[e") {
        out.push_str(&rest[..k + 1]);
        out.push_str("\\\\");
        rest = &rest[k + 1..];
    }
    out.push_str(rest);
    out.push('"');
}

fn json_row(row: &Row) -> String {
    let mut s = String::from("{\"ref\":");
    push_json(&mut s, &row.reference);
    for (k, v) in &row.fields {
        s.push(',');
        push_json(&mut s, k);
        s.push(':');
        match v {
            Value::Text(t) => push_json(&mut s, t),
            Value::Flag(b) => s.push_str(if *b { "true" } else { "false" }),
            Value::List(items) => {
                s.push('[');
                for (i, t) in items.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    push_json(&mut s, t);
                }
                s.push(']');
            }
        }
    }
    s.push('}');
    s
}

fn md_cell(out: &mut String, s: &str) {
    let mut t = String::new();
    push_page_text(&mut t, &crate::text::collapsed(s));
    out.push_str(&t.replace('|', "\\|"));
}

fn md_row(row: &Row, columns: &[String]) -> String {
    let mut s = String::from("| ");
    s.push_str(&row.reference);
    s.push_str(" |");
    for c in columns {
        s.push(' ');
        match row.get(c) {
            Some(Value::Text(t)) => md_cell(&mut s, t),
            Some(Value::Flag(true)) => s.push_str("yes"),
            Some(Value::List(items)) => md_cell(&mut s, &items.join(" · ")),
            _ => {}
        }
        s.push_str(" |");
    }
    s
}

/// The keys of every record, the table's own order first.
fn columns(recs: &Records) -> Vec<String> {
    let mut cols = recs.columns.clone();
    let mut seen: std::collections::HashSet<&str> =
        recs.columns.iter().map(String::as_str).collect();
    for row in &recs.rows {
        for (k, _) in &row.fields {
            if seen.insert(k) {
                cols.push(k.to_string());
            }
        }
    }
    cols
}

/// What the records are, how many, which are here and where more start.
fn first_line(recs: &Records, total: usize, from: usize, end: usize) -> String {
    let mut text = format!("extract {}", recs.what.name());
    if let Some(s) = &recs.source {
        text.push_str(&format!(" [{s}]"));
    }
    if !recs.label.is_empty() {
        text.push(' ');
        push_quoted(&mut text, &recs.label);
    }
    text.push_str(&format!(" · {total} records"));
    if end > from {
        text.push_str(&format!(" · {from}-{} here", end - 1));
    }
    if end < total {
        text.push_str(&format!(" · more from offset {end}"));
    }
    if !recs.note.is_empty() {
        text.push_str(" · ");
        push_quoted(&mut text, &recs.note);
    }
    text
}

/// The `total` records `row` makes, of which `recs` holds those it
/// has, and its label, source and columns.
pub(super) fn write<'a>(
    recs: &Records,
    total: usize,
    opts: &ExtractOpts,
    row: impl Fn(usize) -> Cow<'a, Row>,
) -> View {
    let from = opts.offset.min(total);
    let cols = columns(recs);
    let mut lines = Vec::new();
    let mut spent = 0;
    let mut end = from;
    for i in from..total {
        let row = row(i);
        let line = match opts.format {
            Format::JsonLines => json_row(&row),
            Format::Markdown => md_row(&row, &cols),
        };
        let t = estimate_tokens(&line) + 1;
        if opts.budget.is_some_and(|b| spent + t > b) && end > from {
            break;
        }
        spent += t;
        lines.push(line);
        end += 1;
    }
    let mut text = first_line(recs, total, from, end);
    text.push('\n');
    if opts.format == Format::Markdown && !lines.is_empty() {
        let mut head = String::from("| ref |");
        let mut rule = String::from("| --- |");
        for c in &cols {
            head.push(' ');
            md_cell(&mut head, c);
            head.push_str(" |");
            rule.push_str(" --- |");
        }
        text.push_str(&head);
        text.push('\n');
        text.push_str(&rule);
        text.push('\n');
    }
    for l in &lines {
        text.push_str(l);
        text.push('\n');
    }
    finish(text, total - end)
}
