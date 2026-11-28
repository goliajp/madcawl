use super::*;
use crate::shm::Header;

fn trip(f: &Frame) -> Frame {
    let mut b = Vec::new();
    f.encode(&mut b);
    Frame::decode(&b).unwrap().0
}

fn buffer(w: u32, h: u32) -> Buffer {
    let head = Header::new(w, h, Header::needed(w, h));
    Buffer { bytes: head.size, width: w, height: h, stride: head.stride }
}

fn surface() -> Surface {
    Surface { epoch: 3, scale: 200, buffers: vec![buffer(200, 100), buffer(200, 100)] }
}

fn commit() -> Commit {
    Commit { epoch: 3, seq: 5, buffer: 1, dirty: PxRect { x: 0, y: 0, w: 200, h: 100 }, doc_width: 100, doc_height: 900 }
}

#[test]
fn frames_round_trip() {
    let s = surface();
    assert_eq!(Surface::from_frame(&trip(&s.frame()), 2), Ok(s));
    let c = commit();
    assert_eq!(Commit::from_frame(&trip(&c.frame())), Ok(c));
    assert_eq!(released_seq(&trip(&released(9))), Ok(9));
}

#[test]
fn unknown_kinds_and_tags_are_skipped() {
    let mut b = Vec::new();
    Frame::new(999).bytes(1, b"from a newer peer").encode(&mut b);
    commit().frame().num(250, 3).bytes(251, &[0; 40]).encode(&mut b);
    let (first, used) = Frame::decode(&b).unwrap();
    assert_eq!(first.kind, 999, "a reader sees the kind and passes over it");
    let (second, _) = Frame::decode(&b[used..]).unwrap();
    assert_eq!(Commit::from_frame(&second), Ok(commit()));
}

#[test]
fn a_surface_out_of_range_is_refused() {
    let s = surface();
    assert_eq!(Surface::from_frame(&s.frame(), 1), Err(Bad::Fds));
    assert_eq!(Surface::from_frame(&s.frame(), 3), Err(Bad::Fds));
    let three = Surface { buffers: vec![buffer(8, 8); 3], ..surface() };
    assert_eq!(Surface::from_frame(&three.frame(), 3), Err(Bad::Value(tag::BUFFER)));
    let none = Surface { buffers: vec![], ..surface() };
    assert_eq!(Surface::from_frame(&none.frame(), 0), Err(Bad::Missing(tag::BUFFER)));
    let mixed = Surface { buffers: vec![buffer(8, 8), buffer(16, 8)], ..surface() };
    assert_eq!(Surface::from_frame(&mixed.frame(), 2), Err(Bad::Value(tag::BUFFER)));
    let small = Surface { buffers: vec![Buffer { bytes: 64, ..buffer(8, 8) }], ..surface() };
    assert_eq!(Surface::from_frame(&small.frame(), 1), Err(Bad::Value(tag::BUFFER)));
    let odd = Surface { buffers: vec![Buffer { stride: 40, ..buffer(8, 8) }], ..surface() };
    assert_eq!(Surface::from_frame(&odd.frame(), 1), Err(Bad::Value(tag::BUFFER)));
    let scale = Surface { scale: 10, ..surface() };
    assert_eq!(Surface::from_frame(&scale.frame(), 2), Err(Bad::Value(tag::SCALE)));
    let short = Frame::new(kind::SURFACE).num(tag::EPOCH, 1).num(tag::SCALE, 100).bytes(tag::BUFFER, &[0; 19]);
    assert_eq!(Surface::from_frame(&short, 1), Err(Bad::Value(tag::BUFFER)));
}

#[test]
fn a_header_must_be_the_one_the_surface_states() {
    let s = surface();
    let b = s.buffers[0];
    let mut head = vec![0u8; shm::HEADER_LEN];
    Header::new(b.width, b.height, b.bytes).write(&mut head);
    assert_eq!(s.check_header(0, &head, b.bytes), Ok(()));
    assert!(s.check_header(0, &head, b.bytes - 1).is_err());
    Header::new(b.width - 4, b.height, b.bytes).write(&mut head);
    assert_eq!(s.check_header(0, &head, b.bytes), Err(HeaderError::Mapped));
}

#[test]
fn every_out_of_range_commit_is_refused() {
    let s = surface();
    let ok = commit();
    assert_eq!(ok.check(&s, 4, Some(0)), Ok(()));
    assert_eq!(ok.check(&s, 0, None), Ok(()));
    let with = |f: &dyn Fn(&mut Commit)| {
        let mut c = commit();
        f(&mut c);
        c.check(&s, 4, Some(0))
    };
    assert_eq!(ok.check(&s, 5, None), Err(CommitError::Seq), "the same sequence again");
    assert_eq!(ok.check(&s, 6, None), Err(CommitError::Seq), "an older sequence");
    assert_eq!(with(&|c| c.buffer = 2), Err(CommitError::NoBuffer));
    assert_eq!(with(&|c| c.buffer = u32::MAX), Err(CommitError::NoBuffer));
    assert_eq!(ok.check(&s, 4, Some(1)), Err(CommitError::Busy));
    assert_eq!(with(&|c| c.dirty.w = 0), Err(CommitError::Dirty));
    assert_eq!(with(&|c| c.dirty.h = 0), Err(CommitError::Dirty));
    assert_eq!(with(&|c| c.dirty.x = 1), Err(CommitError::Dirty));
    assert_eq!(with(&|c| c.dirty.y = 1), Err(CommitError::Dirty));
    assert_eq!(with(&|c| c.dirty.w = 201), Err(CommitError::Dirty));
    assert_eq!(with(&|c| c.dirty.h = 101), Err(CommitError::Dirty));
    assert_eq!(with(&|c| c.dirty = PxRect { x: u32::MAX, y: 0, w: 2, h: 1 }), Err(CommitError::Dirty), "no overflow");
    assert_eq!(with(&|c| c.dirty = PxRect { x: 0, y: u32::MAX, w: 1, h: u32::MAX }), Err(CommitError::Dirty));
    assert_eq!(with(&|c| c.doc_width = MAX_DOC + 1), Err(CommitError::Doc));
    assert_eq!(with(&|c| c.doc_height = MAX_DOC + 1), Err(CommitError::Doc));
    assert_eq!(with(&|c| c.dirty = PxRect { x: 10, y: 20, w: 5, h: 5 }), Ok(()), "a part of the buffer");
}

#[test]
fn a_commit_drawn_into_another_surface_is_refused() {
    let s = surface();
    let older = Commit { epoch: s.epoch - 1, ..commit() };
    assert_eq!(older.check(&s, 0, None), Err(CommitError::Epoch), "drawn before the new surface came");
    let newer = Commit { epoch: s.epoch + 1, ..commit() };
    assert_eq!(newer.check(&s, 0, None), Err(CommitError::Epoch), "a surface never sent");
    // the epoch is checked first: the same buffer index and sequence would pass otherwise
    let next = Surface { epoch: s.epoch + 1, ..surface() };
    assert_eq!(Commit { epoch: next.epoch, ..commit() }.check(&next, 0, None), Ok(()));
    assert_eq!(commit().check(&next, 0, None), Err(CommitError::Epoch));
}

#[test]
fn a_surface_or_commit_without_an_epoch_is_refused() {
    let mut f = surface().frame();
    f.fields.retain(|(t, _)| *t != tag::EPOCH);
    assert_eq!(Surface::from_frame(&f, 2), Err(Bad::Missing(tag::EPOCH)));
    let mut f = commit().frame();
    f.fields.retain(|(t, _)| *t != tag::EPOCH);
    assert_eq!(Commit::from_frame(&f), Err(Bad::Missing(tag::EPOCH)));
}

#[test]
fn a_malformed_commit_is_refused_before_it_is_checked() {
    let mut f = commit().frame();
    f.fields.retain(|(t, _)| *t != tag::SEQ);
    assert_eq!(Commit::from_frame(&f), Err(Bad::Missing(tag::SEQ)));
    let mut f = commit().frame();
    f.fields.retain(|(t, _)| *t != tag::DIRTY);
    assert_eq!(Commit::from_frame(&f.clone().bytes(tag::DIRTY, &[0; 15])), Err(Bad::Value(tag::DIRTY)));
    assert_eq!(Commit::from_frame(&f), Err(Bad::Missing(tag::DIRTY)));
    let mut f = commit().frame();
    f.fields.retain(|(t, _)| *t != tag::BUFFER);
    assert_eq!(Commit::from_frame(&f.num(tag::BUFFER, 1 << 33)), Err(Bad::Value(tag::BUFFER)));
    assert_eq!(Commit::from_frame(&released(1)), Err(Bad::Kind(kind::RELEASED)));
    assert_eq!(released_seq(&commit().frame()), Err(Bad::Kind(kind::COMMIT)));
}
