//! Tag names the tree builder treats specially, as integers. Every
//! name is an atom; `Tag` is what the builder branches on.

use madcawl_str::{Atom, HeldAtoms, STATIC_ATOMS};

macro_rules! tags {
    ($($v:ident => $s:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum Tag {
            $($v,)*
            /// Any other name; `Name::atom` tells which.
            Other,
        }

        /// The atom of each tag but `Other`, in declaration order.
        static ATOMS: [Atom; Tag::Other as usize] = [$(madcawl_str::atom!($s),)*];

        /// The tag of each static atom.
        static TAGS_OF: [Tag; STATIC_ATOMS] = {
            let tags = [$(Tag::$v,)*];
            let mut out = [Tag::Other; STATIC_ATOMS];
            let mut i = 0;
            while i < tags.len() {
                out[ATOMS[i].index() as usize] = tags[i];
                i += 1;
            }
            out
        };
    };
}

impl Tag {
    pub(crate) fn of(atom: Atom) -> Tag {
        TAGS_OF
            .get(atom.index() as usize)
            .copied()
            .unwrap_or(Tag::Other)
    }

    /// The atom of a known tag; `Other` has none.
    pub(crate) fn atom(self) -> Atom {
        ATOMS[self as usize]
    }
}

tags! {
    A => "a",
    Address => "address",
    AnnotationXml => "annotation-xml",
    Applet => "applet",
    Area => "area",
    Article => "article",
    Aside => "aside",
    B => "b",
    Base => "base",
    Basefont => "basefont",
    Bgsound => "bgsound",
    Big => "big",
    Blockquote => "blockquote",
    Body => "body",
    Br => "br",
    Button => "button",
    Caption => "caption",
    Center => "center",
    Code => "code",
    Col => "col",
    Colgroup => "colgroup",
    Dd => "dd",
    Desc => "desc",
    Details => "details",
    Dialog => "dialog",
    Dir => "dir",
    Div => "div",
    Dl => "dl",
    Dt => "dt",
    Em => "em",
    Embed => "embed",
    Fieldset => "fieldset",
    Figcaption => "figcaption",
    Figure => "figure",
    Font => "font",
    Footer => "footer",
    ForeignObject => "foreignobject",
    Form => "form",
    Frame => "frame",
    Frameset => "frameset",
    H1 => "h1",
    H2 => "h2",
    H3 => "h3",
    H4 => "h4",
    H5 => "h5",
    H6 => "h6",
    Head => "head",
    Header => "header",
    Hgroup => "hgroup",
    Hr => "hr",
    Html => "html",
    I => "i",
    Iframe => "iframe",
    Image => "image",
    Img => "img",
    Input => "input",
    Keygen => "keygen",
    Label => "label",
    Li => "li",
    Link => "link",
    Listing => "listing",
    Main => "main",
    Malignmark => "malignmark",
    Marquee => "marquee",
    Math => "math",
    Menu => "menu",
    Meta => "meta",
    Mglyph => "mglyph",
    Mi => "mi",
    Mn => "mn",
    Mo => "mo",
    Ms => "ms",
    Mtext => "mtext",
    Nav => "nav",
    Nobr => "nobr",
    Noembed => "noembed",
    Noframes => "noframes",
    Noscript => "noscript",
    Object => "object",
    Ol => "ol",
    Optgroup => "optgroup",
    Option => "option",
    Output => "output",
    P => "p",
    Param => "param",
    Plaintext => "plaintext",
    Pre => "pre",
    Rb => "rb",
    Rp => "rp",
    Rt => "rt",
    Rtc => "rtc",
    Ruby => "ruby",
    S => "s",
    Script => "script",
    Search => "search",
    Section => "section",
    Select => "select",
    Small => "small",
    Source => "source",
    Span => "span",
    Strike => "strike",
    Strong => "strong",
    Style => "style",
    Sub => "sub",
    Summary => "summary",
    Sup => "sup",
    Svg => "svg",
    Table => "table",
    Tbody => "tbody",
    Td => "td",
    Template => "template",
    Textarea => "textarea",
    Tfoot => "tfoot",
    Th => "th",
    Thead => "thead",
    Title => "title",
    Tr => "tr",
    Track => "track",
    Tt => "tt",
    U => "u",
    Ul => "ul",
    Var => "var",
    Wbr => "wbr",
    Xmp => "xmp",
}

/// A local name: its atom, and the tag the builder knows it as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Name {
    pub(crate) tag: Tag,
    pub(crate) atom: Atom,
}

impl Name {
    pub(crate) fn known(tag: Tag) -> Name {
        Name {
            tag,
            atom: tag.atom(),
        }
    }

    pub(crate) fn of(atom: Atom) -> Name {
        Name {
            tag: Tag::of(atom),
            atom,
        }
    }

    /// The name of a tag token, held in `names` if it is not static.
    pub(crate) fn of_tag(t: &crate::token::Tag, names: &HeldAtoms) -> Name {
        Name::of(t.atom.unwrap_or_else(|| names.hold(&t.name)))
    }

    /// The name a tag token for an element named `atom` would have.
    pub(crate) fn lowercase(atom: Atom, names: &HeldAtoms) -> Name {
        let mut lower = Vec::new();
        atom.write_wtf8(&mut lower);
        lower.make_ascii_lowercase();
        Name::of(names.hold(&lower))
    }
}

impl From<Tag> for Name {
    fn from(tag: Tag) -> Name {
        Name::known(tag)
    }
}
