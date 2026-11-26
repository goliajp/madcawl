//! A server on the internet that offers TLS 1.2 and nothing newer, reached
//! with the system's trust. It needs the network, so it runs only when
//! asked: `cargo test -p madcawl-net --test tls12_live -- --ignored`. A
//! failure can also mean the site has started offering TLS 1.3; check with
//! `openssl s_client -connect www.jal.co.jp:443 -tls1_3`.

use madcawl_net::sock::{Bounds, Tcp};
use madcawl_net::tls::TlsStream;
use madcawl_net::trust::{Trust, TrustSource};
use madcawl_net::{Client, Config, Request, Url};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;
use torajs_tls::ProtocolVersion;

const HOST: &str = "www.jal.co.jp";

#[test]
#[ignore = "network: connects to www.jal.co.jp"]
fn a_site_that_offers_only_tls12_is_reached_on_tls12() {
    let addr = (HOST, 443).to_socket_addrs().unwrap().next().unwrap();
    let sock = TcpStream::connect_timeout(&addr, Duration::from_secs(10)).unwrap();
    let tcp = Tcp::new(sock, Bounds::idle(Duration::from_secs(10)));
    let tls = TlsStream::handshake(tcp, HOST, Arc::new(Trust::new(TrustSource::System))).unwrap();
    assert_eq!(tls.protocol_version(), Some(ProtocolVersion::Tls12));
    let c = Client::new(Config::default());
    let r = c.fetch(&Request::get(Url::parse(&format!("https://{HOST}/favicon.ico")).unwrap())).unwrap();
    assert_eq!(r.status, 200);
    assert!(!r.body.is_empty());
    println!("{HOST}: TLS 1.2, GET /favicon.ico {} {}, {} bytes", r.status, r.reason, r.body.len());
}
