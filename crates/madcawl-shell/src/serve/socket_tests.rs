use super::*;
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().canonicalize().unwrap().join(format!("mcs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// What a client that connects the moment the path appears meets.
fn first_connect(path: PathBuf) -> std::io::Result<UnixStream> {
    let t0 = Instant::now();
    loop {
        match UnixStream::connect(&path) {
            Err(e) if e.kind() == ErrorKind::NotFound && t0.elapsed() < Duration::from_secs(10) => {}
            r => return r,
        }
    }
}

#[test]
fn a_client_that_finds_the_path_is_never_refused() {
    let d = dir("race");
    for n in 0..200 {
        let path = d.join(format!("s{n}"));
        let p = path.clone();
        let watcher = std::thread::spawn(move || first_connect(p));
        let listener = bind(&path).unwrap();
        let met = watcher.join().unwrap();
        assert!(met.is_ok(), "bind {n}: {met:?}");
        drop(listener);
    }
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn the_path_is_the_user_s_and_a_second_host_is_turned_away() {
    let d = dir("twice");
    let path = d.join("s");
    let _listener = bind(&path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    assert_eq!(bind(&path).unwrap_err().kind(), ErrorKind::AddrInUse);
    let names: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["s"], "the staged name is gone");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_socket_no_host_serves_on_is_replaced() {
    let d = dir("stale");
    let path = d.join("s");
    drop(bind(&path).unwrap());
    let mut staged = path.clone().into_os_string();
    staged.push(format!(".{}", std::process::id()));
    drop(UnixListener::bind(&staged).unwrap());
    let _listener = bind(&path).unwrap();
    assert!(UnixStream::connect(&path).is_ok());
    std::fs::write(d.join("f"), b"").unwrap();
    assert_eq!(bind(&d.join("f")).unwrap_err().kind(), ErrorKind::AlreadyExists);
    std::fs::remove_dir_all(&d).unwrap();
}
