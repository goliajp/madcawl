//! Tree operations through the parser's sink and the counts they keep.

use madcawl_dom::{Dom, NodeKind};
use madcawl_html::{Attrs, Namespace, Options, Step, Tokenizer, TreeBuilder, TreeSink, atom};

fn parse(input: &str) -> Dom {
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(Dom::new(), Options::default());
    tok.feed(input.as_bytes());
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    let dom = b.into_sink();
    assert_eq!(dom.audit().mismatches(), 0, "{:?}", dom.audit());
    dom
}

fn tree(dom: &Dom) -> String {
    let mut out = Vec::new();
    dom.write_dat(dom.document(), &mut out);
    String::from_utf8(out).unwrap()
}

fn find(dom: &Dom, name: madcawl_html::Atom) -> madcawl_dom::NodeId {
    dom.descendants(dom.document())
        .find(|&n| dom.local_name(n) == Some(name))
        .unwrap()
}

#[test]
fn counts_follow_the_rules() {
    let dom = parse("<p>a<b>c</b>");
    let doc = dom.document();
    // html, head, body, p, "a", b, "c": every one holds the document,
    // which also holds its one child, html
    assert_eq!(dom.ref_count(doc), 7 + 1);
    let p = find(&dom, atom!("p"));
    // its parent, and its two children
    assert_eq!(dom.ref_count(p), 3);
    let text = dom.first_child(p).unwrap();
    assert_eq!(dom.ref_count(text), 1);
    assert!(dom.is_connected(text));
}

#[test]
fn template_contents_belong_to_the_inert_document() {
    let dom = parse("<template><p>x</p></template>");
    let t = find(&dom, atom!("template"));
    let contents = dom.contents(t).unwrap();
    assert_eq!(dom.kind(contents), NodeKind::Fragment);
    let p = dom.first_child(contents).unwrap();
    assert!(
        !dom.is_connected(p),
        "template contents are not in the document"
    );
    // the fragment is held by its template only
    assert_eq!(dom.ref_count(contents), 1 + 1, "its template and its child");
    assert!(tree(&dom).contains("content\n|         <p>\n|           \"x\""));
}

#[test]
fn ids_and_classes_are_interned() {
    let dom =
        parse(r#"<p id=one class="a  b"><i class="a b"></i><b class=" a b a"></b><u class="b a">"#);
    let p = find(&dom, atom!("p"));
    let (i, b, u) = (
        find(&dom, atom!("i")),
        find(&dom, atom!("b")),
        find(&dom, atom!("u")),
    );
    let mut id = Vec::new();
    dom.element_id(p).unwrap().write_wtf8(&mut id);
    assert_eq!(id, b"one");
    assert_eq!(dom.class_list(p).1.len(), 2);
    assert_eq!(dom.class_list(p).0, dom.class_list(i).0);
    assert_eq!(dom.class_list(p).0, dom.class_list(b).0);
    assert_ne!(
        dom.class_list(p).0,
        dom.class_list(u).0,
        "the order differs"
    );
    assert_eq!(dom.element_id(i), None);
}

#[test]
fn a_new_selected_option_replaces_the_copy_and_reclaim_frees_the_old_one() {
    let mut dom = parse(
        "<select><button><selectedcontent></selectedcontent></button>\
         <option>one<b>1</b></option><option selected>two<i>2</i></option></select>",
    );
    let sc = find(&dom, atom!("selectedcontent"));
    let names: Vec<_> = dom.children(sc).map(|c| dom.kind(c)).collect();
    assert_eq!(names, [NodeKind::Text, NodeKind::Element]);
    assert!(
        dom.text(dom.first_child(sc).unwrap())
            .unwrap()
            .eq_str("two")
    );
    // the copy of option one ("one", b, "1") was only taken out
    let in_tree = dom.descendants(dom.document()).count() as u32 + 1;
    assert_eq!(dom.node_count(), in_tree + 3);
    assert_eq!(dom.audit().detached, 3);
    assert_eq!(dom.reclaim(), 3);
    assert_eq!(dom.node_count(), in_tree);
    let a = dom.audit();
    assert_eq!((a.mismatches(), a.detached), (0, 0), "{a:?}");
    assert!(tree(&dom).contains("\"two\""));
}

#[test]
fn reclaim_frees_a_detached_template_and_clears_form_owners() {
    let mut dom = Dom::new();
    let doc = dom.document();
    let body = dom.create_element(Namespace::Html, atom!("body"), Attrs::empty());
    dom.append(doc, body);
    let form = dom.create_element(Namespace::Html, atom!("form"), Attrs::empty());
    let input = dom.create_element(Namespace::Html, atom!("input"), Attrs::empty());
    dom.append(body, input);
    dom.associate_form(input, form, body);
    assert_eq!(dom.form_owner(input), None, "the form is in another tree");
    dom.append(body, form);
    dom.associate_form(input, form, body);
    assert_eq!(dom.form_owner(input), Some(form));
    let t = dom.create_element(Namespace::Html, atom!("template"), Attrs::empty());
    let contents = dom.template_contents(t);
    dom.insert_text(contents, None, b"x");
    dom.remove(form);
    // form, template, its contents and their text
    assert_eq!(dom.reclaim(), 4);
    assert_eq!(dom.form_owner(input), None);
    assert_eq!(dom.audit().mismatches(), 0);
    // a rare entry freed with the template is used again
    let t2 = dom.create_element(Namespace::Html, atom!("template"), Attrs::empty());
    dom.append(body, t2);
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn addresses_are_fixed_and_map_back() {
    let mut dom = Dom::new();
    let el = dom.create_element(Namespace::Html, atom!("div"), Attrs::empty());
    let addr = dom.address(el);
    assert_eq!(addr % 64, 0);
    for _ in 0..1000 {
        let x = dom.create_element(Namespace::Html, atom!("span"), Attrs::empty());
        dom.append(el, x);
    }
    assert_eq!(dom.address(el), addr);
    assert_eq!(dom.node_at(addr), Some(el));
    assert_eq!(dom.node_at(addr + 8), None);
    assert_eq!(Dom::new().node_at(addr), None);
}

#[test]
fn merged_text_reads_whole() {
    let dom = parse("<p>a</x>b</y>c<table>d<tr>e<tr>f</table>");
    let out = tree(&dom);
    // "d" and "e" are fostered in front of the table, next to "abc"
    assert!(
        out.contains("|     <p>\n|       \"abcdef\"\n|       <table>"),
        "{out}"
    );
}

#[test]
fn whitespace_text_shares_a_string() {
    let dom = parse("<div>\n  <p>x</p>\n  <p>y</p>\n  </div>");
    let spaces: Vec<_> = dom
        .descendants(dom.document())
        .filter_map(|n| dom.text(n))
        .filter(|t| t.eq_str("\n  "))
        .collect();
    assert_eq!(spaces.len(), 3);
    let payload = |t: &madcawl_str::DomString| t.as_latin1().unwrap().as_ptr();
    assert!(
        spaces.iter().all(|t| payload(t) == payload(spaces[0])),
        "one block"
    );
}

#[test]
fn an_option_inside_selectedcontent_does_not_free_what_the_parser_holds() {
    // the option is inside the selectedcontent it would be copied into
    let dom = parse("<select><selectedcontent><option>a</option>");
    assert!(tree(&dom).contains("\"a\""));
}

#[test]
fn replaced_content_the_parser_still_holds_stays_valid() {
    // the div is still open when the option inside it finishes
    let dom = parse("<select><selectedcontent><div><option></option><!--c--><!--d--><i>");
    let out = tree(&dom);
    assert!(out.contains("<!-- c -->") && out.contains("<i>"), "{out}");
}

#[test]
#[should_panic(expected = "a container")]
fn appending_to_a_text_node_is_refused() {
    let mut dom = Dom::new();
    let doc = dom.document();
    let body = dom.create_element(Namespace::Html, atom!("body"), Attrs::empty());
    dom.append(doc, body);
    dom.insert_text(body, None, b"x");
    let text = dom.first_child(body).unwrap();
    let el = dom.create_element(Namespace::Html, atom!("i"), Attrs::empty());
    dom.append(text, el);
}

#[test]
#[should_panic(expected = "a container")]
fn adding_text_to_a_comment_is_refused() {
    let mut dom = Dom::new();
    let c = dom.create_comment(b"x");
    dom.insert_text(c, None, b"y");
}
