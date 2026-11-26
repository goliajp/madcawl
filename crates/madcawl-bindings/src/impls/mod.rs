//! The members of the bound interfaces, called by the generated glue
//! with converted arguments.

pub(crate) mod character_data;
pub(crate) mod custom_event;
pub(crate) mod document;
pub(crate) mod document_fragment;
pub(crate) mod dom_exception;
pub(crate) mod element;
pub(crate) mod event;
pub(crate) mod event_target;
pub(crate) mod node;
pub(crate) mod parent;

pub(crate) mod text {
    pub(crate) use super::character_data::{new_text as constructor, split_text, whole_text};
}

pub(crate) mod comment {
    pub(crate) use super::character_data::new_comment as constructor;
}
