//! The glyphs of text run fragments. A text run fragment says only which text node it is of and how wide it
//! is; its segments are found in the shaped text of its inline formatting context, which layout hands over,
//! by taking the node's segments in order while their advances fill the run's width, line by line. A
//! segment of no width once the run is full is left to the next run: a line starts at it.
//!
//! Layout places a line's segments left to right in logical order; inside a run, a stretch of right to
//! left segments is mirrored in the room it takes, so its words read right to left. Reordering across
//! runs and lines (the rest of bidi reordering) is not done.

use std::collections::HashMap;

use madcawl_css::style::WhiteSpaceCollapse;
use madcawl_dom::NodeId;
use madcawl_layout::tree::{BoxTree, Kind, NONE};
use madcawl_layout::{Au, InlineText, Layout};
use madcawl_text::{FaceId, Kind as SegKind};

use crate::list::GlyphPos;

/// A run's glyphs in one face: where they are in the scratch buffer.
pub struct Piece {
    pub face: FaceId,
    pub glyphs: std::ops::Range<usize>,
}

/// The shaped text of the formatting contexts met so far, and how far each text node's runs have got;
/// bounded by the document, and dropped with the painter.
#[derive(Default)]
pub struct TextRuns {
    /// Text node to its text box
    text_box: HashMap<NodeId, u32>,
    contexts: HashMap<u32, Option<InlineText>>,
    /// Text node to (context root, item, next segment)
    cursor: HashMap<NodeId, (u32, usize, usize)>,
    pub scratch: Vec<GlyphPos>,
    pub pieces: Vec<Piece>,
    /// The segments of the current run
    segs: Vec<SegAt>,
}

// a segment of a run: where its pen started, its advance, its glyphs in scratch, and its direction
struct SegAt {
    x: f32,
    adv: f32,
    glyphs: std::ops::Range<usize>,
    rtl: bool,
    space: bool,
}

fn context_root(tree: &BoxTree, mut b: u32) -> u32 {
    loop {
        let p = tree.get(b).parent;
        if p == NONE || tree.get(p).kind != Kind::Inline {
            return p;
        }
        b = p;
    }
}

impl TextRuns {
    pub fn new(tree: &BoxTree) -> TextRuns {
        let mut text_box = HashMap::new();
        for b in 0..tree.len() as u32 {
            if tree.get(b).kind == Kind::Text {
                text_box.entry(tree.get(b).node).or_insert(b);
            }
        }
        TextRuns {
            text_box,
            ..TextRuns::default()
        }
    }

    // the context, item and next segment of `node`, shaping its context the first time
    fn cursor(&mut self, layout: &mut Layout, node: NodeId) -> Option<(u32, usize, usize)> {
        if let Some(&c) = self.cursor.get(&node) {
            return Some(c);
        }
        let &tb = self.text_box.get(&node)?;
        let root = context_root(layout.boxes(), tb);
        if root == NONE {
            return None;
        }
        let ctx = self
            .contexts
            .entry(root)
            .or_insert_with(|| layout.inline_text(root));
        let t = ctx.as_ref()?;
        for (i, it) in t.items.iter().enumerate() {
            self.cursor
                .entry(it.node)
                .or_insert((root, i, it.first_seg));
        }
        self.cursor.get(&node).copied()
    }

    /// Fills `scratch` and `pieces` with the glyphs of the next run of `node`, `width` wide, whose content
    /// area's top left corner is at `(x, y)`; the font size is returned. Nothing is filled when the run's
    /// text cannot be found.
    pub fn run(
        &mut self,
        layout: &mut Layout,
        node: NodeId,
        (x, y, width): (f32, f32, Au),
        collapse: WhiteSpaceCollapse,
    ) -> Option<f32> {
        self.scratch.clear();
        self.pieces.clear();
        let (root, item, mut k) = self.cursor(layout, node)?;
        let t = self.contexts.get(&root)?.as_ref()?;
        let it = &t.items[item];
        let mine = |k: usize| t.segs.get(k).is_some_and(|s| s.item == Some(item));
        // a run after the first starts where a line did: collapsible spaces there hung at the end of a
        // line that made no fragment
        if k != it.first_seg
            && matches!(
                collapse,
                WhiteSpaceCollapse::Collapse | WhiteSpaceCollapse::PreserveBreaks
            )
        {
            while mine(k) && t.segs[k].kind == SegKind::Space {
                k += 1;
            }
        }
        let base = y + it.ascent.px();
        let mut used = Au::ZERO;
        self.segs.clear();
        while mine(k) && used + t.segs[k].adv <= width {
            let s = &t.segs[k];
            if s.adv == Au::ZERO && used == width && used > Au::ZERO {
                break;
            }
            let at = self.scratch.len();
            let mut rtl = false;
            if s.kind == SegKind::Text
                && let Some(sh) = &s.glyphs
            {
                rtl = sh.rtl;
                push_glyphs(&mut self.scratch, &mut self.pieces, sh, x + used.px(), base);
            }
            self.segs.push(SegAt {
                x: x + used.px(),
                adv: s.adv.px(),
                glyphs: at..self.scratch.len(),
                rtl,
                space: s.kind == SegKind::Space,
            });
            used += s.adv;
            k += 1;
        }
        mirror_rtl(&self.segs, &mut self.scratch);
        self.cursor.insert(node, (root, item, k));
        Some(it.size)
    }
}

fn push_glyphs(
    scratch: &mut Vec<GlyphPos>,
    pieces: &mut Vec<Piece>,
    sh: &madcawl_text::Shaped,
    pen: f32,
    base: f32,
) {
    let mut gx = pen;
    for g in sh.glyphs.iter() {
        if pieces.last().is_none_or(|p| p.face != g.face) {
            let at = scratch.len();
            pieces.push(Piece {
                face: g.face,
                glyphs: at..at,
            });
        }
        scratch.push(GlyphPos {
            id: g.id,
            x: gx + g.dx,
            y: base - g.dy,
        });
        if let Some(p) = pieces.last_mut() {
            p.glyphs.end = scratch.len();
        }
        gx += g.advance;
    }
}

// mirrors each stretch of right to left segments, with the spaces between them, in the room it takes
fn mirror_rtl(segs: &[SegAt], scratch: &mut [GlyphPos]) {
    let mut i = 0;
    while i < segs.len() {
        if !segs[i].rtl {
            i += 1;
            continue;
        }
        let mut end = i;
        let mut j = i;
        while j < segs.len() && (segs[j].rtl || segs[j].space) {
            if segs[j].rtl {
                end = j;
            }
            j += 1;
        }
        let (x0, x1) = (segs[i].x, segs[end].x + segs[end].adv);
        for s in &segs[i..=end] {
            let shift = x0 + x1 - 2.0 * s.x - s.adv;
            for g in &mut scratch[s.glyphs.clone()] {
                g.x += shift;
            }
        }
        i = end + 1;
    }
}
