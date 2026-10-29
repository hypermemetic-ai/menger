//! UI-6's testing surface: the tree interpreter, through the recording
//! renderer.

use super::*;
use serde_json::json;
use std::cell::RefCell;

/// UI-4's example tree, as the contract writes it now.
const EXAMPLE: &str = r#"{ "name": "ticket-row", "abi": "engine/1", "props": ["id", "title", "body"], "state": { "expanded": false },
  "live": { "rows": { "snapshot": "tickets.list", "params": { "parent": { "expr": "id" }, "summary": true, "limit": 200 },
                      "changes": "tickets.watch", "key": "id", "on": { "created": "upsert:ticket", "amended": "upsert:ticket", "deleted": "remove:ids" } } },
  "uses": ["tickets.list", "tickets.update", "tickets.watch"],
  "handlers": "0000000000000000000000000000000000000000000000000000000000000000",
  "tree": [
    { "el": "div", "attrs": { "class": "row" }, "on": { "click": "toggle" }, "children": [ { "text": { "expr": "title" } } ] },
    { "if": { "expr": "state.expanded" }, "then": [ { "el": "p", "children": [ { "text": { "expr": "body" } } ] } ], "else": [] },
    { "each": { "expr": "rows.rows", "as": "r", "key": "r.id" }, "body": [ { "component": "ticket-row", "props": { "id": { "expr": "r.id" } } } ] }
  ] }"#;

/// Two texts and an attribute over two state keys, and an `if` whose
/// branch reads a third.
const COUNTER: &str = r#"{ "name": "counter", "abi": "engine/1", "props": [], "state": { "count": 0, "other": 0, "open": false, "label": "a" }, "live": {}, "uses": [],
  "handlers": "0000000000000000000000000000000000000000000000000000000000000000",
  "tree": [
    { "el": "p", "children": [ { "text": { "expr": "state.count" } } ] },
    { "el": "p", "children": [ { "text": "static" } ] },
    { "el": "p", "attrs": { "title": { "expr": "state.other" } }, "children": [] },
    { "if": { "expr": "state.open" }, "then": [ { "el": "b", "children": [ { "text": { "expr": "state.label" } } ] } ], "else": [] }
  ] }"#;

/// The engine's side, recorded: what it was asked to fill and empty.
#[derive(Default)]
struct Kids(RefCell<Vec<String>>);

impl Children<Recording> for Kids {
    fn mount_component(&self, at: &u32, name: &str, props: Map<String, Value>) {
        self.0.borrow_mut().push(format!("mount {name} {} at {at}", Value::Object(props)));
    }
    fn mount_slot(&self, at: &u32, layout: &str, slot: &str, props: Map<String, Value>) {
        self.0.borrow_mut().push(format!("slot {layout}/{slot} {} at {at}", Value::Object(props)));
    }
    fn unmount(&self, at: &u32) {
        self.0.borrow_mut().push(format!("unmount at {at}"));
    }
}

impl Kids {
    /// The element `component` with `props` was mounted into, last.
    fn at(&self, line: &str) -> u32 {
        let l = self.0.borrow().iter().rev().find(|l| l.starts_with(line)).cloned().unwrap_or_else(|| panic!("no `{line}` in {:?}", self.0.borrow()));
        l.rsplit(' ').next().unwrap().parse().unwrap()
    }
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("an object")
}

fn rows(ids: &[&str]) -> Value {
    json!({ "rows": ids.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>(), "status": "live" })
}

/// Every node in the tree was created once, by one call; with `exact`,
/// nothing was created that is not in it (before anything was removed).
fn one_create_per_node(r: &Recording, exact: bool) {
    let made: Vec<u32> = r.calls().iter().filter_map(|c| matches!(c, Call::Create { .. } | Call::CreateText { .. }).then(|| c.node())).collect();
    let mut distinct = made.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(made.len(), distinct.len(), "a node created twice");
    let mut drawn = r.under(0);
    drawn.retain(|n| *n != 0);
    drawn.sort();
    if exact {
        assert_eq!(distinct, drawn, "every node created is drawn, and once");
    } else {
        assert!(drawn.iter().all(|n| distinct.contains(n)), "every node drawn was created");
    }
}

/// UI-4's example tree mounts through the renderer to the expected page,
/// each node created once; a state change patches only what reads it; an
/// `each` keyed by id inserts and removes rows without re-creating the
/// rows that stay. A key read only inside an `if` branch that is not drawn
/// moves nothing; one changed key runs exactly one binding.
#[test]
fn the_tree_mounts_and_patches_through_the_renderer() {
    let rt = Runtime::new();
    let r = Recording::new();
    let kids = Rc::new(Kids::default());
    let tree = ComponentTree::parse(EXAMPLE).expect("the contract's example parses");
    assert_eq!(tree.name, "ticket-row");
    assert_eq!(tree.props, ["id", "title", "body"]);
    assert_eq!(tree.live["rows"].on["deleted"], "remove:ids");

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
    let _mounted = mount(&rt, &r, &tree, scope, &r.root(), kids.clone()).expect("mounts");

    assert_eq!(r.html(0), r#"<div class="row">Kiln</div><ticket-row></ticket-row><ticket-row></ticket-row>"#);
    one_create_per_node(&r, true);
    let (t2, t3) = (kids.at(r#"mount ticket-row {"id":"T-2"}"#), kids.at(r#"mount ticket-row {"id":"T-3"}"#));
    let div = r.find_tag("div")[0];

    // A click goes to its handler by name.
    r.fire(div, "click", json!({}));
    assert_eq!(heard.borrow().first().map(|(h, _)| h.as_str()), Some("toggle"));

    // The fold's return: `expanded` flips; only the `if` moves.
    let at = r.mark();
    state.replace(&obj(json!({ "expanded": true })));
    assert_eq!(r.html(0), r#"<div class="row">Kiln</div><p>Fires</p><ticket-row></ticket-row><ticket-row></ticket-row>"#);
    assert!(r.calls_since(at).iter().all(|c| !matches!(c, Call::Remove { .. })), "nothing removed to show the branch");
    assert_eq!(r.find_tag("div"), [div], "the div kept");
    let p = r.find_tag("p")[0];
    state.replace(&obj(json!({ "expanded": false })));
    assert_eq!(r.calls_since(at).last(), Some(&Call::Remove { parent: 0, child: p }), "the branch's p taken out");
    let at = r.mark();
    state.replace(&obj(json!({ "expanded": false })));
    assert!(r.calls_since(at).is_empty(), "the same state: nothing drawn");

    // Keyed: T-3 goes and T-4 comes; T-2 keeps its element.
    let at = r.mark();
    live_rows.set(rows(&["T-2", "T-4"]));
    let t4 = kids.at(r#"mount ticket-row {"id":"T-4"}"#);
    assert!(kids.0.borrow().contains(&format!("unmount at {t3}")), "T-3 emptied");
    assert!(!r.under(0).contains(&t3), "T-3's element out");
    assert!(r.under(0).contains(&t2), "T-2's element kept");
    let creates = r.calls_since(at).iter().filter(|c| matches!(c, Call::Create { .. } | Call::CreateText { .. })).count();
    assert_eq!(creates, 1, "one element made, T-4's: {:?}", r.calls_since(at));
    // A row in front: before T-2's element, nothing re-created.
    let at = r.mark();
    live_rows.set(rows(&["T-0", "T-2", "T-4"]));
    let t0 = kids.at(r#"mount ticket-row {"id":"T-0"}"#);
    let creates = r.calls_since(at).iter().filter(|c| matches!(c, Call::Create { .. } | Call::CreateText { .. })).count();
    assert_eq!(creates, 1, "one element made, T-0's");
    let order: Vec<u32> = r.find_tag("ticket-row");
    assert_eq!(order, [t0, t2, t4], "in the list's order");
    one_create_per_node(&r, false);

    // A state change patches one text node; another key, one attribute; a
    // key read only in an `if` branch not drawn, nothing.
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
    let (at, runs) = (r.mark(), rt.runs());
    state.replace(&obj(json!({ "count": 1, "other": 0, "open": false, "label": "a" })));
    assert_eq!(r.calls_since(at), vec![Call::SetText { node: count_text.expect("the count's text"), text: "1".into() }], "one text node patched");
    assert_eq!(rt.runs() - runs, 1, "one changed key: exactly one binding ran");
    let (at, runs) = (r.mark(), rt.runs());
    state.replace(&obj(json!({ "count": 1, "other": 7, "open": false, "label": "a" })));
    let p3 = r.find_tag("p")[2];
    assert_eq!(r.calls_since(at), vec![Call::SetAttr { node: p3, name: "title".into(), value: "7".into() }], "one attribute patched");
    assert_eq!(rt.runs() - runs, 1);
    assert_eq!(r.html(0), r#"<p>1</p><p>static</p><p title="7"></p>"#);

    // The branch drawn and taken out again: its key then moves nothing.
    state.replace(&obj(json!({ "open": true })));
    assert_eq!(r.html(0), r#"<p>1</p><p>static</p><p title="7"></p><b>a</b>"#);
    state.replace(&obj(json!({ "label": "b" })));
    assert_eq!(r.html(0), r#"<p>1</p><p>static</p><p title="7"></p><b>b</b>"#);
    state.replace(&obj(json!({ "open": false })));
    let (at, runs) = (r.mark(), rt.runs());
    state.replace(&obj(json!({ "label": "c" })));
    assert!(r.calls_since(at).is_empty(), "a removed branch's key: no calls");
    assert_eq!(rt.runs() - runs, 0, "and no binding ran");
}

/// UI-10: `on:window-keydown` on a component's root listens on the window
/// while the component is mounted, and never after: registered at mount,
/// removed at unmount.
#[test]
fn a_window_key_listener_lives_with_its_component() {
    let rt = Runtime::new();
    let r = Recording::new();
    let tree = ComponentTree::parse(
        r#"{ "name": "jump-palette", "abi": "engine/1", "props": [], "state": {}, "live": {}, "uses": [], "handlers": "",
             "tree": [ { "el": "div", "on": { "window-keydown": "keyed" }, "children": [ { "text": "palette" } ] } ] }"#,
    )
    .expect("parses");
    let heard: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
    let h = heard.clone();
    let scope = Scope {
        props: Map::new(),
        state: State::new(&rt, &Map::new()),
        live: BTreeMap::new(),
        on_event: Rc::new(move |handler: &str, ev: Value| h.borrow_mut().push((handler.to_string(), ev))),
    };
    assert_eq!(r.window_listeners(), 0);
    let m = mount(&rt, &r, &tree, scope, &r.root(), Rc::new(Kids::default())).expect("mounts");
    assert_eq!(r.window_listeners(), 1, "registered at mount");
    r.fire_window("keydown", json!({ "key": "k" }));
    assert_eq!(*heard.borrow(), [("keyed".to_string(), json!({ "key": "k" }))], "the window's keydown reaches the handler");

    m.unmount();
    r.fire_window("keydown", json!({ "key": "j" }));
    assert_eq!(heard.borrow().len(), 1, "never after unmount");
    assert_eq!(r.window_listeners(), 0, "removed");
}

/// UI-10: `{@html expr}` draws the expression's HTML through the
/// sanitizer — elements and text through the renderer, what is not allowed
/// gone — and draws it again when what the expression reads changes.
#[test]
fn an_html_node_draws_the_sanitized_tree() {
    let rt = Runtime::new();
    let r = Recording::new();
    let tree = ComponentTree::parse(
        r#"{ "name": "ticket-body", "abi": "engine/1", "props": [], "state": { "body": "" }, "live": {}, "uses": [], "handlers": "",
             "tree": [ { "el": "article", "children": [ { "html": { "expr": "state.body" } } ] }, { "el": "footer", "children": [] } ] }"#,
    )
    .expect("parses");
    let state = State::new(&rt, &obj(json!({ "body": r#"<p>a<script>alert(1)</script><b onclick="x()">b</b></p>"# })));
    let scope = Scope { props: Map::new(), state: state.clone(), live: BTreeMap::new(), on_event: Rc::new(|_: &str, _: Value| {}) };
    let _m = mount(&rt, &r, &tree, scope, &r.root(), Rc::new(Kids::default())).expect("mounts");
    assert_eq!(r.html(0), "<article><p>a<b>b</b></p></article><footer></footer>");
    let footer = r.find_tag("footer")[0];
    state.replace(&obj(json!({ "body": "<em>c</em> &amp; d" })));
    assert_eq!(r.html(0), "<article><em>c</em> & d</article><footer></footer>", "drawn again; an entity decoded into the text node (the recording draws text as it is)");
    assert_eq!(r.find_tag("footer"), [footer], "nothing else redrawn");
}
