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
