use super::*;
use crate::shm::Header;
use std::os::fd::AsFd as _;

#[test]
fn a_buffer_is_shared_through_a_passed_descriptor() {
    let (a, b) = UnixStream::pair().unwrap();
    let len = Header::needed(4, 2);
    let fd = shm_create(len).unwrap();
    let mut mine = Mapping::map(fd.as_fd(), len).unwrap();
    Header::new(4, 2, len).write(mine.bytes_mut());
    send(&a, b"surface", &[fd.as_fd()]).unwrap();
    drop(fd);
    let mut buf = [0u8; 64];
    let mut fds = Vec::new();
    let n = recv(&b, &mut buf, &mut fds).unwrap();
    assert_eq!((&buf[..n], fds.len()), (&b"surface"[..], 1));
    let mut theirs = Mapping::map(fds[0].as_fd(), len).unwrap();
    assert_eq!(Header::read(theirs.bytes(), len), Ok(Header::new(4, 2, len)));
    theirs.bytes_mut()[HEADER_LEN] = 0xab;
    theirs.set_generation(7);
    assert_eq!((mine.bytes()[HEADER_LEN], mine.generation()), (0xab, 7));
}

#[test]
fn a_mapping_larger_than_the_object_is_refused() {
    let fd = shm_create(4096).unwrap();
    assert!(Mapping::map(fd.as_fd(), 1 << 20).is_err());
    assert!(Mapping::map(fd.as_fd(), 8).is_err(), "shorter than a header");
}

#[test]
fn a_message_with_no_descriptors_and_a_long_one_arrive_whole() {
    let (a, b) = UnixStream::pair().unwrap();
    send(&a, b"plain", &[]).unwrap();
    let mut buf = [0u8; 16];
    let mut fds = Vec::new();
    assert_eq!(recv(&b, &mut buf, &mut fds).unwrap(), 5);
    assert!(fds.is_empty());
    let big = vec![7u8; 1 << 20];
    let fd = shm_create(64).unwrap();
    let t = std::thread::spawn(move || send(&a, &big, &[fd.as_fd()]).unwrap());
    let mut got = 0;
    let mut chunk = vec![0u8; 1 << 16];
    while got < 1 << 20 {
        got += recv(&b, &mut chunk, &mut fds).unwrap();
    }
    t.join().unwrap();
    assert_eq!((got, fds.len()), (1 << 20, 1));
}

#[test]
fn too_many_descriptors_in_one_message_is_an_error() {
    let (a, b) = UnixStream::pair().unwrap();
    let fds: Vec<OwnedFd> = (0..MAX_FDS + 2).map(|_| shm_create(64).unwrap()).collect();
    // sent by hand past the limit `send` keeps to
    let raw: Vec<i32> = fds.iter().map(|f| f.as_raw_fd()).collect();
    let mut control = vec![0u8; HDR + align(raw.len() * 4)];
    let len = HDR + raw.len() * 4;
    let w = std::mem::size_of::<CmsgLen>();
    control[..w].copy_from_slice(&(len as CmsgLen).to_ne_bytes());
    control[w..w + 4].copy_from_slice(&c::SOL_SOCKET.to_ne_bytes());
    control[w + 4..w + 8].copy_from_slice(&SCM_RIGHTS.to_ne_bytes());
    for (i, fd) in raw.iter().enumerate() {
        control[HDR + i * 4..HDR + i * 4 + 4].copy_from_slice(&fd.to_ne_bytes());
    }
    let mut iov = [IoSlice::new(b"x")];
    let msg = MsgHdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: iov.as_mut_ptr().cast(),
        iovlen: 1,
        control: control.as_mut_ptr(),
        controllen: control.len() as _,
        flags: 0,
    };
    // SAFETY: the header and what it points at live for the call
    assert_eq!(unsafe { sendmsg(a.as_raw_fd(), &msg, 0) }, 1);
    let mut buf = [0u8; 4];
    let mut got = Vec::new();
    assert!(recv(&b, &mut buf, &mut got).is_err());
    assert!(got.is_empty());
}
