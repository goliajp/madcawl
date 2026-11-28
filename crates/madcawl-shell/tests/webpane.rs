//! A pane page end to end, with a test display end in place of marspot:
//! the host opens the page in a content process of its own and hands
//! over the display channel, the content process draws the viewport into
//! the display end's shared memory, and what it drew is what `shot`
//! paints of the same page. Once drawn, an idle pane costs no CPU.

mod display_end;

use std::os::unix::net::UnixStream;
use std::time::Duration;

use madcawl_agent::{Frame, kind, tag};
use madcawl_shell::rusage::usage_of;
use madcawl_wire::display::{Commit, CommitError, released};
use madcawl_wire::{Hello, host};
use display_end::{EXE, Host, Link, alive, attach, attach_with, display_end, open_pane, png};

/// What `madcawl-shell shot` paints of `page` at `scale`, decoded.
fn shot(page: &str, out: &std::path::Path, scale: &str) -> png::Rgba {
    let r = std::process::Command::new(EXE).args(["shot", page, "-o", out.to_str().unwrap(), "--scale", scale]).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    png::decode(&std::fs::read(out).unwrap())
}

#[test]
fn a_pane_page_shows_what_shot_paints() {
    let h = Host::start("shot", &[]);
    let mut d = display_end(&h);
    for (scale, name) in [(100u32, "1"), (200, "2")] {
        let px = (1280 * scale / 100, 800 * scale / 100);
        let (opened, ch) = open_pane(&mut d, &h.page(), (1280, 800), scale);
        assert!(opened.pid > 1 && opened.page > 0, "{opened:?}");
        let mut pane = attach(ch, px, scale, 2);
        let c = pane.commit(None);
        assert_eq!((c.dirty.w, c.dirty.h, c.doc_width), (px.0, px.1, 1280));
        let want = shot(&h.page(), &h.dir.join(format!("shot-{name}.png")), name);
        assert_eq!((want.width, want.height), (px.0 as usize, px.1 as usize));
        let mut differ = 0usize;
        let mut colors = std::collections::HashSet::new();
        for y in 0..want.height {
            for x in 0..want.width {
                let [b, g, r, a] = pane.pixel(c.buffer as usize, x, y);
                let w = want.px[y * want.width + x];
                colors.insert([r, g, b]);
                differ += usize::from([r, g, b, a] != w);
            }
        }
        assert!(colors.len() > 16, "the page drew text and boxes, not one color: {}", colors.len());
        assert_eq!(differ, 0, "at scale {name}, {differ} of {} pixels differ from shot", want.width * want.height);
        // released and drawn again only when something changes: nothing does
        pane.ch.send(&released(c.seq));
        pane.ch.s.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut b = [0u8; 16];
        let quiet = std::io::Read::read(&mut pane.ch.s, &mut b);
        assert!(quiet.is_err(), "no commit without a change: {quiet:?}");
    }
}

#[test]
fn a_display_end_out_of_range_is_told_the_hosts_range_and_let_go() {
    let h = Host::start("range", &[]);
    let mut l = Link::new(UnixStream::connect(&h.socket).unwrap());
    let newer = madcawl_wire::Range::new(host::RANGE.cur + 1, host::RANGE.cur + 3);
    let a = l.hello(host::kind::HELLO, newer, Some(host::role::DISPLAY));
    assert_eq!((a.range, a.version), (host::RANGE, None));
    let mut b = [0u8; 8];
    assert_eq!(std::io::Read::read(&mut l.s, &mut b).unwrap(), 0, "closed after the answer");
    let audit = std::fs::read_to_string(h.dir.join("s.audit.log")).unwrap();
    assert!(audit.contains("display-hello") && audit.contains("PeerTooNew"), "{audit}");
}

#[test]
fn a_bad_pane_request_is_refused_and_the_connection_goes_on() {
    let h = Host::start("bad", &[]);
    let mut d = display_end(&h);
    d.send(&Frame::new(host::kind::PANE_OPEN).num(madcawl_wire::tag::SESSION, 1));
    let r = d.next();
    assert_eq!(host::refusal(&r).map(|(c, _)| c), Some(10), "{r:?}");
    let outside = h.dir.parent().unwrap().join("elsewhere.html");
    let o = host::PaneOpen { session: 2, url: outside.display().to_string(), width: 10, height: 10, scale: 100 };
    d.send(&o.frame());
    let r = d.next();
    assert!(host::refusal(&r).is_some(), "a page outside the allowed directory: {r:?}");
    let (opened, _) = open_pane(&mut d, &h.page(), (100, 100), 100);
    assert!(opened.pid > 1);
}

#[test]
fn a_pane_outlives_the_idle_times_while_its_display_end_is_connected() {
    let mut h = Host::start("outlive", &["--idle-ms", "200", "--exit-idle", "1"]);
    let mut d = display_end(&h);
    let (opened, ch) = open_pane(&mut d, &h.page(), (320, 200), 100);
    let mut pane = attach(ch, (320, 200), 100, 1);
    let first = pane.commit(None);
    std::thread::sleep(Duration::from_millis(2500));
    assert!(alive(h.pid()) && alive(opened.pid), "neither the host nor the pane's process left");
    // a new surface on the same channel: the page is still there to draw
    let mut again = attach(pane.ch, (640, 400), 200, 2);
    again.last = first.seq;
    let c = again.commit(None);
    assert_eq!((c.dirty.w, c.dirty.h), (640, 400));
    drop(again);
    drop(d);
    assert!(h.exits_within(10), "with the display end gone the host leaves after its idle time");
}

#[test]
fn a_surface_whose_header_lies_closes_the_display_channel() {
    let h = Host::start("lie", &[]);
    let mut d = display_end(&h);
    let (opened, ch) = open_pane(&mut d, &h.page(), (100, 100), 100);
    let mut liar = attach_with(ch, (100, 100), 100, 1, |m| {
        let size = madcawl_wire::shm::Header::needed(100, 100);
        madcawl_wire::shm::Header::new(96, 100, size).write(m[0].bytes_mut());
    });
    let mut b = [0u8; 64];
    let n = std::io::Read::read(&mut liar.ch.s, &mut b).unwrap();
    assert_eq!(n, 0, "the content process closed the channel instead of drawing");
    assert!(alive(opened.pid), "the page goes on without it");
}

/// Process `pid`'s CPU time and wakeups.
fn cpu(pid: i32) -> (u64, u64) {
    let u = usage_of(pid).unwrap();
    (u.cpu_ns, u.interrupt_wakeups)
}

#[test]
fn an_idle_pane_costs_no_cpu_in_any_process() {
    let h = Host::start("idle", &["--net", "--stats"]);
    let netd = {
        let mut s = UnixStream::connect(&h.socket).unwrap();
        let r = webpane_stats(&mut s);
        let line = r.lines().find(|l| l.starts_with("netd-pid ")).unwrap_or_else(|| panic!("{r}"));
        line[9..].parse::<i32>().unwrap()
    };
    let mut d = display_end(&h);
    let (opened, ch) = open_pane(&mut d, &h.page(), (1280, 800), 200);
    let mut pane = attach(ch, (2560, 1600), 200, 2);
    pane.commit(None);
    let pids = [("host", h.pid()), ("content", opened.pid), ("netd", netd)];
    let window = || {
        let before: Vec<_> = pids.iter().map(|&(_, p)| cpu(p)).collect();
        std::thread::sleep(Duration::from_secs(10));
        let after: Vec<_> = pids.iter().map(|&(_, p)| cpu(p)).collect();
        before.iter().zip(&after).map(|(b, a)| (a.0 - b.0, a.1 - b.1)).collect::<Vec<_>>()
    };
    let show = |d: &[(u64, u64)]| {
        let parts: Vec<String> = pids.iter().zip(d).map(|((n, _), (ns, w))| format!("{n} {ns} ns {w} wakeups")).collect();
        parts.join(", ")
    };
    // the first ten seconds may hold the system ending the dispatch worker
    // threads that drawing text started, about five seconds later, at one
    // wakeup in the content process; the next ten are held to zero
    let first = window();
    eprintln!("the first 10 s after the first commit: {}", show(&first));
    assert!(first[0] == (0, 0) && first[2] == (0, 0), "the host or netd ran: {}", show(&first));
    assert!(first[1].1 <= 2, "the content process woke more than twice: {}", show(&first));
    let second = window();
    eprintln!("the next 10 s: {}", show(&second));
    assert!(second.iter().all(|&d| d == (0, 0)), "a process ran while the pane was idle: {}", show(&second));
    drop(pane);
}

/// The host's counters, from an agent session that then ends.
fn webpane_stats(s: &mut UnixStream) -> String {
    let mut l = Link::new(s.try_clone().unwrap());
    l.send(&Frame::new(kind::STATS));
    let r = l.next();
    let text = r.get_str(tag::TEXT).unwrap_or_else(|| panic!("{r:?}")).to_owned();
    drop(l);
    s.shutdown(std::net::Shutdown::Both).unwrap();
    // the session's content process finishes before the quiet spell
    std::thread::sleep(Duration::from_millis(500));
    text
}

#[test]
fn a_display_hello_longer_than_before_and_in_two_pieces_is_a_display_end() {
    let h = Host::start("hello", &[]);
    let mut l = Link::new(UnixStream::connect(&h.socket).unwrap());
    let hello = Hello { range: host::RANGE, role: Some(host::role::DISPLAY), version: None };
    let mut out = Vec::new();
    hello.frame(host::kind::HELLO).bytes(250, &[7; 200]).encode(&mut out);
    std::io::Write::write_all(&mut l.s, &out[..3]).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    std::io::Write::write_all(&mut l.s, &out[3..]).unwrap();
    let a = Hello::from_frame(&l.next()).unwrap();
    assert_eq!(a.version, Some(host::RANGE.cur), "the unknown field was passed over");
    let mut d = display_end(&h);
    open_pane(&mut d, &h.page(), (100, 100), 100);
    // a host hello without the display role is refused, not taken for an agent's
    let mut l = Link::new(UnixStream::connect(&h.socket).unwrap());
    l.send(&Hello { role: None, ..hello }.frame(host::kind::HELLO));
    let r = l.next();
    assert_eq!(host::refusal(&r).map(|(c, _)| c), Some(10), "{r:?}");
}

#[test]
fn a_new_surface_passes_over_old_releases_and_refuses_old_commits() {
    let h = Host::start("epoch", &[]);
    let mut d = display_end(&h);
    let (opened, ch) = open_pane(&mut d, &h.page(), (100, 100), 100);
    let mut a = attach(ch, (100, 100), 100, 1);
    let old = a.commit(None);
    // the old surface's commit, held when the new surface goes
    let mut b = attach(a.ch, (200, 200), 200, 2);
    assert_eq!(old.check(&b.surface, 0, None), Err(CommitError::Epoch));
    let c = b.commit(None);
    assert_eq!(c.epoch, b.surface.epoch);
    let stale = Commit { seq: c.seq + 1, ..old };
    assert_eq!(stale.check(&b.surface, c.seq, None), Err(CommitError::Epoch), "a later sequence does not make it this surface's");
    // the old buffer dropped: its release frees nothing and the channel goes on
    b.ch.send(&released(old.seq));
    b.ch.send(&released(c.seq));
    let mut next = attach(b.ch, (100, 100), 100, 1);
    next.last = c.seq;
    let n = next.commit(None);
    // a release of a commit never sent breaks the protocol
    next.ch.send(&released(n.seq + 1));
    next.ch.s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut buf = [0u8; 64];
    assert_eq!(std::io::Read::read(&mut next.ch.s, &mut buf).unwrap(), 0, "the content process closed the channel");
    assert!(alive(opened.pid), "the page goes on without it");
}
