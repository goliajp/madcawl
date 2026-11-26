//! The elements a screenshot marks with their references: every shown
//! element an agent acts on (`Role::interactive`) whose box meets the
//! area drawn, in tree order, with the reference the views give it.

use madcawl_dom::Dom;

use crate::agents::{AgentError, Agents, SessionId};
use crate::geom::Rect;
use crate::record::VISIBLE;
use crate::refs::Ref;
use crate::role::Role;
use crate::tree::Inputs;

/// One marked element.
#[derive(Clone, Debug, PartialEq)]
pub struct Marked {
    pub reference: String,
    pub role: Role,
    pub name: String,
    /// Its box on the canvas, in CSS px
    pub rect: Rect,
}

impl Agents {
    /// The interactive elements shown in `area`, at most `limit`, after
    /// bringing the tree up to date; with the sequence they show the page
    /// at. Without geometry there are none.
    pub fn marks(
        &mut self,
        id: SessionId,
        dom: &mut Dom,
        inputs: Inputs,
        area: Rect,
        limit: usize,
    ) -> Result<(u64, Vec<Marked>), AgentError> {
        if !self.has_session(id) {
            return Err(AgentError::NoSession);
        }
        self.settle(dom, inputs, true);
        let seq = self.sync(dom, inputs);
        let (Some(t), Some(g)) = (self.tree(), inputs.geometry) else {
            return Ok((seq, Vec::new()));
        };
        let mut out = Vec::new();
        for n in dom.descendants(dom.document()) {
            if out.len() == limit {
                break;
            }
            let Some(r) = t.record(n) else { continue };
            if !r.role.interactive() || !r.has(VISIBLE) {
                continue;
            }
            let Some(p) = g.place(n).filter(|p| p.shown && p.rect.intersects(area)) else {
                continue;
            };
            let reference = Ref {
                index: n.index(),
                slot_gen: r.generation,
                page_gen: t.page_gen(),
            }
            .text();
            out.push(Marked {
                reference,
                role: r.role,
                name: t.name(r).to_owned(),
                rect: p.rect,
            });
        }
        Ok((seq, out))
    }
}
