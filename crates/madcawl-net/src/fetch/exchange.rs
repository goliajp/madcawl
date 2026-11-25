//! One request and its response over a pooled or new connection.

use super::Client;
use crate::connect::connect;
use crate::error::NetError;
use crate::http1::{self, BodySink, RawResponse, ReadLimits};
use crate::pool::Key;
use crate::sock::{Attached, Bounds, Cancel, Tcp};
use crate::stream::Stream;
use crate::tls::TlsStream;
use alt_url::Url;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Headers this stack adds to the requester's, the body, and when the
/// whole request has to be done.
pub struct Extra<'a> {
    pub cookie: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub body: Option<&'a [u8]>,
    pub deadline: Instant,
    pub cancel: Option<&'a Arc<Cancel>>,
}

/// `limit`, or less when the deadline is nearer; an error once it passed.
fn within(limit: Duration, deadline: Instant) -> Result<Duration, NetError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(NetError::Deadline);
    }
    Ok(limit.min(left))
}

fn attach<'c>(cancel: Option<&'c Arc<Cancel>>, tcp: &TcpStream) -> Result<Option<Attached<'c>>, NetError> {
    cancel.map(|c| c.attach(tcp)).transpose()
}

fn target(url: &Url) -> String {
    let path = if url.path().is_empty() { "/" } else { url.path() };
    match url.query() {
        Some(q) => format!("{path}?{q}"),
        None => path.to_owned(),
    }
}

fn host_header(url: &Url) -> String {
    let host = url.host_str().unwrap_or("");
    match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_owned(),
    }
}

impl Client {
    fn open(&self, key: &Key, bounds: &Bounds) -> Result<Stream, NetError> {
        let deadline = bounds.deadline.unwrap_or_else(|| Instant::now() + self.config.request_timeout);
        let addrs = self.dns.lookup(&key.host, within(self.config.dns_timeout, deadline)?)?;
        let tcp = connect(&addrs, key.port, within(self.config.connect_timeout, deadline)?)?;
        let _attached = attach(bounds.cancel.as_ref(), &tcp)?;
        let tcp = Tcp::new(tcp, bounds.clone());
        if !key.tls {
            return Ok(Stream::Plain(tcp));
        }
        Ok(Stream::Tls(Box::new(TlsStream::handshake(tcp, &key.host, self.trust.clone())?)))
    }

    pub(super) fn exchange(
        &self,
        method: &str,
        url: &Url,
        headers: &[(String, Vec<u8>)],
        extra: Extra<'_>,
        sink: &mut dyn BodySink,
    ) -> Result<RawResponse, NetError> {
        let key = Key {
            tls: url.scheme() == "https",
            host: url.host_str().ok_or_else(|| NetError::BadUrl(format!("{url}: no host")))?.to_owned(),
            port: url.port_or_known_default().ok_or_else(|| NetError::BadUrl(format!("{url}: no port")))?,
        };
        let host = host_header(url);
        let mut fields: Vec<(&str, &[u8])> = vec![
            ("Host", host.as_bytes()),
            ("User-Agent", self.config.user_agent.as_bytes()),
            ("Accept-Encoding", b"gzip, deflate, br"),
        ];
        if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("accept")) {
            fields.push(("Accept", b"*/*"));
        }
        fields.extend(headers.iter().map(|(n, v)| (n.as_str(), v.as_slice())));
        if let Some(c) = extra.cookie {
            fields.push(("Cookie", c.as_bytes()));
        }
        if let Some(o) = extra.origin {
            fields.push(("Origin", o.as_bytes()));
        }
        let target = target(url);
        let limits = ReadLimits { max_body: self.config.max_body };
        let bounds = Bounds { io: self.config.io_timeout, deadline: Some(extra.deadline), cancel: extra.cancel.cloned() };
        let (slot, idle) = self.pool.checkout(&key, within(self.config.connect_timeout, extra.deadline)?)?;
        let mut reused = idle.is_some();
        let mut stream = match idle {
            Some(s) => s,
            None => self.open(&key, &bounds)?,
        };
        loop {
            stream.set_bounds(bounds.clone());
            let attached = attach(extra.cancel, stream.tcp())?;
            let sent = http1::write_request(&mut stream, method, &target, &fields, extra.body);
            let result = match sent {
                Ok(()) => http1::read_response(&mut stream, method, &limits, sink),
                Err(error) => Err(http1::ExchangeError { error, nothing_received: true }),
            };
            drop(attached);
            match result {
                Ok(raw) => {
                    stream.set_bounds(Bounds::idle(self.config.io_timeout));
                    if raw.reusable {
                        self.pool.checkin(slot, stream);
                    } else {
                        stream.close();
                    }
                    return Ok(raw);
                }
                Err(e) if reused && e.nothing_received && !matches!(e.error, NetError::Deadline | NetError::Cancelled) => {
                    stream = self.open(&key, &bounds)?;
                    reused = false;
                }
                Err(e) => return Err(e.error),
            }
        }
    }
}
