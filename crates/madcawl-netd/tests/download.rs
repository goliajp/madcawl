//! Downloads into a profile's directory: what is saved, under what name,
//! within what size, and what the client is told; and, served with a
//! control channel, what a content process may have saved.

mod fixture;

use fixture::{PDF_LEN, assert_refused, connect, exchange, get, guarded, navigate, site, status_body, temp_path};
use madcawl_net::wire::{Frame, kind, tag};
use madcawl_net::{Client, Config};
use madcawl_netd::{Downloads, ServerLimits, serve_with};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;

/// A connection to a network process saving into `downloads`; its
/// socket file is gone once connected.
fn netd(downloads: Downloads) -> UnixStream {
    let path = temp_path("s");
    let l = UnixListener::bind(&path).unwrap();
    let client = Arc::new(Client::new(Config::default()));
    let d = Some(Arc::new(downloads));
    std::thread::spawn(move || serve_with(l.incoming(), client, ServerLimits::default(), d));
    let s = connect(&path);
    std::fs::remove_file(&path).unwrap();
    s
}

fn dir_with(per_file: u64, total: u64) -> Downloads {
    files_with(per_file, total, 1000)
}

fn files_with(per_file: u64, total: u64, max_files: u64) -> Downloads {
    let dir = temp_path("dl");
    std::fs::create_dir_all(&dir).unwrap();
    Downloads::with_caps(dir, per_file, total, max_files)
}

fn kinds(frames: &[Frame]) -> Vec<u16> {
    let mut k: Vec<u16> = frames.iter().map(|f| f.kind).collect();
    k.dedup();
    k
}

fn names(dir: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    n.sort();
    n
}

fn done_name(frames: &[Frame]) -> String {
    let done = frames.iter().find(|f| f.kind == kind::DOWNLOAD_DONE).expect("a DOWNLOAD_DONE frame");
    done.get_str(tag::FILE).unwrap().to_owned()
}

#[test]
fn attachments_get_safe_names_and_never_clobber() {
    let origin = site();
    let d = dir_with(1 << 20, 1 << 30);
    let dir = d.dir.clone();
    let mut s = netd(d);
    let first = navigate(&mut s, 1, &format!("{origin}/attach"));
    assert_eq!(kinds(&first), [kind::HEAD, kind::DOWNLOAD, kind::DOWNLOAD_DONE, kind::END]);
    assert_eq!(first[1].get_str(tag::FILE), Some("report.pdf"));
    assert_eq!((done_name(&first).as_str(), first[2].get_num(tag::BYTES)), ("report.pdf", Some(7)));
    let second = navigate(&mut s, 2, &format!("{origin}/attach"));
    assert_eq!(done_name(&second), "report (1).pdf");
    // a fetch that is not a navigation gets an attachment's body, as script's fetch() does
    assert_eq!(status_body(&get(&mut s, 3, &format!("{origin}/attach"))), (200, "PDFDATA".into()));
    assert_eq!(names(&dir), ["report (1).pdf", "report.pdf"]);
    assert_eq!(std::fs::read(dir.join("report.pdf")).unwrap(), b"PDFDATA");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn navigations_to_types_not_shown_are_saved_with_progress() {
    let origin = site();
    let d = dir_with(1 << 30, 1 << 30);
    let dir = d.dir.clone();
    let mut s = netd(d);
    let nav = exchange(&mut s, Frame::new(kind::FETCH).num(tag::ID, 1).str(tag::URL, &format!("{origin}/doc.pdf")).num(tag::NAVIGATION, 1));
    let progress: Vec<u64> = nav.iter().filter(|f| f.kind == kind::DOWNLOAD_PROGRESS).map(|f| f.get_num(tag::BYTES).unwrap()).collect();
    assert_eq!(progress.len(), 2, "one per MiB");
    assert!(progress.windows(2).all(|w| w[0] < w[1]) && progress.iter().all(|&b| b < PDF_LEN as u64));
    assert_eq!(nav.iter().find(|f| f.kind == kind::DOWNLOAD).unwrap().get_num(tag::TOTAL), Some(PDF_LEN as u64));
    assert_eq!(done_name(&nav), "doc.pdf");
    assert_eq!(std::fs::metadata(dir.join("doc.pdf")).unwrap().len(), PDF_LEN as u64);
    // a subresource fetch of the same type is handed over, not saved
    let sub = exchange(&mut s, Frame::new(kind::FETCH).num(tag::ID, 2).str(tag::URL, &format!("{origin}/doc.pdf")).num(tag::NAVIGATION, 0));
    assert!(sub.iter().all(|f| !matches!(f.kind, kind::DOWNLOAD | kind::DOWNLOAD_DONE)));
    assert_eq!(sub.iter().filter(|f| f.kind == kind::BODY).map(|f| f.field(tag::BODY).unwrap().len()).sum::<usize>(), PDF_LEN);
    // and a fetch may ask for any response to be saved
    let forced = exchange(&mut s, Frame::new(kind::FETCH).num(tag::ID, 3).str(tag::URL, &format!("{origin}/login")).num(tag::DOWNLOAD, 1));
    assert_eq!(done_name(&forced), "login");
    assert_eq!(names(&dir), ["doc.pdf", "login"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn downloads_over_their_caps_fail_and_leave_nothing() {
    let origin = site();
    let d = dir_with(100_000, 1 << 30);
    let dir = d.dir.clone();
    let mut s = netd(d);
    let announced = navigate(&mut s, 1, &format!("{origin}/huge"));
    let last = announced.last().unwrap();
    assert_eq!(last.kind, kind::ERROR);
    assert!(last.get_str(tag::MESSAGE).unwrap().contains("size cap"));
    let streamed = navigate(&mut s, 2, &format!("{origin}/stream"));
    assert_eq!(kinds(&streamed), [kind::HEAD, kind::DOWNLOAD, kind::ERROR]);
    assert!(names(&dir).is_empty(), "no .part is left: {:?}", names(&dir));
    std::fs::remove_dir_all(&dir).unwrap();

    let d = dir_with(1 << 20, 10);
    let dir = d.dir.clone();
    std::fs::write(dir.join("old"), b"0123456789").unwrap();
    let mut s = netd(d);
    let full = navigate(&mut s, 3, &format!("{origin}/attach"));
    assert_eq!(full.last().unwrap().kind, kind::ERROR, "the directory is at its total");
    assert_eq!(names(&dir), ["old"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_content_process_saves_only_what_its_document_may() {
    let origin = site();
    let port = origin.rsplit(':').next().unwrap();
    let d = dir_with(1 << 30, 1 << 30);
    let dir = d.dir.clone();
    let mut netd = guarded(Some(d));
    let mut content = netd.content(&format!("{origin}/page"));
    let ask = |id: u64, url: &str| Frame::new(kind::FETCH).num(tag::ID, id).str(tag::URL, url).num(tag::DOWNLOAD, 1);
    assert_eq!(done_name(&exchange(&mut content, ask(1, &format!("{origin}/attach")))), "report.pdf", "its own origin");
    assert_eq!(status_body(&get(&mut content, 2, &format!("{origin}/attach"))), (200, "PDFDATA".into()));
    assert_refused(&exchange(&mut content, ask(3, &format!("http://localhost:{port}/attach"))));
    assert_refused(&navigate(&mut content, 4, &format!("{origin}/doc.pdf")));
    // nor through a redirect of its own origin to another
    let away = exchange(&mut content, ask(6, &format!("{origin}/away")));
    assert_eq!(away.last().unwrap().kind, kind::ERROR, "{away:?}");
    assert!(away.iter().all(|f| f.kind != kind::DOWNLOAD), "{away:?}");
    assert_eq!(names(&dir), ["report.pdf"]);
    // the host's navigation to a type not shown as a page is saved
    let mut host = netd.host();
    assert_eq!(done_name(&navigate(&mut host, 5, &format!("{origin}/doc.pdf"))), "doc.pdf");
    assert_eq!(names(&dir), ["doc.pdf", "report.pdf"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every frame of the answers to several requests sent at once, until
/// each has ended.
fn all_at_once(s: &mut UnixStream, requests: &[Frame]) -> Vec<Frame> {
    for f in requests {
        f.write_to(s).unwrap();
    }
    let (mut out, mut open) = (Vec::new(), requests.len());
    while open > 0 {
        let f = Frame::read_from(s).unwrap().expect("a frame");
        open -= usize::from(matches!(f.kind, kind::END | kind::ERROR));
        out.push(f);
    }
    out
}

#[test]
fn concurrent_downloads_share_the_directorys_total() {
    let origin = site();
    let d = dir_with(1 << 20, 400_000);
    let dir = d.dir.clone();
    let mut s = netd(d);
    let nav = |id: u64| Frame::new(kind::FETCH).num(tag::ID, id).str(tag::URL, &format!("{origin}/slow")).num(tag::NAVIGATION, 1);
    let frames = all_at_once(&mut s, &[nav(1), nav(2)]);
    let count = |k| frames.iter().filter(|f| f.kind == k).count();
    let (done, failed) = (count(kind::DOWNLOAD_DONE), count(kind::ERROR));
    // as their chunks interleave, one or both run out
    assert_eq!((count(kind::DOWNLOAD), done + failed), (2, 2), "{frames:?}");
    assert!(failed >= 1, "{frames:?}");
    let on_disk: u64 = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum();
    assert_eq!(on_disk, 300_000 * done as u64);
    // what the failed one took is given back
    assert_eq!(done_name(&navigate(&mut s, 3, &format!("{origin}/attach"))), "report.pdf");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_directory_holds_a_bounded_number_of_files() {
    let origin = site();
    let d = files_with(1 << 20, 1 << 30, 2);
    let dir = d.dir.clone();
    let mut s = netd(d);
    assert_eq!(done_name(&navigate(&mut s, 1, &format!("{origin}/attach"))), "report.pdf");
    assert_eq!(done_name(&navigate(&mut s, 2, &format!("{origin}/attach"))), "report (1).pdf");
    let third = navigate(&mut s, 3, &format!("{origin}/attach"));
    assert!(third.last().unwrap().get_str(tag::MESSAGE).unwrap().contains("too many files"), "{third:?}");
    std::fs::remove_dir_all(&dir).unwrap();

    // a directory with more entries than the cap is full however small they are
    let d = files_with(1 << 20, 1 << 30, 2);
    let dir = d.dir.clone();
    for n in ["a", "b", "c"] {
        std::fs::write(dir.join(n), b"").unwrap();
    }
    let mut s = netd(d);
    assert_eq!(navigate(&mut s, 4, &format!("{origin}/attach")).last().unwrap().kind, kind::ERROR);
    assert_eq!(names(&dir), ["a", "b", "c"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn a_saved_file_is_quarantined() {
    unsafe extern "C" {
        fn getxattr(path: *const u8, name: *const u8, value: *mut u8, size: usize, position: u32, options: i32) -> isize;
    }
    let origin = site();
    let d = dir_with(1 << 20, 1 << 30);
    let dir = d.dir.clone();
    let mut s = netd(d);
    assert_eq!(done_name(&navigate(&mut s, 1, &format!("{origin}/attach"))), "report.pdf");
    let path = std::ffi::CString::new(dir.join("report.pdf").into_os_string().into_encoded_bytes()).unwrap();
    let mut buf = [0u8; 256];
    // SAFETY: both names are nul-terminated and the buffer is writable for its length
    let n = unsafe { getxattr(path.as_ptr().cast(), c"com.apple.quarantine".as_ptr().cast(), buf.as_mut_ptr(), buf.len(), 0, 0) };
    assert!(n > 0, "no quarantine attribute: {}", std::io::Error::last_os_error());
    let v = std::str::from_utf8(&buf[..n as usize]).unwrap();
    let parts: Vec<&str> = v.split(';').collect();
    assert_eq!((parts.len(), parts[0], parts[2], parts[3]), (4, "0083", "madcawl", ""), "{v}");
    let at = u64::from_str_radix(parts[1], 16).unwrap() as i64;
    assert!((at - madcawl_net::fetch::now_secs()).abs() < 60, "{v}");
    std::fs::remove_dir_all(&dir).unwrap();
}
