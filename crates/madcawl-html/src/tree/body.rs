//! The "in body" insertion mode: characters and start tags.

use super::sink::{Adjust, Attrs};
use super::stack::{is_heading, is_special};
use super::tag::{Name, Tag};
use super::{Class, Mode, Namespace, Start, Step, Tok, TreeBuilder, TreeSink};
use crate::State;

impl<S: TreeSink> TreeBuilder<S> {
    pub(crate) fn in_body(&mut self, tok: Tok<'_>) -> Step {
        match tok {
            Tok::Text(Class::Nul, _) | Tok::Doctype(_) => {}
            Tok::Text(class, s) => {
                self.reconstruct_active();
                self.insert_chars(s);
                if class == Class::Other {
                    self.frameset_ok = false;
                }
            }
            Tok::Comment(d) => self.insert_comment(d, None),
            Tok::Pi(t, d) => self.insert_pi(t, d, None),
            Tok::Start(s) => return self.body_start(&s, tok),
            Tok::End(n) => return self.body_end(n, tok),
            Tok::Eof => {
                if !self.template_modes.is_empty() {
                    return self.in_template(tok);
                }
            }
        }
        Step::Done
    }

    fn has_template(&self) -> bool {
        self.open.iter().any(|e| e.html(Tag::Template))
    }

    fn body_start(&mut self, s: &Start<'_>, tok: Tok<'_>) -> Step {
        use Tag::*;
        match s.name.tag {
            Html => {
                if !self.has_template() {
                    let html = self.open[0].node;
                    self.sink
                        .add_attrs_if_missing(html, Attrs::new(s.attrs, Adjust::Html));
                }
            }
            Base | Basefont | Bgsound | Link | Meta | Noframes | Script | Style | Template
            | Title => return self.in_head(tok),
            Body => self.start_body(s),
            Frameset => self.start_frameset(s),
            Address | Article | Aside | Blockquote | Center | Details | Dialog | Dir | Div | Dl
            | Fieldset | Figcaption | Figure | Footer | Header | Hgroup | Main | Menu | Nav
            | Ol | P | Search | Section | Summary | Ul => {
                self.close_p_in_button_scope();
                self.insert_html(s);
            }
            H1 | H2 | H3 | H4 | H5 | H6 => {
                self.close_p_in_button_scope();
                if is_heading(self.current()) {
                    self.pop();
                }
                self.insert_html(s);
            }
            Pre | Listing => {
                self.close_p_in_button_scope();
                self.insert_html(s);
                self.skip_lf = true;
                self.frameset_ok = false;
            }
            Form => self.start_form(s),
            Li | Dd | Dt => self.start_list_item(s),
            Plaintext => {
                self.close_p_in_button_scope();
                self.insert_html(s);
                self.next_state = Some(State::Plaintext);
            }
            Button => {
                if self.has_in_scope(Button) {
                    self.generate_implied(None);
                    self.pop_until(|e| e.html(Button));
                }
                self.reconstruct_active();
                self.insert_html(s);
                self.frameset_ok = false;
            }
            A | B | Big | Code | Em | Font | I | S | Small | Strike | Strong | Tt | U | Nobr => {
                self.start_formatting(s)
            }
            Applet | Marquee | Object => {
                self.reconstruct_active();
                self.insert_html(s);
                self.push_marker();
                self.frameset_ok = false;
            }
            Table => {
                if self.quirks != super::QuirksMode::Quirks {
                    self.close_p_in_button_scope();
                }
                self.insert_html(s);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
            }
            _ => self.body_start_more(s),
        }
        Step::Done
    }

    fn body_start_more(&mut self, s: &Start<'_>) {
        use Tag::*;
        match s.name.tag {
            Area | Br | Embed | Img | Keygen | Wbr => {
                self.reconstruct_active();
                self.insert_void(s);
                self.frameset_ok = false;
            }
            Input => self.start_input(s),
            Param | Source | Track => self.insert_void(s),
            Hr => {
                self.close_p_in_button_scope();
                if self.has_in_scope(Select) {
                    self.generate_implied(None);
                }
                self.insert_void(s);
                self.frameset_ok = false;
            }
            Image => {
                let img = Start {
                    name: Name::known(Img),
                    raw: Img.as_bytes(),
                    ..*s
                };
                let _ = self.body_start(&img, Tok::Start(img));
            }
            Textarea => {
                self.insert_html(s);
                self.skip_lf = true;
                self.next_state = Some(State::Rcdata);
                self.original_mode = self.mode;
                self.frameset_ok = false;
                self.mode = Mode::Text;
            }
            Xmp => {
                self.close_p_in_button_scope();
                self.reconstruct_active();
                self.frameset_ok = false;
                self.parse_text_element(s, State::Rawtext);
            }
            Iframe => {
                self.frameset_ok = false;
                self.parse_text_element(s, State::Rawtext);
            }
            Noembed => self.parse_text_element(s, State::Rawtext),
            Noscript if self.opts.scripting => self.parse_text_element(s, State::Rawtext),
            Select => self.start_select(s),
            Option | Optgroup => self.start_option(s),
            Rb | Rtc | Rp | Rt => {
                if self.has_in_scope(Ruby) {
                    let except = if matches!(s.name.tag, Rp | Rt) {
                        Some(Rtc)
                    } else {
                        None
                    };
                    self.generate_implied(except);
                }
                self.insert_html(s);
            }
            Math | Svg => {
                self.reconstruct_active();
                let ns = if s.name.tag == Math {
                    Namespace::MathMl
                } else {
                    Namespace::Svg
                };
                self.insert_element(s, ns);
                if s.self_closing {
                    self.pop();
                }
            }
            Caption | Col | Colgroup | Frame | Head | Tbody | Td | Tfoot | Th | Thead | Tr => {}
            _ => {
                self.reconstruct_active();
                self.insert_html(s);
            }
        }
    }

    fn start_body(&mut self, s: &Start<'_>) {
        if self.open.len() == 1 || !self.open[1].html(Tag::Body) || self.has_template() {
            return;
        }
        self.frameset_ok = false;
        let body = self.open[1].node;
        self.sink
            .add_attrs_if_missing(body, Attrs::new(s.attrs, Adjust::Html));
    }

    fn start_frameset(&mut self, s: &Start<'_>) {
        if self.open.len() == 1 || !self.open[1].html(Tag::Body) || !self.frameset_ok {
            return;
        }
        self.sink.remove(self.open[1].node);
        while self.open.len() > 1 {
            self.pop();
        }
        self.insert_html(s);
        self.mode = Mode::InFrameset;
    }

    fn start_form(&mut self, s: &Start<'_>) {
        let in_template = self.in_template_contents();
        if self.form.is_some() && !in_template {
            return;
        }
        self.close_p_in_button_scope();
        let el = self.insert_html(s);
        if !in_template {
            self.form = Some(el);
        }
    }

    fn start_list_item(&mut self, s: &Start<'_>) {
        self.frameset_ok = false;
        let closes: &[Tag] = match s.name.tag {
            Tag::Li => &[Tag::Li],
            _ => &[Tag::Dd, Tag::Dt],
        };
        for i in (0..self.open.len()).rev() {
            let e = self.open[i];
            if e.ns == Namespace::Html && closes.contains(&e.name.tag) {
                self.generate_implied(Some(e.name.tag));
                self.pop_until(|x| x.html(e.name.tag));
                break;
            }
            if is_special(&e) && !e.html(Tag::Address) && !e.html(Tag::Div) && !e.html(Tag::P) {
                break;
            }
        }
        self.close_p_in_button_scope();
        self.insert_html(s);
    }

    fn start_formatting(&mut self, s: &Start<'_>) {
        let tag = s.name.tag;
        if tag == Tag::A
            && let Some(i) = self.active_after_marker(Tag::A)
        {
            let super::Active::Element { node: existing, .. } = self.active[i] else {
                unreachable!()
            };
            if !self.adoption_agency(Tag::A) {
                self.any_other_end(Name::known(Tag::A));
            }
            if let Some(i) = self.active_index(existing) {
                self.remove_active(i);
            }
            if let Some(i) = self.open_index(existing) {
                self.remove_open(i);
            }
        }
        self.reconstruct_active();
        if tag == Tag::Nobr && self.has_in_scope(Tag::Nobr) {
            if !self.adoption_agency(Tag::Nobr) {
                self.any_other_end(Name::known(Tag::Nobr));
            }
            self.reconstruct_active();
        }
        let el = self.insert_html(s);
        self.push_active(el, tag, s.attrs);
    }

    fn context_is_select(&self) -> bool {
        self.context.is_some_and(|c| c.html(Tag::Select))
    }

    fn start_input(&mut self, s: &Start<'_>) {
        if self.context_is_select() {
            return;
        }
        if self.has_in_scope(Tag::Select) {
            self.pop_until(|e| e.html(Tag::Select));
        }
        self.reconstruct_active();
        self.insert_void(s);
        if !s
            .attr(b"type")
            .is_some_and(|t| t.eq_ignore_ascii_case(b"hidden"))
        {
            self.frameset_ok = false;
        }
    }

    fn start_select(&mut self, s: &Start<'_>) {
        if self.context_is_select() {
            return;
        }
        if self.has_in_scope(Tag::Select) {
            self.pop_until(|e| e.html(Tag::Select));
            return;
        }
        self.reconstruct_active();
        self.insert_html(s);
        self.frameset_ok = false;
    }

    fn start_option(&mut self, s: &Start<'_>) {
        if self.has_in_scope(Tag::Select) {
            let except = if s.name.tag == Tag::Option {
                Some(Tag::Optgroup)
            } else {
                None
            };
            self.generate_implied(except);
        } else if self.current_is(Tag::Option) {
            self.pop();
        }
        self.reconstruct_active();
        self.insert_html(s);
    }
}
