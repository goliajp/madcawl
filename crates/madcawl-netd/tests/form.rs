//! Forms submitted through a guarded network process without script: a
//! GET search and a multipart upload with a file the host allows. A
//! submission is a navigation, so the host sends it on its own
//! connection; the page's content process may not.

mod fixture;

use fixture::{assert_refused, exchange, guarded, site, status_body, temp_path};
use madcawl_dom::{Dom, NodeId};
use madcawl_form::{Context, FileValue, Files, Markup, Method, State, Submission, submission};
use madcawl_html::{Options, Step, Tokenizer, TreeBuilder};
use madcawl_net::Url;
use madcawl_net::submit::{Navigation, fetch_frame};
use madcawl_net::wire::Frame;
use madcawl_str::atom;
use std::os::unix::net::UnixStream;
use std::path::Path;

fn parse(html: &str) -> Dom {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(html.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    b.into_sink()
}

fn first(dom: &Dom, name: madcawl_str::Atom) -> NodeId {
    dom.descendants(dom.document()).find(|&n| dom.local_name(n) == Some(name)).unwrap()
}

fn frame(page: &Url, sub: &Submission) -> Frame {
    let method = if sub.method == Method::Get { "GET" } else { "POST" };
    let nav = Navigation { id: 1, method, url: &sub.url, content_type: sub.content_type.as_deref(), body: sub.body.as_deref(), initiator: Some(page), body_origins: &[] };
    fetch_frame(&nav)
}

fn send(s: &mut UnixStream, page: &Url, sub: &Submission) -> (u64, String) {
    status_body(&exchange(s, frame(page, sub)))
}

fn cx<'a>(page: &'a Url, allow: &'a dyn Fn(&Path) -> bool) -> Context<'a> {
    Context { document_url: page, base_url: page, files: Files { allow, max_body: 1 << 20 } }
}

#[test]
fn a_get_form_and_a_multipart_upload() {
    let origin = site();
    let page = Url::parse(&format!("{origin}/page")).unwrap();
    let mut netd = guarded(None);
    let mut s = netd.host();
    let mut content = netd.content(page.as_str());

    let dom = parse(r#"<form action="/echo?stale=1"><input name=q value="rust & web"><select name=k><option>all</select></form>"#);
    let sub = submission(&dom, first(&dom, atom!("form")), None, &Markup, &cx(&page, &|_| false)).unwrap();
    assert_eq!(send(&mut s, &page, &sub), (200, "GET q=rust+%26+web&k=all\n\n".into()));
    assert_refused(&exchange(&mut content, frame(&page, &sub)));

    let file = temp_path("up.txt");
    std::fs::write(&file, b"line 1\nline 2").unwrap();
    let dom = parse(r#"<form action=/echo method=post enctype=multipart/form-data><input name=title value=notes><input type=file name=doc></form>"#);
    let mut state = State::default();
    let doc = dom.descendants(dom.document()).filter(|&n| dom.local_name(n) == Some(atom!("input"))).nth(1).unwrap();
    state.files.insert(doc, vec![FileValue { filename: "notes.txt".into(), content_type: "text/plain".into(), path: Some(file.clone()) }]);
    let allowed = file.clone();
    let allow = move |p: &Path| p == allowed;
    let sub = submission(&dom, first(&dom, atom!("form")), None, &state, &cx(&page, &allow)).unwrap();
    let (status, echoed) = send(&mut s, &page, &sub);
    let ct = sub.content_type.unwrap();
    let b = ct.strip_prefix("multipart/form-data; boundary=").unwrap();
    let want = format!(
        "POST \n{ct}\n--{b}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nnotes\r\n\
         --{b}\r\nContent-Disposition: form-data; name=\"doc\"; filename=\"notes.txt\"\r\nContent-Type: text/plain\r\n\r\nline 1\nline 2\r\n--{b}--\r\n"
    );
    assert_eq!((status, echoed), (200, want));
    std::fs::remove_file(&file).unwrap();
}

#[test]
fn a_body_is_not_carried_by_a_redirect_to_an_origin_it_was_not_approved_for() {
    let origin = site();
    let page = Url::parse(&format!("{origin}/login")).unwrap();
    let mut netd = guarded(None);
    let mut s = netd.host();
    let approved = [Url::parse(&origin).unwrap().origin().ascii_serialization()];
    let post = |s: &mut UnixStream, path: &str, body_origins: &[String]| {
        let url = Url::parse(&format!("{origin}{path}")).unwrap();
        let ct = Some("application/x-www-form-urlencoded");
        let nav = Navigation { id: 1, method: "POST", url: &url, content_type: ct, body: Some(b"pass=s3cret"), initiator: Some(&page), body_origins };
        exchange(s, fetch_frame(&nav))
    };
    let refused = post(&mut s, "/keep", &approved);
    let why = refused.iter().find(|f| f.kind == madcawl_net::wire::kind::ERROR).and_then(|f| f.get_str(madcawl_net::wire::tag::MESSAGE)).unwrap_or("");
    assert!(why.contains("not approved"), "{refused:?}");
    assert_eq!(status_body(&post(&mut s, "/keep-here", &approved)).0, 200, "a redirect within the approved origin");
    assert!(status_body(&post(&mut s, "/keep", &[])).1.contains("pass=s3cret"), "no list: followed as before");
}
