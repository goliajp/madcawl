//! The "in body" insertion mode: end tags.

use super::stack::{Scope, is_heading, is_special};
use super::tag::{Name, Tag};
use super::{Mode, Namespace, Start, Step, Tok, TreeBuilder, TreeSink};

impl<S: TreeSink> TreeBuilder<S> {
    pub(crate) fn body_end(&mut self, n: Name, tok: Tok<'_>) -> Step {
        use Tag::*;
        match n.tag {
            Template => return self.in_head(tok),
            Body | Html => {
                if !self.has_in_scope(Body) {
                    return Step::Done;
                }
                self.mode = Mode::AfterBody;
                if n.tag == Html {
                    return Step::Again;
                }
            }
            Address | Article | Aside | Blockquote | Button | Center | Details | Dialog | Dir
            | Div | Dl | Fieldset | Figcaption | Figure | Footer | Header | Hgroup | Listing
            | Main | Menu | Nav | Ol | Pre | Search | Section | Select | Summary | Ul => {
                self.close_in_scope(n, None)
            }
            Applet | Marquee | Object => {
                if self.has_in_scope(n.tag) {
                    self.close_in_scope(n, None);
                    self.clear_active_to_marker();
                }
            }
            Form => self.end_form(),
            P => {
                if !self.in_scope(Scope::Button, n) {
                    self.insert_fake(P);
                }
                self.close_p();
            }
            Li => {
                if self.in_scope(Scope::ListItem, n) {
                    self.generate_implied(Some(Li));
                    self.pop_until_html(n);
                }
            }
            Dd | Dt => self.close_in_scope(n, Some(n.tag)),
            H1 | H2 | H3 | H4 | H5 | H6 => {
                if self.in_scope_by(Scope::Default, is_heading) {
                    self.generate_implied(None);
                    self.pop_until(is_heading);
                }
            }
            A | B | Big | Code | Em | Font | I | Nobr | S | Small | Strike | Strong | Tt | U => {
                if !self.adoption_agency(n.tag) {
                    self.any_other_end(n);
                }
            }
            Br => {
                self.reconstruct_active();
                self.insert_void(&Start::fake(Br));
                self.frameset_ok = false;
            }
            _ => self.any_other_end(n),
        }
        Step::Done
    }

    /// If an HTML element named `n` is in scope, generate implied end
    /// tags (but for `except`) and pop it.
    fn close_in_scope(&mut self, n: Name, except: Option<Tag>) {
        if self.in_scope(Scope::Default, n) {
            self.generate_implied(except);
            self.pop_until_html(n);
        }
    }

    fn end_form(&mut self) {
        if self.in_template_contents() {
            if self.has_in_scope(Tag::Form) {
                self.generate_implied(None);
                self.pop_until(|e| e.html(Tag::Form));
            }
            return;
        }
        let Some(node) = self.form.take() else {
            return;
        };
        if !self.in_scope_by(Scope::Default, |e| e.node == node) {
            return;
        }
        self.generate_implied(None);
        if let Some(i) = self.open_index(node) {
            self.remove_open(i);
        }
    }

    pub(crate) fn any_other_end(&mut self, n: Name) {
        for i in (0..self.open.len()).rev() {
            step!(1);
            let e = self.open[i];
            if e.ns == Namespace::Html && e.name == n {
                self.generate_implied(Some(n.tag));
                while self.open.len() > i {
                    self.pop();
                }
                return;
            }
            if is_special(&e) {
                return;
            }
        }
    }
}
