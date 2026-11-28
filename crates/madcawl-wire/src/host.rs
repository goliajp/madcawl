//! A host connection's frames for the display end. A connection whose
//! first frame is `HELLO` with role `DISPLAY` is the display end's: it
//! opens pane pages with `PANE_OPEN`, and each `PANE_OPENED` carries the
//! display channel to that page's content process as a descriptor
//! (`os::send`). Any other connection speaks the agent protocol. A
//! refusal is the agent protocol's `ERROR`, a code and a text.

use crate::frame::Frame;
use crate::version::{Range, num32};
use crate::{Bad, MAX_SIDE, tag};

/// The host connection versions this build speaks.
pub const RANGE: Range = Range::new(1, 1);

pub mod kind {
    /// The agent protocol's refusal: `CODE`, `TEXT`
    pub const ERROR: u16 = 21;
    /// Both ways: `MIN`, `CUR`, `ROLE`; the answer adds `VERSION` when
    /// the ranges meet
    pub const HELLO: u16 = 256;
    /// Display end to host: `SESSION`, `URL`, `WIDTH`, `HEIGHT`, `SCALE`
    pub const PANE_OPEN: u16 = 257;
    /// Host to display end: `SESSION`, `PAGE`, `PID`; the display
    /// channel comes with it as a descriptor
    pub const PANE_OPENED: u16 = 258;
}

pub mod role {
    pub const DISPLAY: u32 = 1;
}

/// The display end's scale bounds, in hundredths.
pub const SCALE_RANGE: std::ops::RangeInclusive<u32> = 50..=400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneOpen {
    pub session: u64,
    pub url: String,
    /// The viewport, CSS px
    pub width: u32,
    pub height: u32,
    /// Device pixels per CSS px, in hundredths
    pub scale: u32,
}

impl PaneOpen {
    pub fn frame(&self) -> Frame {
        Frame::new(kind::PANE_OPEN)
            .num(tag::SESSION, self.session)
            .str(tag::URL, &self.url)
            .num(tag::WIDTH, self.width.into())
            .num(tag::HEIGHT, self.height.into())
            .num(tag::SCALE, self.scale.into())
    }

    pub fn from_frame(f: &Frame) -> Result<PaneOpen, Bad> {
        if f.kind != kind::PANE_OPEN {
            return Err(Bad::Kind(f.kind));
        }
        let side = |t| num32(f, t).and_then(|v| if (1..=MAX_SIDE).contains(&v) { Ok(v) } else { Err(Bad::Value(t)) });
        let scale = num32(f, tag::SCALE)?;
        if !SCALE_RANGE.contains(&scale) {
            return Err(Bad::Value(tag::SCALE));
        }
        let url = f.field(tag::URL).ok_or(Bad::Missing(tag::URL))?;
        let url = std::str::from_utf8(url).map_err(|_| Bad::Value(tag::URL))?;
        Ok(PaneOpen {
            session: f.get_num(tag::SESSION).ok_or(Bad::Missing(tag::SESSION))?,
            url: url.to_owned(),
            width: side(tag::WIDTH)?,
            height: side(tag::HEIGHT)?,
            scale,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneOpened {
    pub session: u64,
    /// The host's number for the page
    pub page: u32,
    /// The content process holding it
    pub pid: i32,
}

impl PaneOpened {
    pub fn frame(&self) -> Frame {
        Frame::new(kind::PANE_OPENED)
            .num(tag::SESSION, self.session)
            .num(tag::PAGE, self.page.into())
            .num(tag::PID, self.pid as u32 as u64)
    }

    pub fn from_frame(f: &Frame) -> Result<PaneOpened, Bad> {
        if f.kind != kind::PANE_OPENED {
            return Err(Bad::Kind(f.kind));
        }
        let pid = i32::try_from(num32(f, tag::PID)?).map_err(|_| Bad::Value(tag::PID))?;
        Ok(PaneOpened {
            session: f.get_num(tag::SESSION).ok_or(Bad::Missing(tag::SESSION))?,
            page: num32(f, tag::PAGE)?,
            pid,
        })
    }
}

/// A refusal's code and text, when `f` is one.
pub fn refusal(f: &Frame) -> Option<(u64, &str)> {
    (f.kind == kind::ERROR).then(|| (f.get_num(tag::CODE).unwrap_or(0), f.get_str(tag::TEXT).unwrap_or("")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trip(f: &Frame) -> Frame {
        let mut b = Vec::new();
        f.encode(&mut b);
        Frame::decode(&b).unwrap().0
    }

    #[test]
    fn pane_frames_round_trip_and_unknown_tags_are_skipped() {
        let o = PaneOpen { session: 41, url: "file:///a.html".into(), width: 800, height: 600, scale: 200 };
        let f = trip(&o.frame().bytes(200, b"later"));
        assert_eq!(PaneOpen::from_frame(&f), Ok(o));
        let d = PaneOpened { session: 41, page: 7, pid: 4242 };
        assert_eq!(PaneOpened::from_frame(&trip(&d.frame().num(201, 1))), Ok(d));
        assert_eq!(PaneOpened::from_frame(&o_frame()), Err(Bad::Kind(kind::PANE_OPEN)));
    }

    fn o_frame() -> Frame {
        PaneOpen { session: 1, url: "x".into(), width: 1, height: 1, scale: 100 }.frame()
    }

    #[test]
    fn a_pane_open_out_of_range_is_refused() {
        let with = |t: u8, v: u64| {
            let mut f = o_frame();
            f.fields.retain(|(x, _)| *x != t);
            PaneOpen::from_frame(&f.num(t, v))
        };
        assert_eq!(with(tag::WIDTH, 0), Err(Bad::Value(tag::WIDTH)));
        assert_eq!(with(tag::HEIGHT, MAX_SIDE as u64 + 1), Err(Bad::Value(tag::HEIGHT)));
        assert_eq!(with(tag::SCALE, 49), Err(Bad::Value(tag::SCALE)));
        assert_eq!(with(tag::SCALE, 401), Err(Bad::Value(tag::SCALE)));
        let mut f = o_frame();
        f.fields.retain(|(t, _)| *t != tag::URL);
        assert_eq!(PaneOpen::from_frame(&f), Err(Bad::Missing(tag::URL)));
        assert_eq!(PaneOpen::from_frame(&f.bytes(tag::URL, &[0xff])), Err(Bad::Value(tag::URL)));
    }
}
