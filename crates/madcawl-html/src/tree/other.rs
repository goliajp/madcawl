//! The "text", "in template", "after body" and frameset insertion modes.

use super::tag::Tag;
use super::{Class, Mode, Step, Tok, TreeBuilder, TreeSink};

impl<S: TreeSink> TreeBuilder<S> {
    pub(crate) fn text_mode(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(_, s) => self.insert_chars(s),
            Tok::Eof => {
                self.pop();
                self.mode = self.original_mode;
                return Step::Again;
            }
            Tok::End(_) => {
                let top = self.open.last().copied();
                self.pop();
                self.mode = self.original_mode;
                if self.pause_scripts
                    && let Some(t) = top.filter(|t| t.html(Tag::Script))
                {
                    self.script = Some(t.node);
                }
            }
            _ => {}
        }
        Step::Done
    }

    fn switch_template_mode(&mut self, mode: Mode) -> Step {
        self.template_modes.pop();
        self.template_modes.push(mode);
        self.mode = mode;
        Step::Again
    }

    pub(crate) fn in_template(&mut self, tok: Tok<'_>) -> Step {
        use Tag::*;
        match tok {
            Tok::Text(..) | Tok::Comment(_) | Tok::Pi(..) | Tok::Doctype(_) => self.in_body(tok),
            Tok::Start(s) => match s.name.tag {
                Base | Basefont | Bgsound | Link | Meta | Noframes | Script | Style | Template
                | Title => self.in_head(tok),
                Caption | Colgroup | Tbody | Tfoot | Thead => {
                    self.switch_template_mode(Mode::InTable)
                }
                Col => self.switch_template_mode(Mode::InColumnGroup),
                Tr => self.switch_template_mode(Mode::InTableBody),
                Td | Th => self.switch_template_mode(Mode::InRow),
                _ => self.switch_template_mode(Mode::InBody),
            },
            Tok::End(n) if n.tag == Template => self.in_head(tok),
            Tok::End(_) => Step::Done,
            Tok::Eof => {
                if !self.open.iter().any(|e| e.html(Template)) {
                    return Step::Done;
                }
                self.pop_until(|e| e.html(Template));
                self.clear_active_to_marker();
                self.template_modes.pop();
                self.reset_mode();
                Step::Again
            }
        }
    }

    pub(crate) fn after_body(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Space, _) => return self.in_body(tok),
            Tok::Comment(d) => self.insert_comment(d, Some((self.open[0].node, None))),
            Tok::Pi(t, d) => self.insert_pi(t, d, Some((self.open[0].node, None))),
            Tok::Doctype(_) | Tok::Eof => {}
            Tok::Start(s) if s.name.tag == Tag::Html => return self.in_body(tok),
            Tok::End(n) if n.tag == Tag::Html => {
                if self.context.is_none() {
                    self.mode = Mode::AfterAfterBody;
                }
            }
            _ => {
                self.mode = Mode::InBody;
                return Step::Again;
            }
        }
        Step::Done
    }

    pub(crate) fn in_frameset(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Space, s) => self.insert_chars(s),
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Start(s) => match s.name.tag {
                Tag::Html => return self.in_body(tok),
                Tag::Frameset => {
                    self.insert_html(&s);
                }
                Tag::Frame => self.insert_void(&s),
                Tag::Noframes => return self.in_head(tok),
                _ => {}
            },
            Tok::End(n) if n.tag == Tag::Frameset && self.open.len() > 1 => {
                self.pop();
                if self.context.is_none() && !self.current_is(Tag::Frameset) {
                    self.mode = Mode::AfterFrameset;
                }
            }
            _ => {}
        }
        Step::Done
    }

    pub(crate) fn after_frameset(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Space, s) => self.insert_chars(s),
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Start(s) if s.name.tag == Tag::Html => return self.in_body(tok),
            Tok::Start(s) if s.name.tag == Tag::Noframes => return self.in_head(tok),
            Tok::End(n) if n.tag == Tag::Html => self.mode = Mode::AfterAfterFrameset,
            _ => {}
        }
        Step::Done
    }

    pub(crate) fn after_after_body(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Comment(d) => self.insert_comment(d, Some((self.doc, None))),
            Tok::Pi(t, d) => self.insert_pi(t, d, Some((self.doc, None))),
            Tok::Doctype(_) | Tok::Text(Class::Space, _) => return self.in_body(tok),
            Tok::Start(s) if s.name.tag == Tag::Html => return self.in_body(tok),
            Tok::Eof => {}
            _ => {
                self.mode = Mode::InBody;
                return Step::Again;
            }
        }
        Step::Done
    }

    pub(crate) fn after_after_frameset(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Comment(d) => self.insert_comment(d, Some((self.doc, None))),
            Tok::Pi(t, d) => self.insert_pi(t, d, Some((self.doc, None))),
            Tok::Doctype(_) | Tok::Text(Class::Space, _) => return self.in_body(tok),
            Tok::Start(s) if s.name.tag == Tag::Html => return self.in_body(tok),
            Tok::Start(s) if s.name.tag == Tag::Noframes => return self.in_head(tok),
            _ => {}
        }
        Step::Done
    }

    /// The sink, for a script that runs while the parser waits.
    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    /// Have the tokenizer stop (`Step::Paused`) after each script
    /// element's end tag, as when scripting runs: the element is then
    /// [`TreeBuilder::take_script`]'s.
    pub fn pause_for_scripts(&mut self, on: bool) {
        self.pause_scripts = on;
    }

    /// The script element the parser stopped after, once.
    pub fn take_script(&mut self) -> Option<S::Handle> {
        self.script.take()
    }

    /// Stop parsing: every open element is done.
    pub(super) fn stop(&mut self) {
        self.flush_text();
        while !self.open.is_empty() {
            self.pop();
        }
        self.stopped = true;
    }
}
