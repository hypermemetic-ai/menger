//! UI-6's testing surface: the tree interpreter, through the recording
//! renderer.

use super::*;
use serde_json::json;
use std::cell::RefCell;

/// UI-4's example tree, to the byte of the contract.
const EXAMPLE: &str = r#"{ "name": "ticket-row", "abi": "engine/1", "props": ["id"], "state": { "expanded": false },
  "live": { "rows": { "snapshot": "tickets.list", "params": { "parent": { "expr": "id" }, "summary": true, "limit": 200 },
                      "changes": "tickets.watch", "key": "id", "on": { "created": "upsert:ticket", "amended": "upsert:ticket", "deleted": "remove:id" } } },
  "uses": ["tickets.list", "tickets.watch", "tickets.update"],
  "handlers": "0000000000000000000000000000000000000000000000000000000000000000",
  "tree": [
    { "el": "div", "attrs": { "class": "row" }, "on": { "click": "toggle" }, "children": [ { "text": { "expr": "title" } } ] },
    { "if": { "expr": "state.expanded" }, "then": [ { "el": "p", "children": [ { "text": { "expr": "body" } } ] } ], "else": [] },
    { "each": { "expr": "rows.rows", "as": "r", "key": "r.id" }, "body": [ { "component": "ticket-row", "props": { "id": { "expr": "r.id" } } } ] }
  ] }"#;

/// Two texts and an attribute over two state keys.
const COUNTER: &str = r#"{ "name": "counter", "abi": "engine/1", "props": [], "state": { "count": 0, "other": 0 }, "live": {}, "uses": [],
  "handlers": "0000000000000000000000000000000000000000000000000000000000000000",
  "tree": [
    { "el": "p", "children": [ { "text": { "expr": "state.count" } } ] },
    { "el": "p", "children": [ { "text": "static" } ] },
    { "el": "p", "attrs": { "title": { "expr": "state.other" } }, "children": [] }
  ] }"#;

/// The engine's side, recorded: what it was asked to fill and empty.
#[derive(Default)]
struct Kids(RefCell<Vec<String>>);

impl Children<Recording> for Kids {
    fn mount_component(&self, at: &u32, name: &str, props: Map<String, Value>) {
        self.0.borrow_mut().push(format!("mount {name} {} at {at}", Value::Object(props)));
    }
    fn mount_slot(&self, at: &u32, slot: &str, props: Map<String, Value>) {
        self.0.borrow_mut().push(format!("slot {slot} {} at {at}", Value::Object(props)));
    }
    fn unmount(&self, at: &u32) {
        self.0.borrow_mut().push(format!("unmount at {at}"));
    }
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("an object")
}

fn rows(ids: &[&str]) -> Value {
    json!({ "rows": ids.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>(), "status": "live" })
}

fn el(node: u32, tag: &str) -> Call {
    Call::Create { node, tag: tag.into() }
}
fn text(node: u32, t: &str) -> Call {
    Call::CreateText { node, text: t.into() }
}
fn ins(parent: u32, child: u32, before: Option<u32>) -> Call {
    Call::Insert { parent, child, before }
}

/// UI-4's example tree mounts to exactly the expected renderer calls (an
/// `if` and an `each` each hold their place with an empty text anchor and
/// insert before it); a state change patches one text node and nothing
/// else; an `each` keyed by id inserts and removes rows without
/// re-creating the rows that stay.
#[test]
fn the_tree_mounts_and_patches_through_the_renderer() {
    let rt = Runtime::new();
    let r = Recording::new();
    let kids = Rc::new(Kids::default());
    let tree = ComponentTree::parse(EXAMPLE).expect("the contract's example parses");
    assert_eq!(tree.name, "ticket-row");
    assert_eq!(tree.props, ["id"]);
    assert_eq!(tree.live["rows"].on["created"], "upsert:ticket");

    // The props in scope: `id`, and `title` / `body`, which the example's
    // template reads (given at mount like any prop).
    let state = State::new(&rt, &tree.state);
    let live_rows = rt.signal(rows(&["T-2", "T-3"]));
    let heard: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
    let h = heard.clone();
    let scope = Scope {
        props: obj(json!({ "id": "T-1", "title": "Kiln", "body": "Fires" })),
        state: state.clone(),
        live: BTreeMap::from([("rows".to_string(), live_rows.clone())]),
        on_event: Rc::new(move |handler: &str, ev: Value| h.borrow_mut().push((handler.to_string(), ev))),
    };
    let mounted = mount(&rt, &r, &tree, scope, &r.root(), kids.clone()).expect("mounts");

    assert_eq!(
        r.calls(),
        vec![
            el(1, "div"),
            Call::SetAttr { node: 1, name: "class".into(), value: "row".into() },
            Call::Listen { node: 1, event: "click".into() },
            text(2, "Kiln"),
            ins(1, 2, None),
            ins(0, 1, None),
            // the `if`: its anchor; `state.expanded` is false, nothing more
            text(3, ""),
            ins(0, 3, None),
            // the `each`: its anchor, then one element per row before it
            text(4, ""),
            ins(0, 4, None),
            el(5, "ticket-row"),
            ins(0, 5, Some(4)),
            el(6, "ticket-row"),
            ins(0, 6, Some(4)),
        ],
        "the example mounts to exactly these calls"
    );
    assert_eq!(r.html(0), r#"<div class="row">Kiln</div><ticket-row></ticket-row><ticket-row></ticket-row>"#);
    assert_eq!(*kids.0.borrow(), [r#"mount ticket-row {"id":"T-2"} at 5"#, r#"mount ticket-row {"id":"T-3"} at 6"#]);
    assert_eq!(mounted.roots, [1, 3, 4, 5, 6], "what it inserted into the parent, in order");

    // A click goes to its handler by name.
    r.fire(1, "click", json!({}));
    assert_eq!(heard.borrow().first().map(|(h, _)| h.as_str()), Some("toggle"));

    // The fold's return: `expanded` flips; only the `if` moves.
    let at = r.mark();
    state.replace(&obj(json!({ "expanded": true })));
    assert_eq!(r.calls_since(at), vec![el(7, "p"), text(8, "Fires"), ins(7, 8, None), ins(0, 7, Some(3))]);
    assert_eq!(r.html(0), r#"<div class="row">Kiln</div><p>Fires</p><ticket-row></ticket-row><ticket-row></ticket-row>"#);
    let at = r.mark();
    state.replace(&obj(json!({ "expanded": false })));
    assert_eq!(r.calls_since(at), vec![Call::Remove { parent: 0, child: 7 }]);
    let at = r.mark();
    state.replace(&obj(json!({ "expanded": false })));
    assert!(r.calls_since(at).is_empty(), "the same state: nothing drawn");

    // Keyed: T-3 goes and T-4 comes; T-2 keeps its element.
    kids.0.borrow_mut().clear();
    let at = r.mark();
    live_rows.set(rows(&["T-2", "T-4"]));
    assert_eq!(r.calls_since(at), vec![Call::Remove { parent: 0, child: 6 }, el(9, "ticket-row"), ins(0, 9, Some(4))]);
    assert_eq!(*kids.0.borrow(), ["unmount at 6", r#"mount ticket-row {"id":"T-4"} at 9"#]);
    // A row in front: inserted before T-2's element, nothing re-created.
    let at = r.mark();
    live_rows.set(rows(&["T-0", "T-2", "T-4"]));
    assert_eq!(r.calls_since(at), vec![el(10, "ticket-row"), ins(0, 10, Some(5))]);
    assert_eq!(r.under(0).len(), 1 + 2 + 2 + 3, "the root, the div and its text, two anchors, three rows");

    // A state change patches one text node; another key, one attribute.
    let r = Recording::new();
    let tree = ComponentTree::parse(COUNTER).expect("parses");
    let state = State::new(&rt, &tree.state);
    let scope = Scope { props: Map::new(), state: state.clone(), live: BTreeMap::new(), on_event: Rc::new(|_: &str, _: Value| {}) };
    let _m = mount(&rt, &r, &tree, scope, &r.root(), Rc::new(Kids::default())).expect("mounts");
    assert_eq!(r.html(0), r#"<p>0</p><p>static</p><p title="0"></p>"#);
    let count_text = r.calls().iter().find_map(|c| match c {
        Call::CreateText { node, text } if text == "0" => Some(*node),
        _ => None,
    });
    let at = r.mark();
    state.replace(&obj(json!({ "count": 1, "other": 0 })));
    assert_eq!(r.calls_since(at), vec![Call::SetText { node: count_text.expect("the count's text"), text: "1".into() }], "one text node patched");
    let at = r.mark();
    state.replace(&obj(json!({ "count": 1, "other": 7 })));
    let p3 = r.find_tag("p")[2];
    assert_eq!(r.calls_since(at), vec![Call::SetAttr { node: p3, name: "title".into(), value: "7".into() }], "one attribute patched");
    assert_eq!(r.html(0), r#"<p>1</p><p>static</p><p title="7"></p>"#);
}
