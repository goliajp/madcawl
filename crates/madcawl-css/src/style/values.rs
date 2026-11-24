//! The computed values of the properties the cascade computes, and how
//! `getComputedStyle` writes each.

macro_rules! keywords {
    ($(#[$m:meta])* $ty:ident { $($v:ident = $s:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $ty {
            $($v),*
        }

        impl $ty {
            pub fn css(self) -> &'static str {
                match self {
                    $($ty::$v => $s),*
                }
            }

            /// The one an ASCII-lowercase keyword names.
            pub fn from_keyword(k: &str) -> Option<$ty> {
                match k {
                    $($s => Some($ty::$v),)*
                    _ => None,
                }
            }
        }
    };
}

keywords!(Display {
    None = "none",
    Contents = "contents",
    Block = "block",
    Inline = "inline",
    InlineBlock = "inline-block",
    FlowRoot = "flow-root",
    ListItem = "list-item",
    InlineListItem = "inline list-item",
    Table = "table",
    InlineTable = "inline-table",
    TableRowGroup = "table-row-group",
    TableHeaderGroup = "table-header-group",
    TableFooterGroup = "table-footer-group",
    TableRow = "table-row",
    TableCell = "table-cell",
    TableColumnGroup = "table-column-group",
    TableColumn = "table-column",
    TableCaption = "table-caption",
    Flex = "flex",
    InlineFlex = "inline-flex",
    Grid = "grid",
    InlineGrid = "inline-grid",
    Ruby = "ruby",
    BlockRuby = "block ruby",
    RubyText = "ruby-text",
    WebkitBox = "-webkit-box",
    WebkitInlineBox = "-webkit-inline-box",
});

impl Display {
    /// The value a box gets when it must be block-level (CSS Display,
    /// "blockify").
    pub fn blockified(self) -> Display {
        use Display::*;
        match self {
            Inline | InlineBlock | TableRowGroup | TableHeaderGroup | TableFooterGroup
            | TableRow | TableCell | TableColumnGroup | TableColumn | TableCaption | RubyText => {
                Block
            }
            InlineListItem => ListItem,
            InlineTable => Table,
            InlineFlex => Flex,
            InlineGrid => Grid,
            Ruby => BlockRuby,
            WebkitInlineBox => WebkitBox,
            v => v,
        }
    }

    /// Whether its children are flex or grid items, which are
    /// blockified.
    pub fn lays_out_items(self) -> bool {
        use Display::*;
        matches!(
            self,
            Flex | InlineFlex | Grid | InlineGrid | WebkitBox | WebkitInlineBox
        )
    }

    /// Whether its box can take size containment, which
    /// `content-visibility: hidden` needs to skip its contents: not with
    /// no box, a non-atomic inline, or an internal table or ruby box
    /// but a cell (CSS Containment, "size containment").
    pub fn can_contain_size(self) -> bool {
        use Display::*;
        !matches!(
            self,
            None | Contents
                | Inline
                | InlineListItem
                | Ruby
                | RubyText
                | TableRowGroup
                | TableHeaderGroup
                | TableFooterGroup
                | TableRow
                | TableColumnGroup
                | TableColumn
        )
    }

    /// The legacy and multi-keyword forms (`block flow`, `inline
    /// flex list-item` and so on) as one value; `None` when the
    /// combination is not one.
    pub(crate) fn from_keywords(ks: &[&str]) -> Option<Display> {
        if let [k] = ks
            && let Some(d) = Display::from_keyword(k)
            && d != Display::InlineListItem
            && d != Display::BlockRuby
        {
            return Some(d);
        }
        let (mut outer, mut inner, mut list) = (None, None, false);
        for &k in ks {
            match k {
                "block" | "inline" | "run-in" if outer.is_none() => outer = Some(k),
                "flow" | "flow-root" | "table" | "flex" | "grid" | "ruby" if inner.is_none() => {
                    inner = Some(k)
                }
                "list-item" if !list => list = true,
                _ => return None,
            }
        }
        let inline = match outer {
            Some("run-in") => return None,
            Some(o) => o == "inline",
            None => inner == Some("ruby"),
        };
        Display::combine(inline, inner.unwrap_or("flow"), list)
    }

    fn combine(inline: bool, inner: &str, list: bool) -> Option<Display> {
        use Display::*;
        Some(match (inline, inner, list) {
            (false, "flow" | "flow-root", true) => ListItem,
            (true, "flow", true) => InlineListItem,
            (_, _, true) => return Option::None,
            (false, "flow", _) => Block,
            (true, "flow", _) => Inline,
            (false, "flow-root", _) => FlowRoot,
            (true, "flow-root", _) => InlineBlock,
            (false, "table", _) => Table,
            (true, "table", _) => InlineTable,
            (false, "flex", _) => Flex,
            (true, "flex", _) => InlineFlex,
            (false, "grid", _) => Grid,
            (true, "grid", _) => InlineGrid,
            (false, "ruby", _) => BlockRuby,
            (true, "ruby", _) => Ruby,
            _ => return Option::None,
        })
    }
}

keywords!(Visibility {
    Visible = "visible",
    Hidden = "hidden",
    Collapse = "collapse",
});

keywords!(ContentVisibility {
    Visible = "visible",
    Auto = "auto",
    Hidden = "hidden",
});

keywords!(Position {
    Static = "static",
    Relative = "relative",
    Absolute = "absolute",
    Fixed = "fixed",
    Sticky = "sticky",
});

keywords!(Float {
    None = "none",
    Left = "left",
    Right = "right",
    InlineStart = "inline-start",
    InlineEnd = "inline-end",
});

/// One element's computed values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedStyle {
    pub display: Display,
    pub visibility: Visibility,
    pub content_visibility: ContentVisibility,
    pub position: Position,
    pub float: Float,
    /// `opacity` as `f32` bits, from 0 to 1
    opacity: u32,
}

impl ComputedStyle {
    pub const INITIAL: ComputedStyle = ComputedStyle {
        display: Display::Inline,
        visibility: Visibility::Visible,
        content_visibility: ContentVisibility::Visible,
        position: Position::Static,
        float: Float::None,
        opacity: 0x3f80_0000,
    };

    pub fn opacity(&self) -> f32 {
        f32::from_bits(self.opacity)
    }

    pub(crate) fn set_opacity(&mut self, v: f32) {
        self.opacity = v.clamp(0.0, 1.0).to_bits();
    }
}
