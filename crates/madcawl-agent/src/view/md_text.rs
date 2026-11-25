//! Page text in Markdown: escaped so it cannot make a link, a reference
//! or a code span of its own, code fenced so it cannot close its fence,
//! and only the text a user sees.

use madcawl_dom::{NodeId, NodeKind};

use super::Reader;
use crate::refs::push_page_text;

/// Appends page text with the characters that would start Markdown
/// structure (`\`, `[`, `]`, `(`, `)`, `` ` ``) escaped.
pub(crate) fn push_md_text(out: &mut String, s: &str) {
    for c in s.chars() {
        if matches!(c, '\\' | '[' | ']' | '(' | ')' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
}

fn longest_backticks(s: &str) -> usize {
    let mut best = 0;
    let mut run = 0;
    for c in s.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        best = best.max(run);
    }
    best
}

/// `code` as an inline code span whose delimiter no run in it matches.
pub(crate) fn push_inline_code(out: &mut String, code: &str) {
    let tick = "`".repeat(longest_backticks(code) + 1);
    let pad = code.starts_with('`') || code.ends_with('`');
    out.push_str(&tick);
    if pad {
        out.push(' ');
    }
    push_page_text(out, code);
    if pad {
        out.push(' ');
    }
    out.push_str(&tick);
}

/// A fenced code block: the fence one backtick longer than any run in
/// the code (three at least), the language kept to `[A-Za-z0-9_+-]`.
pub(crate) fn fenced(lang: &str, code: &str) -> String {
    let fence = "`".repeat((longest_backticks(code) + 1).max(3));
    let lang: String = lang
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
        .collect();
    let mut out = format!("{fence}{lang}\n");
    push_page_text(&mut out, code);
    out.push('\n');
    out.push_str(&fence);
    out
}

/// The text of the text nodes below `n` the view writes: shown ones, or
/// hidden ones too when they are asked for.
pub(crate) fn shown_text(r: &Reader, n: NodeId) -> String {
    let mut out = String::new();
    for t in r.dom.descendants(n) {
        if r.dom.kind(t) == NodeKind::Text
            && r.writes(t).is_some()
            && let Some(s) = r.dom.text(t)
        {
            out.push_str(&crate::text::string(s));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fences_and_spans_outrun_the_backticks_inside() {
        assert_eq!(
            fenced("rust\"><x", "a\n```\nb"),
            "````rustx\na\n```\nb\n````"
        );
        let mut s = String::new();
        push_inline_code(&mut s, "a``b");
        assert_eq!(s, "```a``b```");
        let mut s = String::new();
        push_md_text(&mut s, "[Pay](e5) `x`");
        assert_eq!(s, "\\[Pay\\]\\(e5\\) \\`x\\`");
    }
}
