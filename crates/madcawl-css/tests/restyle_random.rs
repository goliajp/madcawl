//! Restyling stays linear in the elements it reaches, however many
//! changes there are, and a long run of random edits, state changes and
//! appends, some of them batched into one restyle, gives the values a
//! full run gives after each restyle.

#[allow(dead_code)]
mod style;

use madcawl_css::selector::PseudoClass;
use madcawl_css::style::{Change, ElementStates};
use madcawl_dom::NodeId;
use madcawl_html::{Atom, Attrs, Namespace, TreeSink, atom};
use style::{Pair, by_id, check, parse, set, sheets};

#[test]
fn many_sibling_changes_visit_each_element_a_bounded_number_of_times() {
    let n = 2000;
    let mut dom = parse(&format!(
        "<!doctype html><style>.a ~ b {{ color: red }}</style><div id=p>{}</div>",
        "<b></b>".repeat(n)
    ));
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let bs: Vec<NodeId> = dom.children(by_id(&dom, "p")).collect();
    let ch: Vec<Change> = bs
        .iter()
        .map(|&b| set(&mut dom, b, atom!("class"), "a"))
        .collect();
    let r = check(&c, &dom, &mut s, &st, &ch);
    assert_eq!(r.styled, n as u64);
    assert!(r.visited <= 3 * n as u64, "{r:?}");
}

#[test]
fn appending_after_last_child_items_visits_a_bounded_number_each_time() {
    let mut dom = parse(
        "<!doctype html><style>li:last-child { color: red } li:only-of-type { float: left }</style><ul id=u></ul>",
    );
    let (list, st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let u = by_id(&dom, "u");
    let mut visited = 0;
    for k in 0..400 {
        let el = dom.create_element(Namespace::Html, atom!("li"), Attrs::empty());
        dom.append(u, el);
        let r = if k % 50 == 0 {
            check(&c, &dom, &mut s, &st, &[Change::Appended(el)])
        } else {
            c.keyed.restyle(&dom, &mut s, &st, &[Change::Appended(el)])
        };
        assert!(!r.full);
        visited += r.visited;
    }
    // each append: itself, the item before it, and the one sibling each
    // of the two selectors looks back over
    assert!(visited <= 5 * 400, "{visited}");
}

const SHEET: &str = ".a { color: red } .x .b { display: block } .y + li { float: left } \
    .y ~ li .b { margin: 2px } li:nth-child(2n of .x) { padding: 1px } [data-v=on] > i { opacity: 0.5 } \
    li:not(.x) span { font-weight: bold } :is(.y, #q) i { text-align: right } \
    .x { & > i { border: 1px solid } } #q { font-size: 20px } \
    :is(:is(.x) + li, .z) .b { text-indent: 3px } :not(:is(.y ~ li)) > i { z-index: 2 } \
    li:hover .b { color: green } li:focus + li i { float: right } \
    :lang(fr) .b { font-style: italic } :dir(rtl) i { text-align: left } \
    li:last-child { padding: 4px } li:last-of-type i { opacity: 0.2 } li:nth-last-child(2) span { margin: 1px } ul:empty { float: left } \
    p:last-of-type { float: left } :nth-last-of-type(2) { padding: 3px } :last-child > span { margin: 4px }";

/// One random change to `items`, made and described.
fn edit(
    dom: &mut madcawl_dom::Dom,
    st: &mut ElementStates,
    items: &mut Vec<NodeId>,
    step: u32,
    seed: u32,
) -> Change {
    let el = items[(seed >> 8) as usize % items.len()];
    let pick = |v: &[&'static str], shift: u32| v[(seed >> shift) as usize % v.len()];
    match step % 8 {
        0 => set(
            dom,
            el,
            atom!("class"),
            pick(&["a", "a x", "a y", "x y", "z"], 4),
        ),
        1 => set(dom, el, atom!("id"), pick(&["q", ""], 4)),
        2 => {
            let data_v = Atom::lookup(b"data-v").expect("on the page");
            set(dom, el, data_v, pick(&["on", "off"], 5))
        }
        3 => set(
            dom,
            el,
            atom!("style"),
            pick(&["", "color: blue", "display: none"], 6),
        ),
        4 => set(dom, el, atom!("lang"), pick(&["fr", "en", ""], 5)),
        5 => set(dom, el, atom!("dir"), pick(&["rtl", "ltr", "auto"], 5)),
        6 => {
            let pseudo = [PseudoClass::Hover, PseudoClass::Focus][(seed >> 4) as usize % 2];
            st.set(el, pseudo, seed & 64 == 0);
            Change::State { el, pseudo }
        }
        _ => {
            let ul = dom.parent(el).expect("in the list");
            let name =
                [atom!("li"), atom!("p"), atom!("li"), atom!("div")][(seed >> 4) as usize % 4];
            let li = dom.create_element(Namespace::Html, name, Attrs::empty());
            let span = dom.create_element(Namespace::Html, atom!("span"), Attrs::empty());
            dom.set_attribute(span, atom!("class"), b"b");
            dom.append(li, span);
            dom.append(ul, li);
            items.push(li);
            Change::Appended(li)
        }
    }
}

#[test]
fn many_edits_each_match_a_full_run() {
    let cell = "<li class=a><span class=b>x</span><i>y</i></li>";
    let mut dom = parse(&format!(
        "<!doctype html><style>{SHEET}</style><ul id=u>{}</ul><ul id=e></ul>",
        cell.repeat(12) + "<p data-v=x></p>"
    ));
    let (list, mut st) = (sheets(&dom), ElementStates::new());
    let c = Pair::new(&list);
    let mut s = c.compute_with(&dom, &st);
    let mut items: Vec<NodeId> = dom.children(by_id(&dom, "u")).collect();
    let (mut seed, mut styled, mut whole) = (7u32, 0, 0);
    let (mut step, mut batched) = (0, 0);
    while step < 400 {
        // every fifth restyle takes a batch of up to five changes
        let batch = if step % 5 == 4 {
            1 + (seed >> 9) % 5
        } else {
            1
        };
        let mut ch = Vec::new();
        for _ in 0..batch {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            // half the changes of a batch are appends
            let kind = if batch > 1 && seed & 1 << 20 == 0 {
                7
            } else {
                step
            };
            ch.push(edit(&mut dom, &mut st, &mut items, kind, seed));
            step += 1;
        }
        let appends = ch
            .iter()
            .filter(|c| matches!(c, Change::Appended(_)))
            .count();
        batched += usize::from(appends > 1);
        let r = check(&c, &dom, &mut s, &st, &ch);
        assert!(!r.full, "step {step}");
        styled += r.styled;
        whole += c.plain.compute_with(&dom, &st).stats().elements;
    }
    assert!(batched >= 5, "batches with several appends: {batched}");
    assert!(
        styled < whole / 2,
        "fewer than a full run each time: {styled} of {whole}"
    );
}
