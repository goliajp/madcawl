//! The append-only audit log: one line a request, tab-separated — UTC
//! time, agent session, page origin, action, the target's role and name,
//! result. No page text goes in it beyond a target's name, which is cut
//! short.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// The longest a name or an origin is written.
const MAX_FIELD: usize = 120;

pub struct AuditLog(Mutex<File>);

pub struct Line<'a> {
    pub session: u32,
    pub origin: &'a str,
    pub action: &'a str,
    pub role: &'a str,
    pub name: &'a str,
    pub result: &'a str,
}

impl AuditLog {
    /// Opens the log for appending; a new one only the user can read.
    pub fn open(path: &Path) -> std::io::Result<AuditLog> {
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        Ok(AuditLog(Mutex::new(f)))
    }

    pub fn write(&self, l: &Line) {
        let mut s = String::with_capacity(160);
        s.push_str(&utc_now());
        for (i, v) in [l.origin, l.action, l.role, l.name, l.result]
            .iter()
            .enumerate()
        {
            if i == 0 {
                s.push('\t');
                s.push_str(&l.session.to_string());
            }
            s.push('\t');
            push_field(&mut s, v);
        }
        s.push('\n');
        // one write a line, so lines from sessions never interleave
        let mut f = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = f.write_all(s.as_bytes()) {
            eprintln!("madcawl-shell: audit log: {e}");
        }
    }
}

/// `v` with tabs and line breaks as spaces, cut to `MAX_FIELD`
/// characters; `-` when empty.
fn push_field(out: &mut String, v: &str) {
    if v.is_empty() {
        out.push('-');
        return;
    }
    for (i, c) in v.chars().enumerate() {
        if i == MAX_FIELD {
            out.push('…');
            break;
        }
        out.push(if c.is_control() { ' ' } else { c });
    }
}

fn utc_now() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = t.as_secs();
    let (y, m, d) = civil(secs / 86_400);
    let s = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        s / 3600,
        s / 60 % 60,
        s % 60,
        t.subsec_millis()
    )
}

/// The calendar date `days` after 1970-01-01 (Howard Hinnant's
/// days-to-civil).
pub(crate) fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_fields() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_782), (2026, 11, 25));
        assert_eq!(civil(11_016), (2000, 2, 29));
        let mut s = String::new();
        push_field(&mut s, "a\tb\nc");
        assert_eq!(s, "a b c");
        let mut s = String::new();
        push_field(&mut s, &"x".repeat(500));
        assert_eq!(s.chars().count(), MAX_FIELD + 1);
    }
}
