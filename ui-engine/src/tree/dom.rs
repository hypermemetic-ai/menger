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

/// What a handler sees of a DOM event: `value` and `checked` (an input's),
/// `key` (a key event's), the scroll metrics of the element listened on
/// (`scroll_top`, `scroll_height`, `client_height`), and `target` — its
/// tag, id, name and `data-*`.
fn payload(e: &web_sys::Event, on: &web_sys::Node) -> Value {
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &k.into()).ok().filter(|v| !v.is_undefined() && !v.is_null());
    let mut out = serde_json::Map::new();
    let target = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    if let Some(t) = &target {
        out.insert("value".into(), get(t, "value").and_then(|v| v.as_string()).map_or(Value::Null, Value::String));
        if let Some(c) = get(t, "checked").and_then(|v| v.as_bool()) {
            out.insert("checked".into(), Value::Bool(c));
        }
        let mut data = serde_json::Map::new();
        let attrs = t.get_attribute_names();
        for i in 0..attrs.length() {
            if let Some(name) = attrs.get(i).as_string() {
                if let Some(key) = name.strip_prefix("data-") {
                    data.insert(key.to_string(), Value::String(t.get_attribute(&name).unwrap_or_default()));
                }
            }
        }
        out.insert("target".into(), json!({ "tag": t.tag_name().to_ascii_lowercase(), "id": t.id(), "name": t.get_attribute("name"), "data": data }));
    } else {
        out.insert("value".into(), Value::Null);
        out.insert("target".into(), Value::Null);
    }
    if let Some(k) = e.dyn_ref::<web_sys::KeyboardEvent>() {
        out.insert("key".into(), Value::String(k.key()));
    }
    if let Some(el) = on.dyn_ref::<web_sys::Element>() {
        if e.type_() == "scroll" {
            out.insert("scroll_top".into(), json!(el.scroll_top()));
            out.insert("scroll_height".into(), json!(el.scroll_height()));
            out.insert("client_height".into(), json!(el.client_height()));
        }
    }
    Value::Object(out)
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
    fn listen(&self, node: &web_sys::Node, event: &str, handler: Rc<dyn Fn(Value)>) -> super::Unlisten {
        // UI-10: `window-<event>` listens on the window (while `node` is in
        // the page); either way the answer removes the listener and drops
        // its closure — nothing is forgotten.
        let (target, name, on): (web_sys::EventTarget, String, web_sys::Node) = match event.strip_prefix("window-") {
            Some(e) => match web_sys::window() {
                Some(w) => (w.into(), e.to_string(), node.clone()),
                None => return Box::new(|| {}),
            },
            None => (node.clone().into(), event.to_string(), node.clone()),
        };
        let window = event.starts_with("window-");
        let n = on.clone();
        let f = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            if !window || n.is_connected() {
                handler(payload(&e, &n));
            }
        });
        let _ = target.add_event_listener_with_callback(&name, f.as_ref().unchecked_ref());
        Box::new(move || {
            let _ = target.remove_event_listener_with_callback(&name, f.as_ref().unchecked_ref());
            drop(f);
        })
    }
}
