//! The properties the cascade knows, and their declared values parsed
//! from a declaration's value tokens.

use crate::Span;
use crate::parser::Declaration;
use crate::tokenizer::Kind;

use super::tokens;
use super::values::{ContentVisibility, Display, Float, Position, Visibility};

macro_rules! props {
    ($($v:ident = $s:literal, $inherited:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Prop {
            $($v),*
        }

        impl Prop {
            pub const ALL: &[Prop] = &[$(Prop::$v),*];

            pub fn name(self) -> &'static str {
                match self {
                    $(Prop::$v => $s),*
                }
            }

            /// The one named by an ASCII-lowercase name.
            pub fn from_name(name: &str) -> Option<Prop> {
                match name {
                    $($s => Some(Prop::$v),)*
                    _ => None,
                }
            }

            pub fn inherited(self) -> bool {
                match self {
                    $(Prop::$v => $inherited),*
                }
            }
        }
    };
}

props! {
    Display = "display", false;
    Visibility = "visibility", true;
    ContentVisibility = "content-visibility", false;
    Opacity = "opacity", false;
    Position = "position", false;
    Float = "float", false;
}

pub const PROPS: usize = Prop::ALL.len();

/// The CSS-wide keywords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wide {
    Initial,
    Inherit,
    Unset,
    Revert,
    RevertLayer,
}

/// A declared value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Specified {
    Wide(Wide),
    Display(Display),
    Visibility(Visibility),
    ContentVisibility(ContentVisibility),
    Opacity(f32),
    Position(Position),
    Float(Float),
    /// Holds `var()`: invalid at computed-value time until custom
    /// properties are substituted, so it acts as `unset`
    Unresolved,
}

/// The declared value of `prop` in `src[value]`; `None` when it is not
/// one.
pub fn parse(prop: Prop, src: &str, value: Span) -> Option<Specified> {
    let t = tokens(src, value);
    if t.iter()
        .any(|t| t.kind == Kind::Function && (t.value_is(src, "var") || t.value_is(src, "env")))
    {
        return Some(Specified::Unresolved);
    }
    let mut words: Vec<String> = Vec::with_capacity(t.len());
    let mut all_idents = true;
    for tok in &t {
        if tok.kind != Kind::Ident {
            all_idents = false;
            break;
        }
        let mut s = String::new();
        tok.write_value(src, &mut s);
        s.make_ascii_lowercase();
        words.push(s);
    }
    if all_idents && let [w] = &words[..] {
        let wide = match w.as_str() {
            "initial" => Some(Wide::Initial),
            "inherit" => Some(Wide::Inherit),
            "unset" => Some(Wide::Unset),
            "revert" => Some(Wide::Revert),
            "revert-layer" => Some(Wide::RevertLayer),
            _ => None,
        };
        if let Some(w) = wide {
            return Some(Specified::Wide(w));
        }
    }
    if prop == Prop::Opacity {
        let [n] = &t[..] else { return None };
        let v = match n.kind {
            Kind::Number => n.number(src),
            Kind::Percentage => n.number(src) / 100.0,
            _ => return None,
        };
        return Some(Specified::Opacity(v as f32));
    }
    if !all_idents || words.is_empty() {
        return None;
    }
    let one = || match &words[..] {
        [w] => Some(w.as_str()),
        _ => None,
    };
    Some(match prop {
        Prop::Display => {
            let ks: Vec<&str> = words.iter().map(String::as_str).collect();
            Specified::Display(Display::from_keywords(&ks)?)
        }
        Prop::Visibility => Specified::Visibility(Visibility::from_keyword(one()?)?),
        Prop::ContentVisibility => {
            Specified::ContentVisibility(ContentVisibility::from_keyword(one()?)?)
        }
        Prop::Position => {
            let w = one()?;
            let w = if w == "-webkit-sticky" { "sticky" } else { w };
            Specified::Position(Position::from_keyword(w)?)
        }
        Prop::Float => Specified::Float(Float::from_keyword(one()?)?),
        Prop::Opacity => unreachable!("parsed above"),
    })
}

/// One parsed declaration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decl {
    pub prop: Prop,
    pub value: Specified,
    pub important: bool,
}

/// The declarations of a block this engine computes, in order; `all`
/// with a CSS-wide keyword stands for each of them.
pub fn parse_block(src: &str, decls: &[Declaration]) -> Box<[Decl]> {
    let mut out = Vec::new();
    let mut name = String::new();
    for d in decls {
        name.clear();
        d.name.write_value(src, &mut name);
        name.make_ascii_lowercase();
        if name == "all" {
            if let Some(v @ Specified::Wide(_)) = parse(Prop::Display, src, d.value) {
                out.extend(Prop::ALL.iter().map(|&prop| Decl {
                    prop,
                    value: v,
                    important: d.important,
                }));
            }
            continue;
        }
        let Some(prop) = Prop::from_name(&name) else {
            continue;
        };
        if let Some(value) = parse(prop, src, d.value) {
            out.push(Decl {
                prop,
                value,
                important: d.important,
            });
        }
    }
    out.into()
}
