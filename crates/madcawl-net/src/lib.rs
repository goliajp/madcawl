//! The network stack the network process runs: URLs (`alt-url`), DNS
//! through the system's dnssd, TCP with deadlines, TLS 1.3 (`torajs-tls`)
//! with the system's trust verdicts, HTTP/1.1 (`torajs-http1`) over a
//! bounded keep-alive pool, gzip and deflate (`torajs-flate`) with an
//! output cap, a bounded cookie jar, redirects and fetch's main flow, and
//! the frames the network process speaks to content processes.

pub mod connect;
pub mod cookie;
pub mod decode;
pub mod dns;
pub mod error;
pub mod fetch;
pub mod http1;
pub mod pool;
pub mod psl;
pub mod sock;
pub mod stream;
pub mod tls;
pub mod trust;
pub mod wire;

pub use alt_url::Url;
pub use error::NetError;
pub use fetch::{Client, Config, Redirect, Request, Response};
