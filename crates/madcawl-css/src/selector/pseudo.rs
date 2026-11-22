//! The pseudo-classes without arguments and the pseudo-elements this
//! engine knows. A name not here makes the selector invalid, as it
//! does in every engine.

macro_rules! names {
    ($ty:ident { $($v:ident = $s:literal),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $ty {
            $($v),*
        }

        impl $ty {
            pub const ALL: &[$ty] = &[$($ty::$v),*];

            pub fn name(self) -> &'static str {
                match self {
                    $($ty::$v => $s),*
                }
            }

            /// The one named `name`, ASCII case-insensitively.
            pub fn from_name(name: &str) -> Option<$ty> {
                Self::ALL.iter().copied().find(|p| p.name().eq_ignore_ascii_case(name))
            }
        }
    };
}

names!(PseudoClass {
    Active = "active",
    AnyLink = "any-link",
    Autofill = "autofill",
    Checked = "checked",
    Default = "default",
    Defined = "defined",
    Disabled = "disabled",
    Empty = "empty",
    Enabled = "enabled",
    FirstChild = "first-child",
    FirstOfType = "first-of-type",
    Focus = "focus",
    FocusVisible = "focus-visible",
    FocusWithin = "focus-within",
    Fullscreen = "fullscreen",
    Hover = "hover",
    InRange = "in-range",
    Indeterminate = "indeterminate",
    Invalid = "invalid",
    LastChild = "last-child",
    LastOfType = "last-of-type",
    Link = "link",
    Modal = "modal",
    OnlyChild = "only-child",
    OnlyOfType = "only-of-type",
    Open = "open",
    Optional = "optional",
    OutOfRange = "out-of-range",
    PlaceholderShown = "placeholder-shown",
    PopoverOpen = "popover-open",
    ReadOnly = "read-only",
    ReadWrite = "read-write",
    Required = "required",
    Root = "root",
    Scope = "scope",
    Target = "target",
    UserInvalid = "user-invalid",
    UserValid = "user-valid",
    Valid = "valid",
    Visited = "visited",
});

names!(PseudoElement {
    After = "after",
    Backdrop = "backdrop",
    Before = "before",
    FileSelectorButton = "file-selector-button",
    FirstLetter = "first-letter",
    FirstLine = "first-line",
    Marker = "marker",
    Placeholder = "placeholder",
    Selection = "selection",
});

impl PseudoElement {
    /// The four CSS 2 wrote with one colon.
    pub fn legacy(self) -> bool {
        matches!(
            self,
            PseudoElement::After
                | PseudoElement::Before
                | PseudoElement::FirstLetter
                | PseudoElement::FirstLine
        )
    }
}

impl PseudoClass {
    /// The ones that may follow a pseudo-element.
    pub fn user_action(self) -> bool {
        matches!(
            self,
            PseudoClass::Hover
                | PseudoClass::Active
                | PseudoClass::Focus
                | PseudoClass::FocusVisible
                | PseudoClass::FocusWithin
        )
    }
}
