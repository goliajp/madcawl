//! Submitting a form without script: the entries from the controls'
//! state over their markup (`madcawl-form`), sent as a navigation of the
//! host's. A form with a password field goes only to an origin the user
//! approved for this host; files only from directories the user approved;
//! and a password's value never reaches a log, a GET query included.

use std::path::Path;

use madcawl_agent::activate::attr;
use madcawl_agent::controls::Controls;
use madcawl_dom::NodeId;
use madcawl_form::{Context, FileValue, Files, Method, password_controls, submission, urlencoded};
use madcawl_net::Url;
use madcawl_str::atom;

use super::act::{ActError, Acted};
use super::code;
use super::page::{Env, Page, Request};

/// The largest body a submission makes, files included.
const MAX_BODY: usize = 64 << 20;
const MASK: &str = "••••••";

/// The control state as `madcawl-form` reads it.
struct State<'a>(Option<&'a Controls>);

impl madcawl_form::Controls for State<'_> {
    fn value(&self, n: NodeId) -> Option<String> {
        self.0?.value(n).map(str::to_owned)
    }
    fn checked(&self, n: NodeId) -> Option<bool> {
        self.0?.checked(n)
    }
    fn selected(&self, n: NodeId) -> Option<bool> {
        self.0?.selected(n)
    }
    fn files(&self, n: NodeId) -> Option<Vec<FileValue>> {
        let picked = self.0?.get(n)?.files.as_ref()?;
        let f = |p: &madcawl_agent::controls::Picked| FileValue {
            filename: p.filename.clone(),
            content_type: p.content_type.clone(),
            path: Some(p.path.clone()),
        };
        Some(picked.iter().map(f).collect())
    }
}

/// The scheme, host and port of a URL, as `--approve` names an origin.
pub fn origin(u: &Url) -> String {
    let host = u.host_str().unwrap_or("");
    match u.port() {
        Some(p) => format!("{}://{host}:{p}", u.scheme()),
        None => format!("{}://{host}", u.scheme()),
    }
}

impl Page {
    pub(crate) fn submit(&mut self, form: NodeId, submitter: Option<NodeId>, env: &Env) -> Acted {
        let dom = &self.doc.dom;
        // the controls the submission itself reads, so no password it sends escapes the checks below
        let passwords = password_controls(dom, form);
        let page = Url::parse(&self.url).map_err(|_| ActError::Check("check failed: a form on this page has no URL to submit from".into()))?;
        let uploads = env.uploads;
        let allow = move |p: &Path| p.canonicalize().is_ok_and(|p| uploads.iter().any(|d| p.starts_with(d)));
        let cx = Context { document_url: &page, base_url: &page, files: Files { allow: &allow, max_body: MAX_BODY } };
        let controls = self.agents.controls();
        let sub = submission(dom, form, submitter, &State(controls), &cx).map_err(|e| ActError::Check(format!("the form cannot be submitted: {e:?}")))?;
        if !passwords.is_empty() && !env.approve.iter().any(|o| *o == origin(&sub.url)) {
            let msg = format!(
                "a form with a password field goes only to an origin the user approved: start the host with --approve {}",
                origin(&sub.url)
            );
            return Err(ActError::Refused(code::APPROVAL, msg));
        }
        let mask: Vec<String> = passwords.iter().filter_map(|&p| attr(dom, p, atom!("name"))).collect();
        let shown = masked(sub.url.as_str(), &mask);
        // where a redirect may carry a password form's body on to
        let body_origins = match passwords.is_empty() {
            true => Vec::new(),
            false => env.approve.iter().filter_map(|o| Url::parse(o).ok()).map(|u| u.origin().ascii_serialization()).collect(),
        };
        let method = if sub.method == Method::Get { "GET" } else { "POST" };
        let (content_type, body) = (sub.content_type, sub.body);
        let r = Request { method, url: sub.url, content_type, body, shown, entry: None, mask, body_origins };
        self.start(r, env)
    }
}

/// Whether `url` came out of `masked` with a value masked; a serialized
/// URL holds the mask's characters only percent-encoded.
pub(crate) fn is_masked(url: &str) -> bool {
    url.contains(MASK)
}

/// `url` with the values of the query's fields named in `names` masked.
pub(crate) fn masked(url: &str, names: &[String]) -> String {
    let Some((head, query)) = url.split_once('?').filter(|_| !names.is_empty()) else { return url.to_owned() };
    let (query, frag) = query.split_once('#').map_or((query, None), |(q, f)| (q, Some(f)));
    let keys: Vec<String> = names
        .iter()
        .map(|n| urlencoded(&[madcawl_form::Entry { name: n.clone(), value: madcawl_form::Value::Text(String::new()) }]))
        .collect();
    let parts: Vec<String> = query
        .split('&')
        .map(|p| match keys.iter().find(|k| p.starts_with(k.as_str())) {
            Some(k) => format!("{k}{MASK}"),
            None => p.to_owned(),
        })
        .collect();
    let mut out = format!("{head}?{}", parts.join("&"));
    if let Some(f) = frag {
        out.push('#');
        out.push_str(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_in_a_query_is_masked_and_origins_are_written_whole() {
        let u = "http://127.0.0.1:8/login?user=al&pass=hunter2&passx=1#top";
        let pass = ["pass".to_owned()];
        assert_eq!(masked(u, &pass), format!("http://127.0.0.1:8/login?user=al&pass={MASK}&passx=1#top"));
        assert_eq!(masked("http://x/", &pass), "http://x/");
        let both = ["user".to_owned(), "pass".to_owned()];
        assert_eq!(masked(u, &both), format!("http://127.0.0.1:8/login?user={MASK}&pass={MASK}&passx=1#top"));
        assert_eq!(origin(&Url::parse(u).unwrap()), "http://127.0.0.1:8");
        assert_eq!(origin(&Url::parse("https://example.com/a").unwrap()), "https://example.com");
    }
}
