//! Resolving the grid and table properties.

use std::sync::Arc;

use super::computed::ComputedStyle;
use super::length::{Ctx, Lp, SpecLen};
use super::props::{Prop, Specified};

fn len(l: &Ctx) -> impl FnMut(&SpecLen) -> Option<Lp> + '_ {
    move |x| x.resolve(l, true)
}

/// Sets `p` when it is a grid or table property: `Some(false)` when its value is invalid at computed-value
/// time, `None` when `p` is some other property.
pub(crate) fn apply(s: &mut ComputedStyle, p: Prop, v: &Specified, l: &Ctx) -> Option<bool> {
    use Prop as P;
    let ok = match (p, v) {
        (P::GridTemplateColumns | P::GridTemplateRows, Specified::Tracks(t)) => {
            let t = match t {
                Some(t) => match t.map(&mut len(l)) {
                    Some(t) => Some(t),
                    None => return Some(false),
                },
                None => None,
            };
            let r = Arc::make_mut(&mut s.rare);
            if p == P::GridTemplateColumns {
                r.grid_template_columns = t;
            } else {
                r.grid_template_rows = t;
            }
            true
        }
        (P::GridAutoColumns | P::GridAutoRows, Specified::AutoTracks(t)) => {
            let Some(t) = t
                .iter()
                .map(|x| x.map(&mut len(l)))
                .collect::<Option<Arc<[_]>>>()
            else {
                return Some(false);
            };
            let r = Arc::make_mut(&mut s.rare);
            if p == P::GridAutoColumns {
                r.grid_auto_columns = t;
            } else {
                r.grid_auto_rows = t;
            }
            true
        }
        (P::GridTemplateAreas, Specified::Areas(a)) => {
            Arc::make_mut(&mut s.rare).grid_template_areas = a.clone();
            true
        }
        (P::GridAutoFlow, Specified::AutoFlow(f)) => {
            Arc::make_mut(&mut s.rare).grid_auto_flow = *f;
            true
        }
        (
            P::GridRowStart | P::GridColumnStart | P::GridRowEnd | P::GridColumnEnd,
            Specified::GridLine(g),
        ) => {
            let k = match p {
                P::GridRowStart => 0,
                P::GridColumnStart => 1,
                P::GridRowEnd => 2,
                _ => 3,
            };
            Arc::make_mut(&mut s.rare).grid_lines[k] = g.clone();
            true
        }
        (P::JustifyItems, Specified::Align(a)) => {
            Arc::make_mut(&mut s.rare).justify_items = *a;
            true
        }
        (P::JustifySelf, Specified::Align(a)) => {
            Arc::make_mut(&mut s.rare).justify_self = *a;
            true
        }
        (P::TableLayout, Specified::TableLayout(t)) => {
            Arc::make_mut(&mut s.rare).table_layout = *t;
            true
        }
        (P::BorderCollapse, Specified::BorderCollapse(b)) => {
            Arc::make_mut(&mut s.text).border_collapse = *b;
            true
        }
        (P::CaptionSide, Specified::CaptionSide(c)) => {
            Arc::make_mut(&mut s.text).caption_side = *c;
            true
        }
        (P::BorderSpacing, Specified::Spacing(h, v)) => {
            let (Some(h), Some(v)) = (h.resolve(l, true), v.resolve(l, true)) else {
                return Some(false);
            };
            Arc::make_mut(&mut s.text).border_spacing = [h.px.max(0), v.px.max(0)];
            true
        }
        _ => return None,
    };
    Some(ok)
}
