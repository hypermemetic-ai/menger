//! UI-6: **the DOM renderer** — [`Renderer`] through `web-sys`, the page's
//! document. Behind the `dom` feature.

use super::Renderer;
use serde_json::Value;
use std::rc::Rc;

/// The page's document as a renderer.
#[derive(Clone, Debug)]
pub struct Dom {
    pub document: web_sys::Document,
}

impl Renderer for Dom {
    type Node = web_sys::Node;

    fn create_element(&self, tag: &str) -> web_sys::Node {
        let _ = tag;
        todo!("UI-6: Dom::create_element")
    }
    fn create_text(&self, text: &str) -> web_sys::Node {
        let _ = text;
        todo!("UI-6: Dom::create_text")
    }
    fn set_attr(&self, node: &web_sys::Node, name: &str, value: &str) {
        let _ = (node, name, value);
        todo!("UI-6: Dom::set_attr")
    }
    fn set_text(&self, node: &web_sys::Node, text: &str) {
        let _ = (node, text);
        todo!("UI-6: Dom::set_text")
    }
    fn insert(&self, parent: &web_sys::Node, child: &web_sys::Node, before: Option<&web_sys::Node>) {
        let _ = (parent, child, before);
        todo!("UI-6: Dom::insert")
    }
    fn remove(&self, parent: &web_sys::Node, child: &web_sys::Node) {
        let _ = (parent, child);
        todo!("UI-6: Dom::remove")
    }
    fn listen(&self, node: &web_sys::Node, event: &str, handler: Rc<dyn Fn(Value)>) {
        let _ = (node, event, handler);
        todo!("UI-6: Dom::listen")
    }
}
