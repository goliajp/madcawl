//! The compressed meta-block header (section 9.2) after the length:
//! block types, distance parameters, context modes, context maps and the
//! prefix codes, each read as its own unit.

use crate::Error;
use crate::decoder::{Decoder, Flow, Step};
use crate::prefix::{self, Entry, MAX_ALPHABET, symbol};
use crate::tables::BLOCK_COUNT;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Head {
    /// the number of block types of a category (literal, command, distance)
    Types(u8),
    TypeCode(u8),
    CountCode(u8),
    FirstCount(u8),
    /// distance parameters, literal context modes, literal tree count
    Params,
    DistanceTrees,
    /// the code of the literal (0) or distance (1) context map
    MapCode(u8),
    MapValues(u8),
    /// the prefix codes of group (literal, command, distance), from index
    Trees(u8, u16),
}

/// Block switching state of one category
#[derive(Clone, Copy)]
pub(crate) struct Cat {
    pub n: u32,
    pub cur: u32,
    pub prev: u32,
    /// left in the current block; a category with one type never runs out
    pub count: u32,
}

/// The prefix codes of one kind, one table after another
#[derive(Default)]
pub(crate) struct Group {
    pub table: Vec<Entry>,
    pub at: Vec<u32>,
}

pub(crate) struct Tables {
    pub cats: [Cat; 3],
    /// block type and block count codes, two per category
    pub switch: Vec<Entry>,
    pub switch_at: [u32; 6],
    pub npostfix: u32,
    pub ndirect: u32,
    pub modes: [u8; 256],
    pub ntrees: [u32; 2],
    rlemax: u32,
    pub map: [Vec<u8>; 2],
    map_code: Vec<Entry>,
    map_at: usize,
    pub groups: [Group; 3],
}

/// Space for reading one prefix code
pub(crate) struct Scratch {
    lens: Box<[u8; MAX_ALPHABET]>,
    cl: Vec<Entry>,
}

impl Scratch {
    pub fn new() -> Self {
        Scratch { lens: Box::new([0; MAX_ALPHABET]), cl: Vec::new() }
    }
}

impl Tables {
    pub fn new() -> Self {
        let cat = Cat { n: 1, cur: 0, prev: 1, count: u32::MAX };
        Tables {
            cats: [cat; 3],
            switch: Vec::new(),
            switch_at: [0; 6],
            npostfix: 0,
            ndirect: 0,
            modes: [0; 256],
            ntrees: [1; 2],
            rlemax: 0,
            map: [Vec::new(), Vec::new()],
            map_code: Vec::new(),
            map_at: 0,
            groups: Default::default(),
        }
    }
}

/// The 1..=256 count code of block types and trees
fn read_count(d: &mut Decoder) -> u32 {
    let br = &mut d.br;
    if br.take(1) == 0 {
        return 1;
    }
    match br.take(3) {
        0 => 2,
        n => (1 << n) + 1 + br.take(n),
    }
}

fn read_rlemax(d: &mut Decoder) -> u32 {
    if d.br.take(1) == 0 { 0 } else { d.br.take(4) + 1 }
}

fn inverse_mtf(v: &mut [u8]) {
    let mut mtf: [u8; 256] = std::array::from_fn(|i| i as u8);
    for x in v {
        let i = *x as usize;
        let value = mtf[i];
        *x = value;
        mtf.copy_within(0..i, 1);
        mtf[0] = value;
    }
}

impl Decoder {
    pub(crate) fn head(&mut self, h: Head) -> Result<Flow, Error> {
        let next = match h {
            Head::Types(c) => self.block_types(c as usize)?,
            Head::TypeCode(c) => self.switch_code(c as usize, 0)?,
            Head::CountCode(c) => self.switch_code(c as usize, 1)?,
            Head::FirstCount(c) => self.first_count(c as usize)?,
            Head::Params => self.params()?,
            Head::DistanceTrees => self.distance_trees()?,
            Head::MapCode(w) => self.map_code(w as usize)?,
            Head::MapValues(w) => self.map_values(w as usize)?,
            Head::Trees(g, i) => self.trees(g as usize, i as usize)?,
        };
        Ok(match next {
            None => Flow::Input,
            Some(step) => {
                self.step = step;
                Flow::Next
            }
        })
    }

    fn after_category(c: usize) -> Step {
        Step::Head(if c < 2 { Head::Types(c as u8 + 1) } else { Head::Params })
    }

    fn block_types(&mut self, c: usize) -> Result<Option<Step>, Error> {
        if c == 0 {
            self.t.switch.clear();
            for g in &mut self.t.groups {
                g.table.clear();
                g.at.clear();
            }
        }
        let Some(n) = self.unit(|d| Ok(read_count(d)))? else { return Ok(None) };
        self.t.cats[c] = Cat { n, cur: 0, prev: 1, count: u32::MAX };
        Ok(Some(if n >= 2 { Step::Head(Head::TypeCode(c as u8)) } else { Self::after_category(c) }))
    }

    /// Reads a prefix code over `alphabet` symbols onto the end of `pick(t)`
    fn read_code(&mut self, alphabet: usize, pick: fn(&mut Tables) -> &mut Vec<Entry>) -> Result<Option<u32>, Error> {
        let at = pick(&mut self.t).len();
        let r = self.unit(|d| {
            let s = &mut d.scratch;
            prefix::read(&mut d.br, alphabet, &mut s.lens, &mut s.cl, pick(&mut d.t))
        })?;
        if r.is_none() {
            pick(&mut self.t).truncate(at);
        }
        Ok(r.map(|()| at as u32))
    }

    fn switch_code(&mut self, c: usize, which: usize) -> Result<Option<Step>, Error> {
        let alphabet = if which == 0 { self.t.cats[c].n as usize + 2 } else { 26 };
        let Some(at) = self.read_code(alphabet, |t| &mut t.switch)? else { return Ok(None) };
        self.t.switch_at[2 * c + which] = at;
        Ok(Some(Step::Head(if which == 0 { Head::CountCode(c as u8) } else { Head::FirstCount(c as u8) })))
    }

    fn first_count(&mut self, c: usize) -> Result<Option<Step>, Error> {
        let Some(count) = self.unit(|d| {
            let s = symbol(&mut d.br, &d.t.switch[d.t.switch_at[2 * c + 1] as usize..]);
            let (base, extra) = BLOCK_COUNT[s];
            Ok(base + d.br.take(u32::from(extra)))
        })?
        else {
            return Ok(None);
        };
        self.t.cats[c].count = count;
        Ok(Some(Self::after_category(c)))
    }

    fn params(&mut self) -> Result<Option<Step>, Error> {
        let Some((npostfix, ndirect, ntrees, rlemax)) = self.unit(|d| {
            let npostfix = d.br.take(2);
            let ndirect = d.br.take(4) << npostfix;
            for i in 0..d.t.cats[0].n as usize {
                d.t.modes[i] = d.br.take(2) as u8;
            }
            let ntrees = read_count(d);
            let rlemax = if ntrees >= 2 { read_rlemax(d) } else { 0 };
            Ok((npostfix, ndirect, ntrees, rlemax))
        })?
        else {
            return Ok(None);
        };
        let t = &mut self.t;
        (t.npostfix, t.ndirect, t.ntrees[0], t.rlemax) = (npostfix, ndirect, ntrees, rlemax);
        t.map[0].clear();
        t.map[0].resize(64 * t.cats[0].n as usize, 0);
        Ok(Some(Step::Head(if ntrees >= 2 { Head::MapCode(0) } else { Head::DistanceTrees })))
    }

    fn distance_trees(&mut self) -> Result<Option<Step>, Error> {
        let Some((ntrees, rlemax)) = self.unit(|d| {
            let ntrees = read_count(d);
            Ok((ntrees, if ntrees >= 2 { read_rlemax(d) } else { 0 }))
        })?
        else {
            return Ok(None);
        };
        let t = &mut self.t;
        (t.ntrees[1], t.rlemax) = (ntrees, rlemax);
        t.map[1].clear();
        t.map[1].resize(4 * t.cats[2].n as usize, 0);
        Ok(Some(Step::Head(if ntrees >= 2 { Head::MapCode(1) } else { Head::Trees(0, 0) })))
    }

    fn map_code(&mut self, w: usize) -> Result<Option<Step>, Error> {
        self.t.map_code.clear();
        let alphabet = (self.t.ntrees[w] + self.t.rlemax) as usize;
        if self.read_code(alphabet, |t| &mut t.map_code)?.is_none() {
            return Ok(None);
        }
        self.t.map_at = 0;
        Ok(Some(Step::Head(Head::MapValues(w as u8))))
    }

    fn map_values(&mut self, w: usize) -> Result<Option<Step>, Error> {
        let size = self.t.map[w].len();
        let rlemax = self.t.rlemax as usize;
        while self.t.map_at < size {
            let Some((value, run)) = self.unit(|d| {
                let s = symbol(&mut d.br, &d.t.map_code);
                Ok(match s {
                    0 => (0, 1),
                    s if s <= rlemax => (0, (1 << s) + d.br.take(s as u32) as usize),
                    s => ((s - rlemax) as u8, 1),
                })
            })?
            else {
                return Ok(None);
            };
            let at = self.t.map_at;
            if run > size - at {
                return Err(Error::Format("context map run past the end"));
            }
            self.t.map[w][at..at + run].fill(value);
            self.t.map_at += run;
        }
        let Some(imtf) = self.unit(|d| Ok(d.br.take(1)))? else { return Ok(None) };
        if imtf == 1 {
            inverse_mtf(&mut self.t.map[w]);
        }
        Ok(Some(Step::Head(if w == 0 { Head::DistanceTrees } else { Head::Trees(0, 0) })))
    }

    fn trees(&mut self, g: usize, i: usize) -> Result<Option<Step>, Error> {
        let t = &self.t;
        let count = [t.ntrees[0], t.cats[1].n, t.ntrees[1]][g] as usize;
        if i == count {
            return Ok(Some(if g < 2 { Step::Head(Head::Trees(g as u8 + 1, 0)) } else { Step::Command }));
        }
        let alphabet = [256, 704, 16 + t.ndirect as usize + (48 << t.npostfix)][g];
        let pick: fn(&mut Tables) -> &mut Vec<Entry> = match g {
            0 => |t| &mut t.groups[0].table,
            1 => |t| &mut t.groups[1].table,
            _ => |t| &mut t.groups[2].table,
        };
        let Some(at) = self.read_code(alphabet, pick)? else { return Ok(None) };
        self.t.groups[g].at.push(at);
        Ok(Some(Step::Head(Head::Trees(g as u8, i as u16 + 1))))
    }
}
