//! `Dom::set_attribute`: an attribute keeps its place when it is set
//! again, the id and class list follow its value, a replaced value
//! gives its bytes and its id and class table entries back, the tables
//! have a limit of their own, and a refusal from script does not stop
//! the parser.

use madcawl_dom::{Dom, Limits, Refused};
use madcawl_html::{Atom, Attrs, Namespace, Options, Step, Tokenizer, TreeBuilder, TreeSink, atom};

fn text(s: &madcawl_str::DomString) -> String {
    let mut v = Vec::new();
    s.write_wtf8(&mut v);
    String::from_utf8(v).unwrap()
}

fn names(dom: &Dom, el: madcawl_dom::NodeId) -> Vec<String> {
    dom.attributes(el)
        .map(|a| format!("{}={}", text(&a.local.to_dom_string()), text(a.value)))
        .collect()
}

#[test]
fn set_adds_then_replaces_in_place() {
    let mut dom = Dom::new();
    let el = dom.create_element(Namespace::Html, atom!("div"), Attrs::empty());
    dom.append(dom.document(), el);
    assert!(dom.set_attribute(el, atom!("title"), b"a"));
    assert!(dom.set_attribute(el, atom!("class"), b"x y"));
    assert!(dom.set_attribute(el, atom!("id"), b"one"));
    assert!(dom.set_attribute(el, atom!("title"), b"b"));
    assert_eq!(names(&dom, el), ["title=b", "class=x y", "id=one"]);
    assert_eq!(dom.class_list(el).1, [atom!("x"), atom!("y")]);
    assert_eq!(
        dom.element_id(el).map(|a| text(&a.to_dom_string())),
        Some("one".into())
    );

    dom.set_attribute(el, atom!("class"), b"y");
    dom.set_attribute(el, atom!("id"), b"");
    assert_eq!(dom.class_list(el).1, [atom!("y")]);
    assert_eq!(dom.element_id(el), None);
    dom.set_attribute(el, atom!("class"), b"");
    assert_eq!(dom.class_list(el).0, 0, "the empty list");
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn equal_class_values_share_a_list() {
    let mut dom = Dom::new();
    let a = dom.create_element(Namespace::Html, atom!("p"), Attrs::empty());
    let b = dom.create_element(Namespace::Html, atom!("p"), Attrs::empty());
    dom.set_attribute(a, atom!("class"), b"k j");
    dom.set_attribute(b, atom!("class"), b"j");
    dom.set_attribute(b, atom!("class"), b"k j");
    assert_eq!(dom.class_list(a).0, dom.class_list(b).0);
}

fn small() -> (Dom, madcawl_dom::NodeId) {
    let mut dom = Dom::with_limits(Limits {
        nodes: 4096,
        bytes: 4096,
        tables: 1024,
    });
    let el = dom.create_element(Namespace::Html, atom!("div"), Attrs::empty());
    dom.append(dom.document(), el);
    (dom, el)
}

#[test]
fn a_replaced_value_gives_its_bytes_back() {
    let (mut dom, el) = small();
    for i in 0..10_000 {
        let v = format!("value {i:05}");
        assert!(dom.set_attribute(el, atom!("title"), v.as_bytes()), "{i}");
    }
    assert_eq!(names(&dom, el), ["title=value 09999"]);
    assert_eq!(dom.refused(), None);
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn a_value_cut_short_is_false() {
    let (mut dom, el) = small();
    assert!(!dom.set_attribute(el, atom!("title"), &[b'x'; 5000]));
    assert_eq!(dom.refused(), Some(Refused::Bytes));
    assert!(names(&dom, el)[0].len() < 5000);
}

#[test]
fn new_ids_and_class_values_count_against_the_table_limit() {
    for name in [atom!("id"), atom!("class")] {
        let (mut dom, _) = small();
        let doc = dom.document();
        let refused = (0..10_000).find(|i| {
            let el = dom.create_element(Namespace::Html, atom!("p"), Attrs::empty());
            dom.append(doc, el);
            let v = format!("name-{i}");
            !dom.set_attribute(el, name, v.as_bytes())
        });
        assert!(refused.is_some_and(|i| i < 1024 / 16), "{refused:?}");
        assert!(dom.table_bytes() <= 1024);
        assert_eq!(dom.refused(), None, "the byte limit is apart");
    }
}

#[test]
fn fresh_ids_and_classes_on_one_element_stay_within_a_fixed_table_size() {
    let (mut dom, el) = small();
    let mut most = 0;
    for i in 0..20_000 {
        let id = format!("x-soak-id-{i}");
        let class = format!("x-soak-a{i} x-soak-b{i}");
        assert!(dom.set_attribute(el, atom!("id"), id.as_bytes()), "{i}");
        assert!(
            dom.set_attribute(el, atom!("class"), class.as_bytes()),
            "{i}"
        );
        most = most.max(dom.table_bytes());
        let got = dom.element_id(el).map(|a| text(&a.to_dom_string()));
        assert_eq!(got.as_deref(), Some(id.as_str()));
        let classes = dom.class_list(el).1;
        assert_eq!(classes.len(), 2);
        assert_eq!(
            Some(classes[1]),
            Atom::lookup(format!("x-soak-b{i}").as_bytes())
        );
    }
    assert!(most <= 200, "{most}");
    assert_eq!(Atom::lookup(b"x-soak-id-0"), None, "old values are freed");
    assert_eq!(Atom::lookup(b"x-soak-a0"), None);
    dom.set_attribute(el, atom!("class"), b"");
    dom.set_attribute(el, atom!("id"), b"");
    assert_eq!(dom.table_bytes(), 0);
    assert_eq!(dom.refused(), None);
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn freed_elements_give_their_entries_back() {
    let mut dom = Dom::new();
    let doc = dom.document();
    let el = dom.create_element(Namespace::Html, atom!("p"), Attrs::empty());
    dom.append(doc, el);
    dom.set_attribute(el, atom!("id"), b"x-freed-id");
    dom.set_attribute(el, atom!("class"), b"x-freed-class");
    assert!(dom.table_bytes() > 0);
    dom.remove_node(el);
    dom.reclaim();
    assert_eq!(dom.table_bytes(), 0);
    assert_eq!(Atom::lookup(b"x-freed-class"), None);
}

#[test]
fn a_refusal_from_script_does_not_stop_the_parser() {
    let mut dom = Dom::with_limits(Limits {
        nodes: 4096,
        bytes: 4096,
        tables: 64,
    });
    let doc = dom.document();
    let el = dom.create_element(Namespace::Html, atom!("p"), Attrs::empty());
    dom.append(doc, el);
    assert!(
        !dom.set_attribute(el, atom!("id"), &[b'i'; 100]),
        "the table is full"
    );
    assert!(
        dom.new_text(madcawl_str::DomString::from_latin1(&[b't'; 5000]))
            .is_none()
    );
    assert_eq!(dom.refused(), None);
    assert!(!dom.is_full());
    let mut tok = Tokenizer::new();
    let mut b = TreeBuilder::new(dom, Options::default());
    tok.feed(b"<p id=after>x</p>");
    tok.end();
    assert_eq!(tok.run(&mut b), Step::Done);
    let dom = b.into_sink();
    assert!(!dom.parser_stopped());
}

#[test]
fn a_value_gives_back_what_it_was_counted_as() {
    let (mut dom, el) = small();
    // Latin-1, two-byte, three-byte, a pair and a lone surrogate
    for v in ["é", "ā", "€", "😀", "\u{FFFD}x"] {
        assert!(dom.set_attribute(el, atom!("title"), v.as_bytes()));
        let before = dom.bytes();
        assert!(dom.set_attribute(el, atom!("title"), v.as_bytes()));
        assert_eq!(dom.bytes(), before, "{v}");
    }
    let lone = b"\xED\xA0\x80a";
    assert!(dom.set_attribute(el, atom!("title"), lone));
    let before = dom.bytes();
    assert!(dom.set_attribute(el, atom!("title"), lone));
    assert_eq!(dom.bytes(), before);
}

#[test]
fn a_pair_of_surrogate_halves_is_one_value_and_gives_its_bytes_back() {
    // the halves of U+10000 each written as three bytes, as WTF-8
    // joined from two strings may hold them
    let split = [b'k', 0xED, 0xA0, 0x80, 0xED, 0xB0, 0x80];
    let mut dom = Dom::new();
    let el = dom.create_element(Namespace::Html, atom!("div"), Attrs::empty());
    dom.append(dom.document(), el);
    let (bytes, tables) = (dom.bytes(), dom.table_bytes());
    for name in [atom!("class"), atom!("id"), atom!("title")] {
        assert!(dom.set_attribute(el, name, &split));
        let mut v = Vec::new();
        dom.attribute(el, name).unwrap().write_wtf8(&mut v);
        assert_eq!(v, "k\u{10000}".as_bytes());
    }
    let id = dom.element_id(el).unwrap();
    assert_eq!(Some(id), madcawl_str::Atom::lookup("k\u{10000}".as_bytes()));
    assert_eq!(dom.class_list(el).1, [id]);
    for name in [atom!("class"), atom!("id"), atom!("title")] {
        dom.set_attribute(el, name, b"");
    }
    assert_eq!(dom.table_bytes(), tables);
    // three empty values in an attribute block of their own
    assert_eq!(dom.bytes(), bytes + 32 + 2 * 16);
    assert_eq!(dom.audit().mismatches(), 0);
}

#[test]
fn removing_attributes_gives_back_bytes_entries_id_and_classes() {
    let mut dom = Dom::new();
    let el = dom.create_element(Namespace::Html, atom!("div"), Attrs::empty());
    dom.append(dom.document(), el);
    assert!(dom.set_attribute(el, atom!("lang"), b"en"));
    let (bytes, tables) = (dom.bytes(), dom.table_bytes());
    for (name, v) in [(atom!("class"), &b"x-only-here y"[..]), (atom!("id"), b"x-only-id"), (atom!("title"), b"t")] {
        assert!(dom.set_attribute(el, name, v));
    }
    let (tree, class) = (dom.tree_version(), dom.class_version());
    dom.remove_attribute(el, 2);
    assert_eq!((dom.tree_version(), dom.class_version()), (tree, class), "an id is neither");
    assert_eq!(dom.element_id(el), None);
    assert_eq!(names(&dom, el), ["lang=en", "class=x-only-here y", "title=t"]);
    dom.remove_attribute(el, 1);
    assert!(dom.class_version() > class, "a class removal is a class change");
    assert_eq!(dom.tree_version(), tree);
    assert_eq!(dom.class_list(el).0, 0, "the empty list");
    dom.remove_attribute(el, 1);
    assert_eq!(names(&dom, el), ["lang=en"]);
    assert_eq!((dom.bytes(), dom.table_bytes()), (bytes, tables));
    assert!(dom.set_attribute(el, atom!("id"), b"x-only-id"));
    assert_eq!(dom.element_id(el).map(|a| text(&a.to_dom_string())), Some("x-only-id".into()));
    assert_eq!(dom.audit().mismatches(), 0);
}
