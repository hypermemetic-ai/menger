//! UI-6's testing surface: hot-swap, the last good version, the sections.

use super::*;
use crate::live::ManualClock;
use crate::transport::FakeWire;
use crate::tree::{Call, Recording};
use serde_json::json;
use std::rc::Rc;

const BUMP: &str = "fn bump(state, ev) { state.n += 1; state }";
const BUMP_TEN: &str = "fn bump(state, ev) { state.n += 10; state }";
const NONE: &str = "";

/// A compiled tree for `name`: one `p` (clicks go to `bump`) whose text is
/// `text`, a Rhai expression (no `on` when it has no handlers); `state` its declared state; `uses` its methods.
fn tree(name: &str, props: &[&str], state: Value, text: &str, handlers: &str, uses: &[&str]) -> String {
    let on = if handlers.is_empty() { json!({}) } else { json!({ "click": "bump" }) };
    json!({
        "name": name, "abi": "engine/1", "props": props, "state": state, "live": {}, "uses": uses,
        "handlers": sha256_hex(handlers),
        "tree": [ { "el": "p", "on": on, "children": [ { "text": { "expr": text } } ] } ]
    })
    .to_string()
}

/// A manifest of `components` — `(name, tree, handlers, uses)` — each file
/// kept in `files`.
fn manifest(components: &[(&str, &str, &str, &[&str])], files: &mut Files) -> Manifest {
    let mut m = Manifest { abi: "engine/1".into(), contract: "unknown".into(), commit: "menger@0000000".into(), ..Manifest::default() };
    for (name, tree, handlers, uses) in components {
        let entry = ComponentEntry { tree: files.put(tree), handlers: files.put(handlers), uses: uses.iter().map(|u| u.to_string()).collect() };
        m.components.insert(name.to_string(), entry);
    }
    m
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("an object")
}

type TestApp = Rc<App<Recording, FakeWire, ManualClock>>;

fn app() -> (Recording, TestApp) {
    let r = Recording::new();
    let app = App::new(&Runtime::new(), r.clone(), FakeWire::new(), ManualClock::new());
    (r, app)
}

/// The nodes a call since `mark` made or changed.
fn touched(r: &Recording, mark: usize) -> Vec<u32> {
    r.calls_since(mark).iter().map(Call::node).collect()
}

/// Two components mounted; a new manifest changes one's tree hash: that
/// one remounts into its element with the same props and its state (the
/// same shape); the other is not touched. A state of another shape starts
/// anew; a handlers-only change remounts too.
#[test]
fn a_hot_swap_remounts_only_what_changed_and_keeps_state_of_the_same_shape() {
    let (r, app) = app();
    let mut files = Files::default();
    let a1 = tree("a-one", &["id"], json!({ "n": 0 }), r#""one " + id + ": " + state.n"#, BUMP, &[]);
    let b = tree("b-two", &["id"], json!({}), r#""two " + id"#, NONE, &[]);
    let v1 = manifest(&[("a-one", &a1, BUMP, &[]), ("b-two", &b, NONE, &[])], &mut files);
    app.load("v1", v1, &files).expect("v1 loads");
    let a = app.mount(&r.root(), "a-one", obj(json!({ "id": "T-1" }))).expect("a mounts");
    let bm = app.mount(&r.root(), "b-two", obj(json!({ "id": "T-9" }))).expect("b mounts");
    assert_eq!(r.html(0), "<a-one><p>one T-1: 0</p></a-one><b-two><p>two T-9</p></b-two>");

    let p = r.find_tag("p")[0];
    r.fire(p, "click", json!({}));
    r.fire(p, "click", json!({}));
    assert_eq!(app.state_of(a), obj(json!({ "n": 2 })), "two folds ran");
    assert_eq!(r.html(0), "<a-one><p>one T-1: 2</p></a-one><b-two><p>two T-9</p></b-two>");

    let (a_el, b_nodes) = (app.element_of(a), r.under(app.element_of(bm)));
    let at = r.mark();
    let a2 = tree("a-one", &["id"], json!({ "n": 0 }), r#""uno " + id + ": " + state.n"#, BUMP, &[]);
    let v2 = manifest(&[("a-one", &a2, BUMP, &[]), ("b-two", &b, NONE, &[])], &mut files);
    let report = app.swap("v2", v2, &files);
    assert_eq!(report, SwapReport { remounted: vec![a], failed: vec![] }, "only a-one's hash changed");
    assert_eq!(r.html(0), "<a-one><p>uno T-1: 2</p></a-one><b-two><p>two T-9</p></b-two>", "remounted, its state carried over");
    assert_eq!(app.element_of(a), a_el, "into the same element");
    assert_eq!(app.props_of(a), obj(json!({ "id": "T-1" })), "with the same props");
    assert!(touched(&r, at).iter().all(|n| !b_nodes.contains(n)), "b-two untouched: {:?}", r.calls_since(at));
    assert_eq!(app.drawn_from(a).0, "v2");
    assert_eq!(app.drawn_from(bm).0, "v1", "b-two still drawn from v1: its hashes did not change");

    // Another shape: the new initial state.
    let a3 = tree("a-one", &["id"], json!({ "n": 0, "extra": "" }), r#""tres " + id + ": " + state.n"#, BUMP, &[]);
    let v3 = manifest(&[("a-one", &a3, BUMP, &[]), ("b-two", &b, NONE, &[])], &mut files);
    assert_eq!(app.swap("v3", v3, &files).remounted, [a]);
    assert_eq!(app.state_of(a), obj(json!({ "n": 0, "extra": "" })), "a new shape starts from its initial state");
    assert_eq!(r.html(0), "<a-one><p>tres T-1: 0</p></a-one><b-two><p>two T-9</p></b-two>");

    // Only the handlers changed: remounted; the new fold runs.
    let a4 = tree("a-one", &["id"], json!({ "n": 0, "extra": "" }), r#""tres " + id + ": " + state.n"#, BUMP_TEN, &[]);
    let v4 = manifest(&[("a-one", &a4, BUMP_TEN, &[]), ("b-two", &b, NONE, &[])], &mut files);
    assert_eq!(app.swap("v4", v4, &files).remounted, [a]);
    r.fire(r.find_tag("p")[0], "click", json!({}));
    assert_eq!(r.html(0), "<a-one><p>tres T-1: 10</p></a-one><b-two><p>two T-9</p></b-two>");

    // The same manifest again: nothing.
    let at = r.mark();
    let v4 = manifest(&[("a-one", &a4, BUMP_TEN, &[]), ("b-two", &b, NONE, &[])], &mut files);
    assert_eq!(app.swap("v4", v4, &files), SwapReport::default());
    assert!(r.calls_since(at).is_empty());
}

/// The new tree is malformed: the old stays drawn, its state and nodes as
/// they were, and the failure is recorded on the console; handlers refused
/// (a call outside `uses`) likewise. At first mount, a refused component
/// is drawn as failed and the console names the method.
#[test]
fn a_failed_mount_keeps_the_last_good_version() {
    let (r, app) = app();
    let mut files = Files::default();
    let a1 = tree("a-one", &["id"], json!({ "n": 0 }), r#""one " + id + ": " + state.n"#, BUMP, &[]);
    let v1 = manifest(&[("a-one", &a1, BUMP, &[])], &mut files);
    let v1_tree = v1.components["a-one"].tree.clone();
    app.load("v1", v1, &files).expect("loads");
    let a = app.mount(&r.root(), "a-one", obj(json!({ "id": "T-1" }))).expect("mounts");
    r.fire(r.find_tag("p")[0], "click", json!({}));
    let before = r.html(0);
    assert_eq!(before, "<a-one><p>one T-1: 1</p></a-one>");

    let at = r.mark();
    let broken = r#"{ "name": "a-one", "abi": "engine/1", "tree": [ { "el": "#;
    let v2 = manifest(&[("a-one", broken, BUMP, &[])], &mut files);
    let report = app.swap("v2", v2, &files);
    assert!(report.remounted.is_empty());
    assert_eq!(report.failed.len(), 1, "a-one failed: {report:?}");
    assert_eq!(report.failed[0].0, a);
    assert!(r.calls_since(at).is_empty(), "nothing drawn: the old stays as it was");
    assert_eq!(r.html(0), before);
    assert_eq!(app.state_of(a), obj(json!({ "n": 1 })));
    assert_eq!(app.drawn_from(a), ("v1".to_string(), v1_tree.clone(), sha256_hex(BUMP)), "still v1's");
    let said = app.console().join("\n");
    assert!(said.contains("a-one") && said.contains("last good"), "the console says so: {said}");

    // Handlers that call outside `uses`: refused, the old kept.
    let sneaky = "fn bump(state, ev) { call(\"tickets.delete\", #{ ids: [id] }); state }";
    let a3 = tree("a-one", &["id"], json!({ "n": 0 }), r#""three " + id"#, sneaky, &["tickets.update"]);
    let v3 = manifest(&[("a-one", &a3, sneaky, &["tickets.update"])], &mut files);
    let report = app.swap("v3", v3, &files);
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].1.contains("tickets.delete"), "why names the method: {}", report.failed[0].1);
    assert_eq!(r.html(0), before);
    // Still live on its last good: its fold runs.
    r.fire(r.find_tag("p")[0], "click", json!({}));
    assert_eq!(r.html(0), "<a-one><p>one T-1: 2</p></a-one>");

    // At first mount: drawn as failed, the console naming the method.
    let (r, app) = self::app();
    let mut files = Files::default();
    let v1 = manifest(&[("c-bad", &a3, sneaky, &["tickets.update"])], &mut files);
    app.load("v1", v1, &files).expect("a manifest loads; its components mount one by one");
    let refused = app.mount(&r.root(), "c-bad", obj(json!({ "id": "T-1" })));
    assert!(matches!(&refused, Err(SwapError::Failed { component, message }) if component == "c-bad" && message.contains("tickets.delete")), "{refused:?}");
    assert!(r.html(0).starts_with("<c-bad data-failed="), "drawn as failed: {}", r.html(0));
    assert!(app.console().join("\n").contains("tickets.delete"));
}

/// A layout with two sections: `main` takes the manifest's default, `side`
/// is assigned a component of another version (its files given by that
/// version's hash); `slot_changed` for `main` remounts it alone, in its
/// element, its props kept.
#[test]
fn a_section_mounts_its_assignment_and_remounts_on_slot_changed() {
    let (r, app) = app();
    let mut files = Files::default();
    let layout = json!({
        "name": "page-shell", "abi": "engine/1", "props": ["id"], "state": {}, "live": {}, "uses": [],
        "handlers": sha256_hex(NONE),
        "tree": [
            { "el": "header", "children": [ { "text": "head" } ] },
            { "slot": "main", "props": { "id": { "expr": "id" } } },
            { "slot": "side", "props": { "id": { "expr": "id" } } }
        ]
    })
    .to_string();
    let hello = tree("hello-card", &["id"], json!({ "n": 0 }), r#""hello " + id"#, BUMP, &[]);
    let note = tree("note-card", &["id"], json!({}), r#""note " + id"#, NONE, &[]);
    let old_hello = tree("hello-card", &["id"], json!({ "n": 0 }), r#""old hello " + id"#, BUMP, &[]);
    files.put_version("v0", "hello-card", &old_hello, BUMP);
    let mut v1 = manifest(&[("page-shell", &layout, NONE, &[]), ("hello-card", &hello, BUMP, &[]), ("note-card", &note, NONE, &[])], &mut files);
    v1.slots.insert("page-shell".into(), BTreeMap::from([("main".to_string(), "hello-card".to_string()), ("side".to_string(), "note-card".to_string())]));
    app.load("v1", v1, &files).expect("loads");
    app.set_assignments(vec![Assignment { layout: "page-shell".into(), slot: "side".into(), component: "hello-card".into(), version: "v0".into() }]);

    app.mount(&r.root(), "page-shell", obj(json!({ "id": "T-1" }))).expect("the layout mounts");
    assert_eq!(
        r.html(0),
        concat!(
            "<page-shell><header>head</header>",
            r#"<div data-layout="page-shell" data-slot="main"><hello-card><p>hello T-1</p></hello-card></div>"#,
            r#"<div data-layout="page-shell" data-slot="side"><hello-card><p>old hello T-1</p></hello-card></div>"#,
            "</page-shell>"
        ),
        "main: the default; side: its assignment, from v0"
    );
    let (main, side) = (app.section("page-shell", "main").expect("main"), app.section("page-shell", "side").expect("side"));
    assert_eq!(app.drawn_from(side).0, "v0");
    assert_eq!(app.drawn_from(main).0, "v1");
    assert_eq!(app.props_of(main), obj(json!({ "id": "T-1" })), "the slot's attributes are its props");

    let main_div = r.find_attr("data-slot", "main")[0];
    let keep: Vec<u32> = r.under(r.find_attr("data-slot", "side")[0]).into_iter().chain(r.find_tag("header")).collect();
    let at = r.mark();
    app.slot_changed(Assignment { layout: "page-shell".into(), slot: "main".into(), component: "note-card".into(), version: "v1".into() }, &files)
        .expect("remounts");
    assert_eq!(
        r.html(0),
        concat!(
            "<page-shell><header>head</header>",
            r#"<div data-layout="page-shell" data-slot="main"><note-card><p>note T-1</p></note-card></div>"#,
            r#"<div data-layout="page-shell" data-slot="side"><hello-card><p>old hello T-1</p></hello-card></div>"#,
            "</page-shell>"
        )
    );
    assert!(touched(&r, at).iter().all(|n| !keep.contains(n)), "the side and the header untouched: {:?}", r.calls_since(at));
    assert_eq!(r.find_attr("data-slot", "main"), [main_div], "the section's element kept");
    let now = app.section("page-shell", "main").expect("main");
    assert_eq!(app.props_of(now), obj(json!({ "id": "T-1" })), "its props kept");
    assert_eq!(app.section("page-shell", "side"), Some(side), "the side instance is the same one");
}
