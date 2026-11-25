//! One request and its response over a pooled or new connection.

use super::Client;
use crate::connect::connect;
use crate::error::NetError;
use crate::http1::{self, RawResponse};
use crate::pool::Key;
use crate::stream::Stream;
use crate::tls::TlsStream;
use alt_url::Url;

/// Headers this stack adds to the requester's.
pub struct Extra<'a> {
    pub cookie: Option<&'a str>,
    pub origin: Option<&'a str>,
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
    fn open(&self, key: &Key) -> Result<Stream, NetError> {
        let addrs = self.dns.lookup(&key.host, self.config.dns_timeout)?;
        let tcp = connect(&addrs, key.port, self.config.connect_timeout)?;
        tcp.set_read_timeout(Some(self.config.io_timeout))?;
        tcp.set_write_timeout(Some(self.config.io_timeout))?;
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
        body: Option<&[u8]>,
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
            ("Accept-Encoding", b"gzip, deflate"),
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
        let (slot, idle) = self.pool.checkout(&key, self.config.connect_timeout)?;
        let mut reused = idle.is_some();
        let mut stream = match idle {
            Some(s) => s,
            None => self.open(&key)?,
        };
        loop {
            stream.set_timeouts(self.config.io_timeout, self.config.io_timeout)?;
            let sent = http1::write_request(&mut stream, method, &target, &fields, body);
            let result = match sent {
                Ok(()) => http1::read_response(&mut stream, method, self.config.max_body),
                Err(error) => Err(http1::ExchangeError { error, nothing_received: true }),
            };
            match result {
                Ok(raw) => {
                    if raw.reusable {
                        self.pool.checkin(slot, stream);
                    } else {
                        stream.close();
                    }
                    return Ok(raw);
                }
                Err(e) if reused && e.nothing_received => {
                    stream = self.open(&key)?;
                    reused = false;
                }
                Err(e) => return Err(e.error),
            }
        }
    }
}
