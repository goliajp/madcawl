//! Finding and substituting `var()` and `env()`.

use std::cell::Cell;

use crate::Span;
use crate::tokenizer::{Kind, Tokenizer};

/// The longest text substitution may make; longer is guaranteed-invalid,
/// as a few properties that each use the last twice would otherwise
/// grow exponentially.
pub(crate) const MAX_LEN: usize = 1 << 20;

pub(crate) fn has_var(src: &str) -> bool {
    let mut t = Tokenizer::new(src);
    loop {
        let tok = t.next_token();
        match tok.kind {
            Kind::Eof => return false,
            Kind::Function if tok.value_is(src, "var") || tok.value_is(src, "env") => {
                return true;
            }
            _ => {}
        }
    }
}

/// Whether every `var()` and `env()` in `src[span]` is well formed: a
/// declaration with one that is not is invalid when parsed.
pub(crate) fn vars_valid(src: &str, span: Span) -> bool {
    let mut t = Tokenizer::in_span(src, span);
    loop {
        let tok = t.next_token();
        let is_var = tok.value_is(src, "var");
        match tok.kind {
            Kind::Eof => return true,
            Kind::Function if is_var || tok.value_is(src, "env") => {
                match arguments(src, &mut t, is_var) {
                    Some((_, Some(f))) if !vars_valid(src, f) => return false,
                    Some(_) => {}
                    None => return false,
                }
            }
            _ => {}
        }
    }
}

/// The environment variables a page sees in a window with no insets.
fn env(name: &str) -> Option<&'static str> {
    name.starts_with("safe-area-inset-").then_some("0px")
}

/// `src` with every `var()` and `env()` replaced; `None` when one has
/// no value and no fallback, or the result is longer than `MAX_LEN` or
/// than what is left of `budget`. What it makes, or had made when it
/// gave up, comes off `budget`.
/// With `separate`, each substituted value is
/// kept apart from what is around it by empty comments, so that
/// tokenizing the result gives the tokens substitution gave.
pub(crate) fn substitute<V: AsRef<str>>(
    src: &str,
    separate: bool,
    get: &mut dyn FnMut(&str) -> Option<V>,
    budget: &Cell<usize>,
) -> Option<String> {
    let mut out = String::with_capacity(src.len().min(MAX_LEN.min(budget.get())));
    let span = Span::new(0, src.len());
    let done = fill(src, span, separate, get, &mut out, budget);
    budget.set(budget.get().saturating_sub(out.len()));
    done.then_some(out)
}

fn fill<V: AsRef<str>>(
    src: &str,
    span: Span,
    separate: bool,
    get: &mut dyn FnMut(&str) -> Option<V>,
    out: &mut String,
    budget: &Cell<usize>,
) -> bool {
    let mut t = Tokenizer::in_span(src, span);
    loop {
        let tok = t.next_token();
        let is_var = tok.value_is(src, "var");
        match tok.kind {
            Kind::Eof => return true,
            Kind::Function if is_var || tok.value_is(src, "env") => {
                let Some((name, fallback)) = arguments(src, &mut t, is_var) else {
                    return false;
                };
                if separate {
                    out.push_str("/**/");
                }
                let found = if is_var {
                    get(&name).map(|v| out.push_str(v.as_ref()))
                } else {
                    env(&name).map(|v| out.push_str(v))
                };
                let found = match (found, fallback) {
                    (Some(()), _) => true,
                    (None, Some(f)) => fill(src, f, separate, get, out, budget),
                    (None, None) => false,
                };
                if !found {
                    return false;
                }
                if separate {
                    out.push_str("/**/");
                }
            }
            _ => out.push_str(tok.span.of(src)),
        }
        // read again each time: values `get` resolved meanwhile have
        // already taken their share
        if out.len() > MAX_LEN.min(budget.get()) {
            return false;
        }
    }
}

/// After `var(` or `env(`: the name, and the fallback after the comma if
/// there is one, without the whitespace at its ends; the tokenizer is
/// left after the closing parenthesis. `None` when they are not well
/// formed.
fn arguments(src: &str, t: &mut Tokenizer<'_>, is_var: bool) -> Option<(String, Option<Span>)> {
    let mut name = None;
    // the fallback, and whether a token other than whitespace began it
    let mut fallback: Option<(Span, bool)> = None;
    let mut depth = 0u32;
    loop {
        let tok = t.next_token();
        match tok.kind {
            // the end closes what is open, at any depth
            Kind::Eof => break,
            Kind::CloseParen if depth == 0 => break,
            Kind::Function | Kind::OpenParen => depth += 1,
            Kind::CloseParen => depth -= 1,
            _ => {}
        }
        if let Some((f, begun)) = &mut fallback {
            if tok.kind != Kind::Whitespace {
                if !*begun {
                    f.start = tok.span.start;
                    *begun = true;
                }
                f.end = tok.span.end;
            }
            continue;
        }
        match tok.kind {
            Kind::Whitespace => {}
            Kind::Ident if name.is_none() => {
                let mut s = String::new();
                tok.write_value(src, &mut s);
                // `--` alone is reserved
                if is_var && (!s.starts_with("--") || s == "--") {
                    return None;
                }
                name = Some(s);
            }
            Kind::Comma if name.is_some() => {
                let at = tok.span.end as usize;
                fallback = Some((Span::new(at, at), false));
            }
            _ => return None,
        }
    }
    Some((name?, fallback.map(|(f, _)| f)))
}
