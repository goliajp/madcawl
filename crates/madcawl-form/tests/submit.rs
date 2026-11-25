//! A form's submission as a request: action, method, enctype and their
//! submitter overrides (HTML 4.10.21.3).

use alt_url::Url;
use madcawl_dom::{Dom, NodeId};
use madcawl_form::{Context, Files, FormError, Markup, Method, Submission, submission};
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};
use madcawl_str::atom;

fn parse(html: &str) -> Dom {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(html.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    b.into_sink()
}

fn by_id(dom: &Dom, id: &str) -> NodeId {
    dom.descendants(dom.document()).find(|&n| dom.attribute(n, atom!("id")).is_some_and(|v| v.eq_str(id))).unwrap()
}

fn submit(html: &str, submitter: Option<&str>) -> Result<Submission, FormError> {
    let dom = parse(html);
    let doc = Url::parse("https://example.com/dir/page?old=1#top").unwrap();
    let base = Url::parse("https://example.com/base/").unwrap();
    let allow = |_: &std::path::Path| false;
    let cx = Context { document_url: &doc, base_url: &base, files: Files { allow: &allow, max_body: 1 << 20 } };
    submission(&dom, by_id(&dom, "f"), submitter.map(|s| by_id(&dom, s)), &Markup, &cx)
}

fn body(s: &Submission) -> String {
    String::from_utf8(s.body.clone().unwrap()).unwrap()
}

#[test]
fn get_replaces_the_query_and_keeps_the_fragment() {
    let s = submit(r#"<form id=f action="search?x=1#frag"><input name=q value="a b"></form>"#, None).unwrap();
    assert_eq!((s.method, s.url.as_str()), (Method::Get, "https://example.com/base/search?q=a+b#frag"));
    assert_eq!((s.content_type, s.body), (None, None));
    let empty = submit(r#"<form id=f action=/s></form>"#, None).unwrap();
    assert_eq!(empty.url.as_str(), "https://example.com/s?");
}

#[test]
fn no_action_means_the_document() {
    let s = submit(r#"<form id=f><input name=a value=1></form>"#, None).unwrap();
    assert_eq!(s.url.as_str(), "https://example.com/dir/page?a=1#top");
    let s = submit(r#"<form id=f action=""><input name=a value=1></form>"#, None).unwrap();
    assert_eq!(s.url.as_str(), "https://example.com/dir/page?a=1#top");
}

#[test]
fn post_bodies_in_each_enctype() {
    let s = submit(r#"<form id=f method=POST action=/p><input name=a value="x y"><input name=b value=é></form>"#, None).unwrap();
    assert_eq!((s.method, s.url.as_str()), (Method::Post, "https://example.com/p"));
    assert_eq!(s.content_type.as_deref(), Some("application/x-www-form-urlencoded"));
    assert_eq!(body(&s), "a=x+y&b=%C3%A9");
    let s = submit(r#"<form id=f method=post enctype=TEXT/PLAIN action=/p><input name=a value=1></form>"#, None).unwrap();
    assert_eq!((s.content_type.as_deref(), body(&s).as_str()), (Some("text/plain"), "a=1\r\n"));
    let s = submit(r#"<form id=f method=post enctype=multipart/form-data action=/p><input name=a value=1></form>"#, None).unwrap();
    let ct = s.content_type.clone().unwrap();
    let b = ct.strip_prefix("multipart/form-data; boundary=").unwrap();
    assert_eq!(body(&s), format!("--{b}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--{b}--\r\n"));
    let s = submit(r#"<form id=f method=post enctype=bogus action=/p><input name=a value=1></form>"#, None).unwrap();
    assert_eq!(s.content_type.as_deref(), Some("application/x-www-form-urlencoded"));
}

#[test]
fn the_submitter_overrides_action_method_enctype_and_target() {
    let html = r#"<form id=f action=/a method=post target=t1><input name=a value=1>
        <button id=b formaction="/other" formmethod=get formtarget=t2 name=go value=1>go</button>
        <input type=submit id=s formenctype=text/plain name=s value=S>
        <button id=r type=reset>reset</button></form>"#;
    let s = submit(html, Some("b")).unwrap();
    assert_eq!((s.method, s.url.as_str(), s.target.as_deref()), (Method::Get, "https://example.com/other?a=1&go=1", Some("t2")));
    let s = submit(html, Some("s")).unwrap();
    assert_eq!((s.method, s.content_type.as_deref(), body(&s).as_str()), (Method::Post, Some("text/plain"), "a=1\r\ns=S\r\n"));
    assert_eq!(s.target.as_deref(), Some("t1"));
    assert_eq!(submit(html, Some("r")), Err(FormError::NotASubmitButton));
}

#[test]
fn what_makes_no_request() {
    assert_eq!(submit(r#"<form id=f method=dialog></form>"#, None), Err(FormError::Dialog));
    assert_eq!(submit(r#"<form id=f action="mailto:a@b"></form>"#, None), Err(FormError::Scheme("mailto".into())));
    assert_eq!(submit(r#"<div id=f></div>"#, None), Err(FormError::NotAForm));
    assert!(matches!(submit(r#"<form id=f action="http://[x"></form>"#, None), Err(FormError::BadAction(_))));
}
