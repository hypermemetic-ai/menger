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

/// The contract's handlers, as the compiler emits them.
const HANDLERS: &str = r##"
fn toggle(state, ev) { state.expanded = !state.expanded; state }
fn rename(state, ev) { call("tickets.update", #{ id: id, title: ev.value }); state }
fn open(state, ev) { navigate("#/ticket/" + id); state }
"##;

fn uses(methods: &[&str]) -> Vec<String> {
    methods.iter().map(|m| m.to_string()).collect()
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("an object")
}

/// `toggle` flips `expanded` and returns the state; `rename` reaches the
/// host's `call` with the props in scope; a handler calling a method not in
/// `uses` is refused when the component compiles, naming the method; a
/// `fn` that is no fold is refused, naming it.
#[test]
fn a_fold_returns_the_new_state_and_a_call_outside_uses_is_refused() {
    let host = Rc::new(Calls::default());
    let folds = Folds::compile(HANDLERS, &uses(&["tickets.list", "tickets.watch", "tickets.update"]), host.clone()).expect("compiles");
    let mut names = folds.handlers();
    names.sort();
    assert_eq!(names, ["open", "rename", "toggle"]);

    let props = obj(json!({ "id": "T-1" }));
    let state = obj(json!({ "expanded": false, "other": 3 }));
    let next = folds.run("toggle", &state, &json!({}), &props).expect("toggle runs");
    assert_eq!(Value::Object(next.clone()), json!({ "expanded": true, "other": 3 }), "expanded flipped, the rest as it was");
    let back = folds.run("toggle", &next, &json!({}), &props).expect("toggle runs");
    assert_eq!(back["expanded"], false);
    assert!(host.0.borrow().is_empty(), "toggle calls nothing");

    let same = folds.run("rename", &state, &json!({ "value": "Kiln" }), &props).expect("rename runs");
    assert_eq!(same, state, "rename returns the state it was given");
    folds.run("open", &state, &json!({}), &props).expect("open runs");
    assert_eq!(*host.0.borrow(), [r#"call tickets.update {"id":"T-1","title":"Kiln"}"#, "navigate #/ticket/T-1"]);
    assert_eq!(folds.run("nope", &state, &json!({}), &props).err(), Some(FoldError::NoHandler("nope".into())));

    // A call outside `uses`: refused at compile (mount), naming the method.
    let outside = "fn toggle(state, ev) { state }\nfn purge(state, ev) { call(\"tickets.delete\", #{ ids: [id] }); state }";
    let refused = Folds::compile(outside, &uses(&["tickets.update"]), host.clone()).err().expect("refused");
    match &refused {
        FoldError::NotInUses { method, .. } => assert_eq!(method, "tickets.delete"),
        other => panic!("refused for the wrong reason: {other:?}"),
    }
    assert!(refused.to_string().contains("tickets.delete"), "the refusal names the method: {refused}");
    assert_eq!(host.0.borrow().len(), 2, "nothing was called by the refused component");

    // A fn of another shape is no fold.
    let refused = Folds::compile("fn helper(x) { x + 1 }", &uses(&[]), host).err().expect("refused");
    assert_eq!(refused, FoldError::NotAFold { name: "helper".into() });
}
