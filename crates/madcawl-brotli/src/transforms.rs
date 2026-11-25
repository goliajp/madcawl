//! The 121 word transforms of RFC 7932 appendix B: prefix, elementary
//! transform, suffix

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Identity,
    FermentFirst,
    FermentAll,
    OmitFirst(u8),
    OmitLast(u8),
}

use Kind::*;

pub(crate) const TRANSFORMS: [(&[u8], Kind, &[u8]); 121] = [
    (b"", Identity, b""),
    (b"", Identity, b" "),
    (b" ", Identity, b" "),
    (b"", OmitFirst(1), b""),
    (b"", FermentFirst, b" "),
    (b"", Identity, b" the "),
    (b" ", Identity, b""),
    (b"s ", Identity, b" "),
    (b"", Identity, b" of "),
    (b"", FermentFirst, b""),
    (b"", Identity, b" and "),
    (b"", OmitFirst(2), b""),
    (b"", OmitLast(1), b""),
    (b", ", Identity, b" "),
    (b"", Identity, b", "),
    (b" ", FermentFirst, b" "),
    (b"", Identity, b" in "),
    (b"", Identity, b" to "),
    (b"e ", Identity, b" "),
    (b"", Identity, b"\""),
    (b"", Identity, b"."),
    (b"", Identity, b"\">"),
    (b"", Identity, b"\n"),
    (b"", OmitLast(3), b""),
    (b"", Identity, b"]"),
    (b"", Identity, b" for "),
    (b"", OmitFirst(3), b""),
    (b"", OmitLast(2), b""),
    (b"", Identity, b" a "),
    (b"", Identity, b" that "),
    (b" ", FermentFirst, b""),
    (b"", Identity, b". "),
    (b".", Identity, b""),
    (b" ", Identity, b", "),
    (b"", OmitFirst(4), b""),
    (b"", Identity, b" with "),
    (b"", Identity, b"'"),
    (b"", Identity, b" from "),
    (b"", Identity, b" by "),
    (b"", OmitFirst(5), b""),
    (b"", OmitFirst(6), b""),
    (b" the ", Identity, b""),
    (b"", OmitLast(4), b""),
    (b"", Identity, b". The "),
    (b"", FermentAll, b""),
    (b"", Identity, b" on "),
    (b"", Identity, b" as "),
    (b"", Identity, b" is "),
    (b"", OmitLast(7), b""),
    (b"", OmitLast(1), b"ing "),
    (b"", Identity, b"\n\t"),
    (b"", Identity, b":"),
    (b" ", Identity, b". "),
    (b"", Identity, b"ed "),
    (b"", OmitFirst(9), b""),
    (b"", OmitFirst(7), b""),
    (b"", OmitLast(6), b""),
    (b"", Identity, b"("),
    (b"", FermentFirst, b", "),
    (b"", OmitLast(8), b""),
    (b"", Identity, b" at "),
    (b"", Identity, b"ly "),
    (b" the ", Identity, b" of "),
    (b"", OmitLast(5), b""),
    (b"", OmitLast(9), b""),
    (b" ", FermentFirst, b", "),
    (b"", FermentFirst, b"\""),
    (b".", Identity, b"("),
    (b"", FermentAll, b" "),
    (b"", FermentFirst, b"\">"),
    (b"", Identity, b"=\""),
    (b" ", Identity, b"."),
    (b".com/", Identity, b""),
    (b" the ", Identity, b" of the "),
    (b"", FermentFirst, b"'"),
    (b"", Identity, b". This "),
    (b"", Identity, b","),
    (b".", Identity, b" "),
    (b"", FermentFirst, b"("),
    (b"", FermentFirst, b"."),
    (b"", Identity, b" not "),
    (b" ", Identity, b"=\""),
    (b"", Identity, b"er "),
    (b" ", FermentAll, b" "),
    (b"", Identity, b"al "),
    (b" ", FermentAll, b""),
    (b"", Identity, b"='"),
    (b"", FermentAll, b"\""),
    (b"", FermentFirst, b". "),
    (b" ", Identity, b"("),
    (b"", Identity, b"ful "),
    (b" ", FermentFirst, b". "),
    (b"", Identity, b"ive "),
    (b"", Identity, b"less "),
    (b"", FermentAll, b"'"),
    (b"", Identity, b"est "),
    (b" ", FermentFirst, b"."),
    (b"", FermentAll, b"\">"),
    (b" ", Identity, b"='"),
    (b"", FermentFirst, b","),
    (b"", Identity, b"ize "),
    (b"", FermentAll, b"."),
    (b"\xc2\xa0", Identity, b""),
    (b" ", Identity, b","),
    (b"", FermentFirst, b"=\""),
    (b"", FermentAll, b"=\""),
    (b"", Identity, b"ous "),
    (b"", FermentAll, b", "),
    (b"", FermentFirst, b"='"),
    (b" ", FermentFirst, b","),
    (b" ", FermentAll, b"=\""),
    (b" ", FermentAll, b", "),
    (b"", FermentAll, b","),
    (b"", FermentAll, b"("),
    (b"", FermentAll, b". "),
    (b" ", FermentAll, b"."),
    (b"", FermentAll, b"='"),
    (b" ", FermentAll, b". "),
    (b" ", FermentFirst, b"=\""),
    (b" ", FermentAll, b"='"),
    (b" ", FermentFirst, b"='"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transforms_have_the_published_check_value() {
        // each as prefix, 0, a byte for the elementary transform, suffix, 0
        let mut v = Vec::new();
        for (prefix, kind, suffix) in TRANSFORMS {
            v.extend_from_slice(prefix);
            v.push(0);
            v.push(match kind {
                Identity => 0,
                FermentFirst => 1,
                FermentAll => 2,
                OmitFirst(k) => 2 + k,
                OmitLast(k) => 11 + k,
            });
            v.extend_from_slice(suffix);
            v.push(0);
        }
        assert_eq!(v.len(), 648);
        assert_eq!(crate::tables::crc32(&v), 0x3d96_5f81);
    }
}
