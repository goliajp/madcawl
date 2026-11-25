//! The network process's service: content processes connect over a Unix
//! socket and send frames (`madcawl_net::wire`); each fetch runs on its own
//! thread, at most `in_flight` per connection and `clients` connections at
//! a time. Sockets, TLS state and the cookie jar stay here: a response's
//! `Set-Cookie` fields are stripped before it goes out, and script access
//! to cookies never reads or writes `HttpOnly` ones.

use madcawl_net::cookie::{Api, SiteContext, serialize};
use madcawl_net::fetch::{Redirect, Request, now_secs};
use madcawl_net::wire::{BODY_CHUNK, Frame, kind, tag};
use madcawl_net::{Client, Url};
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug)]
pub struct ServerLimits {
    pub clients: usize,
    pub in_flight: usize,
}

impl Default for ServerLimits {
    fn default() -> Self {
        ServerLimits { clients: 64, in_flight: 32 }
    }
}

/// Response headers a content process never sees.
pub fn is_hidden_response_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("set-cookie") || name.eq_ignore_ascii_case("set-cookie2")
}

/// Serves `listener` until it fails; one thread per connected client.
pub fn serve(listener: UnixListener, client: Arc<Client>, limits: ServerLimits) -> io::Result<()> {
    let live = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let stream = stream?;
        if live.fetch_add(1, Ordering::SeqCst) >= limits.clients {
            live.fetch_sub(1, Ordering::SeqCst);
            drop(stream);
            continue;
        }
        let (client, live) = (client.clone(), live.clone());
        std::thread::spawn(move || {
            let _ = serve_client(stream, &client, limits);
            live.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

type Writer = Arc<Mutex<UnixStream>>;

fn send(w: &Writer, f: &Frame) -> io::Result<()> {
    f.write_to(&mut *w.lock().unwrap())
}

fn error(id: u64, message: &str) -> Frame {
    Frame::new(kind::ERROR).num(tag::ID, id).str(tag::MESSAGE, message)
}

fn serve_client(stream: UnixStream, client: &Arc<Client>, limits: ServerLimits) -> io::Result<()> {
    let mut reader = stream.try_clone()?;
    let writer: Writer = Arc::new(Mutex::new(stream));
    let busy = Arc::new(AtomicUsize::new(0));
    while let Some(frame) = Frame::read_from(&mut reader)? {
        let id = frame.get_num(tag::ID).unwrap_or(0);
        match frame.kind {
            kind::FETCH => {
                if busy.fetch_add(1, Ordering::SeqCst) >= limits.in_flight {
                    busy.fetch_sub(1, Ordering::SeqCst);
                    send(&writer, &error(id, "too many requests in flight"))?;
                    continue;
                }
                let (client, writer, busy) = (client.clone(), writer.clone(), busy.clone());
                std::thread::spawn(move || {
                    let _ = run_fetch(&client, &writer, id, &frame);
                    busy.fetch_sub(1, Ordering::SeqCst);
                });
            }
            kind::COOKIE_GET | kind::COOKIE_SET => send(&writer, &cookies(client, id, &frame))?,
            _ => {}
        }
    }
    Ok(())
}

fn parse_url(frame: &Frame, t: u8) -> Option<Url> {
    Url::parse(frame.get_str(t)?).ok()
}

fn request_of(frame: &Frame) -> Result<Request, String> {
    let url = parse_url(frame, tag::URL).ok_or("missing or bad url")?;
    let mut req = Request::get(url);
    if let Some(m) = frame.get_str(tag::METHOD) {
        req.method = m.to_owned();
    }
    req.headers = frame.headers();
    req.body = frame.field(tag::BODY).map(<[u8]>::to_vec);
    req.navigation = frame.get_num(tag::NAVIGATION).unwrap_or(0) != 0;
    req.initiator = parse_url(frame, tag::INITIATOR);
    req.credentials = frame.get_num(tag::CREDENTIALS).unwrap_or(1) != 0;
    req.redirect = match frame.get_num(tag::REDIRECT).unwrap_or(0) {
        1 => Redirect::Manual,
        2 => Redirect::Error,
        _ => Redirect::Follow,
    };
    Ok(req)
}

fn run_fetch(client: &Client, w: &Writer, id: u64, frame: &Frame) -> io::Result<()> {
    let req = match request_of(frame) {
        Ok(r) => r,
        Err(e) => return send(w, &error(id, &e)),
    };
    let resp = match client.fetch(&req) {
        Ok(r) => r,
        Err(e) => return send(w, &error(id, &e.to_string())),
    };
    let mut head = Frame::new(kind::HEAD)
        .num(tag::ID, id)
        .num(tag::STATUS, u64::from(resp.status))
        .str(tag::REASON, &resp.reason);
    for u in &resp.url_list {
        head = head.str(tag::URL, u.as_str());
    }
    for (n, v) in resp.headers.iter().filter(|(n, _)| !is_hidden_response_header(n)) {
        head = head.header(n, v);
    }
    send(w, &head)?;
    for chunk in resp.body.chunks(BODY_CHUNK) {
        send(w, &Frame::new(kind::BODY).num(tag::ID, id).bytes(tag::BODY, chunk))?;
    }
    send(w, &Frame::new(kind::END).num(tag::ID, id))
}

/// `document.cookie` for the document at URL: reading gives the cookies
/// script may see, writing stores one `Set-Cookie`-style line as script.
fn cookies(client: &Client, id: u64, frame: &Frame) -> Frame {
    let Some(url) = parse_url(frame, tag::URL) else { return error(id, "missing or bad url") };
    let mut jar = client.jar().lock().unwrap();
    let reply = Frame::new(kind::COOKIES).num(tag::ID, id);
    if frame.kind == kind::COOKIE_SET {
        let line = frame.get_str(tag::TEXT).unwrap_or("");
        let ok = jar.set(&url, line, Api::Script, now_secs());
        return reply.num(tag::OK, u64::from(ok));
    }
    let list = jar.matching(&url, SiteContext::SameSite, Api::Script, now_secs());
    reply.str(tag::TEXT, &serialize(&list))
}
