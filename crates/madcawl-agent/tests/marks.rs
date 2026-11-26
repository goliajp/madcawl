//! The elements a screenshot marks: shown, interactive, meeting the area, with the references views give.

mod common;

use common::{Page, ref_on};
use madcawl_agent::{Rect, Role};

const HTML: &str = "<body style='margin:0'>\
    <a id=a href='/x' style='display:block;height:20px'>First link</a>\
    <button id=b style='display:block;height:20px'>Press</button>\
    <a id=h href='/y' style='display:none'>Hidden link</a>\
    <p style='height:2000px'>filler</p>\
    <a id=far href='/z'>Far link</a>";

#[test]
fn marks_are_the_shown_interactive_elements_in_the_area_with_their_view_refs() {
    let mut p = Page::new(HTML);
    let s = p.attach();
    let snap = p.snapshot(s);
    let view = Rect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 800.0,
    };
    let inputs = madcawl_agent::Inputs {
        styles: Some(&p.styles),
        geometry: p.geometry.as_ref(),
    };
    let (_, marks) = p.agents.marks(s, &mut p.dom, inputs, view, 100).unwrap();
    let got: Vec<(&str, Role)> = marks.iter().map(|m| (m.name.as_str(), m.role)).collect();
    assert_eq!(got, [("First link", Role::Link), ("Press", Role::Button)]);
    assert_eq!(marks[0].reference, ref_on(&snap, "First link"));
    assert_eq!(marks[1].reference, ref_on(&snap, "Press"));
    assert_eq!((marks[0].rect.y, marks[0].rect.h), (0.0, 20.0));
    assert_eq!(p.resolve(s, &marks[1].reference).unwrap(), p.by_id("b"));
    // the whole page reaches the far link; the limit cuts the list
    let page = Rect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 4000.0,
    };
    let inputs = madcawl_agent::Inputs {
        styles: Some(&p.styles),
        geometry: p.geometry.as_ref(),
    };
    let (_, all) = p.agents.marks(s, &mut p.dom, inputs, page, 100).unwrap();
    assert_eq!(all.last().map(|m| m.name.as_str()), Some("Far link"));
    let (_, one) = p.agents.marks(s, &mut p.dom, inputs, page, 1).unwrap();
    assert_eq!(one.len(), 1);
    assert!(
        p.agents.marks(s + 1, &mut p.dom, inputs, page, 1).is_err(),
        "another session"
    );
}
