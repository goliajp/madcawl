//! TCP connections with a deadline, trying addresses one family after the
//! other in turn (RFC 8305 §4 order, without the racing)

use crate::error::NetError;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// IPv6 and IPv4 addresses interleaved, starting with the family of the
/// first one answered.
pub fn interleave(addrs: &[IpAddr]) -> Vec<IpAddr> {
    let Some(first) = addrs.first() else {
        return Vec::new();
    };
    let (mut a, mut b): (Vec<IpAddr>, Vec<IpAddr>) = addrs.iter().partition(|x| x.is_ipv6() == first.is_ipv6());
    let mut out = Vec::with_capacity(addrs.len());
    a.reverse();
    b.reverse();
    while !a.is_empty() || !b.is_empty() {
        out.extend(a.pop());
        out.extend(b.pop());
    }
    out
}

/// Connects to the first address that answers within `timeout` in all;
/// each attempt gets an even share of what is left, at least 250 ms.
pub fn connect(addrs: &[IpAddr], port: u16, timeout: Duration) -> Result<TcpStream, NetError> {
    let order = interleave(addrs);
    let deadline = Instant::now() + timeout;
    let mut last = NetError::Connect("no addresses".into());
    for (i, ip) in order.iter().enumerate() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(NetError::ConnectTimeout);
        }
        let share = (left / (order.len() - i) as u32).max(Duration::from_millis(250)).min(left);
        match TcpStream::connect_timeout(&SocketAddr::new(*ip, port), share) {
            Ok(s) => {
                s.set_nodelay(true)?;
                return Ok(s);
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => last = NetError::ConnectTimeout,
            Err(e) => last = NetError::Connect(format!("{ip}: {e}")),
        }
    }
    Err(last)
}
