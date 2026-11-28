//! A content process's end of a display channel. The display end sends
//! `SURFACE` with its shared-memory buffers; this side checks each
//! buffer's header against the frame, draws the pane page's viewport
//! into a buffer the display end is not reading, sets the buffer's
//! generation and sends `COMMIT`. It draws when a surface comes, after
//! a request that may have changed the page, and when a buffer comes
//! back while a drawing is owed; nothing else wakes it, so an idle pane
//! costs nothing.
//!
//! The display end is checked as any peer is: a frame that breaks the
//! protocol closes the channel, and the page goes on without it.

use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

use madcawl_layout::Layout;
use madcawl_paint::{Canvas, Raster, RectF};
use madcawl_wire::display::{self, Commit, MAX_BUFFERS, PxRect, Surface, kind};
use madcawl_wire::os::{self, Mapping};
use madcawl_wire::shm::HEADER_LEN;
use madcawl_wire::{Frame, FrameError, Hello, MAX_FRAME, Outcome};

use super::Conn;
use crate::serve::activation;
use crate::serve::page::Page;
use crate::serve::wire::Got;

pub(crate) struct Channel {
    s: UnixStream,
    buf: Vec<u8>,
    fds: Vec<OwnedFd>,
    agreed: bool,
    surface: Option<(Surface, Vec<Mapping>)>,
    /// The commit each buffer holds until the display end releases it
    held: [Option<u64>; MAX_BUFFERS],
    seq: u64,
    owed: bool,
}

impl Channel {
    pub fn new(fd: OwnedFd) -> Channel {
        let s = UnixStream::from(fd);
        Channel { s, buf: Vec::new(), fds: Vec::new(), agreed: false, surface: None, held: [None; MAX_BUFFERS], seq: 0, owed: false }
    }

    fn fd(&self) -> RawFd {
        self.s.as_raw_fd()
    }

    fn send(&self, f: &Frame) -> Result<(), String> {
        let mut out = Vec::new();
        f.encode(&mut out);
        os::send(&self.s, &out, &[]).map_err(|e| format!("writing: {e}"))
    }

    /// Reads what came, answering every whole frame in it.
    fn read(&mut self) -> Result<(), String> {
        let mut chunk = [0u8; 4096];
        let n = os::recv(&self.s, &mut chunk, &mut self.fds).map_err(|e| format!("reading: {e}"))?;
        if n == 0 {
            return Err("the display end hung up".into());
        }
        if self.buf.len() + n > MAX_FRAME + 4 || self.fds.len() > MAX_BUFFERS {
            return Err("more than a frame's worth".into());
        }
        self.buf.extend_from_slice(&chunk[..n]);
        loop {
            let f = match Frame::decode(&self.buf) {
                Ok((f, used)) => {
                    self.buf.drain(..used);
                    f
                }
                Err(FrameError::Short) => return Ok(()),
                Err(e) => return Err(format!("a bad frame: {e:?}")),
            };
            self.frame(&f)?;
        }
    }

    fn frame(&mut self, f: &Frame) -> Result<(), String> {
        if !self.agreed {
            return self.hello(f);
        }
        match f.kind {
            kind::SURFACE => self.surface(f),
            kind::RELEASED => self.released(display::released_seq(f).map_err(|e| e.to_string())?),
            _ => Ok(()),
        }
    }

    fn hello(&mut self, f: &Frame) -> Result<(), String> {
        if f.kind != kind::HELLO {
            return Err(format!("kind {} before the hello", f.kind));
        }
        let theirs = Hello::from_frame(f).map_err(|e| e.to_string())?;
        let (answer, outcome) = Hello::answer(display::RANGE, &theirs);
        self.send(&answer.frame(kind::HELLO))?;
        match outcome {
            Outcome::Agreed(_) => {
                self.agreed = true;
                Ok(())
            }
            o => Err(format!("no version both speak: {o:?}")),
        }
    }

    /// Frees the buffer commit `seq` holds. A commit sent before the
    /// latest surface came holds nothing, and its release (the display
    /// end dropping the old buffer, or a commit crossing the new surface)
    /// is passed over; only a commit never sent breaks the protocol.
    fn released(&mut self, seq: u64) -> Result<(), String> {
        if let Some(slot) = self.held.iter_mut().find(|h| **h == Some(seq)) {
            *slot = None;
            return Ok(());
        }
        match seq <= self.seq {
            true => Ok(()),
            false => Err(format!("a release of commit {seq}, past the last sent, {}", self.seq)),
        }
    }

    /// Takes a new surface: its buffers mapped once their headers are
    /// found to be what the frame states. The old one goes, and nothing
    /// it held is waited on.
    fn surface(&mut self, f: &Frame) -> Result<(), String> {
        let fds = std::mem::take(&mut self.fds);
        let s = Surface::from_frame(f, fds.len()).map_err(|e| e.to_string())?;
        if let Some((old, _)) = &self.surface
            && s.epoch <= old.epoch
        {
            return Err(format!("surface epoch {} after {}", s.epoch, old.epoch));
        }
        let mut maps = Vec::with_capacity(fds.len());
        for (i, fd) in fds.iter().enumerate() {
            let m = Mapping::map(fd.as_fd(), s.buffers[i].bytes).map_err(|e| format!("buffer {i}: {e}"))?;
            s.check_header(i, m.bytes(), m.len() as u64).map_err(|e| format!("buffer {i}: {e}"))?;
            maps.push(m);
        }
        self.surface = Some((s, maps));
        self.held = [None; MAX_BUFFERS];
        self.owed = true;
        Ok(())
    }

    /// Draws `page` into a free buffer and commits it, when a drawing is
    /// owed and a buffer is free.
    fn draw(&mut self, page: &mut Page) -> Result<(), String> {
        let Some((s, maps)) = &mut self.surface else { return Ok(()) };
        let Some(i) = (0..maps.len()).find(|&i| self.held[i].is_none()) else { return Ok(()) };
        if !self.owed {
            return Ok(());
        }
        let (b, epoch) = (s.buffers[i], s.epoch);
        let scale = s.scale as f32 / 100.0;
        let m = &mut maps[i];
        let rows = b.stride as usize * b.height as usize;
        let data = &mut m.bytes_mut()[HEADER_LEN..HEADER_LEN + rows];
        data.fill(0);
        let mut canvas = Canvas { data, width: b.width as usize, height: b.height as usize, stride: b.stride as usize };
        let (doc_width, doc_height) = page.paint_viewport(scale, &mut canvas);
        self.seq += 1;
        m.set_generation(self.seq);
        self.held[i] = Some(self.seq);
        self.owed = false;
        let dirty = PxRect { x: 0, y: 0, w: b.width, h: b.height };
        self.send(&Commit { epoch, seq: self.seq, buffer: i as u32, dirty, doc_width, doc_height }.frame())
    }
}

impl Page {
    /// Lays out, paints and rasters the viewport where it is scrolled to
    /// into `canvas` at `scale`, as a screenshot of the viewport is drawn;
    /// the document's size, CSS px.
    pub(crate) fn paint_viewport(&mut self, scale: f32, canvas: &mut Canvas) -> (u32, u32) {
        let view = self.geometry().viewport;
        let bottom = self.geometry().bottom().max(self.height);
        let area = RectF { x: view.x, y: view.y, w: canvas.width as f32 / scale, h: canvas.height as f32 / scale };
        let mut layout = Layout::new(&self.doc.dom, &self.doc.styles, self.width, self.height);
        layout.fragments();
        let list = madcawl_paint::paint(&mut layout, &self.doc.dom, &self.doc.styles, Some(area));
        Raster { view: area, scale }.draw_into(&list, &mut layout.shaper().fonts, canvas);
        (self.width.ceil() as u32, bottom.ceil() as u32)
    }
}

impl Conn<'_> {
    pub fn with_display(mut self, d: Option<Channel>) -> Self {
        self.display = d;
        self
    }

    /// With a display channel, serves it until the host's channel `host`
    /// has something to read: `None`, for the caller to read it. A pane
    /// page never goes idle, so there is no time limit.
    pub(super) fn serve_display(&mut self, host: RawFd) -> Option<Got> {
        let Some(d) = &self.display else { return None };
        let mut d_fd = d.fd();
        loop {
            if let Err(e) = self.draw_owed() {
                self.close_display(&e);
                return None;
            }
            let (from_host, from_display) = match activation::readable_pair(host, Some(d_fd), None) {
                Ok(r) => r,
                Err(_) => return Some(Got::Closed),
            };
            if from_display {
                let r = self.display.as_mut().map_or(Ok(()), Channel::read);
                if let Err(e) = r {
                    self.close_display(&e);
                    return None;
                }
            }
            if from_host {
                return None;
            }
            d_fd = self.display.as_ref()?.fd();
        }
    }

    fn draw_owed(&mut self) -> Result<(), String> {
        let (Some(d), Some(p)) = (self.display.as_mut(), self.pages.first_mut()) else { return Ok(()) };
        d.draw(p)
    }

    /// A request was answered that may have changed the page.
    pub(super) fn display_owed(&mut self) {
        if let Some(d) = &mut self.display {
            d.owed = true;
        }
    }

    fn close_display(&mut self, why: &str) {
        eprintln!("madcawl-shell: session {}: the display channel closed: {why}", self.id);
        self.display = None;
    }
}
