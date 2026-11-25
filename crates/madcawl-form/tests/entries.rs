//! The entry list and its encodings, case by case from HTML 4.10.21
//! ("constructing the entry list", the three encodings) and the
//! script-free expectations of WPT's html/semantics/forms/form-submission-0
//! and xhr/formdata tests.

use madcawl_dom::{Dom, NodeId};
use madcawl_form::{Controls, Entry, FileValue, Files, Markup, State, Value, entry_list, multipart, text_plain, urlencoded};
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
    dom.descendants(dom.document())
        .find(|&n| dom.attribute(n, atom!("id")).is_some_and(|v| v.eq_str(id)))
        .unwrap_or_else(|| panic!("no #{id}"))
}

fn form(dom: &Dom) -> NodeId {
    by_id(dom, "f")
}

fn pairs_with(html: &str, submitter: Option<&str>, c: &dyn Controls) -> Vec<(String, String)> {
    let dom = parse(html);
    let s = submitter.map(|id| by_id(&dom, id));
    entry_list(&dom, form(&dom), s, c).iter().map(|e| (e.name.clone(), e.text().to_owned())).collect()
}

fn pairs(html: &str) -> Vec<(String, String)> {
    pairs_with(html, None, &Markup)
}

fn p(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

#[test]
fn names_values_and_what_is_left_out() {
    let html = r#"<form id=f><input name=a value=1><input value=noname><input name="" value=empty>
        <input name=d value=x disabled><object name=o></object><output name=out>v</output>
        <input name=c type=checkbox><input name=c2 type=checkbox checked><input name=c3 type=checkbox checked value=yes>
        <input name=t type=bogus value=t><datalist><input name=inlist value=1></datalist></form>"#;
    assert_eq!(pairs(html), p(&[("a", "1"), ("c2", "on"), ("c3", "yes"), ("t", "t")]));
}

#[test]
fn fieldsets_disable_all_but_their_first_legend() {
    let html = r#"<form id=f><fieldset disabled><legend><input name=in_legend value=1></legend>
        <input name=out value=2><legend><input name=second_legend value=3></legend></fieldset>
        <fieldset><input name=enabled value=4></fieldset></form>"#;
    assert_eq!(pairs(html), p(&[("in_legend", "1"), ("enabled", "4")]));
}

#[test]
fn radio_groups_keep_the_last_checked() {
    let html = r#"<form id=f><input type=radio name=r value=a checked><input type=radio name=r value=b checked>
        <input type=radio name=s value=c><input type=radio name=u checked></form>"#;
    assert_eq!(pairs(html), p(&[("r", "b"), ("u", "on")]));
}

#[test]
fn selects() {
    let html = r#"<form id=f><select name=one><option disabled>x<option>  first
        option </option><option>third</select>
        <select name=two><option value=a selected>A<option value=b selected>B</select>
        <select name=many multiple><option value=1 selected><optgroup><option value=2 selected></optgroup>
        <optgroup disabled><option value=3 selected></optgroup><option value=4></select>
        <select name=listbox size=3><option>z</select><select name=empty></select></form>"#;
    assert_eq!(pairs(html), p(&[("one", "first option"), ("two", "b"), ("many", "1"), ("many", "2")]));
}

#[test]
fn only_the_submitter_button_counts() {
    let html = r#"<form id=f><input name=a value=1><button name=b1 value=v1>B1</button>
        <input type=submit name=s1 value=S1 id=s1><input type=submit name=s2 value=S2>
        <input type=image name=img id=img><input type=image id=anon><button id=plain>no name</button>
        <input type=reset name=r value=R><input type=button name=bb value=BB></form>"#;
    assert_eq!(pairs(html), p(&[("a", "1")]));
    assert_eq!(pairs_with(html, Some("s1"), &Markup), p(&[("a", "1"), ("s1", "S1")]));
    assert_eq!(pairs_with(html, Some("img"), &Markup), p(&[("a", "1"), ("img.x", "0"), ("img.y", "0")]));
    assert_eq!(pairs_with(html, Some("anon"), &Markup), p(&[("a", "1"), ("x", "0"), ("y", "0")]));
    assert_eq!(pairs_with(html, Some("plain"), &Markup), p(&[("a", "1")]));
}

#[test]
fn charset_field_files_and_textarea() {
    let html = r#"<form id=f><input type=hidden name=_CHARSET_ value=latin1><input type=text name=_charset_ value=keep>
        <input type=file name=up><textarea name=ta>
line one
line two</textarea></form>"#;
    assert_eq!(pairs(html), p(&[("_CHARSET_", "UTF-8"), ("_charset_", "keep"), ("up", ""), ("ta", "line one\nline two")]));
}

#[test]
fn the_form_attribute_moves_controls_between_forms() {
    let html = r#"<form id=f><input name=inside value=1><input name=elsewhere form=g value=2></form>
        <form id=g></form><input name=outside form=f value=3><input name=nowhere form=missing value=4>"#;
    assert_eq!(pairs(html), p(&[("inside", "1"), ("outside", "3")]));
    let html = r#"<div id=f></div><form id=f><input name=x value=1></form><input name=y form=f value=2>"#;
    let dom = parse(html);
    let real = dom.descendants(dom.document()).find(|&n| dom.local_name(n) == Some(atom!("form"))).unwrap();
    let names: Vec<String> = entry_list(&dom, real, None, &Markup).into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["x"], "form=f names the first element with that ID, which is not a form");
}

#[test]
fn values_are_sanitized_by_type() {
    let html = r##"<form id=f><input name=n type=number value="1e2"><input name=bad type=number value="1,5">
        <input name=r type=range><input name=r2 type=range min=10 max=20 value=99>
        <input name=c type=color value="#ABCDEF"><input name=u type=url value="  http://x/  ">
        <input name=e type=email multiple value=" a@b , c@d "></form>"##;
    assert_eq!(
        pairs(html),
        p(&[("n", "1e2"), ("bad", ""), ("r", "50"), ("r2", "20"), ("c", "#abcdef"), ("u", "http://x/"), ("e", "a@b,c@d")])
    );
    let mut s = State::default();
    let dom = parse(r#"<form id=f><input name=t id=t></form>"#);
    s.values.insert(by_id(&dom, "t"), "a\r\nb\nc".into());
    assert_eq!(entry_list(&dom, form(&dom), None, &s)[0].text(), "abc", "a text input drops newlines");
}

#[test]
fn dirname_reports_the_direction() {
    let html = r#"<form id=f><input name=a dirname=a.dir value=x><div dir=rtl><textarea name=b dirname=b.dir>y</textarea></div>
        <input name=c dirname=c.dir dir=auto value="שלום"></form>"#;
    assert_eq!(pairs(html), p(&[("a", "x"), ("a.dir", "ltr"), ("b", "y"), ("b.dir", "rtl"), ("c", "שלום"), ("c.dir", "rtl")]));
}

#[test]
fn host_state_overrides_the_markup() {
    let html = r#"<form id=f><input name=t id=t value=old><input type=checkbox name=c id=c checked>
        <select name=s><option id=o1 selected>1<option id=o2>2</select><textarea name=ta id=ta>x</textarea></form>"#;
    let dom = parse(html);
    let mut s = State::default();
    s.values.insert(by_id(&dom, "t"), "new".into());
    s.values.insert(by_id(&dom, "ta"), "l1\r\nl2\rl3".into());
    s.checked.insert(by_id(&dom, "c"), false);
    s.selected.insert(by_id(&dom, "o1"), false);
    s.selected.insert(by_id(&dom, "o2"), true);
    let got: Vec<(String, String)> = entry_list(&dom, form(&dom), None, &s).iter().map(|e| (e.name.clone(), e.text().into())).collect();
    assert_eq!(got, p(&[("t", "new"), ("s", "2"), ("ta", "l1\nl2\nl3")]));
}

fn text(name: &str, v: &str) -> Entry {
    Entry { name: name.into(), value: Value::Text(v.into()) }
}

#[test]
fn urlencoded_and_text_plain() {
    let list = [text("a b", "1+2=3&4"), text("é", "*-._~!"), text("nl", "x\ny\rz\r\nw")];
    assert_eq!(urlencoded(&list), "a+b=1%2B2%3D3%264&%C3%A9=*-._%7E%21&nl=x%0D%0Ay%0D%0Az%0D%0Aw");
    assert_eq!(text_plain(&list), "a b=1+2=3&4\r\né=*-._~!\r\nnl=x\r\ny\r\nz\r\nw\r\n");
    let file = Entry { name: "f".into(), value: Value::File(FileValue { filename: "r.txt".into(), content_type: String::new(), path: None }) };
    assert_eq!(urlencoded(&[file]), "f=r.txt");
}

#[test]
fn multipart_escapes_names_and_carries_files() {
    let dir = std::env::temp_dir().join(format!("mcform-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("up.txt");
    std::fs::write(&path, b"file\nbytes").unwrap();
    let file = |p: Option<std::path::PathBuf>, name: &str| Entry {
        name: "up".into(),
        value: Value::File(FileValue { filename: name.into(), content_type: "text/plain".into(), path: p }),
    };
    let list = [text("a\nb\"c", "v\n1"), file(Some(path.clone()), "q\"uote.txt"), file(None, "")];
    let ok = |_: &std::path::Path| true;
    let body = multipart(&list, "BOUND", &Files { allow: &ok, max_body: 1 << 20 }).unwrap();
    let want = "--BOUND\r\nContent-Disposition: form-data; name=\"a%0D%0Ab%22c\"\r\n\r\nv\r\n1\r\n\
        --BOUND\r\nContent-Disposition: form-data; name=\"up\"; filename=\"q%22uote.txt\"\r\nContent-Type: text/plain\r\n\r\nfile\nbytes\r\n\
        --BOUND\r\nContent-Disposition: form-data; name=\"up\"; filename=\"\"\r\nContent-Type: text/plain\r\n\r\n\r\n--BOUND--\r\n";
    assert_eq!(String::from_utf8(body).unwrap(), want);
    let no = |_: &std::path::Path| false;
    let refused = multipart(&list, "B", &Files { allow: &no, max_body: 1 << 20 });
    assert_eq!(refused, Err(madcawl_form::FormError::FileNotAllowed(path.clone())));
    let small = multipart(&list, "B", &Files { allow: &ok, max_body: 90 });
    assert_eq!(small, Err(madcawl_form::FormError::TooLarge));
    std::fs::remove_dir_all(&dir).unwrap();
}
