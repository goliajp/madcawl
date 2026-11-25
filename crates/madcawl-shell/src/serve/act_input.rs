//! The actions that put input into controls: typing and keys into the
//! focused editable control, choosing an option, picking a file the user
//! approved, and filling several fields at once. What is typed is never
//! repeated back: a reply says how much went where, so a password typed
//! never comes out again.

use std::path::Path;

use madcawl_agent::activate::{attr, input_type, is_editable, is_password, is_submit, options, text_of};
use madcawl_agent::controls::Picked;
use madcawl_agent::record::READONLY;
use madcawl_dom::NodeId;
use madcawl_str::atom;

use super::act::{ActError, Acted};
use super::code;
use super::page::{Env, Page};

fn is(page: &Page, n: NodeId, name: madcawl_str::Atom) -> bool {
    page.doc.dom.local_name(n) == Some(name)
}

impl Page {
    /// The control typing goes to: `target`, focused first, or the
    /// focused one.
    fn editable(&mut self, target: Option<&str>) -> Result<NodeId, ActError> {
        let n = match target {
            Some(r) => {
                let n = self.resolve(r)?;
                self.point(n, true)?;
                self.focus = Some(n);
                n
            }
            None => self.focus.ok_or_else(|| ActError::Check("check failed: no control has the focus; give a ref".into()))?,
        };
        if !is_editable(&self.doc.dom, n) {
            return Err(ActError::Check("check failed: not a control text can be typed into".into()));
        }
        if self.agents.record_of(n).is_some_and(|r| r.has(READONLY)) {
            return Err(ActError::Check("check failed: read-only".into()));
        }
        Ok(n)
    }

    /// What the control holds now: its typed value, else its markup's.
    fn current(&self, n: NodeId) -> String {
        if let Some(v) = self.agents.controls().and_then(|c| c.value(n)) {
            return v.to_owned();
        }
        let dom = &self.doc.dom;
        if is(self, n, atom!("textarea")) {
            return madcawl_agent::activate::text_of(dom, n);
        }
        attr(dom, n, atom!("value")).unwrap_or_default()
    }

    fn put_text(&mut self, n: NodeId, v: String) -> Result<(), ActError> {
        let single = !is(self, n, atom!("textarea"));
        let v = if single { v.chars().filter(|&c| c != '\n' && c != '\r').collect() } else { v };
        self.set(n, |s| s.value = Some(v))
    }

    fn described(&self, n: NodeId) -> String {
        let (role, name) = self
            .agents
            .tree()
            .and_then(|t| madcawl_agent::role_and_name(t, n))
            .map_or(("control", String::new()), |(r, s)| (r.name(), s));
        let hidden = if is_password(&self.doc.dom, n) { " (a password: its value is never shown)" } else { "" };
        format!("{role} \"{name}\"{hidden}")
    }

    pub(crate) fn type_text(&mut self, target: Option<&str>, text: &str) -> Acted {
        let n = self.editable(target)?;
        let v = self.current(n) + text;
        self.put_text(n, v)?;
        Ok(format!("typed {} characters into {}", text.chars().count(), self.described(n)))
    }

    pub(crate) fn press(&mut self, target: Option<&str>, key: &str, env: &Env) -> Acted {
        if let Some(r) = target {
            let n = self.resolve(r)?;
            self.point(n, true)?;
            self.focus = Some(n);
        }
        let Some(n) = self.focus else {
            return Err(ActError::Check("check failed: no control has the focus; give a ref".into()));
        };
        match key {
            "Enter" if is(self, n, atom!("textarea")) => self.type_text(None, "\n"),
            "Enter" => {
                let form = madcawl_agent::activate::form_owner(&self.doc.dom, n)
                    .ok_or_else(|| ActError::Check("Enter submits nothing: the control is in no form".into()))?;
                self.submit(form, None, env)
            }
            "Backspace" => {
                self.editable(None)?;
                let mut v = self.current(n);
                v.pop();
                self.put_text(n, v)?;
                Ok("pressed Backspace".into())
            }
            " " | "Space" if is(self, n, atom!("input")) && matches!(input_type(&self.doc.dom, n).as_str(), "checkbox" | "radio") => {
                let on = !self.is_checked(n) || input_type(&self.doc.dom, n) == "radio";
                self.check(n, on)
            }
            "Tab" => {
                let dom = &self.doc.dom;
                let next = dom.descendants(dom.document()).skip_while(|&c| c != n).skip(1).find(|&c| {
                    [atom!("input"), atom!("select"), atom!("textarea"), atom!("button"), atom!("a")]
                        .iter()
                        .any(|&a| dom.local_name(c) == Some(a))
                        && self.agents.record_of(c).is_some_and(|r| r.shown())
                });
                self.focus = next.or(self.focus);
                Ok(format!("pressed Tab: the focus is on {}", next.map_or("the same control".into(), |c| self.described(c))))
            }
            k if k.chars().count() == 1 => self.type_text(None, k),
            k => Ok(format!("pressed {k}; without script the key does nothing here")),
        }
    }

    pub(crate) fn select(&mut self, n: NodeId, option: &str) -> Acted {
        if !is(self, n, atom!("select")) {
            return Err(ActError::Check("check failed: not a select".into()));
        }
        let dom = &self.doc.dom;
        let opts = options(dom, n);
        let want = option.trim();
        let label = |o: NodeId| text_of(dom, o);
        let value = |o: NodeId| attr(dom, o, atom!("value")).unwrap_or_else(|| label(o));
        let chosen = opts
            .iter()
            .find(|&&o| label(o) == want || value(o) == want)
            .or_else(|| opts.iter().find(|&&o| label(o).eq_ignore_ascii_case(want)))
            .copied()
            .ok_or_else(|| ActError::Check(format!("check failed: no option {want:?}")))?;
        if dom.attribute(chosen, atom!("disabled")).is_some() {
            return Err(ActError::Check("check failed: the option is disabled".into()));
        }
        let multiple = dom.attribute(n, atom!("multiple")).is_some();
        let shown = label(chosen);
        for o in opts {
            if o == chosen || !multiple {
                self.set(o, |s| s.selected = Some(o == chosen))?;
            }
        }
        self.set(n, |_| {})?;
        Ok(format!("selected \"{shown}\""))
    }

    pub(crate) fn upload(&mut self, n: NodeId, path: &str, env: &Env) -> Acted {
        let dom = &self.doc.dom;
        if !is(self, n, atom!("input")) || input_type(dom, n) != "file" {
            return Err(ActError::Check("check failed: not a file input".into()));
        }
        // a path that does not exist is refused as one outside the
        // approved directories is, so a refusal says nothing of the disk
        let p = Path::new(path.trim()).canonicalize().ok().filter(|p| env.uploads.iter().any(|d| p.starts_with(d)));
        let Some(p) = p else {
            let msg = "the file is not under a directory the user approved for uploads (--upload-allow)";
            return Err(ActError::Refused(code::APPROVAL, msg.into()));
        };
        if !p.is_file() {
            return Err(ActError::Bad(format!("{}: not a regular file", p.display())));
        }
        let filename = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let picked = Picked { content_type: content_type(&filename).into(), filename: filename.clone(), path: p };
        self.set(n, |s| s.files = Some(vec![picked]))?;
        Ok(format!("picked {filename} for {}", self.described(n)))
    }

    /// Sets each field by what kind of control it is, then submits when
    /// asked.
    pub(crate) fn fill(&mut self, fields: &[(String, String)], submit: bool, submitter: Option<&str>, env: &Env) -> Acted {
        let mut first = None;
        for (r, v) in fields {
            let n = self.resolve(r)?;
            first.get_or_insert(n);
            self.point(n, true)?;
            let dom = &self.doc.dom;
            if is_editable(dom, n) {
                self.editable(Some(r))?;
                self.put_text(n, v.clone())?;
            } else if is(self, n, atom!("select")) {
                self.select(n, v)?;
            } else if is(self, n, atom!("input")) && input_type(dom, n) == "file" {
                self.upload(n, v, env)?;
            } else if is(self, n, atom!("input")) && matches!(input_type(dom, n).as_str(), "checkbox" | "radio") {
                let on = !matches!(v.trim().to_ascii_lowercase().as_str(), "" | "false" | "off" | "no" | "0");
                self.check(n, on)?;
            } else {
                return Err(ActError::Check(format!("check failed: [{r}] is not a field fill_form sets")));
            }
        }
        let done = format!("filled {} fields", fields.len());
        if !submit {
            return Ok(done);
        }
        let by = match submitter {
            Some(r) => self.resolve(r)?,
            None => first.expect("at least one field"),
        };
        let then = self.submit_from(by, env)?;
        Ok(format!("{done}; {then}"))
    }

    /// Submits the form `n` is, or is a control of, with `n` as the
    /// submitter when it is a submit button.
    pub(crate) fn submit_from(&mut self, n: NodeId, env: &Env) -> Acted {
        let dom = &self.doc.dom;
        if is(self, n, atom!("form")) {
            return self.submit(n, None, env);
        }
        let form = madcawl_agent::activate::form_owner(dom, n)
            .ok_or_else(|| ActError::Check("check failed: not a form or a control in one".into()))?;
        let submitter = is_submit(dom, n).then_some(n);
        if let Some(s) = submitter {
            self.point(s, true)?;
        }
        self.submit(form, submitter, env)
    }
}

/// A file's type by its extension, for its part of an upload.
fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("txt" | "md" | "csv") => "text/plain",
        Some("html" | "htm") => "text/html",
        Some("json") => "application/json",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
}
