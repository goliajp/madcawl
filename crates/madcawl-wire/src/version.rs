//! Version ranges. Each side of a channel, and the writer of a surface
//! header, states the oldest version it still speaks and the one it
//! speaks best; both sides use the newest version in both ranges, and
//! when there is none, each can say which side is too old.

use crate::frame::Frame;
use crate::{Bad, tag};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    /// The oldest version still spoken
    pub min: u32,
    /// The version spoken best
    pub cur: u32,
}

/// What two ranges come to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Agreed(u32),
    /// The peer's newest version is older than our oldest
    PeerTooOld(Range),
    /// The peer's oldest version is newer than our newest
    PeerTooNew(Range),
}

impl Range {
    pub const fn new(min: u32, cur: u32) -> Range {
        assert!(min <= cur);
        Range { min, cur }
    }

    pub fn negotiate(self, theirs: Range) -> Outcome {
        if theirs.cur < self.min {
            return Outcome::PeerTooOld(theirs);
        }
        if theirs.min > self.cur {
            return Outcome::PeerTooNew(theirs);
        }
        Outcome::Agreed(self.cur.min(theirs.cur))
    }

    /// The range a frame states in `MIN` and `CUR`.
    pub fn from_frame(f: &Frame) -> Result<Range, Bad> {
        let min = num32(f, tag::MIN)?;
        let cur = num32(f, tag::CUR)?;
        match min <= cur {
            true => Ok(Range { min, cur }),
            false => Err(Bad::Value(tag::MIN)),
        }
    }

    pub fn put(self, f: Frame) -> Frame {
        f.num(tag::MIN, self.min.into()).num(tag::CUR, self.cur.into())
    }
}

/// The first field of `t` as a number of at most 32 bits.
pub(crate) fn num32(f: &Frame, t: u8) -> Result<u32, Bad> {
    let v = f.get_num(t).ok_or(Bad::Missing(t))?;
    u32::try_from(v).map_err(|_| Bad::Value(t))
}

/// The opening frame of either channel: the sender's range, and in an
/// answer the version both use when they agree on one; a host
/// connection's adds the role the connection plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    pub range: Range,
    pub role: Option<u32>,
    pub version: Option<u32>,
}

impl Hello {
    pub fn frame(&self, kind: u16) -> Frame {
        let mut f = self.range.put(Frame::new(kind));
        if let Some(r) = self.role {
            f = f.num(tag::ROLE, r.into());
        }
        if let Some(v) = self.version {
            f = f.num(tag::VERSION, v.into());
        }
        f
    }

    pub fn from_frame(f: &Frame) -> Result<Hello, Bad> {
        let range = Range::from_frame(f)?;
        let opt = |t| f.field(t).map(|_| num32(f, t)).transpose();
        let version = opt(tag::VERSION)?;
        if version.is_some_and(|v| v < range.min || v > range.cur) {
            return Err(Bad::Value(tag::VERSION));
        }
        Ok(Hello { range, role: opt(tag::ROLE)?, version })
    }

    /// The answer to `theirs` from a side whose range is `ours`, and
    /// what the two ranges came to.
    pub fn answer(ours: Range, theirs: &Hello) -> (Hello, Outcome) {
        let o = ours.negotiate(theirs.range);
        let version = match o {
            Outcome::Agreed(v) => Some(v),
            _ => None,
        };
        (Hello { range: ours, role: theirs.role, version }, o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_outcomes() {
        let ours = Range::new(2, 4);
        assert_eq!(ours.negotiate(Range::new(1, 3)), Outcome::Agreed(3));
        assert_eq!(ours.negotiate(Range::new(3, 9)), Outcome::Agreed(4));
        assert_eq!(ours.negotiate(Range::new(4, 4)), Outcome::Agreed(4));
        assert_eq!(ours.negotiate(Range::new(1, 1)), Outcome::PeerTooOld(Range::new(1, 1)));
        assert_eq!(ours.negotiate(Range::new(5, 7)), Outcome::PeerTooNew(Range::new(5, 7)));
        // each side reaches the same verdict about the other
        assert_eq!(Range::new(1, 1).negotiate(ours), Outcome::PeerTooNew(ours));
    }

    #[test]
    fn a_hello_round_trips_and_its_answer_names_the_version() {
        let h = Hello { range: Range::new(1, 2), role: Some(1), version: None };
        let mut b = Vec::new();
        h.frame(9).encode(&mut b);
        let (f, _) = Frame::decode(&b).unwrap();
        let back = Hello::from_frame(&f).unwrap();
        assert_eq!(back, h);
        let (a, o) = Hello::answer(Range::new(2, 3), &back);
        assert_eq!((a.version, o), (Some(2), Outcome::Agreed(2)));
        assert_eq!(Hello::from_frame(&a.frame(9)).unwrap(), a);
        let (a, o) = Hello::answer(Range::new(3, 3), &back);
        assert_eq!((a.version, o), (None, Outcome::PeerTooOld(Range::new(1, 2))));
    }

    #[test]
    fn a_bad_hello_is_refused() {
        assert_eq!(Hello::from_frame(&Frame::new(1).num(tag::MIN, 1)), Err(Bad::Missing(tag::CUR)));
        assert_eq!(Hello::from_frame(&Frame::new(1).num(tag::MIN, 3).num(tag::CUR, 2)), Err(Bad::Value(tag::MIN)));
        let wide = Frame::new(1).num(tag::MIN, 1).num(tag::CUR, 1 << 40);
        assert_eq!(Hello::from_frame(&wide), Err(Bad::Value(tag::CUR)));
        let outside = Range::new(1, 2).put(Frame::new(1)).num(tag::VERSION, 3);
        assert_eq!(Hello::from_frame(&outside), Err(Bad::Value(tag::VERSION)));
    }
}
