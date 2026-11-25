//! The actions an agent asks for, as they travel in an `ACT`
//! frame, and the wait that follows one. Parsing is all a page needs to
//! trust of a frame: every field is bounded here, so what reaches the
//! page is well formed whatever the sender wrote.

use crate::frame::Frame;
use crate::protocol::{kind, tag};

/// The longest text one action carries (typed text, a URL, a value).
pub const MAX_TEXT: usize = 64 << 10;
/// The most fields one `fill_form` sets.
pub const MAX_FIELDS: usize = 64;
/// The most viewports one scroll moves.
pub const MAX_SCROLL: i64 = 100;
/// The longest a wait may be asked to take, in milliseconds.
pub const MAX_TIMEOUT_MS: u64 = 120_000;
/// The wait when the request names none.
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Navigate(String),
    Back,
    Forward,
    Reload,
    /// To make a reference visible, or by whole viewports (negative is up)
    Scroll { target: Option<String>, by: i64 },
    Click(String),
    Hover(String),
    /// Into the reference, or the focused control when there is none
    Type { target: Option<String>, text: String },
    Press { target: Option<String>, key: String },
    Select { target: String, option: String },
    Check { target: String, on: bool },
    Upload { target: String, path: String },
    /// Each field a reference and the value it takes; with `submit`, the
    /// form is submitted after, by `submitter` when one is named
    Fill {
        fields: Vec<(String, String)>,
        submit: bool,
        submitter: Option<String>,
    },
    Submit(String),
}

/// What the page must reach before the reply, besides being stable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Until {
    pub reference: Option<String>,
    pub text: Option<String>,
    pub timeout_ms: u64,
}

fn text(f: &Frame, t: u8) -> Result<Option<String>, &'static str> {
    match f.field(t) {
        None => Ok(None),
        Some(b) if b.len() > MAX_TEXT => Err("a value longer than an action takes"),
        Some(b) => std::str::from_utf8(b)
            .map(|s| Some(s.to_owned()))
            .map_err(|_| "a value that is not UTF-8"),
    }
}

fn need(f: &Frame, t: u8, what: &'static str) -> Result<String, &'static str> {
    text(f, t)?.filter(|s| !s.trim().is_empty()).ok_or(what)
}

fn fields(f: &Frame) -> Result<Vec<(String, String)>, &'static str> {
    let mut out = Vec::new();
    for (t, v) in &f.fields {
        if *t != tag::FIELD {
            continue;
        }
        if out.len() == MAX_FIELDS {
            return Err("more fields than one fill_form sets");
        }
        let s = std::str::from_utf8(v).map_err(|_| "a field that is not UTF-8")?;
        let (r, value) = s.split_once('\0').ok_or("a field without its value")?;
        if value.len() > MAX_TEXT {
            return Err("a value longer than an action takes");
        }
        out.push((r.to_owned(), value.to_owned()));
    }
    if out.is_empty() {
        return Err("fill_form needs at least one field");
    }
    Ok(out)
}

impl Action {
    /// The action an `ACT` frame asks for.
    pub fn parse(f: &Frame) -> Result<Action, &'static str> {
        let reference = || need(f, tag::REF, "this action needs a ref");
        let on = f.get_num(tag::ON).unwrap_or(1) != 0;
        Ok(match text(f, tag::ACTION)?.as_deref().map(str::trim) {
            Some("navigate") => Action::Navigate(need(f, tag::VALUE, "navigate needs a url")?),
            Some("back") => Action::Back,
            Some("forward") => Action::Forward,
            Some("reload") => Action::Reload,
            Some("scroll") => {
                let n = f.get_num(tag::AMOUNT).unwrap_or(1).min(MAX_SCROLL as u64) as i64;
                let by = match text(f, tag::VALUE)?.as_deref().map(str::trim) {
                    None | Some("" | "down") => n,
                    Some("up") => -n,
                    Some(_) => return Err("scroll direction is up or down"),
                };
                Action::Scroll { target: text(f, tag::REF)?.filter(|r| !r.is_empty()), by }
            }
            Some("click") => Action::Click(reference()?),
            Some("hover") => Action::Hover(reference()?),
            Some("type") => Action::Type {
                target: text(f, tag::REF)?.filter(|r| !r.is_empty()),
                text: text(f, tag::VALUE)?.ok_or("type needs text")?,
            },
            Some("press") => Action::Press {
                target: text(f, tag::REF)?.filter(|r| !r.is_empty()),
                key: need(f, tag::VALUE, "press needs a key")?,
            },
            Some("select") => Action::Select {
                target: reference()?,
                option: text(f, tag::VALUE)?.ok_or("select needs an option")?,
            },
            Some("check") => Action::Check { target: reference()?, on },
            Some("upload") => Action::Upload {
                target: reference()?,
                path: need(f, tag::VALUE, "upload needs a path")?,
            },
            Some("fill_form") => Action::Fill {
                fields: fields(f)?,
                submit: f.get_num(tag::ON).unwrap_or(0) != 0,
                submitter: text(f, tag::REF)?.filter(|r| !r.is_empty()),
            },
            Some("submit") => Action::Submit(reference()?),
            _ => return Err("unknown action"),
        })
    }

    /// The action's name, as the audit log and replies give it.
    pub fn name(&self) -> &'static str {
        match self {
            Action::Navigate(_) => "navigate",
            Action::Back => "back",
            Action::Forward => "forward",
            Action::Reload => "reload",
            Action::Scroll { .. } => "scroll",
            Action::Click(_) => "click",
            Action::Hover(_) => "hover",
            Action::Type { .. } => "type",
            Action::Press { .. } => "press",
            Action::Select { .. } => "select",
            Action::Check { .. } => "check",
            Action::Upload { .. } => "upload",
            Action::Fill { .. } => "fill_form",
            Action::Submit(_) => "submit",
        }
    }

    /// The reference the action is aimed at, if one.
    pub fn target(&self) -> Option<&str> {
        match self {
            Action::Click(r) | Action::Hover(r) | Action::Submit(r) => Some(r),
            Action::Select { target, .. } | Action::Check { target, .. } | Action::Upload { target, .. } => {
                Some(target)
            }
            Action::Scroll { target, .. } | Action::Type { target, .. } | Action::Press { target, .. } => {
                target.as_deref()
            }
            Action::Fill { fields, .. } => fields.first().map(|(r, _)| r.as_str()),
            _ => None,
        }
    }

    /// The `ACT` frame for this action on `page`.
    pub fn frame(&self, page: u64, until: &Until) -> Frame {
        let f = Frame::new(kind::ACT).num(tag::PAGE, page).str(tag::ACTION, self.name());
        let f = match self {
            Action::Navigate(u) => f.str(tag::VALUE, u),
            Action::Scroll { target, by } => {
                let f = f.num(tag::AMOUNT, by.unsigned_abs()).str(tag::VALUE, if *by < 0 { "up" } else { "down" });
                target.iter().fold(f, |f, r| f.str(tag::REF, r))
            }
            Action::Click(r) | Action::Hover(r) | Action::Submit(r) => f.str(tag::REF, r),
            Action::Type { target, text: v } | Action::Press { target, key: v } => {
                target.iter().fold(f, |f, r| f.str(tag::REF, r)).str(tag::VALUE, v)
            }
            Action::Select { target, option: v } | Action::Upload { target, path: v } => {
                f.str(tag::REF, target).str(tag::VALUE, v)
            }
            Action::Check { target, on } => f.str(tag::REF, target).num(tag::ON, u64::from(*on)),
            Action::Fill { fields, submit, submitter } => {
                let f = fields.iter().fold(f, |f, (r, v)| f.str(tag::FIELD, &format!("{r}\0{v}")));
                submitter.iter().fold(f.num(tag::ON, u64::from(*submit)), |f, r| f.str(tag::REF, r))
            }
            Action::Back | Action::Forward | Action::Reload => f,
        };
        until.write(f)
    }
}

impl Until {
    pub fn parse(f: &Frame) -> Result<Until, &'static str> {
        Ok(Until {
            reference: text(f, tag::UNTIL_REF)?.filter(|r| !r.trim().is_empty()),
            text: text(f, tag::UNTIL_TEXT)?.filter(|t| !t.trim().is_empty()),
            timeout_ms: f.get_num(tag::TIMEOUT).unwrap_or(DEFAULT_TIMEOUT_MS).min(MAX_TIMEOUT_MS),
        })
    }

    pub fn write(&self, mut f: Frame) -> Frame {
        if let Some(r) = &self.reference {
            f = f.str(tag::UNTIL_REF, r);
        }
        if let Some(t) = &self.text {
            f = f.str(tag::UNTIL_TEXT, t);
        }
        if self.timeout_ms != 0 {
            f = f.num(tag::TIMEOUT, self.timeout_ms);
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_round_trip_through_their_frames() {
        let until = Until { reference: Some("e4".into()), text: Some("Saved".into()), timeout_ms: 500 };
        let all = [
            Action::Navigate("http://127.0.0.1/".into()),
            Action::Back,
            Action::Scroll { target: None, by: -3 },
            Action::Scroll { target: Some("e9".into()), by: 1 },
            Action::Click("e3".into()),
            Action::Type { target: None, text: "hi\0there".into() },
            Action::Press { target: Some("e2".into()), key: "Enter".into() },
            Action::Select { target: "e5".into(), option: "Rust".into() },
            Action::Check { target: "e6".into(), on: false },
            Action::Upload { target: "e7".into(), path: "/tmp/a.txt".into() },
            Action::Fill {
                fields: vec![("e1".into(), "a=b".into()), ("e2".into(), String::new())],
                submit: true,
                submitter: Some("e8".into()),
            },
            Action::Submit("e8".into()),
        ];
        for a in all {
            let f = a.frame(3, &until);
            assert_eq!(Action::parse(&f), Ok(a.clone()), "{a:?}");
            assert_eq!(Until::parse(&f), Ok(until.clone()));
        }
    }

    #[test]
    fn bad_actions_are_refused_with_a_reason() {
        let f = |name: &str| Frame::new(kind::ACT).str(tag::ACTION, name);
        assert_eq!(Action::parse(&f("click")), Err("this action needs a ref"));
        assert_eq!(Action::parse(&f("fly")), Err("unknown action"));
        assert_eq!(Action::parse(&f("fill_form")), Err("fill_form needs at least one field"));
        assert!(Action::parse(&f("type").str(tag::VALUE, &"x".repeat(MAX_TEXT + 1))).is_err());
        assert!(Action::parse(&f("type").bytes(tag::VALUE, &[0xff])).is_err());
        assert!(Action::parse(&f("fill_form").str(tag::FIELD, "e1")).is_err());
        assert!(Action::parse(&f("scroll").str(tag::VALUE, "left")).is_err());
        let many = (0..=MAX_FIELDS).fold(f("fill_form"), |f, i| f.str(tag::FIELD, &format!("e{i}\0v")));
        assert!(Action::parse(&many).is_err());
        let long = Until::parse(&Frame::new(kind::WAIT).num(tag::TIMEOUT, u64::MAX)).unwrap();
        assert_eq!(long.timeout_ms, MAX_TIMEOUT_MS);
    }
}
