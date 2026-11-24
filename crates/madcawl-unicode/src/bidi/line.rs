//! Rules applied per line: L1 and the reordering of L2.

use super::Bidi;
use crate::BidiClass::*;
use std::ops::Range;

impl Bidi {
    /// The levels of the code points of one line, after rule L1, into `out` (one per code point of `line`,
    /// which lies within one paragraph).
    pub fn line_levels(&self, line: Range<usize>, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(&self.levels[line.clone()]);
        let para = self
            .paragraphs
            .iter()
            .find(|p| p.range.contains(&line.start))
            .map_or(0, |p| p.level);
        let mut trailing = true;
        for k in (0..out.len()).rev() {
            let i = line.start + k;
            match self.orig[i] {
                S | B => {
                    out[k] = para;
                    trailing = true;
                }
                WS | FSI | LRI | RLI | PDI if trailing => out[k] = para,
                _ if trailing && self.removed[i] => out[k] = para,
                _ => trailing = false,
            }
        }
    }

    /// Rule L2: the logical indices `line.start + k` of a line in visual order, from its levels as
    /// [`Bidi::line_levels`] gives them. `scratch` holds a copy of the levels while reversing.
    pub fn reorder(levels: &[u8], start: usize, out: &mut Vec<usize>, scratch: &mut Vec<u8>) {
        out.clear();
        out.extend(start..start + levels.len());
        scratch.clear();
        scratch.extend_from_slice(levels);
        let Some(&max) = levels.iter().max() else {
            return;
        };
        let Some(min_odd) = levels.iter().map(|&l| l | 1).min() else {
            return;
        };
        let mut level = max;
        while level >= min_odd {
            let mut k = 0;
            while k < scratch.len() {
                if scratch[k] < level {
                    k += 1;
                    continue;
                }
                let run = k;
                while k < scratch.len() && scratch[k] >= level {
                    k += 1;
                }
                out[run..k].reverse();
                scratch[run..k].reverse();
            }
            if level == 0 {
                break;
            }
            level -= 1;
        }
    }
}
