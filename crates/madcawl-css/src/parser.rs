//! The parser of CSS Syntax Level 3 over the token stream: rules,
//! declarations and the contents of blocks, each found as spans of the
//! source. A block is skipped to its matching close with a count of
//! nesting, not parsed, so a rule's prelude and body are parsed again
//! from their spans only when something wants them.
//!
//! The entry points follow css-parsing-tests: a rule list at the top of
//! a stylesheet (CDO and CDC dropped) or anywhere else, the old list of
//! declarations, the contents of a block (declarations and nested
//! rules, a declaration tried first and the same tokens tried again as
//! a rule), one rule and one declaration.

use crate::Span;
use crate::tokenizer::{Kind, Token, Tokenizer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsedRule {
    At {
        name: Token,
        prelude: Span,
        block: Option<Span>,
    },
    Qualified {
        prelude: Span,
        block: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declaration {
    /// The identifier token of the name
    pub name: Token,
    /// From the colon to the end, without `!important`; whitespace kept
    pub value: Span,
    pub important: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockItem {
    Declaration(Declaration),
    Rule(ParsedRule),
    /// Something that was neither, skipped
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxError {
    Empty,
    Invalid,
    ExtraInput,
}

/// Where a declaration's value ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Until {
    /// The end of the input
    End,
    /// A `;` or the end
    Semicolon,
    /// A `;`, a `}` or the end, in a block that may hold rules too
    Nested,
}

pub struct Parser<'a> {
    pub src: &'a str,
    tok: Tokenizer<'a>,
    /// The next token, and the position the tokenizer was at before it
    peeked: Option<(Token, usize)>,
    closers: Vec<Kind>,
    /// The last block skipped had a `{` or an at-keyword directly in it
    nested: bool,
}

pub(crate) fn closer(k: Kind) -> Option<Kind> {
    match k {
        Kind::Function | Kind::OpenParen => Some(Kind::CloseParen),
        Kind::OpenSquare => Some(Kind::CloseSquare),
        Kind::OpenCurly => Some(Kind::CloseCurly),
        _ => None,
    }
}

impl<'a> Parser<'a> {
    pub fn new(src: &'a str, span: Span) -> Self {
        Parser {
            src,
            tok: Tokenizer::in_span(src, span),
            peeked: None,
            closers: Vec::new(),
            nested: false,
        }
    }

    pub fn whole(src: &'a str) -> Self {
        Self::new(src, Span::new(0, src.len()))
    }

    pub fn peek(&mut self) -> Token {
        if let Some((t, _)) = self.peeked {
            return t;
        }
        let before = self.tok.position();
        let t = self.tok.next_token();
        self.peeked = Some((t, before));
        t
    }

    pub fn next_token(&mut self) -> Token {
        match self.peeked.take() {
            Some((t, _)) => t,
            None => self.tok.next_token(),
        }
    }

    /// A position `restore` can go back to.
    pub fn mark(&self) -> usize {
        match self.peeked {
            Some((_, before)) => before,
            None => self.tok.position(),
        }
    }

    pub fn restore(&mut self, mark: usize) {
        self.peeked = None;
        self.tok.reset(mark);
    }

    pub fn end(&self) -> usize {
        self.tok.end()
    }

    pub fn skip_whitespace(&mut self) {
        while self.peek().kind == Kind::Whitespace {
            self.next_token();
        }
    }

    /// After `open` was consumed, consume the rest of its component
    /// value; where its contents end (before the closing token, or at
    /// the end of the input).
    pub fn skip_block(&mut self, open: Kind) -> usize {
        let Some(first) = closer(open) else {
            return self.mark();
        };
        self.closers.clear();
        self.closers.push(first);
        self.nested = false;
        loop {
            let t = self.next_token();
            if t.kind == Kind::Eof {
                return t.span.start as usize;
            }
            if self.closers.len() == 1 && matches!(t.kind, Kind::OpenCurly | Kind::AtKeyword) {
                self.nested = true;
            }
            if Some(&t.kind) == self.closers.last() {
                self.closers.pop();
                if self.closers.is_empty() {
                    return t.span.start as usize;
                }
            } else if let Some(c) = closer(t.kind) {
                self.closers.push(c);
            }
        }
    }

    /// Whether the block of the rule just returned may hold rules: it
    /// had a `{` or an at-keyword at its own level. A block without
    /// either holds only declarations.
    pub fn block_may_nest(&self) -> bool {
        self.nested
    }

    fn at_rule(&mut self, name: Token, nested: bool) -> ParsedRule {
        let start = name.span.end as usize;
        loop {
            let t = self.peek();
            let at = t.span.start as usize;
            match t.kind {
                Kind::Semicolon | Kind::Eof | Kind::CloseCurly
                    if t.kind != Kind::CloseCurly || nested =>
                {
                    if t.kind == Kind::Semicolon {
                        self.next_token();
                    }
                    return ParsedRule::At {
                        name,
                        prelude: Span::new(start, at),
                        block: None,
                    };
                }
                Kind::OpenCurly => {
                    self.next_token();
                    let end = self.skip_block(Kind::OpenCurly);
                    return ParsedRule::At {
                        name,
                        prelude: Span::new(start, at),
                        block: Some(Span::new(t.span.end as usize, end)),
                    };
                }
                k => {
                    self.next_token();
                    self.skip_block(k);
                }
            }
        }
    }

    /// A qualified rule from the next token on; `None` when the input,
    /// a `}` (nested) or a `;` (`stop_at_semicolon`) comes before its
    /// block, which is left unconsumed.
    fn qualified_rule(&mut self, nested: bool, stop_at_semicolon: bool) -> Option<ParsedRule> {
        let start = self.peek().span.start as usize;
        loop {
            let t = self.peek();
            match t.kind {
                Kind::Eof => return None,
                Kind::Semicolon if stop_at_semicolon => return None,
                Kind::CloseCurly if nested => return None,
                Kind::OpenCurly => {
                    self.next_token();
                    let end = self.skip_block(Kind::OpenCurly);
                    return Some(ParsedRule::Qualified {
                        prelude: Span::new(start, t.span.start as usize),
                        block: Span::new(t.span.end as usize, end),
                    });
                }
                k => {
                    self.next_token();
                    self.skip_block(k);
                }
            }
        }
    }

    fn declaration(&mut self, until: Until) -> Option<Declaration> {
        let name = self.next_token();
        self.skip_whitespace();
        if self.peek().kind != Kind::Colon {
            self.skip_until(until);
            return None;
        }
        let colon = self.next_token();
        let start = colon.span.end as usize;
        // the last two non-whitespace component values: (kind, start, is `important`)
        let mut last: [Option<(Kind, u32, bool)>; 2] = [None, None];
        let (mut block, mut other) = (false, false);
        let end = loop {
            let t = self.peek();
            match t.kind {
                Kind::Eof => break t.span.start,
                Kind::Semicolon if until != Until::End => break t.span.start,
                Kind::CloseCurly if until == Until::Nested => break t.span.start,
                Kind::Whitespace => {
                    self.next_token();
                }
                k => {
                    self.next_token();
                    self.skip_block(k);
                    block |= k == Kind::OpenCurly;
                    other |= k != Kind::OpenCurly;
                    let important = k == Kind::Ident && t.value_is(self.src, "important");
                    last = [last[1], Some((k, t.span.start, important))];
                }
            }
        };
        let important = matches!(
            last,
            [
                Some((Kind::Delim(b'!'), _, _)),
                Some((Kind::Ident, _, true))
            ]
        );
        let end = match last[0] {
            Some((_, bang, _)) if important => bang,
            _ => end,
        };
        // the name as it decodes: `\2d-x` is a custom property too
        let mut chars = name.chars(self.src);
        let custom = chars.next() == Some('-') && chars.next() == Some('-');
        if until == Until::Nested && !custom && block && other {
            return None;
        }
        Some(Declaration {
            name,
            value: Span::new(start, end as usize),
            important,
        })
    }

    /// Consume component values up to a `;` (left for the caller) or
    /// the end, or a `}` in a nested block.
    fn skip_until(&mut self, until: Until) {
        loop {
            let t = self.peek();
            match t.kind {
                Kind::Eof => return,
                Kind::Semicolon if until != Until::End => return,
                Kind::CloseCurly if until == Until::Nested => return,
                k => {
                    self.next_token();
                    self.skip_block(k);
                }
            }
        }
    }

    /// The next item of a list of rules; `top_level` for a stylesheet,
    /// where CDO and CDC are dropped.
    pub fn next_rule(&mut self, top_level: bool) -> Option<BlockItem> {
        loop {
            let t = self.peek();
            match t.kind {
                Kind::Whitespace => {}
                Kind::Cdo | Kind::Cdc if top_level => {}
                Kind::Eof => return None,
                Kind::AtKeyword => {
                    self.next_token();
                    return Some(BlockItem::Rule(self.at_rule(t, false)));
                }
                _ => {
                    return Some(match self.qualified_rule(false, false) {
                        Some(r) => BlockItem::Rule(r),
                        None => BlockItem::Invalid,
                    });
                }
            }
            self.next_token();
        }
    }

    /// The next item of the old list of declarations, where a rule is
    /// only ever an at-rule.
    pub fn next_declaration(&mut self) -> Option<BlockItem> {
        loop {
            let t = self.peek();
            match t.kind {
                Kind::Whitespace | Kind::Semicolon => {}
                Kind::Eof => return None,
                Kind::AtKeyword => {
                    self.next_token();
                    return Some(BlockItem::Rule(self.at_rule(t, false)));
                }
                Kind::Ident => {
                    return Some(match self.declaration(Until::Semicolon) {
                        Some(d) => BlockItem::Declaration(d),
                        None => BlockItem::Invalid,
                    });
                }
                _ => {
                    self.skip_until(Until::Semicolon);
                    return Some(BlockItem::Invalid);
                }
            }
            self.next_token();
        }
    }

    /// The next item of a block's contents: a declaration, a nested
    /// rule or an at-rule. A `}` ends the contents.
    pub fn next_block_item(&mut self) -> Option<BlockItem> {
        loop {
            let t = self.peek();
            match t.kind {
                Kind::Whitespace | Kind::Semicolon => {}
                Kind::Eof | Kind::CloseCurly => return None,
                Kind::AtKeyword => {
                    self.next_token();
                    return Some(BlockItem::Rule(self.at_rule(t, true)));
                }
                k => {
                    let mark = self.mark();
                    if k == Kind::Ident {
                        if let Some(d) = self.declaration(Until::Nested) {
                            return Some(BlockItem::Declaration(d));
                        }
                        self.restore(mark);
                    }
                    return Some(match self.qualified_rule(true, true) {
                        Some(r) => BlockItem::Rule(r),
                        None => BlockItem::Invalid,
                    });
                }
            }
            self.next_token();
        }
    }
}

fn collect(mut p: Parser<'_>, f: impl Fn(&mut Parser<'_>) -> Option<BlockItem>) -> Vec<BlockItem> {
    std::iter::from_fn(|| f(&mut p)).collect()
}

/// Parse a stylesheet's rules (`top_level`) or a list of rules.
pub fn parse_rule_list(src: &str, top_level: bool) -> Vec<BlockItem> {
    collect(Parser::whole(src), |p| p.next_rule(top_level))
}

pub fn parse_declaration_list(src: &str) -> Vec<BlockItem> {
    collect(Parser::whole(src), |p| p.next_declaration())
}

pub fn parse_block_contents(src: &str, span: Span) -> Vec<BlockItem> {
    collect(Parser::new(src, span), |p| p.next_block_item())
}

pub fn parse_rule(src: &str) -> Result<ParsedRule, SyntaxError> {
    let mut p = Parser::whole(src);
    p.skip_whitespace();
    let t = p.peek();
    let rule = match t.kind {
        Kind::Eof => return Err(SyntaxError::Empty),
        Kind::AtKeyword => {
            p.next_token();
            p.at_rule(t, false)
        }
        _ => p.qualified_rule(false, false).ok_or(SyntaxError::Invalid)?,
    };
    p.skip_whitespace();
    match p.peek().kind {
        Kind::Eof => Ok(rule),
        _ => Err(SyntaxError::ExtraInput),
    }
}

pub fn parse_declaration(src: &str) -> Result<Declaration, SyntaxError> {
    let mut p = Parser::whole(src);
    p.skip_whitespace();
    match p.peek().kind {
        Kind::Eof => Err(SyntaxError::Empty),
        Kind::Ident => p.declaration(Until::End).ok_or(SyntaxError::Invalid),
        _ => Err(SyntaxError::Invalid),
    }
}
