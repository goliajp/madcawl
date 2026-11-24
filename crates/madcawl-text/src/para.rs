//! Inline text of one paragraph: itemized into segments (one style, one bidi level, one script, no line break
//! opportunity inside), each shaped through the cache; then greedy line breaking and the visual order of a line.

use crate::cache::Shaped;
use crate::shaper::{Shaper, Style};
use madcawl_unicode::{
    Bidi, BidiClass, Direction, LineBreak as Lb, LineBreaks, Script, Source, Utf16, props,
};
use std::ops::Range;
use std::sync::Arc;

/// A styled range of the paragraph's text, in UTF-16 code units. Spans are sorted and cover the text.
#[derive(Clone, Debug)]
pub struct Span {
    pub range: Range<usize>,
    pub style: Style,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    /// Only white space (bidi class WS); it hangs at the end of a line.
    Space,
    /// A forced break (a newline or another hard line break character); not shaped.
    Hard,
}

/// What may follow a segment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum After {
    /// The next segment continues the same word.
    Join,
    /// A line break opportunity.
    Break,
    /// A mandatory line break.
    Mandatory,
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub range: Range<usize>,
    /// The first code point, as an index into the paragraph's code points.
    pub first_cp: usize,
    pub level: u8,
    pub span: usize,
    pub kind: Kind,
    pub after: After,
    pub shaped: Option<Arc<Shaped>>,
}

impl Segment {
    pub fn advance(&self) -> f32 {
        self.shaped.as_ref().map_or(0.0, |s| s.advance)
    }
}

/// A line: a range of segments and its width without the white space hanging at its end.
#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    pub segments: Range<usize>,
    pub width: f32,
}

// buffers above this many elements are shrunk back to it after a paragraph
pub(crate) const KEEP: usize = 1 << 16;

/// The shaped paragraph; buffers are reused from one paragraph to the next, up to a bound.
#[derive(Default)]
pub struct Paragraph {
    bidi: Bidi,
    segments: Vec<Segment>,
    breaks: Vec<(usize, bool)>,
    levels: Vec<u8>,
    seg_levels: Vec<u8>,
    order: Vec<usize>,
    scratch: Vec<u8>,
}

fn kind(cp: u32) -> Kind {
    let p = props(cp);
    match p.bidi {
        BidiClass::B => Kind::Hard,
        BidiClass::WS => Kind::Space,
        _ if matches!(p.line, Lb::BK | Lb::CR | Lb::LF | Lb::NL) => Kind::Hard,
        _ => Kind::Text,
    }
}

impl Paragraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Itemizes and shapes `text` with `spans`, at base direction `dir`.
    /// Empty `spans` give no segments.
    pub fn shape(&mut self, shaper: &mut Shaper, text: &[u16], spans: &[Span], dir: Direction) {
        if spans.is_empty() {
            self.segments.clear();
            return;
        }
        let src = Utf16(text);
        self.bidi.resolve(&src, dir);
        self.breaks.clear();
        self.breaks
            .extend(LineBreaks::new(&src).map(|b| (b.pos, b.mandatory)));
        self.itemize(text, spans);
        for seg in &mut self.segments {
            if seg.kind != Kind::Hard {
                let style = spans[seg.span].style;
                seg.shaped =
                    Some(shaper.shape_word(&text[seg.range.clone()], &style, seg.level & 1 == 1));
            }
        }
        for v in [&mut self.levels, &mut self.seg_levels, &mut self.scratch] {
            v.shrink_to(KEEP);
        }
        self.segments.shrink_to(KEEP);
        self.breaks.shrink_to(KEEP);
        self.order.shrink_to(KEEP);
    }

    // cuts the text into segments at line break opportunities and at changes of bidi level, span, kind and
    // script
    fn itemize(&mut self, text: &[u16], spans: &[Span]) {
        let src = Utf16(text);
        self.segments.clear();
        let (mut brk, mut span, mut script) = (0, 0, Script::Common);
        for i in 0..self.bidi.len() {
            let at = self.bidi.offset(i);
            let (cp, _) = src.next(at);
            let level = self.bidi.levels()[i];
            let k = kind(cp);
            while span + 1 < spans.len() && spans[span].range.end <= at {
                span += 1;
            }
            let s = props(cp).script;
            let real = !matches!(s, Script::Common | Script::Inherited | Script::Unknown);
            while brk < self.breaks.len() && self.breaks[brk].0 < at {
                brk += 1;
            }
            let opportunity = self.breaks.get(brk).filter(|b| b.0 == at).map(|b| b.1);
            let changed = |last: &Segment| {
                opportunity.is_some()
                    || (last.level, last.span, last.kind) != (level, span, k)
                    || k == Kind::Hard
                    || (real && script != Script::Common && s != script)
            };
            match self.segments.last_mut() {
                Some(last) if !changed(last) => {}
                last => {
                    if let Some(last) = last {
                        last.after = match opportunity {
                            Some(true) => After::Mandatory,
                            Some(false) => After::Break,
                            None => After::Join,
                        };
                    }
                    let (range, after, shaped) = (at..at, After::Join, None);
                    let seg = Segment {
                        range,
                        first_cp: i,
                        level,
                        span,
                        kind: k,
                        after,
                        shaped,
                    };
                    self.segments.push(seg);
                }
            }
            if real {
                script = s;
            }
            if let Some(last) = self.segments.last_mut() {
                last.range.end = src.next(at).1;
            }
        }
        if let Some(last) = self.segments.last_mut() {
            last.after = After::Mandatory;
        }
    }

    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        [
            self.segments.capacity(),
            self.breaks.capacity(),
            self.levels.capacity(),
            self.seg_levels.capacity(),
            self.order.capacity(),
            self.scratch.capacity(),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn bidi(&self) -> &Bidi {
        &self.bidi
    }

    /// Greedy line breaking at `width` pixels: each line takes as many words as fit, white space at a line's
    /// end hangs, a word wider than the line gets a line of its own, a mandatory break ends the line.
    pub fn break_lines(&self, width: f32, out: &mut Vec<Line>) {
        out.clear();
        let segs = &self.segments;
        let (mut start, mut w, mut hang) = (0, 0.0f32, 0.0f32);
        let mut k = 0;
        while k < segs.len() {
            // one word: segments joined up to the next opportunity
            let word = k;
            let (mut ww, mut wh, mut text) = (0.0f32, 0.0f32, false);
            while k < segs.len() {
                let a = segs[k].advance();
                if segs[k].kind == Kind::Text {
                    (wh, text) = (0.0, true)
                } else {
                    wh += a
                }
                ww += a;
                k += 1;
                if segs[k - 1].after != After::Join {
                    break;
                }
            }
            // once a word follows, the space hanging at the line's end is inner space
            if word > start && text && w + ww - wh > width {
                out.push(Line {
                    segments: start..word,
                    width: w - hang,
                });
                (start, w, hang) = (word, 0.0, 0.0);
            }
            w += ww;
            hang = if text { wh } else { hang + wh };
            if segs[k - 1].after == After::Mandatory {
                out.push(Line {
                    segments: start..k,
                    width: w - hang,
                });
                (start, w, hang) = (k, 0.0, 0.0);
            }
        }
    }

    /// The segments of `line` in visual order, left to right (rules L1 and L2 over the line's code points).
    pub fn visual_order(&mut self, line: &Line, out: &mut Vec<usize>) {
        out.clear();
        let segs = &self.segments[line.segments.clone()];
        let Some(first) = segs.first() else { return };
        let end_cp = self
            .segments
            .get(line.segments.end)
            .map_or(self.bidi.len(), |s| s.first_cp);
        self.bidi
            .line_levels(first.first_cp..end_cp, &mut self.levels);
        self.seg_levels.clear();
        self.seg_levels.extend(
            segs.iter()
                .map(|s| self.levels[s.first_cp - first.first_cp]),
        );
        Bidi::reorder(
            &self.seg_levels,
            line.segments.start,
            &mut self.order,
            &mut self.scratch,
        );
        out.extend_from_slice(&self.order);
    }
}
