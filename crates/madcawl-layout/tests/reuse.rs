//! Reusing a box's last result gives every element the border box it has when every box is laid out every
//! time.

mod common;

use common::*;

fn page(body: &str) -> String {
    format!("<!doctype html><body style='margin:0;font:16px/20px Menlo'>{body}")
}

#[test]
fn a_clamped_auto_height_is_not_a_fixed_one() {
    for body in [
        // a column item whose container's maximum stops its auto height: its children overflow it
        "<div style='display:flex;flex-direction:column'><div id=x style='display:flex;\
         flex-direction:column;max-height:100px'><div id=c style='height:200px'></div></div></div>",
        // its minimum raises it: a growing child fills it
        "<div style='display:flex;flex-direction:column'><div id=x style='display:flex;\
         flex-direction:column;min-height:100px'><div id=c style='flex-grow:1'>x</div></div></div>",
        // across a row its maximum is what a stretched child takes
        "<div style='display:flex'><div id=x style='display:flex;max-height:100px'>\
         <div id=c><div style='height:200px'></div></div></div></div>",
        "<div style='display:flex'><div id=x style='display:flex;min-height:100px'>\
         <div id=c>x</div></div></div>",
        "<div style='display:flex;flex-direction:column;height:300px'><div id=x style='display:flex;\
         max-height:50px'><div id=c style='height:80px'></div></div></div>",
    ] {
        same_without_reuse(&page(body), 800.0, 600.0);
    }
}

#[test]
fn nested_flex_with_percentages_of_the_height() {
    let nest = |open: &str, leaf: &str, n: usize| {
        page(&format!(
            "{}<div id=leaf style='{leaf}'>x</div>{}",
            open.repeat(n),
            "</div>".repeat(n)
        ))
    };
    for open in [
        "<div style='display:flex'>",
        "<div style='display:flex;flex-direction:column'>",
        "<div style='display:flex;flex-direction:column;min-height:100%'>",
        "<div style='display:flex;row-gap:1%;flex-wrap:wrap'>",
        "<div style='display:flex;min-height:10%'>",
    ] {
        for leaf in ["height:50%", "min-height:100%", ""] {
            same_without_reuse(&nest(open, leaf, 6), 800.0, 600.0);
        }
    }
}

#[test]
fn stretched_and_measured_items() {
    for body in [
        "<div style='display:flex;height:120px'><div id=a><div id=b style='height:50%'>x</div></div>\
         <div id=c style='align-self:center'>y</div></div>",
        "<div style='display:flex;flex-direction:column;height:200px'><div id=a style='flex:1'>\
         <div id=b style='height:25%'></div></div><div id=c style='max-height:20px;flex:1'></div></div>",
        "<div style='display:flex;flex-wrap:wrap;width:100px'><div id=a style='width:60px'>x</div>\
         <div id=b style='width:60px;min-height:30px'>y</div></div>",
        "<div style='display:flex'><div id=a style='display:flex;flex-direction:column;min-height:40px'>\
         <div id=b style='margin:5px'>x</div></div><div id=c style='height:90px'></div></div>",
    ] {
        same_without_reuse(&page(body), 800.0, 600.0);
    }
}

#[test]
fn floats_held_by_a_formatting_context_root_are_not_a_minimum() {
    // the item's floats raise its auto height: laid out at that height as a fixed one, it is the same box
    same_without_reuse(
        &page(
            "<div style=display:inline-flex><div style=display:flex><div id=c>\
             <div style='float:left;min-height:30px'></div></div></div></div>",
        ),
        800.0,
        600.0,
    );
}
