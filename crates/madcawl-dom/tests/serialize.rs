//! Every WPT tree construction case the parsing gauge runs, parsed,
//! serialized by the HTML fragment serialization algorithm, and parsed
//! again: the second tree must equal the first. The cases where the
//! algorithm itself does not give the tree back are listed, each with
//! why, in `tests/expect/serialize-no-roundtrip.txt`; a listed case that
//! does round trip fails the test as much as an unlisted one that does
//! not.

#[path = "../../madcawl-html/tests/support/wpt.rs"]
#[allow(dead_code)]
mod wpt;

use std::collections::BTreeMap;
use std::path::Path;

use madcawl_dom::{Dom, NodeId};
use madcawl_html::{
    Attrs, FragmentContext, HeldAtoms, Namespace, Options, QuirksMode, Tokenizer, TreeBuilder,
    TreeSink, atom,
};
use wpt::{Feed, Test, feed_all, parse_tests};

/// The DOM `data` parses into; the context element of a fragment, a
/// new element the fragment's nodes are moved into (a template's
/// contents for a template), as setting `innerHTML` would, `None` for a
/// document; and the node whose children are the result.
fn parse_in(
    data: &[u8],
    fragment: Option<&[u8]>,
    scripting: bool,
) -> (Dom, Option<NodeId>, NodeId) {
    let opts = Options {
        scripting,
        ..Options::default()
    };
    let mut dom = Dom::new();
    let mut tok = Tokenizer::new();
    let names = HeldAtoms::new();
    let mut builder = match fragment {
        None => TreeBuilder::new(dom, opts),
        Some(ctx) => {
            let (ns, local) = match ctx.split(|&b| b == b' ').collect::<Vec<_>>()[..] {
                [b"svg", l] => (Namespace::Svg, l),
                [b"math", l] => (Namespace::MathMl, l),
                [l] => (Namespace::Html, l),
                _ => panic!("bad fragment context"),
            };
            let form = (ns == Namespace::Html && local == b"form")
                .then(|| dom.create_element(Namespace::Html, atom!("form"), Attrs::empty()));
            let b = TreeBuilder::new_fragment(
                dom,
                opts,
                FragmentContext {
                    ns,
                    local: names.hold(local),
                    attrs: &[],
                    form,
                    quirks: QuirksMode::NoQuirks,
                },
            );
            tok.set_state(b.tokenizer_state());
            b
        }
    };
    feed_all(&mut tok, &mut builder, data, Feed::Whole);
    let mut dom = builder.into_sink();
    let Some(ctx) = fragment else {
        let doc = dom.document();
        return (dom, None, doc);
    };
    let root = dom
        .first_child(dom.document())
        .expect("the fragment's root");
    let (ns, local) = match ctx.split(|&b| b == b' ').collect::<Vec<_>>()[..] {
        [b"svg", l] => (Namespace::Svg, l),
        [b"math", l] => (Namespace::MathMl, l),
        [l] => (Namespace::Html, l),
        _ => unreachable!(),
    };
    let el = dom.create_element(ns, names.hold(local), Attrs::empty());
    let into = dom.contents(el).unwrap_or(el);
    let first = dat(&dom, root);
    dom.reparent_children(root, into);
    assert_eq!(dat(&dom, into), first, "moving the fragment kept it");
    (dom, Some(el), into)
}

fn dat(dom: &Dom, root: NodeId) -> Vec<u8> {
    let mut out = Vec::new();
    dom.write_dat(root, &mut out);
    out
}

/// The first scripting mode in which `test` does not round trip, with
/// the serialization and both trees.
fn round_trip(test: &Test) -> Result<(), String> {
    let modes: &[bool] = match test.script {
        Some(s) => {
            if s {
                &[true]
            } else {
                &[false]
            }
        }
        None => &[false, true],
    };
    for &scripting in modes {
        let ctx = test.fragment.as_deref();
        let (dom, el, root) = parse_in(&test.data, ctx, scripting);
        let first = dat(&dom, root);
        let mut want = test.document.clone();
        want.push(b'\n');
        assert_eq!(
            String::from_utf8_lossy(&first),
            String::from_utf8_lossy(&want),
            "line {}: the first parse differs from the expected tree",
            test.line
        );
        let mut html = Vec::new();
        // as `innerHTML` reads the context element
        dom.write_html(el.unwrap_or(root), scripting, &mut html);
        let (dom2, _, root2) = parse_in(&html, ctx, scripting);
        let second = dat(&dom2, root2);
        if first != second {
            return Err(format!(
                "scripting={scripting}{}\n#data\n{}\n#serialized\n{}\n#first\n{}#second\n{}",
                ctx.map(|f| format!(" fragment={}", String::from_utf8_lossy(f)))
                    .unwrap_or_default(),
                String::from_utf8_lossy(&test.data),
                String::from_utf8_lossy(&html),
                String::from_utf8_lossy(&first),
                String::from_utf8_lossy(&second),
            ));
        }
    }
    Ok(())
}

/// `<file>:<line> <class> <why>` lines, without comments and blanks.
fn listed(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut w = l.split_whitespace();
            let case = w.next().unwrap().to_string();
            (case, w.next().expect("a class").to_string())
        })
        .collect()
}

#[test]
fn parse_serialize_reparse_gives_the_same_tree() {
    let html_tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("../madcawl-html/tests");
    let mut files: Vec<_> = std::fs::read_dir(html_tests.join("data/wpt-tree-construction"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "dat"))
        .collect();
    files.sort();
    let skip = listed(&html_tests.join("expect/wpt-parsing-skip.txt"));
    let expect = listed(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expect/serialize-no-roundtrip.txt"),
    );
    let (mut total, mut skipped, mut same) = (0, 0, 0);
    let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
    let mut unexpected = Vec::new();
    let mut seen = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (mut f_total, mut f_same) = (0, 0);
        for test in parse_tests(&std::fs::read(path).unwrap()) {
            let case = format!("{name}:{}", test.line);
            if skip.contains_key(&case) {
                skipped += 1;
                continue;
            }
            total += 1;
            f_total += 1;
            let got = round_trip(&test);
            match (&got, expect.get(&case)) {
                (Ok(()), None) => {}
                (Err(_), Some(class)) => *by_class.entry(class.clone()).or_default() += 1,
                (Ok(()), Some(class)) => {
                    unexpected.push(format!("{case} listed as {class} but round trips"))
                }
                (Err(e), None) => unexpected.push(format!("{case} does not round trip\n{e}")),
            }
            if expect.contains_key(&case) {
                seen.push(case);
            }
            if got.is_ok() {
                same += 1;
                f_same += 1;
            }
        }
        eprintln!("file {name} same {f_same}/{f_total}");
    }
    for u in &unexpected {
        eprintln!("{u}\n");
    }
    for (class, n) in &by_class {
        eprintln!("no round trip by the algorithm itself: {n} {class}");
    }
    let missing: Vec<_> = expect.keys().filter(|k| !seen.contains(k)).collect();
    eprintln!(
        "{same}/{total} parse, serialize, re-parse to the same tree, {} listed as not \
         round tripping by the algorithm, {} unexpected, {skipped} skipped as listed",
        by_class.values().sum::<usize>(),
        unexpected.len()
    );
    assert!(
        missing.is_empty(),
        "listed but not in the data: {missing:?}"
    );
    assert!(
        unexpected.is_empty(),
        "{} cases unexpected",
        unexpected.len()
    );
}

fn html_of(input: &str, scripting: bool) -> String {
    let (dom, _, root) = parse_in(input.as_bytes(), None, scripting);
    let body = dom
        .descendants(root)
        .find(|&n| dom.local_name(n) == Some(atom!("body")))
        .unwrap();
    let mut out = Vec::new();
    dom.write_html(body, scripting, &mut out);
    String::from_utf8(out).unwrap()
}

#[test]
fn escaping_and_names_follow_the_algorithm() {
    let cases = [
        // text mode: &, U+00A0, < and >; attribute mode adds "
        (
            "<p title='a&amp;b&nbsp;\"<>'>a&amp;b&nbsp;\"&lt;&gt;</p>",
            "<p title=\"a&amp;b&nbsp;&quot;&lt;&gt;\">a&amp;b&nbsp;\"&lt;&gt;</p>",
        ),
        // void elements, and the ones that only serialize as void
        (
            "<br><img src=x><input><wbr><param><keygen><frameset>",
            "<br><img src=\"x\"><input><wbr><param><keygen>",
        ),
        // raw text in HTML elements only; svg style text is escaped
        (
            "<xmp>a&<b></xmp><svg><style>a&amp;&lt;</style></svg>",
            "<xmp>a&<b></xmp><svg><style>a&amp;&lt;</style></svg>",
        ),
        // attribute prefixes by namespace, foreign names keep their case
        (
            "<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='l' xlink:href='#a' \
             xml:lang='en' viewbox='0'><foreignobject></foreignobject></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"l\" xlink:href=\"#a\" \
             xml:lang=\"en\" viewBox=\"0\"><foreignObject></foreignObject></svg>",
        ),
        // an HTML element's xlink:href is a plain attribute of that name
        ("<a xlink:href=b></a>", "<a xlink:href=\"b\"></a>"),
        // template contents, comments as they are
        (
            "<body><template><p>a</p><!--x--y--></template>",
            "<template><p>a</p><!--x--y--></template>",
        ),
        ("<pre>\n\na</pre>", "<pre>\na</pre>"),
    ];
    for (input, want) in cases {
        assert_eq!(html_of(input, false), want, "{input}");
    }
    // template contents have no browsing context, so their noscript text
    // is escaped even with scripting enabled
    assert_eq!(
        html_of(
            "<body><template><noscript>&amp;<b></noscript></template>",
            true
        ),
        "<template><noscript>&amp;amp;&lt;b&gt;</noscript></template>"
    );
    // noscript text is literal only when scripting is enabled
    assert_eq!(
        html_of("<body><noscript>&amp;<b></noscript>", true),
        "<noscript>&amp;<b></noscript>"
    );
    assert_eq!(
        html_of("<body><noscript>a&amp;</noscript>", false),
        "<noscript>a&amp;</noscript>"
    );
}

#[test]
fn a_document_and_an_element_written_whole() {
    let (dom, _, doc) = parse_in(b"<!DOCTYPE html PUBLIC 'x'><!--c--><p id=a>t", None, false);
    let mut out = Vec::new();
    dom.write_outer_html(doc, false, &mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "<!DOCTYPE html><!--c--><html><head></head><body><p id=\"a\">t</p></body></html>"
    );
    let p = dom
        .descendants(doc)
        .find(|&n| dom.local_name(n) == Some(atom!("p")))
        .unwrap();
    let mut out = Vec::new();
    dom.write_outer_html(p, false, &mut out);
    assert_eq!(String::from_utf8(out).unwrap(), "<p id=\"a\">t</p>");
}
