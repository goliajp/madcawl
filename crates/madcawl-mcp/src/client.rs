//! The connection to the madcawl host: one Unix socket, made on the
//! first call and again after it breaks. The host ties pages to the
//! connection, so pages opened before a break are gone after it.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::frame::{Frame, MAX_FRAME};

/// How long one reply may take: opening a large page parses, styles and
/// lays it out first.
const REPLY_WAIT: Duration = Duration::from_secs(120);

pub struct Client {
    socket: PathBuf,
    conn: Option<UnixStream>,
}

impl Client {
    pub fn new(socket: PathBuf) -> Client {
        Client { socket, conn: None }
    }

    /// The host's reply to `f`; Err with what went wrong, after which the
    /// next call connects again.
    pub fn call(&mut self, f: &Frame) -> Result<Frame, String> {
        let r = self.exchange(f);
        if r.is_err() {
            self.conn = None;
        }
        r
    }

    fn exchange(&mut self, f: &Frame) -> Result<Frame, String> {
        let conn = match &mut self.conn {
            Some(c) => c,
            None => {
                let c = UnixStream::connect(&self.socket).map_err(|e| {
                    format!(
                        "the madcawl host at {} is not reachable ({e}); start it with `madcawl-shell --serve {}`",
                        self.socket.display(),
                        self.socket.display()
                    )
                })?;
                c.set_read_timeout(Some(REPLY_WAIT))
                    .map_err(|e| e.to_string())?;
                self.conn.insert(c)
            }
        };
        let lost = |e: std::io::Error| {
            format!("the connection to the madcawl host broke ({e}); pages opened on it are closed")
        };
        conn.write_all(&f.encode()).map_err(lost)?;
        let mut head = [0u8; 4];
        conn.read_exact(&mut head).map_err(lost)?;
        let len = u32::from_le_bytes(head) as usize;
        if len > MAX_FRAME {
            return Err(format!(
                "the host sent a reply of {len} bytes, more than {MAX_FRAME}"
            ));
        }
        let mut body = vec![0u8; len];
        conn.read_exact(&mut body).map_err(lost)?;
        Frame::decode_body(&body).ok_or_else(|| "the host sent a malformed frame".into())
    }
}
