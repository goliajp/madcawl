//! `@supports` conditions. A declaration is supported when the property
//! is one this engine parses and the value parses, or, for the
//! properties it does not compute yet, when the name is one WebKit
//! supports and the value is not empty: the answer a page gets from
//! Safari, which is what decides the rules it writes for it.

use crate::Span;
use crate::names::Names;
use crate::selector::{Context, parse_selector_list};
use crate::tokenizer::{Kind, Token};

use super::props;
use super::tokens;

/// Whether the condition `src[span]` holds.
pub fn supports(src: &str, span: Span) -> bool {
    let t = tokens(src, span);
    let mut e = Eval { src, t: &t, i: 0 };
    e.condition() && e.i == t.len()
}

struct Eval<'a> {
    src: &'a str,
    t: &'a [Token],
    i: usize,
}

impl Eval<'_> {
    fn ident(&self, name: &str) -> bool {
        self.t
            .get(self.i)
            .is_some_and(|t| t.kind == Kind::Ident && t.value_is(self.src, name))
    }

    fn condition(&mut self) -> bool {
        if self.ident("not") {
            self.i += 1;
            return !self.in_parens();
        }
        let mut r = self.in_parens();
        let mut joiner: Option<bool> = None;
        loop {
            let is_and = self.ident("and");
            if !is_and && !self.ident("or") || joiner.is_some_and(|j| j != is_and) {
                return r;
            }
            joiner = Some(is_and);
            self.i += 1;
            let next = self.in_parens();
            r = if is_and { r && next } else { r || next };
        }
    }

    fn in_parens(&mut self) -> bool {
        let Some(&open) = self.t.get(self.i) else {
            return false;
        };
        let mut depth = 0;
        let mut close = self.t.len();
        for (k, t) in self.t.iter().enumerate().skip(self.i) {
            match t.kind {
                Kind::OpenParen | Kind::Function => depth += 1,
                Kind::CloseParen => {
                    depth -= 1;
                    if depth == 0 {
                        close = k;
                        break;
                    }
                }
                _ => {}
            }
        }
        let inner = &self.t[(self.i + 1).min(close)..close];
        self.i = (close + 1).min(self.t.len());
        match open.kind {
            Kind::OpenParen => self.parens(inner),
            Kind::Function if open.value_is(self.src, "selector") => {
                let (Some(a), Some(b)) = (inner.first(), inner.last()) else {
                    return false;
                };
                let span = Span::new(a.span.start as usize, b.span.end as usize);
                let mut names = Names::new();
                parse_selector_list(self.src, span, &mut names, &Context::default()).is_ok()
            }
            _ => false,
        }
    }

    fn parens(&self, inner: &[Token]) -> bool {
        let Some(first) = inner.first() else {
            return false;
        };
        let decl = first.kind == Kind::Ident
            && inner.get(1).is_some_and(|t| t.kind == Kind::Colon)
            && !first.value_is(self.src, "not");
        if !decl {
            let mut sub = Eval {
                src: self.src,
                t: inner,
                i: 0,
            };
            return sub.condition() && sub.i == inner.len();
        }
        let Some(last) = inner.last().filter(|_| inner.len() > 2) else {
            return false;
        };
        let mut name = String::new();
        first.write_value(self.src, &mut name);
        name.make_ascii_lowercase();
        let value = Span::new(inner[2].span.start as usize, last.span.end as usize);
        supported(&name, self.src, value)
    }
}

fn supported(name: &str, src: &str, value: Span) -> bool {
    if name.starts_with("--") {
        return true;
    }
    match props::declaration_parses(name, src, value) {
        Some(ok) => ok,
        None => KNOWN.binary_search(&name).is_ok(),
    }
}

/// Properties WebKit supports that this engine does not compute yet,
/// sorted.
pub(crate) const KNOWN: &[&str] = &[
    "-webkit-appearance",
    "-webkit-backdrop-filter",
    "-webkit-box-orient",
    "-webkit-font-smoothing",
    "-webkit-line-clamp",
    "-webkit-mask",
    "-webkit-mask-image",
    "-webkit-overflow-scrolling",
    "-webkit-tap-highlight-color",
    "-webkit-text-fill-color",
    "-webkit-text-size-adjust",
    "-webkit-text-stroke",
    "-webkit-user-select",
    "accent-color",
    "align-content",
    "align-items",
    "align-self",
    "all",
    "animation",
    "animation-delay",
    "animation-direction",
    "animation-duration",
    "animation-fill-mode",
    "animation-iteration-count",
    "animation-name",
    "animation-play-state",
    "animation-timing-function",
    "appearance",
    "aspect-ratio",
    "backdrop-filter",
    "backface-visibility",
    "background",
    "background-attachment",
    "background-blend-mode",
    "background-clip",
    "background-color",
    "background-image",
    "background-origin",
    "background-position",
    "background-repeat",
    "background-size",
    "block-size",
    "border",
    "border-block",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-bottom-style",
    "border-bottom-width",
    "border-collapse",
    "border-color",
    "border-image",
    "border-inline",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-radius",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-spacing",
    "border-style",
    "border-top",
    "border-top-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-top-style",
    "border-top-width",
    "border-width",
    "bottom",
    "box-decoration-break",
    "box-shadow",
    "box-sizing",
    "break-after",
    "break-before",
    "break-inside",
    "caption-side",
    "caret-color",
    "clear",
    "clip",
    "clip-path",
    "color",
    "color-scheme",
    "column-count",
    "column-gap",
    "column-rule",
    "column-width",
    "columns",
    "contain",
    "contain-intrinsic-size",
    "container",
    "container-name",
    "container-type",
    "content",
    "counter-increment",
    "counter-reset",
    "counter-set",
    "cursor",
    "direction",
    "empty-cells",
    "fill",
    "filter",
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-flow",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "font",
    "font-display",
    "font-family",
    "font-feature-settings",
    "font-kerning",
    "font-optical-sizing",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-synthesis",
    "font-variant",
    "font-variant-caps",
    "font-variant-ligatures",
    "font-variant-numeric",
    "font-variation-settings",
    "font-weight",
    "gap",
    "grid",
    "grid-area",
    "grid-auto-columns",
    "grid-auto-flow",
    "grid-auto-rows",
    "grid-column",
    "grid-column-end",
    "grid-column-start",
    "grid-row",
    "grid-row-end",
    "grid-row-start",
    "grid-template",
    "grid-template-areas",
    "grid-template-columns",
    "grid-template-rows",
    "hanging-punctuation",
    "height",
    "hyphens",
    "image-rendering",
    "inline-size",
    "inset",
    "inset-block",
    "inset-block-end",
    "inset-block-start",
    "inset-inline",
    "inset-inline-end",
    "inset-inline-start",
    "isolation",
    "justify-content",
    "justify-items",
    "justify-self",
    "left",
    "letter-spacing",
    "line-break",
    "line-height",
    "list-style",
    "list-style-image",
    "list-style-position",
    "list-style-type",
    "margin",
    "margin-block",
    "margin-block-end",
    "margin-block-start",
    "margin-bottom",
    "margin-inline",
    "margin-inline-end",
    "margin-inline-start",
    "margin-left",
    "margin-right",
    "margin-top",
    "mask",
    "mask-image",
    "max-block-size",
    "max-height",
    "max-inline-size",
    "max-width",
    "min-block-size",
    "min-height",
    "min-inline-size",
    "min-width",
    "mix-blend-mode",
    "object-fit",
    "object-position",
    "offset",
    "order",
    "orphans",
    "outline",
    "outline-color",
    "outline-offset",
    "outline-style",
    "outline-width",
    "overflow",
    "overflow-anchor",
    "overflow-wrap",
    "overflow-x",
    "overflow-y",
    "overscroll-behavior",
    "padding",
    "padding-block",
    "padding-block-end",
    "padding-block-start",
    "padding-bottom",
    "padding-inline",
    "padding-inline-end",
    "padding-inline-start",
    "padding-left",
    "padding-right",
    "padding-top",
    "page-break-after",
    "page-break-before",
    "page-break-inside",
    "paint-order",
    "perspective",
    "perspective-origin",
    "place-content",
    "place-items",
    "place-self",
    "pointer-events",
    "quotes",
    "resize",
    "right",
    "rotate",
    "row-gap",
    "scale",
    "scroll-behavior",
    "scroll-margin",
    "scroll-padding",
    "scroll-snap-align",
    "scroll-snap-type",
    "scrollbar-gutter",
    "shape-outside",
    "stroke",
    "stroke-width",
    "tab-size",
    "table-layout",
    "text-align",
    "text-align-last",
    "text-decoration",
    "text-decoration-color",
    "text-decoration-line",
    "text-decoration-style",
    "text-decoration-thickness",
    "text-emphasis",
    "text-indent",
    "text-overflow",
    "text-rendering",
    "text-shadow",
    "text-transform",
    "text-underline-offset",
    "text-underline-position",
    "text-wrap",
    "top",
    "touch-action",
    "transform",
    "transform-origin",
    "transform-style",
    "transition",
    "transition-delay",
    "transition-duration",
    "transition-property",
    "transition-timing-function",
    "translate",
    "unicode-bidi",
    "user-select",
    "vertical-align",
    "white-space",
    "widows",
    "width",
    "will-change",
    "word-break",
    "word-spacing",
    "word-wrap",
    "writing-mode",
    "z-index",
    "zoom",
];

#[cfg(test)]
mod tests {
    #[test]
    fn known_is_sorted() {
        assert!(super::KNOWN.windows(2).all(|w| w[0] < w[1]));
    }
}
