//! Encodings: which one a document is in (HTML 13.2.3) and turning its
//! bytes into the WTF-8 the tokenizer reads.

mod decode;
mod labels;
mod prescan;

pub use decode::Decoder;
pub use prescan::{extract_from_content, prescan};

/// An encoding of the Encoding Standard, in the order of its table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Encoding {
    Utf8,
    Ibm866,
    Iso8859_2,
    Iso8859_3,
    Iso8859_4,
    Iso8859_5,
    Iso8859_6,
    Iso8859_7,
    Iso8859_8,
    Iso8859_8I,
    Iso8859_10,
    Iso8859_13,
    Iso8859_14,
    Iso8859_15,
    Iso8859_16,
    Koi8R,
    Koi8U,
    Macintosh,
    Windows874,
    Windows1250,
    Windows1251,
    Windows1252,
    Windows1253,
    Windows1254,
    Windows1255,
    Windows1256,
    Windows1257,
    Windows1258,
    XMacCyrillic,
    Gbk,
    Gb18030,
    Big5,
    EucJp,
    Iso2022Jp,
    ShiftJis,
    EucKr,
    Replacement,
    Utf16be,
    Utf16le,
    XUserDefined,
}

impl Encoding {
    /// The encoding's name, as the standard spells it.
    pub const fn name(self) -> &'static str {
        use Encoding::*;
        match self {
            Utf8 => "UTF-8",
            Ibm866 => "IBM866",
            Iso8859_2 => "ISO-8859-2",
            Iso8859_3 => "ISO-8859-3",
            Iso8859_4 => "ISO-8859-4",
            Iso8859_5 => "ISO-8859-5",
            Iso8859_6 => "ISO-8859-6",
            Iso8859_7 => "ISO-8859-7",
            Iso8859_8 => "ISO-8859-8",
            Iso8859_8I => "ISO-8859-8-I",
            Iso8859_10 => "ISO-8859-10",
            Iso8859_13 => "ISO-8859-13",
            Iso8859_14 => "ISO-8859-14",
            Iso8859_15 => "ISO-8859-15",
            Iso8859_16 => "ISO-8859-16",
            Koi8R => "KOI8-R",
            Koi8U => "KOI8-U",
            Macintosh => "macintosh",
            Windows874 => "windows-874",
            Windows1250 => "windows-1250",
            Windows1251 => "windows-1251",
            Windows1252 => "windows-1252",
            Windows1253 => "windows-1253",
            Windows1254 => "windows-1254",
            Windows1255 => "windows-1255",
            Windows1256 => "windows-1256",
            Windows1257 => "windows-1257",
            Windows1258 => "windows-1258",
            XMacCyrillic => "x-mac-cyrillic",
            Gbk => "GBK",
            Gb18030 => "gb18030",
            Big5 => "Big5",
            EucJp => "EUC-JP",
            Iso2022Jp => "ISO-2022-JP",
            ShiftJis => "Shift_JIS",
            EucKr => "EUC-KR",
            Replacement => "replacement",
            Utf16be => "UTF-16BE",
            Utf16le => "UTF-16LE",
            XUserDefined => "x-user-defined",
        }
    }

    /// The standard's "get an encoding": `label` without leading and
    /// trailing ASCII whitespace, matched without ASCII case. `None` is
    /// the standard's failure.
    pub fn for_label(label: &[u8]) -> Option<Encoding> {
        let label = label.trim_ascii();
        if label.len() > 32 {
            return None;
        }
        let mut lower = [0u8; 32];
        let lower = &mut lower[..label.len()];
        lower.copy_from_slice(label);
        lower.make_ascii_lowercase();
        let at = labels::LABELS
            .binary_search_by(|(l, _)| (*l).cmp(&*lower))
            .ok()?;
        Some(labels::LABELS[at].1)
    }

    pub(crate) fn is_utf16(self) -> bool {
        matches!(self, Encoding::Utf16be | Encoding::Utf16le)
    }
}

/// How sure the parser is of the encoding (13.2.3.1): a tentative one
/// may still change when the parser meets a `meta` that names another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    Tentative,
    Certain,
}

/// The encoding a byte order mark at the start of `bytes` names, and
/// how long the mark is. `None` when too few bytes have arrived to
/// tell and more may come.
pub(crate) fn bom(bytes: &[u8], done: bool) -> Option<Option<(Encoding, usize)>> {
    let found = match bytes {
        [0xEF, 0xBB, 0xBF, ..] => Some((Encoding::Utf8, 3)),
        [0xFE, 0xFF, ..] => Some((Encoding::Utf16be, 2)),
        [0xFF, 0xFE, ..] => Some((Encoding::Utf16le, 2)),
        [0xEF] | [0xEF, 0xBB] | [0xFE] | [0xFF] | [] if !done => return None,
        _ => None,
    };
    Some(found)
}
