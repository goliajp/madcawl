//! The keyword-valued computed values of the box, text and flex
//! properties.

keywords!(BoxSizing {
    ContentBox = "content-box",
    BorderBox = "border-box",
});

keywords!(BorderStyle {
    None = "none",
    Hidden = "hidden",
    Dotted = "dotted",
    Dashed = "dashed",
    Solid = "solid",
    Double = "double",
    Groove = "groove",
    Ridge = "ridge",
    Inset = "inset",
    Outset = "outset",
});

keywords!(Overflow {
    Visible = "visible",
    Hidden = "hidden",
    Clip = "clip",
    Scroll = "scroll",
    Auto = "auto",
});

keywords!(Clear {
    None = "none",
    Left = "left",
    Right = "right",
    Both = "both",
    InlineStart = "inline-start",
    InlineEnd = "inline-end",
});

keywords!(WhiteSpace {
    Normal = "normal",
    Pre = "pre",
    Nowrap = "nowrap",
    PreWrap = "pre-wrap",
    PreLine = "pre-line",
    BreakSpaces = "break-spaces",
});

impl WhiteSpace {
    /// The two longhands the `white-space` keyword stands for.
    pub fn longhands(self) -> (WhiteSpaceCollapse, TextWrapMode) {
        use TextWrapMode::*;
        use WhiteSpaceCollapse::*;
        match self {
            WhiteSpace::Normal => (Collapse, Wrap),
            WhiteSpace::Nowrap => (Collapse, Nowrap),
            WhiteSpace::Pre => (Preserve, Nowrap),
            WhiteSpace::PreWrap => (Preserve, Wrap),
            WhiteSpace::PreLine => (PreserveBreaks, Wrap),
            WhiteSpace::BreakSpaces => (BreakSpaces, Wrap),
        }
    }

    /// The keyword two longhands make, when one does.
    pub fn of(c: WhiteSpaceCollapse, w: TextWrapMode) -> Option<WhiteSpace> {
        [
            WhiteSpace::Normal,
            WhiteSpace::Nowrap,
            WhiteSpace::Pre,
            WhiteSpace::PreWrap,
            WhiteSpace::PreLine,
            WhiteSpace::BreakSpaces,
        ]
        .into_iter()
        .find(|k| k.longhands() == (c, w))
    }
}

keywords!(WhiteSpaceCollapse {
    Collapse = "collapse",
    Preserve = "preserve",
    PreserveBreaks = "preserve-breaks",
    PreserveSpaces = "preserve-spaces",
    BreakSpaces = "break-spaces",
});

keywords!(TextWrapMode {
    Wrap = "wrap",
    Nowrap = "nowrap",
});

keywords!(TextAlign {
    Start = "start",
    End = "end",
    Left = "left",
    Right = "right",
    Center = "center",
    Justify = "justify",
    MatchParent = "match-parent",
    WebkitLeft = "-webkit-left",
    WebkitRight = "-webkit-right",
    WebkitCenter = "-webkit-center",
});

keywords!(Direction {
    Ltr = "ltr",
    Rtl = "rtl",
});

keywords!(WritingMode {
    HorizontalTb = "horizontal-tb",
    VerticalRl = "vertical-rl",
    VerticalLr = "vertical-lr",
    SidewaysRl = "sideways-rl",
    SidewaysLr = "sideways-lr",
});

impl WritingMode {
    /// Including the SVG 1.1 values, which map to these.
    pub fn parse(k: &str) -> Option<WritingMode> {
        Some(match k {
            "lr" | "lr-tb" | "rl" | "rl-tb" => WritingMode::HorizontalTb,
            "tb" | "tb-rl" => WritingMode::VerticalRl,
            _ => WritingMode::from_keyword(k)?,
        })
    }

    pub fn vertical(self) -> bool {
        self != WritingMode::HorizontalTb
    }
}

keywords!(ListStyleType {
    Disc = "disc",
    Circle = "circle",
    Square = "square",
    Decimal = "decimal",
    DecimalLeadingZero = "decimal-leading-zero",
    LowerRoman = "lower-roman",
    UpperRoman = "upper-roman",
    LowerGreek = "lower-greek",
    LowerAlpha = "lower-alpha",
    LowerLatin = "lower-latin",
    UpperAlpha = "upper-alpha",
    UpperLatin = "upper-latin",
    Armenian = "armenian",
    Georgian = "georgian",
    Hebrew = "hebrew",
    CjkDecimal = "cjk-decimal",
    CjkIdeographic = "cjk-ideographic",
    Hiragana = "hiragana",
    Katakana = "katakana",
    HiraganaIroha = "hiragana-iroha",
    KatakanaIroha = "katakana-iroha",
    DisclosureOpen = "disclosure-open",
    DisclosureClosed = "disclosure-closed",
    None = "none",
});

keywords!(ListStylePosition {
    Outside = "outside",
    Inside = "inside",
});

keywords!(FontStyle {
    Normal = "normal",
    Italic = "italic",
    Oblique = "oblique",
});

keywords!(FlexDirection {
    Row = "row",
    RowReverse = "row-reverse",
    Column = "column",
    ColumnReverse = "column-reverse",
});

keywords!(FlexWrap {
    Nowrap = "nowrap",
    Wrap = "wrap",
    WrapReverse = "wrap-reverse",
});

keywords!(
    /// The values of `justify-content` and the `align-*` properties,
    /// each property taking some of them.
    Align {
        Normal = "normal",
        Auto = "auto",
        Stretch = "stretch",
        Center = "center",
        Start = "start",
        End = "end",
        FlexStart = "flex-start",
        FlexEnd = "flex-end",
        SelfStart = "self-start",
        SelfEnd = "self-end",
        Left = "left",
        Right = "right",
        Baseline = "baseline",
        LastBaseline = "last baseline",
        SpaceBetween = "space-between",
        SpaceAround = "space-around",
        SpaceEvenly = "space-evenly",
    }
);

keywords!(VerticalAlign {
    Baseline = "baseline",
    Sub = "sub",
    Super = "super",
    TextTop = "text-top",
    TextBottom = "text-bottom",
    Middle = "middle",
    Top = "top",
    Bottom = "bottom",
    WebkitBaselineMiddle = "-webkit-baseline-middle",
});

keywords!(
    /// The keywords `width`, `height` and their minimums and maximums
    /// take besides lengths.
    SizeKw {
        Auto = "auto",
        None = "none",
        MinContent = "min-content",
        MaxContent = "max-content",
        FitContent = "fit-content",
        Stretch = "-webkit-fill-available",
        Content = "content",
    }
);
