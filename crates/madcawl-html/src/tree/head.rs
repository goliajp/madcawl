//! The insertion modes up to the body: "initial" through "after head".

use super::quirks::doctype_mode;
use super::tag::{Name, Tag};
use super::{Class, Mode, Namespace, Open, Start, Step, Tok, TreeBuilder, TreeSink};
use crate::State;

impl<S: TreeSink> TreeBuilder<S> {
    fn set_quirks(&mut self, mode: super::QuirksMode) {
        self.quirks = mode;
        self.sink.set_quirks_mode(mode);
    }

    pub(crate) fn initial(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Space, _) => {}
            Tok::Comment(d) => self.insert_comment(d, Some((self.doc, None))),
            Tok::Pi(t, d) => self.insert_pi(t, d, Some((self.doc, None))),
            Tok::Doctype(d) => {
                fn part(p: &Option<Vec<u8>>) -> &[u8] {
                    p.as_deref().unwrap_or(b"")
                }
                self.sink
                    .append_doctype(part(&d.name), part(&d.public_id), part(&d.system_id));
                if !self.opts.iframe_srcdoc {
                    self.set_quirks(doctype_mode(d));
                }
                self.mode = Mode::BeforeHtml;
            }
            _ => {
                if !self.opts.iframe_srcdoc {
                    self.set_quirks(super::QuirksMode::Quirks);
                }
                self.mode = Mode::BeforeHtml;
                return Step::Again;
            }
        }
        Step::Done
    }

    fn insert_root(&mut self, s: &Start<'_>) {
        let el = self.create(s, Namespace::Html, self.doc);
        self.sink.append(self.doc, el);
        self.open.push(Open {
            node: el,
            ns: Namespace::Html,
            name: Name::known(Tag::Html),
            html_ip: false,
        });
        self.mode = Mode::BeforeHead;
    }

    pub(crate) fn before_html(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Doctype(_) | Tok::Text(Class::Space, _) => {}
            Tok::Comment(d) => self.insert_comment(d, Some((self.doc, None))),
            Tok::Pi(t, d) => self.insert_pi(t, d, Some((self.doc, None))),
            Tok::Start(s) if s.name.tag == Tag::Html => self.insert_root(&s),
            Tok::End(n) if !matches!(n.tag, Tag::Head | Tag::Body | Tag::Html | Tag::Br) => {}
            _ => {
                self.insert_root(&Start::fake(Tag::Html));
                return Step::Again;
            }
        }
        Step::Done
    }

    pub(crate) fn before_head(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Doctype(_) | Tok::Text(Class::Space, _) => {}
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Start(s) if s.name.tag == Tag::Html => return self.in_body(tok),
            Tok::Start(s) if s.name.tag == Tag::Head => {
                self.head = Some(self.insert_html(&s));
                self.mode = Mode::InHead;
            }
            Tok::End(n) if !matches!(n.tag, Tag::Head | Tag::Body | Tag::Html | Tag::Br) => {}
            _ => {
                self.head = Some(self.insert_fake(Tag::Head));
                self.mode = Mode::InHead;
                return Step::Again;
            }
        }
        Step::Done
    }

    pub(crate) fn in_head(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Space, s) => self.insert_chars(s),
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Doctype(_) => {}
            Tok::Start(s) => match s.name.tag {
                Tag::Html => return self.in_body(tok),
                Tag::Base | Tag::Basefont | Tag::Bgsound | Tag::Link | Tag::Meta => {
                    self.insert_void(&s)
                }
                Tag::Title => self.parse_text_element(&s, State::Rcdata),
                Tag::Noscript if self.opts.scripting => self.parse_text_element(&s, State::Rawtext),
                Tag::Noframes | Tag::Style => self.parse_text_element(&s, State::Rawtext),
                Tag::Noscript => {
                    self.insert_html(&s);
                    self.mode = Mode::InHeadNoscript;
                }
                Tag::Script => self.parse_text_element(&s, State::ScriptData),
                Tag::Template => {
                    self.push_marker();
                    self.frameset_ok = false;
                    self.mode = Mode::InTemplate;
                    self.template_modes.push(Mode::InTemplate);
                    self.insert_html(&s);
                }
                Tag::Head => {}
                _ => return self.leave_head(),
            },
            Tok::End(n) => match n.tag {
                Tag::Head => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                }
                Tag::Body | Tag::Html | Tag::Br => return self.leave_head(),
                Tag::Template => self.end_template(),
                _ => {}
            },
            _ => return self.leave_head(),
        }
        Step::Done
    }

    fn leave_head(&mut self) -> Step {
        self.pop();
        self.mode = Mode::AfterHead;
        Step::Again
    }

    pub(crate) fn end_template(&mut self) {
        if !self.open.iter().any(|e| e.html(Tag::Template)) {
            return;
        }
        self.generate_implied_thoroughly();
        self.pop_until(|e| e.html(Tag::Template));
        self.clear_active_to_marker();
        self.template_modes.pop();
        self.reset_mode();
    }

    pub(crate) fn in_head_noscript(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Doctype(_) => Step::Done,
            Tok::Start(s) if s.name.tag == Tag::Html => self.in_body(tok),
            Tok::End(n) if n.tag == Tag::Noscript => {
                self.pop();
                self.mode = Mode::InHead;
                Step::Done
            }
            Tok::Text(Class::Space, _) | Tok::Comment(_) | Tok::Pi(..) => self.in_head(tok),
            Tok::Start(s)
                if matches!(
                    s.name.tag,
                    Tag::Basefont
                        | Tag::Bgsound
                        | Tag::Link
                        | Tag::Meta
                        | Tag::Noframes
                        | Tag::Style
                ) =>
            {
                self.in_head(tok)
            }
            Tok::Start(s) if matches!(s.name.tag, Tag::Head | Tag::Noscript) => Step::Done,
            Tok::End(n) if n.tag != Tag::Br => Step::Done,
            _ => {
                self.pop();
                self.mode = Mode::InHead;
                Step::Again
            }
        }
    }

    pub(crate) fn after_head(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Space, s) => self.insert_chars(s),
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Doctype(_) => {}
            Tok::Start(s) => match s.name.tag {
                Tag::Html => return self.in_body(tok),
                Tag::Body => {
                    self.insert_html(&s);
                    self.frameset_ok = false;
                    self.mode = Mode::InBody;
                }
                Tag::Frameset => {
                    self.insert_html(&s);
                    self.mode = Mode::InFrameset;
                }
                Tag::Base
                | Tag::Basefont
                | Tag::Bgsound
                | Tag::Link
                | Tag::Meta
                | Tag::Noframes
                | Tag::Script
                | Tag::Style
                | Tag::Template
                | Tag::Title => {
                    let head = self.head.expect("a head element");
                    self.open.push(Open {
                        node: head,
                        ns: Namespace::Html,
                        name: Name::known(Tag::Head),
                        html_ip: false,
                    });
                    let step = self.in_head(tok);
                    if let Some(i) = self.open_index(head) {
                        self.remove_open(i);
                    }
                    return step;
                }
                Tag::Head => {}
                _ => return self.leave_after_head(),
            },
            Tok::End(n) => match n.tag {
                Tag::Template => return self.in_head(tok),
                Tag::Body | Tag::Html | Tag::Br => return self.leave_after_head(),
                _ => {}
            },
            _ => return self.leave_after_head(),
        }
        Step::Done
    }

    fn leave_after_head(&mut self) -> Step {
        self.insert_fake(Tag::Body);
        self.frameset_ok = true;
        self.mode = Mode::InBody;
        Step::Again
    }
}
