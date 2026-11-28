//! One fetch for a content process: the request out of its frame, the
//! response back as frames.

use crate::download::{Downloads, Tee};
use crate::{Writer, error, is_hidden_response_header, parse_url, send};
use madcawl_net::fetch::{Credentials, Mode, Redirect, Request, Sink, is_forbidden_method};
use madcawl_net::partition::PROFILE;
use madcawl_net::sock::Cancel;
use madcawl_net::wire::{BODY_CHUNK, Frame, kind, tag};
use madcawl_net::{Client, NetError, Response};
use std::io;
use std::sync::Arc;

fn request_of(mut frame: Frame) -> Result<Request, String> {
    let url = parse_url(&frame, tag::URL).ok_or("missing or bad url")?;
    let mut req = Request::get(url);
    if let Some(m) = frame.get_str(tag::METHOD) {
        if is_forbidden_method(m) {
            return Err(format!("forbidden method {m}"));
        }
        req.method = m.to_owned();
    }
    req.headers = frame.headers();
    req.body = frame.take(tag::BODY);
    req.navigation = frame.get_num(tag::NAVIGATION).unwrap_or(0) != 0;
    req.initiator = parse_url(&frame, tag::INITIATOR);
    req.credentials = match frame.get_num(tag::CREDENTIALS).unwrap_or(1) {
        0 => Credentials::Omit,
        1 => Credentials::Include,
        2 => Credentials::SameOrigin,
        v => return Err(format!("credentials value {v}")),
    };
    req.mode = match frame.get_num(tag::MODE).unwrap_or(0) {
        0 => Mode::NoCors,
        1 => Mode::Cors,
        2 => Mode::SameOrigin,
        v => return Err(format!("mode value {v}")),
    };
    req.partition = frame.get_num(tag::PARTITION).unwrap_or(PROFILE);
    req.body_origins = frame.all(tag::BODY_ORIGIN).map(|o| String::from_utf8_lossy(o).into_owned()).collect();
    req.redirect = match frame.get_num(tag::REDIRECT).unwrap_or(0) {
        1 => Redirect::Manual,
        2 => Redirect::Error,
        _ => Redirect::Follow,
    };
    Ok(req)
}

/// The response as frames: HEAD, then BODY frames of up to `BODY_CHUNK`
/// bytes as the body is decoded, then END.
struct Frames<'a> {
    w: &'a Writer,
    id: u64,
    pending: Vec<u8>,
    cancel: &'a Cancel,
}

fn io_error(e: io::Error) -> NetError {
    NetError::Io(format!("to the content process: {e}"))
}

impl Frames<'_> {
    fn flush(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let f = Frame::new(kind::BODY).num(tag::ID, self.id).bytes(tag::BODY, &self.pending);
        self.pending.clear();
        send(self.w, &f)
    }
}

impl Sink for Frames<'_> {
    fn head(&mut self, r: &Response) -> Result<(), NetError> {
        let mut head = Frame::new(kind::HEAD)
            .num(tag::ID, self.id)
            .num(tag::STATUS, u64::from(r.status))
            .str(tag::REASON, &r.reason);
        for u in &r.url_list {
            head = head.str(tag::URL, u.as_str());
        }
        for (n, v) in r.headers.iter().filter(|(n, _)| !is_hidden_response_header(n)) {
            head = head.header(n, v);
        }
        send(self.w, &head).map_err(io_error)
    }

    fn body(&mut self, mut chunk: &[u8]) -> Result<(), NetError> {
        if self.cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        while !chunk.is_empty() {
            let n = (BODY_CHUNK - self.pending.len()).min(chunk.len());
            self.pending.extend_from_slice(&chunk[..n]);
            chunk = &chunk[n..];
            if self.pending.len() == BODY_CHUNK {
                self.flush().map_err(io_error)?;
            }
        }
        Ok(())
    }
}

pub(crate) fn run_fetch(
    client: &Client,
    w: &Writer,
    id: u64,
    frame: Frame,
    cancel: Arc<Cancel>,
    downloads: Option<&Downloads>,
) -> io::Result<()> {
    let forced = frame.get_num(tag::DOWNLOAD).unwrap_or(0) != 0;
    let mut req = match request_of(frame) {
        Ok(r) => r,
        Err(e) => return send(w, &error(id, &e)),
    };
    req.cancel = Some(cancel.clone());
    let mut out = Frames { w, id, pending: Vec::new(), cancel: &cancel };
    let mut tee = Tee::new(&mut out, w, id, &cancel, downloads);
    (tee.navigation, tee.forced) = (req.navigation, forced);
    let r = client.fetch_into(&req, &mut tee);
    let downloaded = match tee.finish(r) {
        Ok(d) => d,
        Err(e) => {
            if let NetError::Untrusted(reason) = &e {
                eprintln!("madcawl-netd: {}: certificate not trusted: {reason}", req.url);
            }
            return send(w, &error(id, &e.to_string()));
        }
    };
    if !downloaded {
        out.flush()?;
    }
    send(w, &Frame::new(kind::END).num(tag::ID, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_and_mode_come_from_their_values() {
        let fetch = |creds: u64, mode: u64| request_of(Frame::new(kind::FETCH).str(tag::URL, "https://a.test/").num(tag::CREDENTIALS, creds).num(tag::MODE, mode));
        let got = |c, m| fetch(c, m).map(|r| (r.credentials, r.mode));
        assert_eq!(got(0, 0), Ok((Credentials::Omit, Mode::NoCors)));
        assert_eq!(got(1, 1), Ok((Credentials::Include, Mode::Cors)));
        assert_eq!(got(2, 2), Ok((Credentials::SameOrigin, Mode::SameOrigin)));
        assert!(fetch(3, 0).is_err() && fetch(0, 3).is_err());
        let plain = request_of(Frame::new(kind::FETCH).str(tag::URL, "https://a.test/")).unwrap();
        assert_eq!((plain.credentials, plain.mode), (Credentials::Include, Mode::NoCors));
    }
}
