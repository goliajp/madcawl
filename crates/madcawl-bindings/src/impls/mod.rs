//! The members of the bound interfaces, called by the generated glue
//! with converted arguments.

pub(crate) mod abort_controller;
pub(crate) mod abort_signal;
pub(crate) mod attr;
pub(crate) mod attributes;
pub(crate) mod blob;
pub(crate) mod character_data;
pub(crate) mod collections;
pub(crate) mod css_style_declaration;
pub(crate) mod crypto;
pub(crate) mod custom_event;
pub(crate) mod document;
pub(crate) mod document_fragment;
pub(crate) mod document_state;
pub(crate) mod dom_exception;
pub(crate) mod dom_token_list;
pub(crate) mod element;
pub(crate) mod event;
pub(crate) mod event_target;
pub(crate) mod history;
pub(crate) mod html_element;
mod html_elements;
pub(crate) mod html_script_element;
pub(crate) mod location;
mod markup;
pub(crate) use markup::mark_started;
pub(crate) mod named_node_map;
pub(crate) mod node;
pub(crate) mod parent;
pub(crate) mod performance;
pub(crate) mod performance_observer;
mod reflect;
pub(crate) mod text_decoder;
pub(crate) mod text_encoder;
pub(crate) mod url;
pub(crate) mod url_search_params;

pub(crate) use html_element::body as html_body_element;
pub(crate) use html_elements::{anchor as html_anchor_element, iframe as html_iframe_element, image as html_image_element};
pub(crate) use html_elements::link as html_link_element;

pub(crate) mod text {
    pub(crate) use super::character_data::{new_text as constructor, split_text, whole_text};
}

pub(crate) mod node_list {
    pub(crate) use super::collections::{item, length};
}

pub(crate) mod html_collection {
    pub(crate) use super::collections::{item, length, named_item};
}

pub(crate) mod comment {
    pub(crate) use super::character_data::new_comment as constructor;
}
