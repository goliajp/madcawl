//! The table insertion modes.

use super::stack::Scope;
use super::tag::{Name, Tag};
use super::{Class, Mode, Namespace, Step, Tok, TreeBuilder, TreeSink, class_of};

const TABLE_CONTEXT: &[Tag] = &[Tag::Table, Tag::Template, Tag::Html];
const TABLE_BODY_CONTEXT: &[Tag] = &[Tag::Tbody, Tag::Tfoot, Tag::Thead, Tag::Template, Tag::Html];
const TABLE_ROW_CONTEXT: &[Tag] = &[Tag::Tr, Tag::Template, Tag::Html];

impl<S: TreeSink> TreeBuilder<S> {
    fn table_scope(&self, tag: Tag) -> bool {
        self.in_scope(Scope::Table, Name::known(tag))
    }

    pub(crate) fn in_table(&mut self, tok: Tok<'_>) -> Step {
        use Tag::*;
        match tok {
            Tok::Text(..)
                if self.open.last().is_some_and(|e| {
                    e.ns == Namespace::Html
                        && matches!(e.name.tag, Table | Tbody | Template | Tfoot | Thead | Tr)
                }) =>
            {
                self.table_text.clear();
                self.table_text_dirty = false;
                self.original_mode = self.mode;
                self.mode = Mode::InTableText;
                return Step::Again;
            }
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Doctype(_) => {}
            Tok::Start(s) => match s.name.tag {
                Caption => {
                    self.clear_back_to(TABLE_CONTEXT);
                    self.push_marker();
                    self.insert_html(&s);
                    self.mode = Mode::InCaption;
                }
                Colgroup => {
                    self.clear_back_to(TABLE_CONTEXT);
                    self.insert_html(&s);
                    self.mode = Mode::InColumnGroup;
                }
                Col => return self.table_insert_fake(Colgroup, Mode::InColumnGroup),
                Tbody | Tfoot | Thead => {
                    self.clear_back_to(TABLE_CONTEXT);
                    self.insert_html(&s);
                    self.mode = Mode::InTableBody;
                }
                Td | Th | Tr => return self.table_insert_fake(Tbody, Mode::InTableBody),
                Table => {
                    if self.table_scope(Table) {
                        self.pop_until(|e| e.html(Table));
                        self.reset_mode();
                        return Step::Again;
                    }
                }
                Style | Script | Template => return self.in_head(tok),
                Input
                    if s.attr(b"type")
                        .is_some_and(|t| t.eq_ignore_ascii_case(b"hidden")) =>
                {
                    self.insert_void(&s)
                }
                Form => {
                    let in_template = self.in_template_contents();
                    if self.form.is_none() || in_template {
                        let el = self.insert_html(&s);
                        if !in_template {
                            self.form = Some(el);
                        }
                        self.pop();
                    }
                }
                _ => return self.foster_in_body(tok),
            },
            Tok::End(n) => match n.tag {
                Table => {
                    if self.table_scope(Table) {
                        self.pop_until(|e| e.html(Table));
                        self.reset_mode();
                    }
                }
                Body | Caption | Col | Colgroup | Html | Tbody | Td | Tfoot | Th | Thead | Tr => {}
                Template => return self.in_head(tok),
                _ => return self.foster_in_body(tok),
            },
            Tok::Eof => return self.in_body(tok),
            Tok::Text(..) => return self.foster_in_body(tok),
        }
        Step::Done
    }

    fn table_insert_fake(&mut self, tag: Tag, mode: Mode) -> Step {
        self.clear_back_to(TABLE_CONTEXT);
        self.insert_fake(tag);
        self.mode = mode;
        Step::Again
    }

    fn foster_in_body(&mut self, tok: Tok<'_>) -> Step {
        self.foster = true;
        let step = self.in_body(tok);
        self.foster = false;
        step
    }

    pub(crate) fn in_table_text(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Nul, _) => return Step::Done,
            Tok::Text(class, s) => {
                self.table_text.extend_from_slice(s);
                self.table_text_dirty |= class == Class::Other;
                return Step::Done;
            }
            _ => {}
        }
        let text = std::mem::take(&mut self.table_text);
        if self.table_text_dirty {
            let mut s = &text[..];
            while let Some(&first) = s.first() {
                let class = class_of(first);
                let n = s
                    .iter()
                    .position(|&b| class_of(b) != class)
                    .unwrap_or(s.len());
                let _ = self.foster_in_body(Tok::Text(class, &s[..n]));
                s = &s[n..];
            }
        } else {
            self.insert_chars(&text);
        }
        self.table_text = text;
        self.flush_text();
        self.mode = self.original_mode;
        Step::Again
    }

    pub(crate) fn in_caption(&mut self, tok: Tok<'_>) -> Step {
        use Tag::*;
        let reprocess = match tok {
            Tok::End(n) if n.tag == Caption => false,
            Tok::End(n) if n.tag == Table => true,
            Tok::Start(s)
                if matches!(
                    s.name.tag,
                    Caption | Col | Colgroup | Tbody | Td | Tfoot | Th | Thead | Tr
                ) =>
            {
                true
            }
            Tok::End(n)
                if matches!(
                    n.tag,
                    Body | Col | Colgroup | Html | Tbody | Td | Tfoot | Th | Thead | Tr
                ) =>
            {
                return Step::Done;
            }
            _ => return self.in_body(tok),
        };
        if !self.table_scope(Caption) {
            return Step::Done;
        }
        self.generate_implied(None);
        self.pop_until(|e| e.html(Caption));
        self.clear_active_to_marker();
        self.mode = Mode::InTable;
        if reprocess { Step::Again } else { Step::Done }
    }

    pub(crate) fn in_column_group(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Space, s) => self.insert_chars(s),
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Doctype(_) => {}
            Tok::Start(s) if s.name.tag == Tag::Html => return self.in_body(tok),
            Tok::Start(s) if s.name.tag == Tag::Col => self.insert_void(&s),
            Tok::End(n) if n.tag == Tag::Colgroup => {
                if self.current_is(Tag::Colgroup) {
                    self.pop();
                    self.mode = Mode::InTable;
                }
            }
            Tok::End(n) if n.tag == Tag::Col => {}
            Tok::Start(s) if s.name.tag == Tag::Template => return self.in_head(tok),
            Tok::End(n) if n.tag == Tag::Template => return self.in_head(tok),
            Tok::Eof => return self.in_body(tok),
            _ => {
                if self.current_is(Tag::Colgroup) {
                    self.pop();
                    self.mode = Mode::InTable;
                    return Step::Again;
                }
            }
        }
        Step::Done
    }

    pub(crate) fn in_table_body(&mut self, tok: Tok<'_>) -> Step {
        use Tag::*;
        match tok {
            Tok::Start(s) if s.name.tag == Tr => {
                self.clear_back_to(TABLE_BODY_CONTEXT);
                self.insert_html(&s);
                self.mode = Mode::InRow;
            }
            Tok::Start(s) if matches!(s.name.tag, Th | Td) => {
                self.clear_back_to(TABLE_BODY_CONTEXT);
                self.insert_fake(Tr);
                self.mode = Mode::InRow;
                return Step::Again;
            }
            Tok::End(n) if matches!(n.tag, Tbody | Tfoot | Thead) => {
                if self.table_scope(n.tag) {
                    self.clear_back_to(TABLE_BODY_CONTEXT);
                    self.pop();
                    self.mode = Mode::InTable;
                }
            }
            Tok::Start(s)
                if matches!(s.name.tag, Caption | Col | Colgroup | Tbody | Tfoot | Thead) =>
            {
                return self.leave_table_body();
            }
            Tok::End(n) if n.tag == Table => return self.leave_table_body(),
            Tok::End(n)
                if matches!(n.tag, Body | Caption | Col | Colgroup | Html | Td | Th | Tr) => {}
            _ => return self.in_table(tok),
        }
        Step::Done
    }

    fn leave_table_body(&mut self) -> Step {
        if !self.in_scope_by(Scope::Table, |e| {
            e.html(Tag::Tbody) || e.html(Tag::Thead) || e.html(Tag::Tfoot)
        }) {
            return Step::Done;
        }
        self.clear_back_to(TABLE_BODY_CONTEXT);
        self.pop();
        self.mode = Mode::InTable;
        Step::Again
    }

    pub(crate) fn in_row(&mut self, tok: Tok<'_>) -> Step {
        use Tag::*;
        match tok {
            Tok::Start(s) if matches!(s.name.tag, Th | Td) => {
                self.clear_back_to(TABLE_ROW_CONTEXT);
                self.insert_html(&s);
                self.mode = Mode::InCell;
                self.push_marker();
            }
            Tok::End(n) if n.tag == Tr => {
                if self.table_scope(Tr) {
                    self.clear_back_to(TABLE_ROW_CONTEXT);
                    self.pop();
                    self.mode = Mode::InTableBody;
                }
            }
            Tok::Start(s)
                if matches!(
                    s.name.tag,
                    Caption | Col | Colgroup | Tbody | Tfoot | Thead | Tr
                ) =>
            {
                return self.leave_row();
            }
            Tok::End(n) if n.tag == Table => return self.leave_row(),
            Tok::End(n) if matches!(n.tag, Tbody | Tfoot | Thead) => {
                if self.table_scope(n.tag) {
                    return self.leave_row();
                }
            }
            Tok::End(n) if matches!(n.tag, Body | Caption | Col | Colgroup | Html | Td | Th) => {}
            _ => return self.in_table(tok),
        }
        Step::Done
    }

    fn leave_row(&mut self) -> Step {
        if !self.table_scope(Tag::Tr) {
            return Step::Done;
        }
        self.clear_back_to(TABLE_ROW_CONTEXT);
        self.pop();
        self.mode = Mode::InTableBody;
        Step::Again
    }

    pub(crate) fn in_cell(&mut self, tok: Tok<'_>) -> Step {
        use Tag::*;
        match tok {
            Tok::End(n) if matches!(n.tag, Td | Th) => {
                if self.table_scope(n.tag) {
                    self.generate_implied(None);
                    self.pop_until_html(n);
                    self.clear_active_to_marker();
                    self.mode = Mode::InRow;
                }
                Step::Done
            }
            Tok::Start(s)
                if matches!(
                    s.name.tag,
                    Caption | Col | Colgroup | Tbody | Td | Tfoot | Th | Thead | Tr
                ) =>
            {
                if self.in_scope_by(Scope::Table, |e| e.html(Td) || e.html(Th)) {
                    self.close_cell();
                    return Step::Again;
                }
                Step::Done
            }
            Tok::End(n) if matches!(n.tag, Body | Caption | Col | Colgroup | Html) => Step::Done,
            Tok::End(n) if matches!(n.tag, Table | Tbody | Tfoot | Thead | Tr) => {
                if self.table_scope(n.tag) {
                    self.close_cell();
                    return Step::Again;
                }
                Step::Done
            }
            _ => self.in_body(tok),
        }
    }

    fn close_cell(&mut self) {
        self.generate_implied(None);
        self.pop_until(|e| e.html(Tag::Td) || e.html(Tag::Th));
        self.clear_active_to_marker();
        self.mode = Mode::InRow;
    }
}
