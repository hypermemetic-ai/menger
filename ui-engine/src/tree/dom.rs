//! UI-6: **the DOM renderer** — [`Renderer`] through `web-sys`, the page's
//! document. Behind the `dom` feature.

use super::Renderer;
use serde_json::{json, Value};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// The page's document as a renderer.
#[derive(Clone, Debug)]
pub struct Dom {
    pub document: web_sys::Document,
}

impl Dom {
    pub fn new(document: web_sys::Document) -> Self {
        Self { document }
    }
}

/// What a handler sees of a DOM event: `{ value, target }` — the target's
/// value (an input's), and its tag, name, id and `data-*`.
fn payload(e: &web_sys::Event) -> Value {
    let Some(t) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return json!({ "value": null, "target": null }) };
    let value = js_sys::Reflect::get(&t, &"value".into()).ok().and_then(|v| v.as_string());
    let mut data = serde_json::Map::new();
    let attrs = t.get_attribute_names();
    for i in 0..attrs.length() {
        if let Some(name) = attrs.get(i).as_string() {
            if let Some(key) = name.strip_prefix("data-") {
                data.insert(key.to_string(), Value::String(t.get_attribute(&name).unwrap_or_default()));
            }
        }
    }
    json!({
        "value": value,
        "target": { "tag": t.tag_name().to_ascii_lowercase(), "id": t.id(), "name": t.get_attribute("name"), "data": data },
    })
}

impl Renderer for Dom {
    type Node = web_sys::Node;

    fn create_element(&self, tag: &str) -> web_sys::Node {
        self.document.create_element(tag).expect("an element").into()
    }
    fn create_text(&self, text: &str) -> web_sys::Node {
        self.document.create_text_node(text).into()
    }
    fn set_attr(&self, node: &web_sys::Node, name: &str, value: &str) {
        if let Some(e) = node.dyn_ref::<web_sys::Element>() {
            let _ = e.set_attribute(name, value);
        }
    }
    fn set_text(&self, node: &web_sys::Node, text: &str) {
        node.set_text_content(Some(text));
    }
    fn insert(&self, parent: &web_sys::Node, child: &web_sys::Node, before: Option<&web_sys::Node>) {
        let _ = parent.insert_before(child, before);
    }
    fn remove(&self, parent: &web_sys::Node, child: &web_sys::Node) {
        let _ = parent.remove_child(child);
    }
    fn listen(&self, node: &web_sys::Node, event: &str, handler: Rc<dyn Fn(Value)>) {
        let f = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| handler(payload(&e)));
        let _ = node.add_event_listener_with_callback(event, f.as_ref().unchecked_ref());
        // The listener lives as long as its node: the page's.
        f.forget();
    }
}
