//! Which cookie partition a connection's frames use. The host puts each
//! registration in a partition (the profile's own jar unless it names
//! one); a content process may repeat that partition in a frame but never
//! name another, and its frames are always fetched in its registration's.

use madcawl_net::partition::{PROFILE, PartitionId};
use madcawl_net::wire::{Frame, kind, tag};

/// The partition's value as a frame carries it.
fn encoded(p: PartitionId) -> Vec<u8> {
    Frame::new(kind::FETCH).num(tag::PARTITION, p).fields.swap_remove(0).1
}

/// The frame with one `PARTITION` value, `own`; refused when it named any
/// other.
pub(super) fn screen(own: PartitionId, mut frame: Frame) -> Result<Frame, &'static str> {
    if frame.field(tag::PARTITION).is_none() {
        return Ok(frame.num(tag::PARTITION, own));
    }
    let want = encoded(own);
    if frame.all(tag::PARTITION).any(|v| v != want.as_slice()) {
        return Err("a cookie partition other than its own");
    }
    frame.fields.retain(|(t, _)| *t != tag::PARTITION);
    Ok(frame.num(tag::PARTITION, own))
}

/// The partition a REGISTER puts its document in: `PROFILE` when it
/// names none; `Err` when a value is not a number or the values differ.
pub(super) fn of_register(f: &Frame) -> Result<PartitionId, ()> {
    let Some(first) = f.field(tag::PARTITION) else { return Ok(PROFILE) };
    let p = f.get_num(tag::PARTITION).ok_or(())?;
    let want = encoded(p);
    match first == want.as_slice() && f.all(tag::PARTITION).all(|v| v == want.as_slice()) {
        true => Ok(p),
        false => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(values: &[u64]) -> Frame {
        values.iter().fold(Frame::new(kind::FETCH).str(tag::URL, "http://127.0.0.1/"), |f, &v| f.num(tag::PARTITION, v))
    }

    #[test]
    fn a_frame_is_put_in_its_registrations_partition_and_may_name_no_other() {
        for (values, own) in [(&[][..], 7), (&[7][..], 7), (&[7, 7][..], 7), (&[][..], PROFILE), (&[300][..], 300)] {
            let f = screen(own, fetch(values)).unwrap();
            assert_eq!(f.all(tag::PARTITION).collect::<Vec<_>>(), [encoded(own).as_slice()], "{values:?}");
        }
        for (values, own) in [(&[8][..], 7), (&[7, 8][..], 7), (&[0][..], 7), (&[7][..], PROFILE)] {
            assert!(screen(own, fetch(values)).is_err(), "{values:?} in {own}");
        }
        // the same number, written in more bytes than it takes, is another claim
        let long = Frame::new(kind::FETCH).bytes(tag::PARTITION, &[0x87, 0x00]);
        assert!(screen(7, long).is_err());
    }

    #[test]
    fn a_register_naming_a_partition_it_does_not_spell_out_is_refused() {
        let reg = || Frame::new(kind::REGISTER).num(tag::NAVIGATION, 1);
        assert_eq!(of_register(&reg()), Ok(PROFILE));
        assert_eq!(of_register(&reg().num(tag::PARTITION, 7).num(tag::PARTITION, 7)), Ok(7));
        for bad in [
            reg().bytes(tag::PARTITION, &[0x80]),
            reg().bytes(tag::PARTITION, &[]),
            reg().bytes(tag::PARTITION, &[0x87, 0x00]),
            reg().num(tag::PARTITION, 7).num(tag::PARTITION, 8),
        ] {
            assert_eq!(of_register(&bad), Err(()), "{bad:?}");
        }
    }
}
