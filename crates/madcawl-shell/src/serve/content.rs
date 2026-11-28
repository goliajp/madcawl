//! A content process (`--isolate`): `madcawl-shell --content <session>
//! <run> <socket> [serve options]`, with its host's end of the channel
//! on standard input. It holds the pages of one agent session and
//! answers the session's requests as the host forwards them, as a host
//! without `--isolate` does on a session thread; the host keeps the
//! agent's connection, the network service and the page numbers. It has
//! a watchdog on its request loop and a memory cap, and exits with its
//! host. It holds no descriptor of its host's but the channel: not the
//! audit log, whose lines it sends its host to write. One that holds a
//! pane page has a display channel too (`--display-fd`), on which it
//! draws the page into the display end's shared memory.

use std::ffi::OsString;
use std::os::fd::{AsFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use super::conn::{Channel, Conn};
use super::link::Link;
use super::memory::Cap;
use super::{AuditLog, Config, Net, Shared, parse_args};

/// The watchdog's slow threshold in a content process unless the host
/// was given one.
pub const DEFAULT_WATCH: Duration = Duration::from_millis(500);

/// The command line that starts a content process for session
/// `session` of a host configured with `c`.
pub fn args(c: &Config, session: u32, run: &str) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["--content".into(), session.to_string().into(), run.into(), c.socket.clone().into()];
    let mut opt = |k: &str, v: OsString| {
        a.push(k.into());
        a.push(v);
    };
    for d in &c.allow {
        opt("--allow", d.into());
    }
    opt("--pages", c.max_pages.to_string().into());
    opt("--total-pages", c.max_total_pages.to_string().into());
    opt("--idle-ms", c.idle.as_millis().max(1).to_string().into());
    opt("--rate", c.rate.to_string().into());
    opt("--burst", c.burst.to_string().into());
    opt("--max-request", c.max_request.to_string().into());
    opt("--max-reply", c.max_reply.to_string().into());
    if let Some(d) = &c.downloads {
        opt("--downloads", d.into());
    }
    for d in &c.uploads {
        opt("--upload-allow", d.into());
    }
    for o in &c.approve {
        opt("--approve", o.into());
    }
    let watch = c.watch.unwrap_or(DEFAULT_WATCH);
    opt("--watch-ms", watch.as_millis().max(1).to_string().into());
    let cap = c.memory_cap.unwrap_or_else(|| Cap::for_machine().hard);
    opt("--memory-cap-mib", (cap >> 20).max(1).to_string().into());
    opt("--script-ms", c.script_limit.map_or(0, |l| l.as_millis().max(1)).to_string().into());
    for (on, flag) in [(c.net, "--net"), (c.stats, "--stats")] {
        if on {
            a.push(flag.into());
        }
    }
    a
}

/// The content process's main.
pub fn child_main(args: &[&str]) -> ExitCode {
    // read before anything is parsed: nothing but the display channel may
    // stay open past this
    let display = args.windows(2).find(|w| w[0] == "--display-fd").and_then(|w| w[1].parse().ok());
    super::orphan::close_inherited_fds(display);
    let [session, run, rest @ ..] = args else { return usage() };
    let (Ok(session), Some(config)) = (session.parse::<u32>(), parse_args(rest)) else { return usage() };
    super::orphan::exit_with_parent();
    match serve(session, run, config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("madcawl-shell: content process for session {session}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: madcawl-shell --content <session> <run> <socket> [serve options], started by a host with --isolate");
    ExitCode::from(2)
}

fn serve(session: u32, run: &str, config: Config) -> std::io::Result<()> {
    let stdin = std::io::stdin().as_fd().try_clone_to_owned()?;
    let stream = UnixStream::from(stdin);
    let link = Arc::new(Link::new(stream.try_clone()?));
    let log = AuditLog::Host(link.clone());
    let net = config.net.then(|| {
        let mut sock = config.socket.clone().into_os_string();
        sock.push(".net");
        Arc::new(Net::through(link.clone(), Path::new(&sock), config.downloads.clone()))
    });
    // SAFETY: the host passed this descriptor on purpose, and the sweep
    // of inherited ones left it open for this process to own
    let display = config.display_fd.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) });
    let shared = Shared::new(config, log, net, run.to_owned(), Some(link));
    Conn::new(session, &shared).with_display(display.map(Channel::new)).run(stream);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_content_command_line_gives_the_hosts_configuration() {
        let mut c = Config::new(PathBuf::from("/s"));
        c.net = true;
        c.stats = true;
        c.approve = vec!["http://a".into()];
        c.memory_cap = Some(256 << 20);
        c.rate = 1.5;
        c.script_limit = Some(Duration::from_millis(250));
        let a = args(&c, 7, "run");
        let s: Vec<&str> = a.iter().map(|x| x.to_str().unwrap()).collect();
        assert_eq!(&s[..4], ["--content", "7", "run", "/s"]);
        let back = parse_args(&s[3..]).unwrap();
        assert!(back.net && back.stats);
        assert_eq!(back.approve, c.approve);
        assert_eq!(back.memory_cap, Some(256 << 20));
        assert_eq!(back.watch, Some(DEFAULT_WATCH));
        assert_eq!(back.script_limit, c.script_limit);
        c.script_limit = None;
        let a = args(&c, 7, "run");
        let s: Vec<&str> = a.iter().map(|x| x.to_str().unwrap()).collect();
        assert_eq!(parse_args(&s[3..]).unwrap().script_limit, None);
        assert_eq!((back.max_pages, back.idle, back.rate, back.burst), (c.max_pages, c.idle, 1.5, c.burst));
        assert_eq!((back.max_request, back.max_reply), (c.max_request, c.max_reply));
    }
}
