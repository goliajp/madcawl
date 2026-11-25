//! One HTTP/1.1 exchange over a stream, with `torajs-http1` doing the
//! message syntax.

use crate::error::NetError;
use crate::stream::Stream;
use std::io::{Read, Write};
use torajs_http1::{BodyDecoder, BodyKind, Framing, Limits, Progress, ResponseHead, Version};

pub const MAX_HEAD: usize = 64 * 1024;

/// How many interim (1xx) responses one request may get before its final
/// one.
pub const MAX_INTERIM: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawHead {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, Vec<u8>)>,
}

impl RawHead {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_slice())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawResponse {
    pub head: RawHead,
    /// whether the connection can carry another request
    pub reusable: bool,
}

/// Where a response goes as it is read: the final head, then the body as
/// sent (before content decoding) in pieces.
pub trait BodySink {
    /// false reads the body to the end without handing it on
    fn head(&mut self, head: &RawHead) -> Result<bool, NetError>;
    fn body(&mut self, bytes: &[u8]) -> Result<(), NetError>;
}

/// Bounds on reading one response; the time it may take is the
/// stream's (see `Stream::set_bounds`).
#[derive(Clone, Copy, Debug)]
pub struct ReadLimits {
    /// the body as sent
    pub max_body: usize,
}

/// Why an exchange failed, and whether the server sent nothing at all, in
/// which case a reused connection that had gone stale is the likely cause
/// and the request may go again on a new one.
#[derive(Debug)]
pub struct ExchangeError {
    pub error: NetError,
    pub nothing_received: bool,
}

fn fail(error: NetError, nothing_received: bool) -> ExchangeError {
    ExchangeError { error, nothing_received }
}

pub fn write_request(
    stream: &mut Stream,
    method: &str,
    target: &str,
    headers: &[(&str, &[u8])],
    body: Option<&[u8]>,
) -> Result<(), NetError> {
    let mut head = Vec::with_capacity(512);
    let framing = match body {
        Some(b) => Framing::Length(b.len() as u64),
        None => Framing::Unframed,
    };
    torajs_http1::encode_request_head(&mut head, method, target, Version::Http11, headers, framing)
        .map_err(|e| NetError::Http(format!("request head: {e:?}")))?;
    stream.write_all(&head)?;
    if let Some(b) = body {
        stream.write_all(b)?;
    }
    stream.flush()?;
    Ok(())
}

/// Reads the response to a `method` request, handing the final head and
/// the body to `sink` as they arrive.
pub fn read_response(
    stream: &mut Stream,
    method: &str,
    limits: &ReadLimits,
    sink: &mut dyn BodySink,
) -> Result<RawResponse, ExchangeError> {
    let parse_limits = Limits::new().max_head_len(MAX_HEAD);
    let mut buf = Vec::with_capacity(16 * 1024);
    let mut chunk = vec![0u8; 16 * 1024];
    let mut interim = 0;
    let (head, mut used) = loop {
        match ResponseHead::parse(&buf, &parse_limits) {
            Ok(Progress::Complete { consumed, value }) if (100..200).contains(&value.status()) => {
                if value.status() == 101 {
                    return Err(fail(NetError::Http("protocol upgrade not supported".into()), false));
                }
                interim += 1;
                if interim > MAX_INTERIM {
                    return Err(fail(NetError::Http("too many interim responses".into()), false));
                }
                buf.drain(..consumed);
                continue;
            }
            Ok(Progress::Complete { consumed, value }) => break (value, consumed),
            Ok(Progress::Incomplete) => {}
            Err(e) => return Err(fail(NetError::Http(format!("response head: {e:?}")), false)),
        }
        let n = stream.read(&mut chunk).map_err(NetError::from).map_err(|e| fail(e, buf.is_empty()))?;
        if n == 0 {
            let error = NetError::Http("connection closed before the response head".into());
            return Err(fail(error, buf.is_empty()));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let raw_head = RawHead {
        status: head.status(),
        reason: String::from_utf8_lossy(head.reason()).into_owned(),
        headers: head.headers().iter().map(|h| (h.name().to_owned(), h.value().to_vec())).collect(),
    };
    let wanted = sink.head(&raw_head).map_err(|e| fail(e, false))?;
    let kind = head.body_kind(method);
    let mut decoder = BodyDecoder::with_limits(kind, &parse_limits);
    let mut body = Vec::new();
    let mut total = 0usize;
    let mut leftover = false;
    loop {
        let input = &buf[used..];
        let progress = decoder.decode(input, &mut body).map_err(|e| fail(NetError::Http(format!("body: {e:?}")), false))?;
        total += body.len();
        if total > limits.max_body {
            return Err(fail(NetError::TooLarge("response body"), false));
        }
        if wanted && !body.is_empty() {
            sink.body(&body).map_err(|e| fail(e, false))?;
        }
        body.clear();
        if let Progress::Complete { consumed, .. } = progress {
            leftover = consumed < input.len();
            break;
        }
        buf.clear();
        used = 0;
        let n = stream.read(&mut chunk).map_err(NetError::from).map_err(|e| fail(e, false))?;
        if n == 0 {
            decoder.finish().map_err(|e| fail(NetError::Http(format!("body: {e:?}")), false))?;
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(RawResponse {
        head: raw_head,
        reusable: head.keep_alive() && kind != BodyKind::UntilClose && decoder.is_done() && !leftover,
    })
}
