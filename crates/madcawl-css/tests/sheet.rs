//! The stylesheet model: rules found with their selectors, declaration
//! blocks parsed only when asked for, nested rules, the order rules must
//! come in, and parsing on the worker thread.

use madcawl_css::selector::{Combinator, Component, Namespace};
use madcawl_css::{MAX_NESTING, Rule, Source, Stylesheet, Worker};
use madcawl_str::HeldAtoms;

fn style(r: &Rule) -> &madcawl_css::StyleRule {
    match r {
        Rule::Style(s) => s,
        r => panic!("not a style rule: {r:?}"),
    }
}

fn decls(sheet: &Stylesheet, r: &Rule) -> Vec<(String, String, bool)> {
    let src = sheet.source();
    sheet
        .declarations(&style(r).body)
        .iter()
        .map(|d| {
            let mut name = String::new();
            d.name.write_value(src, &mut name);
            (name, d.value.of(src).trim().to_string(), d.important)
        })
        .collect()
}

#[test]
fn bodies_are_parsed_when_asked_for() {
    let src = ".a { color: red } .b { margin: 0 !important; padding: 1px } .c { }".repeat(10);
    let sheet = Stylesheet::parse(src);
    assert_eq!(sheet.rules().len(), 30);
    assert_eq!(sheet.bodies(), (30, 0));
    let d = decls(&sheet, &sheet.rules()[1]);
    assert_eq!(
        d,
        [
            ("margin".into(), "0".into(), true),
            ("padding".into(), "1px".into(), false)
        ]
    );
    assert_eq!(sheet.bodies(), (30, 1));
    // asked again, the same parse
    assert_eq!(decls(&sheet, &sheet.rules()[1]).len(), 2);
    assert_eq!(sheet.bodies(), (30, 1));
}

#[test]
fn nested_rules_are_parsed_with_the_sheet() {
    let sheet = Stylesheet::parse(
        ".a { color: red; &:hover { color: blue } > .b { x: y } z: w; @media print { q: r } }"
            .into(),
    );
    let a = style(&sheet.rules()[0]);
    assert!(a.body.is_parsed());
    assert_eq!(decls(&sheet, &sheet.rules()[0]).len(), 1);
    assert_eq!(a.rules.len(), 4);
    let hover = style(&a.rules[0]);
    assert_eq!(hover.selectors.0[0].components[0], Component::Nesting);
    let b = style(&a.rules[1]);
    assert_eq!(
        b.selectors.0[0].components.last(),
        Some(&Component::Combinator(Combinator::Child))
    );
    let Rule::NestedDeclarations(after) = &a.rules[2] else {
        panic!("{:?}", a.rules[2])
    };
    assert_eq!(after.len(), 1);
    let Rule::Media { rules, .. } = &a.rules[3] else {
        panic!("{:?}", a.rules[3])
    };
    assert!(matches!(&rules[0], Rule::NestedDeclarations(d) if d.len() == 1));
}

#[test]
fn rules_keep_their_order_and_place() {
    let sheet = Stylesheet::parse(
        r#"@charset "utf-8";
        @import "a.css" screen;
        @layer base, theme;
        @namespace svg url(http://www.w3.org/2000/svg);
        @namespace "http://www.w3.org/1999/xhtml";
        svg|rect, a { fill: red }
        @import "late.css";
        @namespace late "x";
        @unknown { a { b: c } }
        $bad { x: y }
        @media (min-width: 1px) { p { q: r } @import "x"; }
        @supports (display: grid) { @layer l { s { t: u } } }
        @font-face { font-family: x }
        @keyframes k { from { a: b } }
        @page :first { margin: 0 }"#
            .into(),
    );
    let kinds: Vec<&str> = sheet
        .rules()
        .iter()
        .map(|r| match r {
            Rule::Import { .. } => "import",
            Rule::Layer { .. } => "layer",
            Rule::Namespace { .. } => "namespace",
            Rule::Style(_) => "style",
            Rule::Media { .. } => "media",
            Rule::Supports { .. } => "supports",
            Rule::FontFace(_) => "font-face",
            Rule::Keyframes { .. } => "keyframes",
            Rule::Page { .. } => "page",
            Rule::NestedDeclarations(_) => "declarations",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "import",
            "layer",
            "namespace",
            "namespace",
            "style",
            "media",
            "supports",
            "font-face",
            "keyframes",
            "page"
        ]
    );
    let rect = style(&sheet.rules()[4]);
    let Component::Type { ns, .. } = rect.selectors.0[0].components[0] else {
        panic!()
    };
    assert!(matches!(ns, Namespace::Prefix { .. }));
    let Component::Type { ns, .. } = rect.selectors.0[1].components[0] else {
        panic!()
    };
    assert!(matches!(ns, Namespace::Implicit(Some(_))));
    let Rule::Media { rules, .. } = &sheet.rules()[5] else {
        panic!()
    };
    assert_eq!(rules.len(), 1, "@import inside @media is dropped");
    assert_eq!(sheet.bodies(), (5, 0));
}

#[test]
fn nesting_is_bounded() {
    let n = 200;
    let src = format!("{}{}", "a { b: c; ".repeat(n), "}".repeat(n));
    let sheet = Stylesheet::parse(src);
    let mut depth = 0;
    let mut r = &sheet.rules()[0];
    loop {
        depth += 1;
        match style(r).rules.first() {
            Some(next) => r = next,
            None => break,
        }
    }
    assert_eq!(depth, MAX_NESTING as usize);
}

#[test]
fn sheets_parse_on_the_worker_thread() {
    fn shared<T: Send + Sync>() {}
    shared::<Stylesheet>();
    let worker = Worker::new(2);
    worker.submit(1, Source::Text(".x { a: b }".into()));
    worker.submit(
        2,
        Source::Bytes {
            bytes: b"\xEF\xBB\xBF#caf\xC3\xA9 { a: b }".to_vec(),
            protocol: None,
            environment: None,
        },
    );
    worker.submit(3, Source::Text("not a rule".into()));
    let held = HeldAtoms::new();
    let mut seen = Vec::new();
    for _ in 0..3 {
        let p = worker.recv();
        let atoms = p.sheet.names().atoms(&held);
        assert_eq!(atoms.len(), p.sheet.names().len());
        seen.push((p.id, p.sheet.rules().len()));
        if p.id == 2 {
            assert_eq!(
                p.sheet.names().get(madcawl_css::Name::from_index(0)),
                "café"
            );
        }
    }
    assert_eq!(seen, [(1, 1), (2, 1), (3, 0)]);
    assert!(worker.try_recv().is_none());
    drop(worker);
}
