//! The agent protocol target: a mutated stream of request frames. Read
//! whole and in chunks it must give the same frames, and each frame the
//! same answers: its action and wait parsed, and the page's own answer
//! (`Agents::handle`) on a fixed page, where a kind it does not know is
//! skipped. Read whole, the frames also go to a windowless host serving
//! that page, re-aimed at a page the connection opened, then whatever
//! bytes did not make a frame; a panic on any of the host's threads
//! fails the input.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::time::Duration;

use madcawl_agent::act::{Action, Until};
use madcawl_agent::{Agents, Frame, FrameError, Inputs, Ref, kind, tag};

use crate::PANICS;
use crate::wpt::{Feed, Rng};

const PAGE: &str = r##"<!doctype html><title>Fuzz</title><h1>Form</h1><a href="other.html">Other</a>
<form method=post action="http://127.0.0.1:9/x"><label>Name <input name=n value=v></label>
<label>Secret <input type=password name=p></label><textarea name=t>x</textarea>
<label><input type=checkbox name=c> Tick</label><input type=radio name=r><input type=radio name=r checked>
<select name=s><option>a<option value=b>B</select><input type=file name=f><button>Go</button>
<button disabled>Off</button></form><div style="position:relative"><button>Under</button>
<div style="position:absolute;left:0;top:0;width:200px;height:30px">Cover</div></div>
<a href="#" style="display:none">Hidden</a><div style="height:2000px"></div><a href="/">Far</a>"##;

pub const PIECES: &[&[u8]] = &[
    b"\x00", b"\xff", b"\x80", b"\x7f", b"\x0c\x00", b"\x0d\x00", b"\x22\x00", b"e1", b"ex", b"y", b"click",
    b"type", b"fill_form", b"\x00\x00\x00\x00", b"\xff\xff\xff\x7f",
];

fn seeds() -> Vec<Frame> {
    let until = Until { reference: Some("e5".into()), text: Some("Form".into()), timeout_ms: 50 };
    let refs: Vec<String> = (1..48).map(|index| Ref { index, slot_gen: 0, page_gen: 1 }.text()).collect();
    let mut out = vec![
        Frame::new(kind::READ).num(tag::VIEW, 1).num(tag::SCOPE, 0),
        Frame::new(kind::READ).num(tag::VIEW, 3).str(tag::TEXT, "Form").num(tag::LIMIT, 3),
        Frame::new(kind::READ).num(tag::VIEW, 4).str(tag::WHAT, "forms"),
        Frame::new(kind::READ).num(tag::VIEW, 1).num(tag::SCOPE, 2).str(tag::REF, "e5"),
        Frame::new(kind::DIFF).num(tag::SINCE, 0),
        until.write(Frame::new(kind::WAIT)),
        Frame::new(kind::REQUESTS).num(tag::LIMIT, 5),
        Frame::new(kind::PAGES),
        Frame::new(0x7777).str(200, "unknown"),
    ];
    for r in refs.iter().map(String::as_str) {
        let all = [
            Action::Click(r.into()),
            Action::Type { target: Some(r.into()), text: "hello".into() },
            Action::Press { target: Some(r.into()), key: "Enter".into() },
            Action::Select { target: r.into(), option: "B".into() },
            Action::Check { target: r.into(), on: true },
            Action::Upload { target: r.into(), path: "/etc/hosts".into() },
            Action::Fill { fields: vec![(r.into(), "x".into()), ("e9".into(), "true".into())], submit: true, submitter: None },
            Action::Scroll { target: Some(r.into()), by: 2 },
            Action::Submit(r.into()),
        ];
        out.extend(all.iter().map(|a| a.frame(1, &until)));
    }
    out.extend([Action::Back, Action::Navigate("other.html".into()), Action::Scroll { target: None, by: -1 }].iter().map(|a| a.frame(1, &until)));
    out
}

pub fn corpus() -> Vec<Vec<u8>> {
    let all = seeds();
    let mut out: Vec<Vec<u8>> = all.iter().map(|f| encode(std::slice::from_ref(f))).collect();
    out.push(encode(&all));
    out.extend(all.chunks(4).map(encode));
    out
}

fn encode(frames: &[Frame]) -> Vec<u8> {
    let mut b = Vec::new();
    for f in frames {
        f.encode(&mut b);
    }
    b
}

/// The frames of `input` and the bytes after the last one, read whole
/// or as they would arrive in chunks.
fn frames(input: &[u8], feed: Feed) -> (Vec<Frame>, Option<FrameError>, usize) {
    let mut rng = match feed {
        Feed::Whole => None,
        Feed::Bytewise => Some((Rng(1), 1)),
        Feed::Chunks { seed, max } => Some((Rng(seed | 1), max)),
    };
    let (mut buf, mut at, mut out) = (Vec::new(), 0, Vec::new());
    loop {
        let n = match &mut rng {
            None => input.len() - at,
            Some((r, max)) => (1 + r.next() as usize % *max).min(input.len() - at),
        };
        buf.extend_from_slice(&input[at..at + n]);
        at += n;
        loop {
            match Frame::decode(&buf) {
                Ok((f, used)) => {
                    out.push(f);
                    buf.drain(..used);
                }
                Err(FrameError::Short) if at < input.len() => break,
                Err(e) => return (out, (!buf.is_empty()).then_some(e), input.len() - buf.len()),
            }
        }
    }
}

/// What one input gave, flattened: the same fed whole and in chunks.
pub fn run(input: &[u8], feed: Feed) -> Vec<u8> {
    let (got, err, used) = frames(input, feed);
    let mut dom = madcawl_shell::load::parse(PAGE.as_bytes(), None).0;
    let mut agents = Agents::new();
    let session = agents.attach(&mut dom, Inputs::default()).expect("one session");
    let mut flat = format!("{} frames, then {err:?}\n", got.len()).into_bytes();
    for f in &got {
        let mut again = Vec::new();
        f.encode(&mut again);
        assert_eq!(Frame::decode(&again).map(|(g, _)| g).as_ref(), Ok(f), "a frame read back differently");
        let mut asked = f.clone();
        asked.fields.retain(|(t, _)| *t != tag::SESSION);
        let asked = asked.num(tag::SESSION, u64::from(session));
        let answer = agents.handle(&mut dom, Inputs::default(), &asked);
        let line = format!("{:?} {:?} {:?}\n", Action::parse(f), Until::parse(f), answer.map(|a| a.kind));
        flat.extend_from_slice(line.as_bytes());
    }
    if matches!(feed, Feed::Whole) {
        host_pass(&got, &input[used..]);
    }
    flat
}

struct Host {
    socket: PathBuf,
}

static HOST: OnceLock<Host> = OnceLock::new();

/// Removes the host's directory, at the end of a run.
pub fn cleanup() {
    if let Some(dir) = HOST.get().and_then(|h| h.socket.parent()) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn host() -> &'static Host {
    HOST.get_or_init(|| {
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("page.html"), PAGE).unwrap();
        std::fs::write(dir.join("other.html"), "<p>other <a href=page.html>back</a>").unwrap();
        let mut c = madcawl_shell::serve::Config::new(dir.join("s"));
        c.allow = vec![dir.clone()];
        c.uploads = vec![dir.clone()];
        c.audit = dir.join("audit.log");
        (c.rate, c.burst) = (1e9, 1e9);
        (c.max_sessions, c.max_total_pages) = (1000, 100_000);
        let server = madcawl_shell::serve::Server::bind(c).unwrap();
        std::thread::spawn(move || server.run());
        Host { socket: dir.join("s") }
    })
}

/// One reply, or `None` when the host closed the connection.
fn reply(s: &mut UnixStream) -> Option<Frame> {
    let mut head = [0u8; 4];
    s.read_exact(&mut head).ok()?;
    let mut body = vec![0u8; u32::from_le_bytes(head) as usize];
    s.read_exact(&mut body).ok()?;
    let mut all = head.to_vec();
    all.extend_from_slice(&body);
    Frame::decode(&all).ok().map(|(f, _)| f)
}

fn host_pass(got: &[Frame], tail: &[u8]) {
    let before = PANICS.load(Ordering::SeqCst);
    let mut s = UnixStream::connect(&host().socket).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let open = Frame::new(kind::OPEN).str(tag::URL, &host().socket.with_file_name("page.html").display().to_string());
    s.write_all(&encode(&[open])).unwrap();
    let page = reply(&mut s).and_then(|r| r.get_num(tag::PAGE)).expect("the page opens");
    for f in got {
        let mut f = f.clone();
        if f.kind != kind::OPEN {
            f.fields.retain(|(t, _)| *t != tag::PAGE);
            f = f.num(tag::PAGE, page);
        }
        if s.write_all(&encode(&[f])).is_err() || reply(&mut s).is_none() {
            break;
        }
    }
    if !tail.is_empty() && s.write_all(tail).is_ok() {
        let _ = s.shutdown(std::net::Shutdown::Write);
        while reply(&mut s).is_some() {}
    }
    drop(s);
    assert_eq!(PANICS.load(Ordering::SeqCst), before, "the host panicked");
}
