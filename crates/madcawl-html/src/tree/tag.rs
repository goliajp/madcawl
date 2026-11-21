//! Tag names the tree builder treats specially, as integers, and the
//! rest interned so that names compare as integers too.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

macro_rules! tags {
    ($($v:ident => $s:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum Tag {
            $($v,)*
            /// Any other name; `Name::atom` tells which.
            Other,
        }

        impl Tag {
            pub(crate) fn from_bytes(s: &[u8]) -> Tag {
                match s {
                    $($s => Tag::$v,)*
                    _ => Tag::Other,
                }
            }

            /// The name as a token spells it. `Other` has none.
            pub(crate) fn as_bytes(self) -> &'static [u8] {
                match self {
                    $(Tag::$v => $s,)*
                    Tag::Other => b"",
                }
            }
        }
    };
}

tags! {
    A => b"a",
    Address => b"address",
    AnnotationXml => b"annotation-xml",
    Applet => b"applet",
    Area => b"area",
    Article => b"article",
    Aside => b"aside",
    B => b"b",
    Base => b"base",
    Basefont => b"basefont",
    Bgsound => b"bgsound",
    Big => b"big",
    Blockquote => b"blockquote",
    Body => b"body",
    Br => b"br",
    Button => b"button",
    Caption => b"caption",
    Center => b"center",
    Code => b"code",
    Col => b"col",
    Colgroup => b"colgroup",
    Dd => b"dd",
    Desc => b"desc",
    Details => b"details",
    Dialog => b"dialog",
    Dir => b"dir",
    Div => b"div",
    Dl => b"dl",
    Dt => b"dt",
    Em => b"em",
    Embed => b"embed",
    Fieldset => b"fieldset",
    Figcaption => b"figcaption",
    Figure => b"figure",
    Font => b"font",
    Footer => b"footer",
    ForeignObject => b"foreignobject",
    Form => b"form",
    Frame => b"frame",
    Frameset => b"frameset",
    H1 => b"h1",
    H2 => b"h2",
    H3 => b"h3",
    H4 => b"h4",
    H5 => b"h5",
    H6 => b"h6",
    Head => b"head",
    Header => b"header",
    Hgroup => b"hgroup",
    Hr => b"hr",
    Html => b"html",
    I => b"i",
    Iframe => b"iframe",
    Image => b"image",
    Img => b"img",
    Input => b"input",
    Keygen => b"keygen",
    Label => b"label",
    Li => b"li",
    Link => b"link",
    Listing => b"listing",
    Main => b"main",
    Malignmark => b"malignmark",
    Marquee => b"marquee",
    Math => b"math",
    Menu => b"menu",
    Meta => b"meta",
    Mglyph => b"mglyph",
    Mi => b"mi",
    Mn => b"mn",
    Mo => b"mo",
    Ms => b"ms",
    Mtext => b"mtext",
    Nav => b"nav",
    Nobr => b"nobr",
    Noembed => b"noembed",
    Noframes => b"noframes",
    Noscript => b"noscript",
    Object => b"object",
    Ol => b"ol",
    Optgroup => b"optgroup",
    Option => b"option",
    Output => b"output",
    P => b"p",
    Param => b"param",
    Plaintext => b"plaintext",
    Pre => b"pre",
    Rb => b"rb",
    Rp => b"rp",
    Rt => b"rt",
    Rtc => b"rtc",
    Ruby => b"ruby",
    S => b"s",
    Script => b"script",
    Search => b"search",
    Section => b"section",
    Select => b"select",
    Small => b"small",
    Source => b"source",
    Span => b"span",
    Strike => b"strike",
    Strong => b"strong",
    Style => b"style",
    Sub => b"sub",
    Summary => b"summary",
    Sup => b"sup",
    Svg => b"svg",
    Table => b"table",
    Tbody => b"tbody",
    Td => b"td",
    Template => b"template",
    Textarea => b"textarea",
    Tfoot => b"tfoot",
    Th => b"th",
    Thead => b"thead",
    Title => b"title",
    Tr => b"tr",
    Track => b"track",
    Tt => b"tt",
    U => b"u",
    Ul => b"ul",
    Var => b"var",
    Wbr => b"wbr",
    Xmp => b"xmp",
}

/// A local name: a known tag, or an interned other one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Name {
    pub(crate) tag: Tag,
    pub(crate) atom: u32,
}

/// The atom of a name never interned, which matches no element.
const NO_ATOM: u32 = u32::MAX;

impl Name {
    pub(crate) const fn known(tag: Tag) -> Name {
        Name { tag, atom: 0 }
    }
}

impl From<Tag> for Name {
    fn from(tag: Tag) -> Name {
        Name::known(tag)
    }
}

#[derive(Default)]
struct Fx(u64);

impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
}

/// Names of elements that are not known tags. It grows with the
/// number of distinct names in one document, which the input bounds.
#[derive(Default)]
pub(crate) struct Atoms {
    map: HashMap<Box<[u8]>, u32, BuildHasherDefault<Fx>>,
}

impl Atoms {
    /// The name of a start tag, interning it if it is new.
    pub(crate) fn intern(&mut self, s: &[u8]) -> Name {
        let tag = Tag::from_bytes(s);
        if tag != Tag::Other {
            return Name::known(tag);
        }
        if let Some(&atom) = self.map.get(s) {
            return Name { tag, atom };
        }
        let atom = self.map.len() as u32 + 1;
        self.map.insert(s.into(), atom);
        Name { tag, atom }
    }

    /// The name of an end tag: one never interned closes nothing.
    pub(crate) fn lookup(&self, s: &[u8]) -> Name {
        let tag = Tag::from_bytes(s);
        if tag != Tag::Other {
            return Name::known(tag);
        }
        let atom = self.map.get(s).copied().unwrap_or(NO_ATOM);
        Name { tag, atom }
    }
}
