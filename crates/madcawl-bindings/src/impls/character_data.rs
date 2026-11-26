//! CharacterData, with Text and Comment: the data of a text, comment or
//! processing instruction node, edited by UTF-16 code unit offsets.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_js::Engine;
use madcawl_str::DomString;

use crate::convert::{Thrown, throw_dom};
use crate::host::DomHost;

pub(crate) use super::collections::{next_element_sibling, previous_element_sibling};

fn text_of<E: Engine>(h: &DomHost<E>, n: NodeId) -> &DomString {
    h.dom.text(n).expect("character data")
}

/// `s[start..end]`, by code unit.
fn slice(s: &DomString, start: usize, end: usize) -> DomString {
    match s.as_latin1() {
        Some(b) => DomString::from_latin1(&b[start..end]),
        None => DomString::from_utf16(&s.units().skip(start).take(end - start).collect::<Vec<_>>()),
    }
}

fn concat(parts: &[&DomString]) -> DomString {
    if parts.iter().all(|p| p.is_latin1()) {
        let b: Vec<u8> = parts.iter().flat_map(|p| p.as_latin1().expect("latin-1").iter().copied()).collect();
        return DomString::from_latin1(&b);
    }
    DomString::from_utf16(&parts.iter().flat_map(|p| p.units()).collect::<Vec<_>>())
}

fn out_of_range<E: Engine>(e: &mut E, h: &mut DomHost<E>) -> Thrown {
    throw_dom(e, h, "IndexSizeError", "the offset is past the end of the data")
}

fn full<E: Engine>(e: &mut E, h: &mut DomHost<E>) -> Thrown {
    throw_dom(e, h, "QuotaExceededError", "the document holds all it may")
}

fn set<E: Engine>(e: &mut E, h: &mut DomHost<E>, n: NodeId, value: DomString) -> Result<(), Thrown> {
    if h.dom.set_text(n, value) { Ok(()) } else { Err(full(e, h)) }
}

/// The standard's "replace data".
fn replace<E: Engine>(e: &mut E, h: &mut DomHost<E>, n: NodeId, offset: u32, count: u32, with: &DomString) -> Result<(), Thrown> {
    let old = text_of(h, n).clone();
    let (offset, len) = (offset as usize, old.len());
    if offset > len {
        return Err(out_of_range(e, h));
    }
    let end = offset + (count as usize).min(len - offset);
    let new = concat(&[&slice(&old, 0, offset), with, &slice(&old, end, len)]);
    set(e, h, n, new)
}

pub(crate) fn data<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    Ok(text_of(h, this).clone())
}

pub(crate) fn set_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, value: DomString) -> Result<(), Thrown> {
    set(e, h, this, value)
}

pub(crate) fn length<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<u32, Thrown> {
    Ok(text_of(h, this).len() as u32)
}

pub(crate) fn substring_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, offset: u32, count: u32) -> Result<DomString, Thrown> {
    let s = text_of(h, this).clone();
    let (offset, len) = (offset as usize, s.len());
    if offset > len {
        return Err(out_of_range(e, h));
    }
    Ok(slice(&s, offset, offset + (count as usize).min(len - offset)))
}

pub(crate) fn append_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, d: DomString) -> Result<(), Thrown> {
    let len = text_of(h, this).len() as u32;
    replace(e, h, this, len, 0, &d)
}

pub(crate) fn insert_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, offset: u32, d: DomString) -> Result<(), Thrown> {
    replace(e, h, this, offset, 0, &d)
}

pub(crate) fn delete_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, offset: u32, count: u32) -> Result<(), Thrown> {
    replace(e, h, this, offset, count, &DomString::empty())
}

pub(crate) fn replace_data<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, offset: u32, count: u32, d: DomString) -> Result<(), Thrown> {
    replace(e, h, this, offset, count, &d)
}

pub(crate) fn remove<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<(), Thrown> {
    super::element::remove(e, h, this)
}

/// `new Text(data)`, `new Comment(data)`: a node of the window's document.
pub(crate) fn new_text<E: Engine>(e: &mut E, h: &mut DomHost<E>, d: DomString) -> Result<NodeId, Thrown> {
    match h.dom.new_text(d) {
        Some(n) => Ok(n),
        None => Err(full(e, h)),
    }
}

pub(crate) fn new_comment<E: Engine>(e: &mut E, h: &mut DomHost<E>, d: DomString) -> Result<NodeId, Thrown> {
    match h.dom.new_comment(d) {
        Some(n) => Ok(n),
        None => Err(full(e, h)),
    }
}

/// `splitText`: the data from `offset` on moves to a new text node put
/// after this one.
pub(crate) fn split_text<E: Engine>(e: &mut E, h: &mut DomHost<E>, this: NodeId, offset: u32) -> Result<NodeId, Thrown> {
    let s = text_of(h, this).clone();
    let (offset, len) = (offset as usize, s.len());
    if offset > len {
        return Err(out_of_range(e, h));
    }
    let new = new_text(e, h, slice(&s, offset, len))?;
    if let Some(p) = h.dom.parent(this) {
        let next = h.dom.next_sibling(this);
        h.dom.insert_node(p, new, next);
    }
    set(e, h, this, slice(&s, 0, offset))?;
    Ok(new)
}

/// The data of this text node and the text nodes next to it, in order.
pub(crate) fn whole_text<E: Engine>(_: &mut E, h: &mut DomHost<E>, this: NodeId) -> Result<DomString, Thrown> {
    let dom = &h.dom;
    let is_text = |n: &NodeId| dom.kind(*n) == NodeKind::Text;
    let mut first = this;
    while let Some(p) = dom.previous_sibling(first).filter(is_text) {
        first = p;
    }
    let run: Vec<&DomString> = std::iter::successors(Some(first), |&n| dom.next_sibling(n).filter(is_text))
        .map(|n| dom.text(n).expect("a text node"))
        .collect();
    Ok(concat(&run))
}
