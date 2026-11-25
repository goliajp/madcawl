//! A form's submission as a request (HTML 4.10.21.3, "form submission
//! algorithm"): the action, method and enctype from the form or the
//! submitter's `formaction` / `formmethod` / `formenctype`, the action
//! resolved against the document's base URL, then a GET whose query is
//! replaced by the URL-encoded entries or a POST with the encoded body.
//! Only `http` and `https` actions make requests.

use crate::encode::{Files, boundary, multipart, text_plain, urlencoded};
use crate::entries::entry_list;
use crate::node::{attr, is_html, is_submit_button, keyword};
use crate::{Controls, FormError};
use alt_url::Url;
use madcawl_dom::{Dom, NodeId};
use madcawl_str::{Atom, atom};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enctype {
    UrlEncoded,
    Multipart,
    TextPlain,
}

/// Where the submission comes from.
pub struct Context<'a> {
    /// the form's document's URL: the action when the form names none
    pub document_url: &'a Url,
    /// what a relative action resolves against
    pub base_url: &'a Url,
    pub files: Files<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    pub method: Method,
    pub url: Url,
    /// the `Content-Type` of a POST body
    pub content_type: Option<String>,
    pub body: Option<Vec<u8>>,
    /// the browsing context named by `formtarget` or `target`
    pub target: Option<String>,
}

/// The submitter's override when it is a submit button, else the form's.
fn pick(dom: &Dom, form: NodeId, submitter: Option<NodeId>, over: Atom, own: Atom) -> Option<String> {
    submitter.and_then(|s| attr(dom, s, over)).or_else(|| attr(dom, form, own))
}

fn pick_keyword(dom: &Dom, form: NodeId, submitter: Option<NodeId>, over: Atom, own: Atom) -> Option<String> {
    submitter.and_then(|s| keyword(dom, s, over)).or_else(|| keyword(dom, form, own))
}

/// `url` with its query replaced by `query`; the fragment stays.
fn with_query(url: &Url, query: &str) -> Result<Url, FormError> {
    let s = url.as_str();
    let tail = url.query().map_or(0, |q| q.len() + 1) + url.fragment().map_or(0, |f| f.len() + 1);
    let mut out = s[..s.len() - tail].to_owned();
    out.push('?');
    out.push_str(query);
    if let Some(f) = url.fragment() {
        out.push('#');
        out.push_str(f);
    }
    Url::parse(&out).map_err(|e| FormError::BadAction(format!("{out}: {e:?}")))
}

/// The body's `Content-Type` and bytes.
fn encode(enctype: Enctype, entries: &[crate::Entry], files: &Files) -> Result<(String, Vec<u8>), FormError> {
    Ok(match enctype {
        Enctype::UrlEncoded => ("application/x-www-form-urlencoded".to_owned(), urlencoded(entries).into_bytes()),
        Enctype::TextPlain => ("text/plain".to_owned(), text_plain(entries).into_bytes()),
        Enctype::Multipart => {
            let b = boundary();
            let body = multipart(entries, &b, files)?;
            (format!("multipart/form-data; boundary={b}"), body)
        }
    })
}

/// The request submitting `form` with `submitter` would make.
pub fn submission(
    dom: &Dom,
    form: NodeId,
    submitter: Option<NodeId>,
    controls: &dyn Controls,
    cx: &Context,
) -> Result<Submission, FormError> {
    if !is_html(dom, form, atom!("form")) {
        return Err(FormError::NotAForm);
    }
    if submitter.is_some_and(|s| !is_submit_button(dom, s)) {
        return Err(FormError::NotASubmitButton);
    }
    let action = pick(dom, form, submitter, atom!("formaction"), atom!("action")).filter(|a| !a.is_empty());
    let url = match action {
        Some(a) => cx.base_url.join(&a).map_err(|e| FormError::BadAction(format!("{a}: {e:?}")))?,
        None => cx.document_url.clone(),
    };
    let method = match pick_keyword(dom, form, submitter, atom!("formmethod"), atom!("method")).as_deref() {
        Some("post") => Method::Post,
        Some("dialog") => return Err(FormError::Dialog),
        _ => Method::Get,
    };
    let enctype = match pick_keyword(dom, form, submitter, atom!("formenctype"), atom!("enctype")).as_deref() {
        Some("multipart/form-data") => Enctype::Multipart,
        Some("text/plain") => Enctype::TextPlain,
        _ => Enctype::UrlEncoded,
    };
    let target = pick(dom, form, submitter, atom!("formtarget"), atom!("target"));
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FormError::Scheme(url.scheme().to_owned()));
    }
    let entries = entry_list(dom, form, submitter, controls);
    if method == Method::Get {
        let url = with_query(&url, &urlencoded(&entries))?;
        return Ok(Submission { method, url, content_type: None, body: None, target });
    }
    let (content_type, body) = encode(enctype, &entries, &cx.files)?;
    if body.len() > cx.files.max_body {
        return Err(FormError::TooLarge);
    }
    Ok(Submission { method, url, content_type: Some(content_type), body: Some(body), target })
}
