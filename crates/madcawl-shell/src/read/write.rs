//! The text form: a header, then one line per item,
//! `<indent><kind> [<ref>] <content>`, two spaces of indent per level.
//! A preformatted block's lines follow it, each after `> `.

use super::text::{push_reference, quoted};
use super::{Kind, Read};

pub const FORMAT: &str = "madcawl-read 1";

fn kind(out: &mut String, k: Kind) {
    match k {
        Kind::Region(r) | Kind::Control(r) => out.push_str(r),
        Kind::Heading(l) => {
            out.push('h');
            out.push(char::from(b'0' + l));
        }
        Kind::Text => out.push_str("text"),
        Kind::ListItem => out.push_str("li"),
        Kind::Table => out.push_str("table"),
        Kind::Row => out.push_str("row"),
        Kind::HeadRow => out.push_str("headrow"),
        Kind::Pre => out.push_str("pre"),
        Kind::Choice => out.push_str("option"),
    }
}

pub fn write_text(r: &Read, page: &str, out: &mut String) {
    out.push_str(FORMAT);
    out.push('\n');
    out.push_str("page ");
    quoted(out, page);
    out.push('\n');
    if let Some(t) = &r.title {
        out.push_str("title ");
        quoted(out, t);
        out.push('\n');
    }
    out.push_str("---\n");
    for it in &r.items {
        let indent = 2 * it.depth as usize;
        out.extend(std::iter::repeat_n(' ', indent));
        kind(out, it.kind);
        out.push_str(" [");
        push_reference(out, it.node);
        out.push(']');
        if it.kind == Kind::Pre {
            out.push('\n');
            for l in it.text.lines() {
                out.extend(std::iter::repeat_n(' ', indent + 2));
                out.push_str("> ");
                out.push_str(l);
                out.push('\n');
            }
            continue;
        }
        if !it.text.is_empty() {
            out.push(' ');
            out.push_str(&it.text);
        }
        out.push('\n');
    }
}
