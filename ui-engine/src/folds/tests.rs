//! UI-6's testing surface: the folds.

use super::*;
use serde_json::json;
use std::cell::RefCell;

/// The host, recorded.
#[derive(Default)]
struct Calls(RefCell<Vec<String>>);

impl Host for Calls {
    fn call(&self, method: &str, params: Value) {
        self.0.borrow_mut().push(format!("call {method} {params}"));
    }
    fn navigate(&self, route: &str) {
        self.0.borrow_mut().push(format!("navigate {route}"));
    }
}

/// The contract's handlers, as the compiler emits them (a handler sees only
/// its two arguments: the props are `ev.props`), and one that reads the
/// rest of `ev`.
const HANDLERS: &str = r##"
fn toggle(state, ev) { state.expanded = !state.expanded; state }
fn rename(state, ev) { call("tickets.update", #{ id: ev.props.id, title: ev.value }); state }
fn open(state, ev) { navigate("#/ticket/" + ev.props.id); state }
fn look(state, ev) { state.seen = #{ value: ev.value, target: ev.target, rows: ev.live.rows.len(), first: ev.live.rows[0].id }; state }
"##;

fn uses(methods: &[&str]) -> Vec<String> {
    methods.iter().map(|m| m.to_string()).collect()
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("an object")
}

/// `toggle` flips `expanded` and returns the state; `rename` reaches the
/// host's `call` with `ev.props` and `ev.value`; `ev.target` and
/// `ev.live.<name>` reach a handler; a handler calling a method not in
/// `uses` is refused when the component compiles, naming the method — a
/// name joined from strings is named too, and one computed some other way
/// is refused; a `fn` that is no fold is refused, naming it.
#[test]
fn a_fold_returns_the_new_state_and_a_call_outside_uses_is_refused() {
    let host = Rc::new(Calls::default());
    let folds = Folds::compile(HANDLERS, &uses(&["tickets.list", "tickets.update", "tickets.watch"]), host.clone()).expect("compiles");
    let mut names = folds.handlers();
    names.sort();
    assert_eq!(names, ["look", "open", "rename", "toggle"]);

    let props = obj(json!({ "id": "T-1" }));
    let live = obj(json!({ "rows": [ { "id": "T-2" }, { "id": "T-3" } ] }));
    let state = obj(json!({ "expanded": false, "other": 3 }));
    let next = folds.run("toggle", &state, &json!({}), &props, &live).expect("toggle runs");
    assert_eq!(Value::Object(next.clone()), json!({ "expanded": true, "other": 3 }), "expanded flipped, the rest as it was");
    let back = folds.run("toggle", &next, &json!({}), &props, &live).expect("toggle runs");
    assert_eq!(back["expanded"], false);
    assert!(host.0.borrow().is_empty(), "toggle calls nothing");

    let same = folds.run("rename", &state, &json!({ "value": "Kiln" }), &props, &live).expect("rename runs");
    assert_eq!(same, state, "rename returns the state it was given");
    folds.run("open", &state, &json!({}), &props, &live).expect("open runs");
    assert_eq!(*host.0.borrow(), [r#"call tickets.update {"id":"T-1","title":"Kiln"}"#, "navigate #/ticket/T-1"]);
    let seen = folds.run("look", &state, &json!({ "value": "v", "target": { "tag": "input", "name": "title" } }), &props, &live).expect("look runs");
    assert_eq!(seen["seen"], json!({ "value": "v", "target": { "tag": "input", "name": "title" }, "rows": 2, "first": "T-2" }), "ev.value, ev.target and ev.live.rows");
    assert_eq!(folds.run("nope", &state, &json!({}), &props, &live).err(), Some(FoldError::NoHandler("nope".into())));

    // A call outside `uses`: refused at compile (mount), naming the method.
    let outside = "fn toggle(state, ev) { state }\nfn purge(state, ev) { call(\"tickets.delete\", #{ ids: [ev.props.id] }); state }";
    let refused = Folds::compile(outside, &uses(&["tickets.update"]), host.clone()).err().expect("refused");
    match &refused {
        FoldError::NotInUses { method, .. } => assert_eq!(method, "tickets.delete"),
        other => panic!("refused for the wrong reason: {other:?}"),
    }
    assert!(refused.to_string().contains("tickets.delete") && refused.to_string().contains("purge"), "names the method and the handler: {refused}");
    assert_eq!(host.0.borrow().len(), 2, "nothing was called by the refused component");

    // A method name joined from strings: refused, named.
    let joined = "fn purge(state, ev) { call(\"tickets.\" + \"delete\", #{ ids: [ev.props.id] }); state }";
    let refused = Folds::compile(joined, &uses(&["tickets.update"]), host.clone()).err().expect("refused");
    assert!(refused.to_string().contains("tickets.delete"), "the joined name is named: {refused}");
    // Computed some other way: refused, naming the handler.
    let computed = "fn purge(state, ev) { let m = ev.value; call(m, #{}); state }";
    let refused = Folds::compile(computed, &uses(&["tickets.update"]), host.clone()).err().expect("refused");
    assert!(matches!(&refused, FoldError::Computed { handler, .. } if handler == "purge"), "{refused:?}");

    // A fn of another shape is no fold.
    let refused = Folds::compile("fn helper(x) { x + 1 }", &uses(&[]), host).err().expect("refused");
    assert_eq!(refused, FoldError::NotAFold { name: "helper".into() });
}

/// UI-10: a handler's event carries what the DOM gave — a scroll's
/// `scroll_top` / `scroll_height` / `client_height`, a keydown's `key`, a
/// checkbox's `checked` (and `value`, `target`) — through the tree's
/// listeners (the recording renderer) to the fold.
#[test]
fn an_event_carries_what_the_dom_gave() {
    use crate::signals::Runtime;
    use crate::tree::{mount, Children, ComponentTree, Recording, Scope, State};
    use std::collections::BTreeMap;

    struct NoKids;
    impl Children<Recording> for NoKids {
        fn mount_component(&self, _: &u32, _: &str, _: Map<String, Value>) {}
        fn mount_slot(&self, _: &u32, _: &str, _: &str, _: Map<String, Value>) {}
        fn unmount(&self, _: &u32) {}
    }

    let tree = ComponentTree::parse(
        r#"{ "name": "dom-events", "abi": "engine/1", "props": [], "state": {}, "live": {}, "uses": [], "handlers": "",
             "tree": [
               { "el": "div", "attrs": { "class": "rows" }, "on": { "scroll": "scrolled" }, "children": [] },
               { "el": "input", "attrs": { "name": "q" }, "on": { "keydown": "typed" }, "children": [] },
               { "el": "input", "attrs": { "type": "checkbox" }, "on": { "change": "ticked" }, "children": [] }
             ] }"#,
    )
    .expect("parses");
    let handlers = r#"
fn scrolled(state, ev) { state.scroll = [ev.scroll_top, ev.scroll_height, ev.client_height]; state }
fn typed(state, ev) { state.key = ev.key; state.value = ev.value; state.target = ev.target.name; state }
fn ticked(state, ev) { state.checked = ev.checked; state }
"#;
    let folds = Rc::new(Folds::compile(handlers, &[], Rc::new(Calls::default())).expect("compiles"));
    let rt = Runtime::new();
    let r = Recording::new();
    let state = State::new(&rt, &Map::new());
    let (f, s) = (folds.clone(), state.clone());
    let on_event: Rc<dyn Fn(&str, Value)> = Rc::new(move |h: &str, ev: Value| {
        let next = f.run(h, &s.snapshot(), &ev, &Map::new(), &Map::new()).expect("the fold runs");
        s.replace(&next);
    });
    let scope = Scope { props: Map::new(), state: state.clone(), live: BTreeMap::new(), on_event };
    let _m = mount(&rt, &r, &tree, scope, &r.root(), Rc::new(NoKids)).expect("mounts");

    let (rows, q, box_) = (r.find_tag("div")[0], r.find_tag("input")[0], r.find_tag("input")[1]);
    r.fire(rows, "scroll", json!({ "scroll_top": 480, "scroll_height": 12000, "client_height": 800 }));
    r.fire(q, "keydown", json!({ "key": "Enter", "value": "kiln", "target": { "tag": "input", "name": "q" } }));
    r.fire(box_, "change", json!({ "checked": true, "value": "on" }));
    let now = state.snapshot();
    assert_eq!(now["scroll"], json!([480, 12000, 800]), "a scroll's metrics");
    assert_eq!((now["key"].clone(), now["value"].clone(), now["target"].clone()), (json!("Enter"), json!("kiln"), json!("q")), "a keydown's key, the value, the target");
    assert_eq!(now["checked"], json!(true), "a checkbox's checked");
}
