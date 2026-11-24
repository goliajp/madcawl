//! Reading pages: what the text form holds, and that nothing hidden is
//! in it.

use std::path::Path;

use madcawl_css::style::Styles;
use madcawl_dom::{Dom, NodeKind};
use madcawl_html::Encoding;
use madcawl_shell::audit::{Verdict, audit, references};
use madcawl_shell::load::{self, Sheets};
use madcawl_shell::read::{parse_reference, reference, write_text};
use madcawl_shell::{Shown, read, rusage};
use madcawl_str::Atom;

struct Page {
    dom: Dom,
    styles: Styles,
    sheets: Sheets,
}

fn page_at(html: &[u8], base: Option<&Path>, charset: Option<Encoding>) -> Page {
    let (dom, e) = load::parse(html, charset);
    let mut sheets = Sheets::default();
    let styles = load::style(&dom, base, e, &mut sheets);
    Page {
        dom,
        styles,
        sheets,
    }
}

fn page(html: &str) -> Page {
    page_at(html.as_bytes(), None, None)
}

fn text_of(p: &Page) -> String {
    let shown = Shown::new(&p.dom, &p.styles);
    let r = read(&shown);
    let mut out = String::new();
    write_text(&r, "t.html", &mut out);
    out
}

const HIDING: &str = r#"<!doctype html><title>t</title>
<style>
.gone { display: none } .ghost { visibility: hidden } .clear { opacity: 0 }
.skip { content-visibility: hidden }
@media (min-width: 900px) { .wide { display: none } }
@media (max-width: 600px) { .narrow { display: none } }
</style>
<p>visible-plain <a class="gone" href="/x">SECRET-link</a> tail</p>
<p class="gone">SECRET-sheet</p>
<p style="display:none">SECRET-style-attribute</p>
<p hidden>SECRET-hidden-attribute</p>
<div class="ghost">SECRET-visibility <span style="visibility:visible">visible-again</span></div>
<div class="clear"><p>SECRET-opacity <b style="opacity:1">SECRET-opacity-child</b></p></div>
<div class="skip">SECRET-content-visibility</div>
<p class="wide">SECRET-media</p><p class="narrow">visible-narrow-rule</p>
<details><summary>visible-summary</summary>SECRET-details</details>
<table><tr><th>visible-head</th></tr><tr class="gone"><td>SECRET-row</td></tr>
<tr><td>visible-cell</td><td class="ghost">SECRET-cell</td></tr></table>
<form><input type="hidden" name="SECRET-input" value="SECRET-input-value">
<span id="lbl" class="gone">SECRET-label</span><input aria-labelledby="lbl" name="q">
<select name="s"><option>visible-option</option><option class="gone">SECRET-option</option></select>
<button class="clear">SECRET-button</button></form>"#;

#[test]
fn nothing_hidden_is_read() {
    let p = page(HIDING);
    let out = text_of(&p);
    assert!(!out.contains("SECRET"), "{out}");
    for v in [
        "visible-plain tail",
        "visible-again",
        "visible-narrow-rule",
        "visible-summary",
        "| visible-head |",
        "| visible-cell |  |",
        "\"visible-option\"",
    ] {
        assert!(out.contains(v), "{v} missing from\n{out}");
    }
}

#[test]
fn both_checks_pass_on_a_page_that_hides_things() {
    let p = page(HIDING);
    let shown = Shown::new(&p.dom, &p.styles);
    let r = read(&shown);
    let mut out = String::new();
    write_text(&r, "t.html", &mut out);
    let a = audit(&p.dom, &p.styles, &r, &out);
    assert_eq!(a.verdict(), Verdict::Pass, "{a:?}");
    assert!(a.hidden_tested >= 10, "{a:?}");
    assert_eq!(a.shown_found, a.shown_tested);
}

fn node_with_id(dom: &Dom, id: &str) -> madcawl_dom::NodeId {
    let want = Atom::lookup(id.as_bytes()).expect("an id in the page");
    dom.descendants(dom.document())
        .find(|&n| dom.kind(n) == NodeKind::Element && dom.element_id(n) == Some(want))
        .expect("the element")
}

#[test]
fn each_check_catches_a_leak() {
    let p =
        page(r#"<p>shown text here</p><p id="x" style="display:none">a hidden sentence here</p>"#);
    let shown = Shown::new(&p.dom, &p.styles);
    let mut r = read(&shown);
    let mut out = String::new();
    write_text(&r, "t.html", &mut out);
    assert_eq!(audit(&p.dom, &p.styles, &r, &out).verdict(), Verdict::Pass);
    let x = node_with_id(&p.dom, "x");
    let leaked = format!("{out}text [{}] a hidden sentence here\n", reference(x));
    let a = audit(&p.dom, &p.styles, &r, &leaked);
    assert_eq!((a.refs_hidden.len(), a.hidden_found.len()), (1, 1), "{a:?}");
    r.used.push(x);
    assert_eq!(
        audit(&p.dom, &p.styles, &r, &out).used_hidden,
        vec![x.index()]
    );
}

#[test]
fn nothing_shown_is_not_a_pass() {
    let p = page(r#"<div style="display:none">only hidden content here</div>"#);
    let shown = Shown::new(&p.dom, &p.styles);
    let r = read(&shown);
    let mut out = String::new();
    write_text(&r, "t.html", &mut out);
    assert_eq!(audit(&p.dom, &p.styles, &r, &out).verdict(), Verdict::Empty);
}

#[test]
fn structure_reads_as_lines() {
    let p = page(
        r#"<header><nav aria-label="Main"><ul><li><a href="/a">Alpha</a><ul><li>deep</li></ul></li></ul></nav></header>
<main><h1>Title</h1><p>Some <b>bold</b> and a <a href="/x y">link (x)</a>.</p>
<h2><a href="/card">Card</a></h2><a href="/c2"><h3>Two</h3></a>
<table><caption>Prices</caption><tr><th>Item</th><th>Cost</th></tr><tr><td>Apple</td><td>1 | 2</td></tr></table>
<form><label for="q">Query</label><input id="q" name="q" placeholder="type">
<label>Pass <input type="password" name="pw" value="hunter2"></label>
<input type="checkbox" name="c" checked aria-label="Agree">
<select name="s"><option value="1">One</option><option selected value="2">Two</option></select>
<textarea name="t">a
b</textarea><button>Send</button></form>
<pre>  x  y
z</pre><div role="button" aria-pressed="true">Toggle</div></main>"#,
    );
    let out = text_of(&p);
    let lines: Vec<&str> = out
        .lines()
        .map(|l| l.split_once(" [e").map_or(l, |(k, _)| k))
        .collect();
    let want = [
        "banner",
        "  navigation",
        "    li",
        "      li",
        "main",
        "  h1",
        "  text",
        "  h2",
        "  h3",
        "  table",
        "    headrow",
        "    row",
        "  form",
    ];
    let at = lines.iter().position(|&l| l == "banner").expect("a banner");
    assert_eq!(&lines[at..at + want.len()], want, "{out}");
    for v in [
        "] Some bold and a [link (x)](/x%20y)[e",
        "] [Card](/card)[e",
        "] [Two](/c2)[e",
        "] 2x2 \"Prices\"",
        "] | Apple | 1 \\| 2 |",
        "textbox [e",
        "] \"Query\" name=\"q\" placeholder=\"type\"",
        "] \"Pass\" name=\"pw\" type=\"password\"\n",
        "] \"Agree\" name=\"c\" checked",
        "] \"\" name=\"s\" value=\"Two\"",
        "] \"Two\" value=\"2\" selected",
        "] \"\" name=\"t\" value=\"a\\nb\"",
        "button [e",
        "] \"Send\"",
        "    >   x  y\n    > z\n",
        "] \"Toggle\" pressed",
    ] {
        assert!(out.contains(v), "{v:?} missing from\n{out}");
    }
    assert!(!out.contains("hunter2"));
}

#[test]
fn references_stay_the_same_across_reads() {
    let a = text_of(&page(HIDING));
    let b = text_of(&page(HIDING));
    assert_eq!(a, b);
    let p = page("<p id=x>t</p>");
    let x = node_with_id(&p.dom, "x");
    assert_eq!(parse_reference(&reference(x)), Some(x.index()));
    assert_eq!(parse_reference("e"), None);
}

#[test]
fn linked_sheets_are_read_from_beside_the_page() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("linked-sheets");
    std::fs::create_dir_all(dir.join("css")).expect("a scratch directory");
    std::fs::write(dir.join("css/site.css"), ".gone { display: none }").expect("written");
    let html = br#"<link rel=stylesheet href="css/site.css?v=1">
<link rel=stylesheet href="https://example.com/remote.css">
<p>kept</p><p class=gone>SECRET-linked</p>"#;
    let p = page_at(html, Some(&dir), None);
    assert_eq!(p.sheets.linked, 1);
    assert_eq!(p.sheets.missing, ["https://example.com/remote.css"]);
    let out = text_of(&p);
    assert!(out.contains("kept") && !out.contains("SECRET"), "{out}");
}

#[test]
fn a_charset_from_outside_decides_the_encoding() {
    let html = "<p>caf\u{e9}</p>".as_bytes();
    assert!(text_of(&page_at(html, None, Some(Encoding::Utf8))).contains("caf\u{e9}"));
    assert!(text_of(&page_at(html, None, None)).contains("caf\u{c3}\u{a9}"));
}

#[test]
fn peak_memory_is_measured() {
    let before = rusage::peak_rss();
    let v = vec![1u8; 64 << 20];
    assert!(v.iter().map(|&b| b as u64).sum::<u64>() > 0);
    assert!(rusage::peak_rss() >= before + (32 << 20), "{before}");
}

#[test]
fn references_are_parsed_where_the_text_form_puts_them() {
    let text = "madcawl-read 1\npage \"[e1]\"\ntitle \")[e2]\"\n---\n\
text [e3] see [a](/b)[e4] and [e5] in prose\n  pre [e6]\n    > )[e7] [e8]\n  li [e9] x\n";
    assert_eq!(references(text), [3, 4, 6, 9]);
}

fn read_and_audit(html: &str) -> String {
    let p = page(html);
    let shown = Shown::new(&p.dom, &p.styles);
    let r = read(&shown);
    let mut out = String::new();
    write_text(&r, "t.html", &mut out);
    let a = audit(&p.dom, &p.styles, &r, &out);
    assert_ne!(a.verdict(), Verdict::Fail, "{a:?}\n{out}");
    out
}

#[test]
fn links_are_named_by_the_images_in_them() {
    let out = read_and_audit(
        r#"<p>text so the page shows something</p>
<a href="/gh"><svg><title>GitHub home page</title><path d="M0"/></svg></a>
<a href="/star"><span role="img" aria-label="Star this"><svg><title>star icon</title></svg></span></a>
<a href="/lab"><svg aria-label="Labelled svg"><title>not this</title></svg></a>
<a href="/gone"><svg style="display:none"><title>SECRET-svg-title</title></svg></a>
<a href="/text">Words <svg><title>not used</title></svg></a>"#,
    );
    for v in [
        "[GitHub home page](/gh)",
        "[Star this](/star)",
        "[Labelled svg](/lab)",
        "[](/gone)",
        "[Words](/text)",
    ] {
        assert!(out.contains(v), "{v} missing from\n{out}");
    }
    assert!(
        !out.contains("SECRET") && !out.contains("star icon"),
        "{out}"
    );
}

#[test]
fn submit_and_reset_without_a_value_get_their_default_names() {
    let out = read_and_audit(
        r#"<form><input type=submit><input type=reset value=""><input type=submit value="Go">
<input type=image alt="Search"><input type=image></form>"#,
    );
    let names: Vec<&str> = out
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("button [e"))
        .map(|l| l.split_once("] ").expect("a name").1)
        .collect();
    assert_eq!(
        names,
        [
            "\"Submit\"",
            "\"Reset\" type=\"reset\"",
            "\"Go\"",
            "\"Search\" type=\"image\"",
            "\"Submit\" type=\"image\""
        ],
        "{out}"
    );
}

#[test]
fn many_controls_named_by_one_node_share_its_text() {
    let words: String = (0..500).map(|i| format!("<span>w{i}</span> ")).collect();
    let inputs: String = (0..500)
        .map(|i| format!(r#"<input name="i{i}" aria-labelledby="big small">"#))
        .collect();
    let out = read_and_audit(&format!(
        r#"<p id="big">{words}</p><b id="small">tail</b><form>{inputs}</form>"#
    ));
    let name = format!(
        "\"{} tail\"",
        (0..500)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let named = out.lines().filter(|l| l.contains(&name)).count();
    assert_eq!(named, 500, "{out}");
}

#[test]
fn a_page_is_read_from_stdin() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_madcawl-shell"))
        .args(["read", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(b"<title>piped</title><p>from standard input</p>")
        .expect("written");
    let o = child.wait_with_output().expect("it exits");
    assert!(o.status.success());
    let out = String::from_utf8(o.stdout).expect("utf-8");
    assert!(
        out.starts_with("madcawl-read 1\npage \"-\"\ntitle \"piped\"\n---\n"),
        "{out}"
    );
    assert!(
        out.contains("text [e") && out.contains("] from standard input\n"),
        "{out}"
    );
    let stats = String::from_utf8(o.stderr).expect("utf-8");
    assert!(
        stats.contains("\nbytes 46\n") || stats.starts_with("bytes 46\n"),
        "{stats}"
    );
}
