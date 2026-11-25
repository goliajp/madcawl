//! What painting reads of an inline formatting context: its text shaped as line layout shaped it, so the
//! glyphs of each text run on a line can be found from the run's node and width. Reading it changes no
//! layout result.

use std::ops::Range;
use std::sync::Arc;

use madcawl_dom::NodeId;
use madcawl_text::{FaceId, Kind as SegKind, Shaped, Shaper};

use super::collect::{self, Ifc, ItemKind};
use crate::au::Au;
use crate::cx::Cx;
use crate::{Layout, tree};

/// A text item of the context: one text node's text, and the font it is shaped in.
#[derive(Clone, Debug)]
pub struct TextItem {
    pub node: NodeId,
    pub face: FaceId,
    pub size: f32,
    /// From the top of the text's content area to its baseline, as a text run's fragment is placed by
    pub ascent: Au,
    /// The first of its segments in [`InlineText::segs`]
    pub first_seg: usize,
}

/// A segment of the context's text with no break opportunity inside.
#[derive(Clone, Debug)]
pub struct TextSeg {
    /// Its item in [`InlineText::items`]; `None` for atomic inlines and line breaks
    pub item: Option<usize>,
    pub kind: SegKind,
    pub range: Range<usize>,
    /// The advance line layout gives it
    pub adv: Au,
    pub glyphs: Option<Arc<Shaped>>,
}

/// The shaped text of one inline formatting context.
#[derive(Clone, Debug, Default)]
pub struct InlineText {
    pub text: Vec<u16>,
    pub items: Vec<TextItem>,
    pub segs: Vec<TextSeg>,
}

impl Layout<'_> {
    /// The shaped text of the inline formatting context of block container box `root`; `None` when the box
    /// tree is not built or `root` lays out no lines.
    pub fn inline_text(&mut self, root: u32) -> Option<InlineText> {
        let st = self.state.as_mut()?;
        if root as usize >= st.tree.len()
            || !st.tree.get(root).kind.is_block_container()
            || !st.tree.has_inline_children(root)
        {
            return None;
        }
        let mut cx = Cx {
            dom: self.dom,
            styles: self.styles,
            tree: &st.tree,
            shaper: &mut self.shaper,
            fonts: &mut self.fonts,
            para: &mut self.para,
            cache: &mut st.cache,
            height_readers: &st.height_readers,
            intrinsic: &mut st.intrinsic,
            icb: self.viewport,
            excl: &mut Vec::new(),
            counters: &mut crate::Counters::default(),
            reuse: self.reuse,
            scale: self.scale,
        };
        let mut ifc = Ifc::default();
        collect::collect(&mut cx, root, &mut ifc);
        let widths = vec![Au::ZERO; ifc.items.len()];
        collect::shape(&mut cx, root, &mut ifc, &widths);
        Some(read(&mut cx, ifc))
    }

    /// The fonts and shaping cache layout used; painting draws the glyphs it shaped with these faces.
    pub fn shaper(&mut self) -> &mut Shaper {
        &mut self.shaper
    }
}

fn read(cx: &mut Cx, ifc: Ifc) -> InlineText {
    let mut items = Vec::new();
    let mut by_item = vec![None; ifc.items.len()];
    for (k, it) in ifc.items.iter().enumerate() {
        if it.kind != ItemKind::Text || it.b == tree::NONE {
            continue;
        }
        let f = cx.font(it.b);
        by_item[k] = Some(items.len());
        items.push(TextItem {
            node: it.node,
            face: f.face,
            size: f.size,
            ascent: f.ascent,
            first_seg: usize::MAX,
        });
    }
    let shaped = cx.para.segments();
    let mut segs = Vec::with_capacity(ifc.segs.len());
    for (k, s) in ifc.segs.iter().enumerate() {
        let item = by_item[s.item];
        if let Some(i) = item {
            items[i].first_seg = items[i].first_seg.min(k);
        }
        segs.push(TextSeg {
            item,
            kind: s.kind,
            range: s.range.clone(),
            adv: s.adv,
            glyphs: shaped.get(k).and_then(|p| p.shaped.clone()),
        });
    }
    InlineText {
        text: ifc.text,
        items,
        segs,
    }
}
