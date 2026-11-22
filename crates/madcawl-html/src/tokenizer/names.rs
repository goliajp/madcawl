//! Tag and attribute names as they are read: the bytes, the hash that
//! gives their static atom, and repeated attributes.

use super::{LINEAR_ATTRS, Sink, Tokenizer};
use crate::error::ParseError;
use crate::input::push_char;
use madcawl_str::NameHash;

impl Tokenizer {
    pub(super) fn new_tag(&mut self, end: bool) {
        self.is_end_tag = end;
        self.tag.name.clear();
        self.tag_hash = NameHash::new();
        self.tag.self_closing = false;
        self.attr_is_duplicate = false;
        self.attr_names.clear();
        self.spare_attrs.append(&mut self.tag.attrs);
    }

    pub(super) fn new_attribute(&mut self) {
        self.end_attribute();
        let mut a = self.spare_attrs.pop().unwrap_or_default();
        a.name.clear();
        a.atom = None;
        a.value.clear();
        self.attr_hash = NameHash::new();
        self.tag.attrs.push(a);
    }

    /// Append to the tag name, lowercased already.
    pub(super) fn tag_name_push(&mut self, c: u32) {
        let n = self.tag.name.len();
        push_char(&mut self.tag.name, c);
        self.tag_hash.feed(&self.tag.name[n..]);
    }

    /// Append to the name of the attribute being read, lowercased
    /// already.
    pub(super) fn attr_name_push(&mut self, c: u32) {
        let name = &mut self
            .tag
            .attrs
            .last_mut()
            .expect("an attribute is being read")
            .name;
        let n = name.len();
        push_char(name, c);
        self.attr_hash.feed(&name[n..]);
    }

    /// The name of the attribute being read is complete.
    pub(super) fn attribute_name_done<S: Sink>(&mut self, sink: &mut S) {
        let Some((last, earlier)) = self.tag.attrs.split_last_mut() else {
            return;
        };
        last.atom = self.attr_hash.finish(&last.name);
        let repeat = if earlier.len() < LINEAR_ATTRS {
            step!(earlier.len());
            earlier.iter().any(|a| a.name == last.name)
        } else {
            if self.attr_names.is_empty() {
                self.attr_names
                    .extend(earlier.iter().map(|a| a.name.clone()));
            }
            !self.attr_names.insert(last.name.clone())
        };
        if repeat {
            sink.error(ParseError::DuplicateAttribute);
            self.attr_is_duplicate = true;
        }
    }

    pub(super) fn end_attribute(&mut self) {
        if self.attr_is_duplicate {
            self.attr_is_duplicate = false;
            if let Some(a) = self.tag.attrs.pop() {
                self.spare_attrs.push(a);
            }
        }
    }

    pub(super) fn attr_value(&mut self) -> &mut Vec<u8> {
        &mut self
            .tag
            .attrs
            .last_mut()
            .expect("an attribute is being read")
            .value
    }

    /// The end tag being read closes the element whose text this is.
    pub(super) fn appropriate_end_tag(&self) -> bool {
        !self.last_start_tag.is_empty() && self.tag.name == self.last_start_tag
    }
}
