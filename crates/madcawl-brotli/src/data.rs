//! The compressed data of a meta-block (section 9.3): commands, their
//! literals, distances, back-references and dictionary words.

use crate::Error;
use crate::decoder::{Decoder, Flow, Step};
use crate::dict;
use crate::prefix::symbol;
use crate::tables::{BLOCK_COUNT, COPY, INSERT, literal_context, split_command};

/// Distance symbols 0..16: which of the last distances, and what to add
const SHORT_CODES: [(usize, isize); 16] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (3, 0),
    (0, -1),
    (0, 1),
    (0, -2),
    (0, 2),
    (0, -3),
    (0, 3),
    (1, -1),
    (1, 1),
    (1, -2),
    (1, 2),
    (1, -3),
    (1, 3),
];

impl Decoder {
    /// Reads a block switch of category `c` if its block ran out, without
    /// applying it: the new type and count
    #[inline]
    fn read_switch(&mut self, c: usize) -> Option<(u32, u32)> {
        let cat = self.t.cats[c];
        if cat.count != 0 {
            return None;
        }
        let t = &self.t;
        let s = symbol(&mut self.br, &t.switch[t.switch_at[2 * c] as usize..]);
        let ty = match s {
            0 => cat.prev,
            1 => (cat.cur + 1) % cat.n,
            s => s as u32 - 2,
        };
        let (base, extra) = BLOCK_COUNT[symbol(&mut self.br, &t.switch[t.switch_at[2 * c + 1] as usize..])];
        Some((ty, base + self.br.take(u32::from(extra))))
    }

    fn apply_switch(&mut self, c: usize, (ty, count): (u32, u32)) {
        let cat = &mut self.t.cats[c];
        cat.prev = cat.cur;
        cat.cur = ty;
        cat.count = count;
    }

    /// Applies a switch read by `read_switch` and counts one more use
    #[inline]
    fn use_block(&mut self, c: usize, switch: Option<(u32, u32)>) {
        if let Some(s) = switch {
            self.apply_switch(c, s);
        }
        self.t.cats[c].count -= 1;
    }

    pub(crate) fn command(&mut self) -> Result<Flow, Error> {
        if self.left == 0 {
            self.meta_block_done();
            return Ok(Flow::Next);
        }
        let m = self.br.mark();
        let switch = self.read_switch(1);
        let ty = switch.map_or(self.t.cats[1].cur, |s| s.0) as usize;
        let g = &self.t.groups[1];
        let cmd = symbol(&mut self.br, &g.table[g.at[ty] as usize..]);
        let (ic, cc) = split_command(cmd);
        let (ib, ie) = INSERT[ic];
        let insert = ib + self.br.take(u32::from(ie));
        let (cb, ce) = COPY[cc];
        let copy = cb + self.br.take(u32::from(ce));
        if self.br.short {
            self.br.reset(m);
            return Ok(Flow::Input);
        }
        self.use_block(1, switch);
        if insert as usize > self.left {
            return Err(Error::Format("insert past the end of the meta-block"));
        }
        self.insert = insert as usize;
        self.copy_len = copy as usize;
        self.implicit_distance = cmd < 128;
        self.step = Step::Literals;
        Ok(Flow::Next)
    }

    pub(crate) fn literals(&mut self) -> Flow {
        while self.insert > 0 {
            if self.t.cats[0].count == 0 {
                // a block switch is a unit of its own
                let m = self.br.mark();
                let switch = self.read_switch(0);
                if self.br.short {
                    self.br.reset(m);
                    return Flow::Input;
                }
                if let Some(s) = switch {
                    self.apply_switch(0, s);
                }
                continue;
            }
            let n = self.insert.min(self.out.free()).min(self.t.cats[0].count as usize);
            if n == 0 {
                return Flow::Output;
            }
            let done = self.literal_run(n);
            self.insert -= done;
            self.left -= done;
            self.t.cats[0].count -= done as u32;
            if done < n {
                return Flow::Input;
            }
        }
        if self.left == 0 {
            // the copy of the last command is ignored
            self.meta_block_done();
        } else {
            self.step = Step::Distance;
        }
        Flow::Next
    }

    /// Decodes up to `n` literals of the current block type, fewer when
    /// the input runs out, and returns how many
    #[inline]
    fn literal_run(&mut self, n: usize) -> usize {
        let ty = self.t.cats[0].cur as usize;
        let mode = self.t.modes[ty];
        let map = &self.t.map[0][64 * ty..64 * ty + 64];
        let g = &self.t.groups[0];
        let (mut p1, mut p2) = (self.out.last(1), self.out.last(2));
        for i in 0..n {
            let m = self.br.mark();
            let tree = map[literal_context(mode, p1, p2)] as usize;
            let lit = symbol(&mut self.br, &g.table[g.at[tree] as usize..]) as u8;
            if self.br.short {
                self.br.reset(m);
                return i;
            }
            self.out.push(lit);
            (p2, p1) = (p1, lit);
        }
        n
    }

    pub(crate) fn distance(&mut self) -> Result<Flow, Error> {
        let (code, dist) = if self.implicit_distance {
            (0, self.last_distance(0))
        } else {
            let m = self.br.mark();
            let switch = self.read_switch(2);
            let ty = switch.map_or(self.t.cats[2].cur, |s| s.0) as usize;
            let ctx = self.copy_len.min(5) - 2;
            let tree = self.t.map[1][4 * ty + ctx] as usize;
            let g = &self.t.groups[2];
            let code = symbol(&mut self.br, &g.table[g.at[tree] as usize..]);
            let dist = self.resolve(code);
            if self.br.short {
                self.br.reset(m);
                return Ok(Flow::Input);
            }
            self.use_block(2, switch);
            (code, dist?)
        };
        let max = self.max_backward.min(self.out.written);
        if dist > max {
            // an empty word needs transform 34 or above, so id >= 34 << 5:
            // never an implicit, short (id <= 18) or direct (dist <= 120)
            // distance, so the command read extra bits and the number of
            // commands that output nothing is bounded by the input
            return self.dictionary(dist - max - 1);
        }
        if self.copy_len > self.left {
            return Err(Error::Format("copy past the end of the meta-block"));
        }
        if code != 0 {
            self.dist[self.dist_at & 3] = dist;
            self.dist_at += 1;
        }
        self.copy_dist = dist;
        self.step = Step::Copy;
        Ok(Flow::Next)
    }

    fn last_distance(&self, back: usize) -> usize {
        self.dist[self.dist_at.wrapping_sub(1 + back) & 3]
    }

    /// The distance of symbol `code`, reading its extra bits
    fn resolve(&mut self, code: usize) -> Result<usize, Error> {
        let ndirect = self.t.ndirect as usize;
        if code < 16 {
            let (back, add) = SHORT_CODES[code];
            let d = self.last_distance(back) as isize + add;
            return if d > 0 { Ok(d as usize) } else { Err(Error::Format("distance below 1")) };
        }
        if code < 16 + ndirect {
            return Ok(code - 15);
        }
        let npostfix = self.t.npostfix;
        let x = code - ndirect - 16;
        let nbits = 1 + (x >> (npostfix + 1)) as u32;
        let hcode = x >> npostfix;
        let lcode = x & ((1 << npostfix) - 1);
        let offset = ((2 + (hcode & 1)) << nbits) - 4;
        let extra = self.br.take(nbits) as usize;
        Ok(((offset + extra) << npostfix) + lcode + ndirect + 1)
    }

    fn dictionary(&mut self, id: usize) -> Result<Flow, Error> {
        if !(4..=24).contains(&self.copy_len) {
            return Err(Error::Format("dictionary word length out of range"));
        }
        let n = dict::word(self.copy_len, id, &mut self.word)?;
        if n > self.left {
            return Err(Error::Format("dictionary word past the end of the meta-block"));
        }
        self.word_len = n;
        self.step = Step::Word;
        Ok(Flow::Next)
    }

    pub(crate) fn copy(&mut self) -> Flow {
        while self.copy_len > 0 {
            let n = self.copy_len.min(self.out.free());
            if n == 0 {
                return Flow::Output;
            }
            self.out.copy(self.copy_dist, n);
            self.copy_len -= n;
            self.left -= n;
        }
        self.step = Step::Command;
        Flow::Next
    }

    pub(crate) fn put_word(&mut self) -> Flow {
        if self.out.free() < self.word_len {
            return Flow::Output;
        }
        self.out.write(&self.word[..self.word_len]);
        self.left -= self.word_len;
        self.step = Step::Command;
        Flow::Next
    }
}
