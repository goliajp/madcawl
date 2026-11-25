use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    BadUrl(String),
    /// a scheme fetch does not take to the network here
    Scheme(String),
    Dns(String),
    DnsTimeout,
    Connect(String),
    ConnectTimeout,
    Tls(String),
    /// the server's chain is not trusted for the host
    Untrusted(String),
    Io(String),
    ReadTimeout,
    /// the whole request, redirects included, ran past its time
    Deadline,
    Http(String),
    /// the response head, body or decoded body passed its cap
    TooLarge(&'static str),
    Decode(String),
    TooManyRedirects,
    /// the request was refused before it went out
    Refused(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::BadUrl(s) => write!(f, "bad url: {s}"),
            NetError::Scheme(s) => write!(f, "scheme not fetched over the network: {s}"),
            NetError::Dns(s) => write!(f, "dns: {s}"),
            NetError::DnsTimeout => f.write_str("dns: timed out"),
            NetError::Connect(s) => write!(f, "connect: {s}"),
            NetError::ConnectTimeout => f.write_str("connect: timed out"),
            NetError::Tls(s) => write!(f, "tls: {s}"),
            NetError::Untrusted(s) => write!(f, "certificate not trusted: {s}"),
            NetError::Io(s) => write!(f, "io: {s}"),
            NetError::ReadTimeout => f.write_str("read: timed out"),
            NetError::Deadline => f.write_str("request: timed out"),
            NetError::Http(s) => write!(f, "http: {s}"),
            NetError::TooLarge(what) => write!(f, "{what} over its size cap"),
            NetError::Decode(s) => write!(f, "content decoding: {s}"),
            NetError::TooManyRedirects => f.write_str("too many redirects"),
            NetError::Refused(s) => write!(f, "refused: {s}"),
        }
    }
}

impl std::error::Error for NetError {}

impl From<std::io::Error> for NetError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => NetError::ReadTimeout,
            _ => NetError::Io(e.to_string()),
        }
    }
}
