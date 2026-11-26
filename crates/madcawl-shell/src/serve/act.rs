//! Carrying out an action on a page without script: the
//! target resolved and checked, then what the action does — a click by
//! hit testing a point of the target and running the activation
//! behaviour of what is there, a navigation started, a control's state
//! set. Waiting for the page to settle is `wait.rs`'s.

use madcawl_agent::act::Action;
use madcawl_agent::activate::{Activation, activation, form_controls, radio_group};
use madcawl_agent::{AgentError, Frame, Geometry, kind, scroll_into_view, tag};
use madcawl_dom::NodeId;
use madcawl_net::Url;

use super::code;
use super::doc::asks_geometry;
use super::page::{Env, Page, Request};

pub(crate) enum ActError {
    /// A pre-action check failed, or the target cannot take the action
    Check(String),
    Agent(AgentError),
    /// Refused by the host's policy, with the host's error code
    Refused(u64, String),
    Bad(String),
}

impl From<AgentError> for ActError {
    fn from(e: AgentError) -> Self {
        ActError::Agent(e)
    }
}

pub(crate) type Acted = Result<String, ActError>;

impl Page {
    pub(crate) fn act(&mut self, a: &Action, env: &Env) -> Acted {
        match a {
            Action::Navigate(u) => {
                let url = self.join(u)?;
                self.start(Request::get(url, None), env)
            }
            Action::Back | Action::Forward | Action::Reload => self.travel(a, env),
            Action::Scroll { target, by } => self.scroll(target.as_deref(), *by),
            Action::Click(r) => self.click(r, env),
            Action::Hover(r) => {
                let n = self.resolve(r)?;
                self.point(n, false)?;
                Ok("hovered; without script nothing reacts to hovering".into())
            }
            Action::Type { target, text } => self.type_text(target.as_deref(), text),
            Action::Press { target, key } => self.press(target.as_deref(), key, env),
            Action::Select { target, option } => {
                let n = self.resolve(target)?;
                self.point(n, true)?;
                self.select(n, option)
            }
            Action::Check { target, on } => {
                let n = self.resolve(target)?;
                self.point(n, true)?;
                self.check(n, *on)
            }
            Action::Upload { target, path } => {
                let n = self.resolve(target)?;
                self.point(n, true)?;
                self.upload(n, path, env)
            }
            Action::Fill { fields, submit, submitter } => self.fill(fields, *submit, submitter.as_deref(), env),
            Action::Submit(r) => {
                let n = self.resolve(r)?;
                self.submit_from(n, env)
            }
        }
    }

    /// `u` against the document's URL; on a page not from the network,
    /// an absolute path names a local file.
    fn join(&self, u: &str) -> Result<Url, ActError> {
        let u = u.trim();
        let bad = |e| ActError::Bad(format!("not a URL: {u}: {e:?}"));
        if u.starts_with('/') && !self.url.starts_with("http") {
            return Url::parse(&super::target::file_url(std::path::Path::new(u))).map_err(bad);
        }
        match Url::parse(&self.url) {
            Ok(b) if b.scheme() != "about" => b.join(u).map_err(bad),
            _ => Url::parse(u).map_err(bad),
        }
    }

    /// `url` when a link of the page may lead to it: a local file only
    /// from a page that is one, so a page from the network cannot have a
    /// click open the files the host may read (forms go only to http and
    /// https).
    fn linked(&self, url: Url) -> Result<Url, ActError> {
        if url.scheme() == "file" && !self.url.starts_with("file:") {
            let msg = "a page from the network may not lead to a local file; navigate to it yourself";
            return Err(ActError::Refused(code::NAVIGATION, msg.into()));
        }
        Ok(url)
    }

    pub(crate) fn start(&mut self, r: Request, env: &Env) -> Acted {
        let what = format!("{} {}", r.method, r.shown);
        self.goto(r, env).map_err(|e| ActError::Refused(code::NAVIGATION, e))?;
        Ok(format!("navigation: {what}"))
    }

    fn travel(&mut self, a: &Action, env: &Env) -> Acted {
        let to = match a {
            Action::Back => self.at.checked_sub(1),
            Action::Forward => Some(self.at + 1).filter(|&i| i < self.history.len()),
            _ => Some(self.at).filter(|&i| i < self.history.len()),
        };
        let Some(i) = to else {
            return Err(ActError::Check(format!("check failed: no history entry to go {}", a.name())));
        };
        let visit = &self.history[i];
        if visit.masked {
            let why = "came from a form with a password, which is not sent again";
            return Err(ActError::Check(format!("check failed: {} {why}; re-submit the form", visit.url)));
        }
        let url = Url::parse(&visit.url).map_err(|e| ActError::Bad(format!("{e:?}")))?;
        self.start(Request::get(url, Some(i)), env)
    }

    fn scroll(&mut self, target: Option<&str>, by: i64) -> Acted {
        if let Some(r) = target {
            let n = self.resolve(r)?;
            let geo = self.geometry();
            if geo.place(n).is_none() {
                return Err(ActError::Check("check failed: not laid out, nothing to scroll to".into()));
            }
            scroll_into_view(geo, n);
        } else {
            let geo = self.geometry();
            let y = geo.viewport.y + by as f32 * geo.viewport.h;
            geo.scroll_to(y);
        }
        let geo = self.geometry();
        Ok(format!("scrolled: the viewport is at y={} of {}", geo.viewport.y as i64, geo.bottom() as i64))
    }

    /// The document's geometry, laid out now if nothing asked for it
    /// before. The semantic tree takes it in at once, between two syncs,
    /// so what the geometry refines can be told from what changed.
    pub(crate) fn geometry(&mut self) -> &mut Geometry {
        if self.doc.geometry.is_none() && self.agents.is_attached() {
            super::watch::stage("layout");
            let before = self.sync();
            self.doc.lay_out();
            self.doc.laid_out_at = Some((before, self.sync()));
        }
        self.doc.lay_out().0
    }

    /// Where a diff since `since` starts: past the layout when nothing
    /// changed between `since` and it. The visibility the first layout
    /// refines (an element without area or off the canvas is no longer
    /// shown) is not a change of the page; one laid out at open never
    /// showed it. A diff from before an earlier change keeps it, so no
    /// change is dropped.
    pub(crate) fn diff_floor(&self, since: u64) -> u64 {
        match self.doc.laid_out_at {
            Some((before, after)) if (before..after).contains(&since) => after,
            _ => since,
        }
    }

    /// Lays the document out first when request `f` needs geometry.
    pub(crate) fn lay_out_for(&mut self, f: &Frame) {
        if asks_geometry(f) {
            self.geometry();
        }
    }

    /// `f`, a diff request whose start is moved past the first layout
    /// when that drops no change (`diff_floor`).
    pub(crate) fn floored(&self, f: Frame) -> Frame {
        match f.get_num(tag::SINCE) {
            Some(s) if f.kind == kind::DIFF && self.diff_floor(s) != s => {
                let mut f = f;
                f.fields.retain(|(t, _)| *t != tag::SINCE);
                f.num(tag::SINCE, self.diff_floor(s))
            }
            _ => f,
        }
    }

    /// The target scrolled into view and checked; the point to act at.
    pub(crate) fn point(&mut self, n: NodeId, enabled: bool) -> Result<(f32, f32), ActError> {
        scroll_into_view(self.geometry(), n);
        let geo = self.doc.geometry.as_ref().expect("laid out");
        self.agents.check_target(&self.doc.dom, geo, n, enabled).map_err(|c| ActError::Check(c.message()))
    }

    fn click(&mut self, r: &str, env: &Env) -> Acted {
        let n = self.resolve(r)?;
        let (x, y) = self.point(n, true)?;
        let hit = self.geometry().hit(x, y).unwrap_or(n);
        let dom = &self.doc.dom;
        match activation(dom, hit) {
            Activation::Follow(a) => {
                let href = madcawl_agent::activate::attr(dom, a, madcawl_str::atom!("href")).unwrap_or_default();
                let url = self.linked(self.join(&href)?)?;
                self.start(Request::get(url, None), env)
            }
            Activation::Submit { form, submitter } => self.submit(form, Some(submitter), env),
            Activation::Toggle(c) => {
                let on = !self.is_checked(c);
                self.set(c, |s| s.checked = Some(on))?;
                Ok(format!("clicked: the checkbox is now {}", if on { "checked" } else { "not checked" }))
            }
            Activation::Choose(c) => self.check(c, true),
            Activation::Reset(form) => {
                for c in form_controls(&self.doc.dom, form) {
                    self.set(c, |s| *s = Default::default())?;
                }
                Ok("clicked: the form's controls are back to their initial values".into())
            }
            Activation::Focus(c) => {
                self.focus = Some(c);
                Ok("clicked: the control has the focus".into())
            }
            Activation::Nothing => Ok("clicked; without script nothing reacts to it".into()),
        }
    }

    pub(crate) fn is_checked(&self, n: NodeId) -> bool {
        let markup = self.doc.dom.attribute(n, madcawl_str::atom!("checked")).is_some();
        self.agents.controls().and_then(|c| c.checked(n)).unwrap_or(markup)
    }

    pub(crate) fn set(&mut self, n: NodeId, f: impl FnOnce(&mut madcawl_agent::controls::Control)) -> Result<(), ActError> {
        Ok(self.agents.set_control(self.session, &self.doc.dom, n, f)?)
    }

    /// Ticks or clears a checkbox; checks a radio button, clearing its
    /// group.
    pub(crate) fn check(&mut self, n: NodeId, on: bool) -> Acted {
        let dom = &self.doc.dom;
        let ty = madcawl_agent::activate::input_type(dom, n);
        let is_input = dom.local_name(n) == Some(madcawl_str::atom!("input"));
        if !is_input || !matches!(ty.as_str(), "checkbox" | "radio") {
            return Err(ActError::Check("check failed: not a checkbox or radio button".into()));
        }
        if ty == "radio" && on {
            for o in radio_group(dom, n) {
                self.set(o, |s| s.checked = Some(o == n))?;
            }
        } else {
            self.set(n, |s| s.checked = Some(on))?;
        }
        Ok(format!("the {ty} is now {}", if on { "checked" } else { "not checked" }))
    }
}

impl Request {
    pub(crate) fn get(url: Url, entry: Option<usize>) -> Request {
        let shown = url.as_str().to_owned();
        Request { method: "GET", url, content_type: None, body: None, shown, entry, mask: Vec::new(), body_origins: Vec::new() }
    }
}
