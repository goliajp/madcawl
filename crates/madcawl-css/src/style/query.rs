//! `querySelector` and `querySelectorAll`: the elements under a node
//! that a selector list matches, by the same matcher the cascade uses.
//! The list is parsed on its own first, so that text a selector list
//! cannot hold (a brace, an unbalanced bracket) is refused rather than
//! read as more rules.

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::QuirksMode;
use madcawl_str::HeldAtoms;

use crate::Span;
use crate::names::Names;
use crate::selector::{Context, SelectorError, parse_selector_list};
use crate::sheet::Stylesheet;

use super::device::Device;
use super::matching::Matcher;
use super::rules::{Origin, Rules};
use super::states::ElementStates;

/// The elements among the descendants of `scope`, in tree order, that
/// `selectors` matches, at most `limit` of them; an error when it is
/// not a selector list.
pub fn query(dom: &Dom, scope: NodeId, selectors: &str, limit: usize) -> Result<Vec<NodeId>, SelectorError> {
    let span = Span::new(0, selectors.len());
    parse_selector_list(selectors, span, &mut Names::new(), &Context::default())?;
    if selectors.contains(['{', '}', ';']) {
        return Err(SelectorError);
    }
    let sheet = Stylesheet::parse(format!("{selectors}{{}}")).map_err(|_| SelectorError)?;
    let held = HeldAtoms::new();
    let mut rules = Rules::new();
    rules.add(&sheet, Origin::Author, &held, &Device::default());
    if rules.entries.is_empty() {
        return Err(SelectorError);
    }
    let states = ElementStates::new();
    let m = Matcher {
        dom,
        rules: &rules,
        quirks: dom.quirks_mode() == QuirksMode::Quirks,
        places: Default::default(),
        states: &states,
        pseudo: Default::default(),
    };
    let mut out = Vec::new();
    for n in dom.descendants(scope) {
        if out.len() >= limit {
            break;
        }
        if dom.kind(n) == NodeKind::Element && rules.entries.iter().any(|e| m.entry(e, n)) {
            out.push(n);
        }
    }
    Ok(out)
}
