//! The selector parser: what parses, what does not, the order the
//! components are kept in, specificity, and the serialization a parse of
//! gives back the same selector.

use madcawl_css::selector::{
    Combinator, Component, Context, Namespace, NthKind, PseudoClass, PseudoElement, SelectorList,
    parse_selector_list,
};
use madcawl_css::{Names, Span};
use madcawl_str::{Atom, HeldAtoms};

fn parse_in(src: &str, names: &mut Names, cx: &Context<'_>) -> Option<SelectorList> {
    parse_selector_list(src, Span::new(0, src.len()), names, cx).ok()
}

fn parse(src: &str) -> Option<(SelectorList, Names)> {
    let mut names = Names::new();
    let cx = Context {
        nesting: true,
        ..Context::default()
    };
    parse_in(src, &mut names, &cx).map(|l| (l, names))
}

fn css(src: &str) -> String {
    let (l, names) = parse(src).unwrap_or_else(|| panic!("{src:?} did not parse"));
    let mut out = String::new();
    l.to_css(&names, &mut out);
    out
}

#[test]
fn serializes_what_it_parsed() {
    for (src, want) in [
        ("div", "div"),
        ("  DIV  ", "DIV"),
        ("*", "*"),
        ("a b", "a b"),
        ("a>b+c~d", "a > b + c ~ d"),
        ("a /**/ > /**/ b", "a > b"),
        ("#x.y.z", "#x.y.z"),
        ("[href]", "[href]"),
        ("[ href = 'a\"b' i ]", "[href=\"a\\\"b\" i]"),
        (
            "[a|=b][c~=d][e^=f][g$=h][i*=j s]",
            "[a|=\"b\"][c~=\"d\"][e^=\"f\"][g$=\"h\"][i*=\"j\" s]",
        ),
        ("a:hover::before", "a:hover::before"),
        ("p:first-line", "p::first-line"),
        ("p::after:hover", "p::after:hover"),
        (":NOT(.a, .b)", ":not(.a, .b)"),
        (":is(.a, :hover, 1x, .b)", ":is(.a, :hover, .b)"),
        (":where()", ":where()"),
        (":is(:has(:has(a)))", ":is()"),
        (":has(> img, + p, .x)", ":has(> img, + p, .x)"),
        (":nth-child(2n+1)", ":nth-child(2n+1)"),
        (":nth-child( odd of .a, .b )", ":nth-child(2n+1 of .a, .b)"),
        (":nth-last-of-type(-n + 3)", ":nth-last-of-type(-n+3)"),
        (":nth-of-type(5)", ":nth-of-type(5)"),
        (":lang(en, \"fr-CA\")", ":lang(\"en\", \"fr-CA\")"),
        (":dir(RTL)", ":dir(rtl)"),
        ("&.a > &", "&.a > &"),
        (".\\31 0", ".\\31 0"),
        (".a\\:b", ".a\\:b"),
        ("#-\\-x", "#--x"),
        ("a, b , c", "a, b, c"),
        ("u+a", "u + a"),
    ] {
        assert_eq!(css(src), want, "{src:?}");
        assert_eq!(css(&css(src)), css(src), "{src:?} again");
    }
}

#[test]
fn refuses_what_is_not_a_selector() {
    for src in [
        "",
        " ",
        "a,",
        ",a",
        "a >",
        "> a",
        "a $ b",
        "#1x",
        ".1x",
        ". a",
        "[]",
        "[a=]",
        "[a=b c]",
        "[a=b i x]",
        ":nonsense",
        "::nonsense",
        ":before()",
        "::before.a",
        "::before > a",
        "::before::after",
        ":not(::before)",
        ":not(.a,)",
        ":has(:has(a))",
        ":has(a, :not(:has(b)))",
        ":nth-child(2n+)",
        ":nth-of-type(2n of .a)",
        ":dir(up)",
        ":lang()",
        "ns|a",
        "a:hover(",
        "a!",
    ] {
        assert!(parse(src).is_none(), "{src:?} parsed");
    }
}

#[test]
fn components_run_right_to_left() {
    let (l, names) = parse("ul > li.item a").unwrap();
    let s = &l.0[0];
    let kinds: Vec<String> = s
        .components
        .iter()
        .map(|c| match c {
            Component::Type { name, .. } => names.get(*name).to_string(),
            Component::Class(n) => format!(".{}", names.get(*n)),
            Component::Combinator(Combinator::Child) => ">".into(),
            Component::Combinator(Combinator::Descendant) => " ".into(),
            c => panic!("{c:?}"),
        })
        .collect();
    assert_eq!(kinds, ["a", " ", "li", ".item", ">", "ul"]);
}

#[test]
fn specificity() {
    let spec = |src: &str| {
        let (l, _) = parse(src).unwrap();
        let s = l.0[0].specificity;
        (s >> 20, (s >> 10) & 1023, s & 1023)
    };
    assert_eq!(spec("*"), (0, 0, 0));
    assert_eq!(spec("li"), (0, 0, 1));
    assert_eq!(spec("ul li"), (0, 0, 2));
    assert_eq!(spec("ul ol+li"), (0, 0, 3));
    assert_eq!(spec("h1 + *[rel=up]"), (0, 1, 1));
    assert_eq!(spec("ul ol li.red"), (0, 1, 3));
    assert_eq!(spec("li.red.level"), (0, 2, 1));
    assert_eq!(spec("#x34y"), (1, 0, 0));
    assert_eq!(spec("#s12:not(foo)"), (1, 0, 1));
    assert_eq!(spec(".foo :is(.bar, #baz)"), (1, 1, 0));
    assert_eq!(spec(":where(#a, .b) p"), (0, 0, 1));
    assert_eq!(spec(":nth-child(2n of #a, .b)"), (1, 1, 0));
    assert_eq!(spec("p::before"), (0, 0, 2));
    assert_eq!(spec("a:has(> #x)"), (1, 0, 1));
}

#[test]
fn pseudo_classes_and_nth() {
    let (l, _) = parse("a:FIRST-CHILD:nth-last-child(-2n-3)").unwrap();
    let c = &l.0[0].components;
    assert_eq!(c[1], Component::PseudoClass(PseudoClass::FirstChild));
    let Component::Nth(n) = &c[2] else { panic!() };
    assert_eq!((n.kind, n.a, n.b), (NthKind::LastChild, -2, -3));
    let (l, _) = parse("p:before").unwrap();
    assert_eq!(
        l.0[0].components[1],
        Component::PseudoElement(PseudoElement::Before)
    );
    let (l, _) = parse(":has(+ a, b)").unwrap();
    let Component::Has(rel) = &l.0[0].components[0] else {
        panic!()
    };
    assert_eq!(
        rel.0[0].components.last(),
        Some(&Component::Combinator(Combinator::NextSibling))
    );
    assert_eq!(
        rel.0[1].components.last(),
        Some(&Component::Combinator(Combinator::Descendant))
    );
}

#[test]
fn namespaces_come_from_the_context() {
    let mut names = Names::new();
    let svg = names.intern("http://www.w3.org/2000/svg");
    let html = names.intern("http://www.w3.org/1999/xhtml");
    let prefix = names.intern("svg");
    let prefixes = [(prefix, svg)];
    let cx = Context {
        default_ns: Some(html),
        prefixes: &prefixes,
        nesting: false,
    };
    let l = parse_in("svg|rect, *|a, |b, c, [svg|href], [href]", &mut names, &cx).unwrap();
    let ns = |i: usize| match &l.0[i].components[0] {
        Component::Type { ns, .. } | Component::Universal(ns) => *ns,
        Component::Attribute(a) => a.ns,
        c => panic!("{c:?}"),
    };
    assert_eq!(ns(0), Namespace::Prefix { prefix, url: svg });
    assert_eq!(ns(1), Namespace::Any);
    assert_eq!(ns(2), Namespace::None);
    assert_eq!(ns(3), Namespace::Implicit(Some(html)));
    assert_eq!(ns(4), Namespace::Prefix { prefix, url: svg });
    assert_eq!(ns(5), Namespace::Implicit(None));
    assert!(parse_in("math|a", &mut names, &cx).is_none());
    assert!(
        parse_in("&", &mut names, &cx).is_none(),
        "& outside nesting"
    );
}

#[test]
fn nesting_depth_is_bounded() {
    let deep = |n: usize| format!("{}a{}", ":is(".repeat(n), ")".repeat(n));
    assert!(parse(&deep(32)).is_some());
    let (l, names) = parse(&deep(40)).unwrap();
    // forgiving: the inner list past the limit is dropped, not an error
    let mut out = String::new();
    l.to_css(&names, &mut out);
    assert_eq!(out, format!("{}{}", ":is(".repeat(32), ")".repeat(32)));
    let not = format!("{}a{}", ":not(".repeat(40), ")".repeat(40));
    assert!(parse(&not).is_none());
}

#[test]
fn names_become_atoms_on_the_main_thread() {
    let (_, names) = parse("div.some-class#an-id [data-x]").unwrap();
    let held = HeldAtoms::new();
    let atoms = names.atoms(&held);
    assert_eq!(atoms.len(), names.len());
    let div = 0;
    assert_eq!(names.get(madcawl_css::Name::from_index(div)), "div");
    assert_eq!(atoms[div], Atom::lookup_static(b"div").unwrap());
    let class = (0..names.len())
        .find(|&i| names.get(madcawl_css::Name::from_index(i)) == "some-class")
        .unwrap();
    assert!(!atoms[class].is_static());
    assert_eq!(Atom::lookup(b"some-class"), Some(atoms[class]));
}
