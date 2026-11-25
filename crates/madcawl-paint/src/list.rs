//! The display list: a flat array of entries in paint order, each an item with its bounds and the clip it is
//! drawn under, the glyphs of every glyph run in one shared array, and the clip nodes. It holds no reference
//! to the DOM, the styles or the fragment tree, so a raster needs nothing else.

use madcawl_text::FaceId;

/// Entries one page's list holds at most; past it entries are counted in [`DisplayList::dropped`].
pub const MAX_ENTRIES: usize = 1 << 18;
/// Glyphs over all glyph runs, at most.
pub const MAX_GLYPHS: usize = 1 << 21;
/// Clip nodes, at most; a box past it clips to its nearest clip that made it in.
pub const MAX_CLIPS: usize = 1 << 14;

/// A color, sRGB, not premultiplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);

    pub fn is_clear(self) -> bool {
        self.0[3] == 0
    }

    /// The color with its alpha scaled by `k`.
    pub fn fade(self, k: f32) -> Rgba {
        let [r, g, b, a] = self.0;
        Rgba([r, g, b, (a as f32 * k).round().clamp(0.0, 255.0) as u8])
    }
}

/// A rectangle in CSS px on the page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn intersect(&self, o: &RectF) -> RectF {
        let (x, y) = (self.x.max(o.x), self.y.max(o.y));
        RectF {
            x,
            y,
            w: (self.right().min(o.right()) - x).max(0.0),
            h: (self.bottom().min(o.bottom()) - y).max(0.0),
        }
    }

    pub fn intersects(&self, o: &RectF) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

/// How one side of a border is drawn; `none` and `hidden` sides have no width and are not drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

/// A glyph of a run: its id and where its origin is on the baseline, in CSS px on the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphPos {
    pub id: u16,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A solid rectangle: a background color
    Rect { rect: RectF, color: Rgba },
    /// A box's border: its border box, the width, color and line of each side, top, right, bottom, left
    Border {
        rect: RectF,
        widths: [f32; 4],
        colors: [Rgba; 4],
        lines: [Line; 4],
    },
    /// Glyphs of one face at one size in one color, `glyphs` indexing [`DisplayList::glyphs`]
    Glyphs {
        face: FaceId,
        size: f32,
        color: Rgba,
        glyphs: std::ops::Range<u32>,
    },
    /// Where an image or other replaced content goes; its pixels are not decoded yet
    Image { rect: RectF },
    /// A text decoration line, as a rectangle of its thickness
    Decoration { rect: RectF, color: Rgba },
}

/// An entry: an item, its bounds on the page, and the clip node it is drawn under (0: none).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub item: Item,
    pub bounds: RectF,
    pub clip: u32,
}

/// A clip node: the rectangle it clips to, already intersected with its parent's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub rect: RectF,
    pub parent: u32,
}

#[derive(Debug)]
pub struct DisplayList {
    /// What the canvas is filled with before any entry
    pub canvas: Rgba,
    pub entries: Vec<Entry>,
    pub glyphs: Vec<GlyphPos>,
    /// Clip node 0 clips nothing
    pub clips: Vec<Clip>,
    /// Entries left out for the caps
    pub dropped: u64,
    /// Entries left out for lying outside the area asked for
    pub culled: u64,
}

impl Default for DisplayList {
    fn default() -> Self {
        let all = RectF {
            x: f32::MIN / 4.0,
            y: f32::MIN / 4.0,
            w: f32::MAX / 2.0,
            h: f32::MAX / 2.0,
        };
        DisplayList {
            canvas: Rgba::WHITE,
            entries: Vec::new(),
            glyphs: Vec::new(),
            clips: vec![Clip {
                rect: all,
                parent: 0,
            }],
            dropped: 0,
            culled: 0,
        }
    }
}

impl DisplayList {
    /// Appends an entry unless the cap is reached.
    pub fn push(&mut self, item: Item, bounds: RectF, clip: u32) {
        if self.entries.len() >= MAX_ENTRIES {
            self.dropped += 1;
            return;
        }
        self.entries.push(Entry { item, bounds, clip });
    }

    /// A clip node inside `parent` clipping to `rect`; `parent` itself once the cap is reached.
    pub fn clip(&mut self, parent: u32, rect: RectF) -> u32 {
        if self.clips.len() >= MAX_CLIPS {
            return parent;
        }
        let rect = rect.intersect(&self.clips[parent as usize].rect);
        self.clips.push(Clip { rect, parent });
        (self.clips.len() - 1) as u32
    }

    /// Room for `n` more glyphs under the cap.
    pub fn glyph_room(&self, n: usize) -> bool {
        self.glyphs.len() + n <= MAX_GLYPHS
    }
}
