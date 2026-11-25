//! Roles: the ARIA roles a record can have, by the names ARIA (and the
//! HTML-AAM's `sectionheader`, `sectionfooter`) gives them, and what
//! each role means to the views.

macro_rules! roles {
    ($($v:ident = $s:literal),* $(,)?) => {
        /// A record's role; `None` is a node with no role at all (a
        /// comment, a doctype, an element the tree leaves out).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
        #[repr(u8)]
        pub enum Role {
            #[default]
            None,
            $($v),*
        }

        impl Role {
            const ALL: &'static [Role] = &[$(Role::$v),*];

            /// The role's name as ARIA writes it; "" for `None`.
            pub fn name(self) -> &'static str {
                match self {
                    Role::None => "",
                    $(Role::$v => $s),*
                }
            }
        }
    };
}

roles! {
    Alert = "alert", AlertDialog = "alertdialog", Application = "application",
    Article = "article", Banner = "banner", Blockquote = "blockquote", Button = "button",
    Caption = "caption", Cell = "cell", Checkbox = "checkbox", Code = "code",
    ColumnHeader = "columnheader", Combobox = "combobox", Complementary = "complementary",
    ContentInfo = "contentinfo", Definition = "definition", Deletion = "deletion",
    Dialog = "dialog", Directory = "directory", Document = "document", Emphasis = "emphasis",
    Feed = "feed", Figure = "figure", Form = "form", Generic = "generic", Grid = "grid",
    GridCell = "gridcell", Group = "group", Heading = "heading", Image = "image",
    Insertion = "insertion", Link = "link", List = "list", ListBox = "listbox",
    ListItem = "listitem", Log = "log", Main = "main", Mark = "mark", Marquee = "marquee",
    Math = "math", Menu = "menu", MenuBar = "menubar", MenuItem = "menuitem",
    MenuItemCheckbox = "menuitemcheckbox", MenuItemRadio = "menuitemradio", Meter = "meter",
    Navigation = "navigation", Presentation = "none", Note = "note", Option = "option",
    Paragraph = "paragraph", ProgressBar = "progressbar", Radio = "radio",
    RadioGroup = "radiogroup", Region = "region", Row = "row", RowGroup = "rowgroup",
    RowHeader = "rowheader", ScrollBar = "scrollbar", Search = "search", SearchBox = "searchbox",
    SectionFooter = "sectionfooter", SectionHeader = "sectionheader", Separator = "separator",
    Slider = "slider", SpinButton = "spinbutton", Status = "status", Strong = "strong",
    Subscript = "subscript", Superscript = "superscript", Switch = "switch", Tab = "tab",
    Table = "table", TabList = "tablist", TabPanel = "tabpanel", Term = "term",
    TextBox = "textbox", Time = "time", Timer = "timer", Toolbar = "toolbar",
    Tooltip = "tooltip", Tree = "tree", TreeGrid = "treegrid", TreeItem = "treeitem",
    Text = "text",
}

impl Role {
    /// The concrete role an ARIA `role` token names; ARIA 1.2's
    /// synonyms map to their newer names. Abstract roles name none.
    pub fn from_aria(token: &str) -> Option<Role> {
        let t = token.to_ascii_lowercase();
        let r = match t.as_str() {
            "img" => Role::Image,
            "presentation" | "none" => Role::Presentation,
            "text" | "" => return None,
            s => *Role::ALL.iter().find(|r| r.name() == s)?,
        };
        Some(r)
    }

    /// The node's name may come from what it contains (accname step
    /// 2F): the roles ARIA marks "name from: contents".
    pub fn name_from_content(self) -> bool {
        use Role::*;
        matches!(
            self,
            Button
                | Cell
                | Checkbox
                | ColumnHeader
                | GridCell
                | Heading
                | Link
                | MenuItem
                | MenuItemCheckbox
                | MenuItemRadio
                | Option
                | Radio
                | Row
                | RowHeader
                | Switch
                | Tab
                | Tooltip
                | TreeItem
                | Text
        )
    }

    /// Something an agent acts on: it gets a reference in every view.
    pub fn interactive(self) -> bool {
        use Role::*;
        matches!(
            self,
            Button
                | Checkbox
                | Combobox
                | Link
                | ListBox
                | MenuItem
                | MenuItemCheckbox
                | MenuItemRadio
                | Option
                | Radio
                | SearchBox
                | Slider
                | SpinButton
                | Switch
                | Tab
                | TextBox
                | TreeItem
                | GridCell
                | ScrollBar
        )
    }

    /// A landmark the outline lists.
    pub fn landmark(self) -> bool {
        use Role::*;
        matches!(
            self,
            Banner
                | Complementary
                | ContentInfo
                | Form
                | Main
                | Navigation
                | Region
                | Search
                | Dialog
                | AlertDialog
                | Article
        )
    }

    /// The role adds nothing a reader needs: its children stand in its
    /// place in the views.
    pub fn transparent(self) -> bool {
        matches!(self, Role::None | Role::Generic | Role::Presentation)
    }

    /// An ARIA name never comes from an author for these (ARIA's
    /// "name prohibited").
    pub fn name_prohibited(self) -> bool {
        use Role::*;
        matches!(
            self,
            Caption
                | Code
                | Definition
                | Deletion
                | Emphasis
                | Generic
                | Insertion
                | Mark
                | Paragraph
                | Presentation
                | Strong
                | Subscript
                | Superscript
                | Term
                | Time
        )
    }

    /// Lists whose items the views fold past the first few.
    pub fn many_children(self) -> bool {
        use Role::*;
        matches!(
            self,
            List | ListBox
                | Menu
                | MenuBar
                | Table
                | Grid
                | TreeGrid
                | RowGroup
                | Tree
                | Feed
                | TabList
                | RadioGroup
                | Toolbar
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aria_tokens_name_concrete_roles() {
        assert_eq!(Role::from_aria("BUTTON"), Some(Role::Button));
        assert_eq!(Role::from_aria("img"), Some(Role::Image));
        assert_eq!(Role::from_aria("presentation"), Some(Role::Presentation));
        assert_eq!(Role::from_aria("widget"), None, "abstract");
        assert_eq!(Role::Presentation.name(), "none");
        assert!(Role::ALL.len() < 255, "fits a byte");
    }
}
