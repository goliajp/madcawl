//! What the network tests share: a small site on 127.0.0.1 whose routes
//! each test file gives, and agent sessions on a host in this process or
//! in the executable.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use madcawl_mcp::json::{self, Value};
use madcawl_shell::serve::{Config, Server};

pub const JS: &str = "Content-Type: text/javascript\r\n";

pub fn respond(s: &mut TcpStream, status: &str, extra: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

/// A route: answers the request for a lowercased path, given the
/// request's `Cookie` header; whether it knew the path.
pub type Route = fn(&mut TcpStream, &str, &str) -> bool;

fn handle(mut s: TcpStream, route: Route) {
    let mut buf = Vec::new();
    let mut b = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&b[..n]),
        }
    }
    let req = String::from_utf8_lossy(&buf).to_ascii_lowercase();
    let path = req.split_whitespace().nth(1).unwrap_or("").to_owned();
    let cookie = req.lines().find_map(|l| l.strip_prefix("cookie:")).unwrap_or("").to_owned();
    if !route(&mut s, &path, &cookie) {
        respond(&mut s, "404 Not Found", JS, b"log('a 404 body ran')");
    }
}

/// A site on 127.0.0.1 answering with `route`; its origin.
pub fn site(route: Route) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || handle(s, route));
        }
    });
    format!("http://{addr}")
}

pub struct Host {
    pub dir: PathBuf,
    pub mcp: madcawl_mcp::server::Server,
}

impl Host {
    pub fn start() -> Host {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("mcc-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        madcawl_shell::serve::scripts::set_scripts(madcawl_content::open);
        let mut config = Config::new(dir.join("s"));
        config.allow = vec![dir.clone()];
        config.net = true;
        let server = Server::bind(config.clone()).unwrap();
        std::thread::spawn(move || server.run());
        let mcp = madcawl_mcp::server::Server::new(config.socket.clone(), dir.clone());
        Host { dir, mcp }
    }

    /// The reply's text, and whether it is an error.
    pub fn call_any(&mut self, tool: &str, args: &str) -> (String, bool) {
        let line = format!(r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#);
        let v = self.mcp.handle(&line).unwrap();
        let r = v.get("result").unwrap();
        let Some(Value::Arr(c)) = r.get("content") else { panic!("{}", v.to_json()) };
        (c[0].str_at("text").unwrap().to_owned(), r.bool_at("isError").unwrap())
    }

    pub fn call(&mut self, tool: &str, args: &str) -> String {
        let (text, error) = self.call_any(tool, args);
        assert!(!error, "{tool} {args}: {text}");
        text
    }
}

/// A host process, ended when the test ends, a failed one too.
pub struct Child(std::process::Child);

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The executable, listening on a socket in `dir`, and an agent session
/// on it.
pub fn executable(dir: &std::path::Path, extra: &[&str]) -> (Child, Host) {
    std::fs::create_dir_all(dir).unwrap();
    let sock = dir.join("s");
    let child = Child(
        std::process::Command::new(env!("CARGO_BIN_EXE_madcawl-content"))
            .args(["--serve", sock.to_str().unwrap(), "--net", "--exit-idle", "30"])
            .args(extra)
            .spawn()
            .unwrap(),
    );
    let t0 = Instant::now();
    while !sock.exists() {
        assert!(t0.elapsed() < Duration::from_secs(10), "the host never listened");
        std::thread::sleep(Duration::from_millis(20));
    }
    (child, Host { dir: dir.to_owned(), mcp: madcawl_mcp::server::Server::new(sock, dir.to_owned()) })
}

pub fn open_at(h: &mut Host, url: &str) -> (String, bool) {
    h.call_any("open", &format!("{{\"path\":{}}}", json::s(url).to_json()))
}
