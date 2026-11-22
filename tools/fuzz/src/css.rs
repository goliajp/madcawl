//! The two CSS targets. Each decodes the input as a stylesheet's bytes
//! (`decode_stylesheet`: BOM, `@charset`, UTF-8) and returns what came
//! out flattened to bytes, and the tokenizer's work.
//!
//! The tokenizer: fed whole, every token of the input, and the input
//! parsed as a `Stylesheet`, each style rule's declarations (asked for,
//! or parsed with the sheet when the block held nested rules, then
//! those after a nested rule) checked against the declarations of its
//! block parsed on its own; fed in chunks,
//! the input is cut at a seeded choice of the boundaries the rule
//! parser reports (preludes, blocks, declaration values, down to
//! `MAX_DEPTH` nested blocks) and each piece tokenized on its own. The
//! two streams must be equal: that is what lets a stylesheet keep a
//! rule's prelude and body as spans and tokenize them again later.
//!
//! The selectors: every qualified rule's prelude, and the whole input,
//! parsed as a selector list. Fed whole, each result serialized; fed in
//! chunks, each serialization parsed and serialized again. The two must
//! be equal: a serialized selector parses back to itself.

use madcawl_css::selector::{Context, parse_selector_list};
use madcawl_css::{
    BlockItem, Declaration, Kind, Names, ParsedRule, Parser, Rule, Span, Stylesheet, Token,
    Tokenizer, decode_stylesheet, parse_block_contents, steps,
};

use crate::targets::Out;
use crate::wpt::{Feed, Rng};

/// How deep into nested blocks the boundaries are collected. Parsing a
/// block again from its span costs its length once per level.
const MAX_DEPTH: usize = 8;

fn text(input: &[u8]) -> String {
    decode_stylesheet(input, None, None).0
}

fn put_token(t: &Token, src: &str, out: &mut Vec<u8>) {
    let tag = format!("{:?}", t.kind);
    out.extend_from_slice(tag.as_bytes());
    out.push(t.flags);
    match t.kind {
        Kind::Number | Kind::Percentage | Kind::Dimension => {
            out.extend_from_slice(t.value.of(src).as_bytes());
            out.extend_from_slice(&t.number(src).to_bits().to_le_bytes());
        }
        Kind::Whitespace | Kind::Delim(_) => {}
        _ => {
            for c in t.chars(src) {
                let mut b = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            }
        }
    }
    if t.kind == Kind::Dimension {
        out.push(b'/');
        for c in t.unit_chars(src) {
            let mut b = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
        }
    }
    out.push(0xFF);
}

fn tokens(src: &str, span: Span, out: &mut Vec<u8>) {
    let mut tok = Tokenizer::in_span(src, span);
    loop {
        let t = tok.next_token();
        if t.kind == Kind::Eof {
            return;
        }
        put_token(&t, src, out);
    }
}

/// Every span the rule parser reports, into `rules` (qualified rule
/// preludes) and `cuts` (every start and end).
fn walk(src: &str, rules: &mut Vec<Span>, cuts: &mut Vec<u32>) {
    // (span, a block's contents or the stylesheet, depth)
    let mut work = vec![(Span::new(0, src.len()), false, 0)];
    while let Some((span, block, depth)) = work.pop() {
        let mut p = Parser::new(src, span);
        loop {
            let item = if block {
                p.next_block_item()
            } else {
                p.next_rule(true)
            };
            let Some(item) = item else { break };
            let rule = match item {
                BlockItem::Declaration(d) => {
                    cuts.extend([d.name.span.start, d.value.start, d.value.end]);
                    continue;
                }
                BlockItem::Invalid => continue,
                BlockItem::Rule(r) => r,
            };
            let (prelude, body) = match rule {
                ParsedRule::At { prelude, block, .. } => (prelude, block),
                ParsedRule::Qualified { prelude, block } => {
                    rules.push(prelude);
                    (prelude, Some(block))
                }
            };
            cuts.extend([prelude.start, prelude.end]);
            if let Some(b) = body {
                cuts.extend([b.start, b.end]);
                if depth < MAX_DEPTH {
                    work.push((b, true, depth + 1));
                }
            }
        }
    }
}

/// Every style rule's declarations as the sheet has them: those its
/// body gives, then those of each `NestedDeclarations` among its rules,
/// must be the declarations of its block parsed on its own, in order.
fn check(sheet: &Stylesheet, rules: &[Rule]) {
    for r in rules {
        match r {
            Rule::Style(s) => {
                let mut got: Vec<Declaration> = sheet.declarations(&s.body).to_vec();
                for n in &s.rules {
                    if let Rule::NestedDeclarations(d) = n {
                        got.extend_from_slice(d);
                    }
                }
                let want: Vec<Declaration> = parse_block_contents(sheet.source(), s.body.span)
                    .into_iter()
                    .filter_map(|i| match i {
                        BlockItem::Declaration(d) => Some(d),
                        _ => None,
                    })
                    .collect();
                assert!(
                    got == want,
                    "declarations of the block at {:?}",
                    s.body.span
                );
                check(sheet, &s.rules);
            }
            Rule::FontFace(b) | Rule::Page { body: b, .. } => {
                sheet.declarations(b);
            }
            Rule::Media { rules, .. } | Rule::Supports { rules, .. } => check(sheet, rules),
            Rule::Layer { rules: Some(r), .. } => check(sheet, r),
            _ => {}
        }
    }
}

pub fn tokenizer(input: &[u8], feed: Feed) -> Out {
    let src = &text(input);
    steps::take();
    let mut flat = Vec::new();
    match feed {
        Feed::Chunks { seed, max } => {
            let (mut rules, mut cuts) = (Vec::new(), Vec::new());
            walk(src, &mut rules, &mut cuts);
            cuts.sort_unstable();
            cuts.dedup();
            let mut rng = Rng(seed | 1);
            let keep = 1 + (max as u64 % 4);
            let mut from = 0;
            for &c in &cuts {
                if rng.next().is_multiple_of(keep) {
                    tokens(src, Span::new(from, c as usize), &mut flat);
                    from = c as usize;
                }
            }
            tokens(src, Span::new(from, src.len()), &mut flat);
        }
        _ => {
            tokens(src, Span::new(0, src.len()), &mut flat);
            let sheet = Stylesheet::parse(src.clone()).expect("under 4 GiB");
            check(&sheet, sheet.rules());
        }
    }
    Out {
        flat,
        steps: steps::take(),
        audit: None,
    }
}

pub fn selectors(input: &[u8], feed: Feed) -> Out {
    let src = &text(input);
    steps::take();
    let mut names = Names::new();
    let prefix = names.intern("svg");
    let url = names.intern("http://www.w3.org/2000/svg");
    let prefixes = [(prefix, url)];
    let cx = Context {
        default_ns: None,
        prefixes: &prefixes,
        nesting: true,
    };
    let mut spans = vec![Span::new(0, src.len())];
    walk(src, &mut spans, &mut Vec::new());
    let again = matches!(feed, Feed::Chunks { .. });
    let mut flat = Vec::new();
    let mut css = String::new();
    for span in spans {
        let Ok(list) = parse_selector_list(src, span, &mut names, &cx) else {
            flat.extend_from_slice(b"invalid\n");
            continue;
        };
        css.clear();
        list.to_css(&names, &mut css);
        if again {
            let first = std::mem::take(&mut css);
            let span = Span::new(0, first.len());
            match parse_selector_list(&first, span, &mut names, &cx) {
                Ok(l) => l.to_css(&names, &mut css),
                Err(_) => css.push_str("serialization did not parse"),
            }
        }
        flat.extend_from_slice(css.as_bytes());
        flat.push(b'\n');
    }
    Out {
        flat,
        steps: steps::take(),
        audit: None,
    }
}
