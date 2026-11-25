//! One windowless page the host holds for an agent session: the DOM of
//! a local file, styled and laid out in the viewport the agent asked
//! for, and the one agent session on it the host speaks for.

use std::path::{Path, PathBuf};
use std::time::Instant;

use madcawl_agent::{Agents, Frame, Geometry, Inputs, SessionId, role_and_name, tag};
use madcawl_css::style::{Device, Styles};
use madcawl_dom::Dom;
use madcawl_html::Encoding;
use madcawl_layout::Layout;

use crate::load::{self, Sheets};

pub struct Page {
    pub id: u32,
    pub url: String,
    pub width: f32,
    pub height: f32,
    pub last_used: Instant,
    dom: Dom,
    styles: Styles,
    geometry: Geometry,
    agents: Agents,
    session: SessionId,
}

impl Page {
    pub fn open(
        id: u32,
        path: &Path,
        url: String,
        (width, height): (f32, f32),
        charset: Option<Encoding>,
        allow: &[PathBuf],
    ) -> std::io::Result<Page> {
        let bytes = load::read_file(path, load::MAX_FILE)?;
        let (mut dom, encoding) = load::parse(&bytes, charset);
        let device = Device {
            width: width as f64,
            height: height as f64,
            ..Device::default()
        };
        let base = path.parent();
        let mut sheets = Sheets::default();
        let styles = load::style_within(&dom, base, encoding, &mut sheets, device, Some(allow));
        let mut layout = Layout::new(&dom, &styles, width, height);
        let geometry = Geometry::from_layout(&mut layout, &styles, width, height);
        drop(layout);
        let mut agents = Agents::new();
        let inputs = Inputs {
            styles: Some(&styles),
            geometry: Some(&geometry),
        };
        let session = agents
            .attach(&mut dom, inputs)
            .map_err(|e| std::io::Error::other(e.message()))?;
        Ok(Page {
            id,
            url,
            width,
            height,
            last_used: Instant::now(),
            dom,
            styles,
            geometry,
            agents,
            session,
        })
    }

    /// Answers a view or diff request as the page's own session, whatever
    /// session the frame names.
    pub fn handle(&mut self, f: &Frame) -> Option<Frame> {
        self.last_used = Instant::now();
        let mut f = f.clone();
        f.fields
            .retain(|(t, _)| !matches!(*t, tag::SESSION | tag::PAGE));
        let f = f.num(tag::SESSION, self.session as u64);
        let inputs = Inputs {
            styles: Some(&self.styles),
            geometry: Some(&self.geometry),
        };
        self.agents.handle(&mut self.dom, inputs, &f)
    }

    /// The role and name of the element a reference names, for the audit
    /// log.
    pub fn target(&mut self, r: &str) -> Option<(&'static str, String)> {
        let inputs = Inputs {
            styles: Some(&self.styles),
            geometry: Some(&self.geometry),
        };
        let n = self
            .agents
            .resolve(self.session, &mut self.dom, inputs, r)
            .ok()?;
        let (role, name) = role_and_name(self.agents.tree()?, n)?;
        Some((role.name(), name))
    }

    pub fn close(mut self) {
        self.agents.detach(self.session, &mut self.dom);
    }
}
