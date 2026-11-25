//! Checking an action's target before acting on it: shown,
//! enabled, in the viewport, and not covered, which is found the way a
//! real click finds it, by hit testing a point of the target's own boxes;
//! and setting a control's state for an action.

use madcawl_dom::{Dom, NodeId};

use crate::agents::{AgentError, Agents, SessionId};
use crate::controls::{Control, Controls};
use crate::geom::{Geometry, Rect};
use crate::record::{DISABLED, Record, VISIBLE};
use crate::refs::Ref;
use crate::role::Role;

/// The check an action's target failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    NotVisible,
    Disabled,
    /// Laid out nowhere: no box to act on
    NoBox,
    /// Another element is on top at every point tried: its reference
    Obscured(Option<String>),
    NotEditable,
}

impl Check {
    pub fn message(&self) -> String {
        match self {
            Check::NotVisible => "check failed: not visible (hidden, or of no size)".into(),
            Check::Disabled => "check failed: disabled".into(),
            Check::NoBox => "check failed: not laid out, nothing to act on".into(),
            Check::Obscured(Some(r)) => format!("check failed: obscured by [{r}]"),
            Check::Obscured(None) => "check failed: obscured".into(),
            Check::NotEditable => "check failed: not a control text can be typed into".into(),
        }
    }
}

/// Moves the viewport so some of `n` is in it; false when it was already.
pub fn scroll_into_view(geo: &mut Geometry, n: NodeId) -> bool {
    if geo.in_viewport(n) != Some(false) {
        return false;
    }
    let Some(p) = geo.place(n) else { return false };
    // a little above, so the target is not flush with the top edge
    let y = p.rect.y - (geo.viewport.h - p.rect.h.min(geo.viewport.h)) / 3.0;
    geo.scroll_to(y);
    true
}

/// The most of a target's boxes in the viewport a check tries, so a
/// target of many fragments costs no more than this many hit tests a point.
pub const MAX_RECTS: usize = 8;

/// A point of `n` where a hit test finds `n` or something inside it;
/// else what covers the first point tried.
pub fn pick_point(geo: &Geometry, dom: &Dom, n: NodeId) -> Result<(f32, f32), Option<NodeId>> {
    let view = geo.viewport;
    let mut first_hit = None;
    let in_view = |r: Rect| r.intersect_with(view);
    let mut rects: Vec<Rect> = geo.boxes_of(n).filter_map(in_view).take(MAX_RECTS).collect();
    if rects.is_empty() && geo.boxes_of(n).next().is_none() {
        rects.extend(geo.place(n).and_then(|p| in_view(p.rect)));
    }
    for r in rects {
        let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
        let inset = |a: f32, b: f32| if b - a > 2.0 { (a + 1.0, b - 1.0) } else { (a, a) };
        let ((ix0, ix1), (iy0, iy1)) = (inset(x0, x1), inset(y0, y1));
        let points = [((x0 + x1) / 2.0, (y0 + y1) / 2.0), (ix0, iy0), (ix1, iy0), (ix0, iy1), (ix1, iy1)];
        for (x, y) in points {
            match geo.hit(x, y) {
                Some(h) if dom.is_inclusive_ancestor_of(n, h) => return Ok((x, y)),
                h => {
                    first_hit = first_hit.or(h);
                }
            }
        }
    }
    Err(first_hit)
}

impl Agents {
    /// The record of `n` while an agent is attached.
    pub fn record_of(&self, n: NodeId) -> Option<Record> {
        self.tree()?.record(n).copied()
    }

    /// The reference of `n`, or of its nearest ancestor a view would
    /// write one for.
    pub fn reference_of(&self, dom: &Dom, n: NodeId) -> Option<String> {
        let t = self.tree()?;
        let mut up = Some(n);
        while let Some(a) = up {
            if let Some(r) = t.record(a).filter(|r| r.role != Role::None && !r.role.transparent()) {
                let rf = Ref { index: a.index(), slot_gen: r.generation, page_gen: t.page_gen() };
                return Some(rf.text());
            }
            up = dom.parent(a);
        }
        None
    }

    /// The checks an action makes of its target, once it is scrolled into
    /// the viewport: the point to act at, or the check that failed.
    pub fn check_target(&self, dom: &Dom, geo: &Geometry, n: NodeId, enabled: bool) -> Result<(f32, f32), Check> {
        let rec = self.record_of(n).unwrap_or_default();
        let shown = if rec.role == Role::None {
            geo.place(n).is_some_and(|p| p.shown)
        } else {
            rec.has(VISIBLE)
        };
        if !shown {
            return Err(Check::NotVisible);
        }
        if enabled && rec.has(DISABLED) {
            return Err(Check::Disabled);
        }
        if geo.place(n).is_none() {
            return Err(Check::NoBox);
        }
        pick_point(geo, dom, n).map_err(|h| Check::Obscured(h.and_then(|h| self.reference_of(dom, h))))
    }

    /// Changes the state of control `n` for session `id`; its record is
    /// read again at the next sync, so a diff shows the change.
    pub fn set_control(
        &mut self,
        id: SessionId,
        dom: &Dom,
        n: NodeId,
        f: impl FnOnce(&mut Control),
    ) -> Result<(), AgentError> {
        if !self.has_session(id) {
            return Err(AgentError::NoSession);
        }
        let t = self.tree_mut().ok_or(AgentError::NoSession)?;
        let c = t.controls.entry(n);
        f(c);
        if let Some(v) = c.value.take() {
            c.value = Some(crate::controls::capped(v));
        }
        t.mark(n);
        t.mark_up(dom, dom.parent(n));
        Ok(())
    }

    /// The control state actions have set on this page.
    pub fn controls(&self) -> Option<&Controls> {
        self.tree().map(|t| &t.controls)
    }
}
