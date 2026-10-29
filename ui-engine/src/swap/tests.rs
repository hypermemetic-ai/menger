//! UI-6's testing surface: hot-swap, the last good version, the sections,
//! the section menu.

use super::*;
use crate::live::ManualClock;
use crate::transport::FakeWire;
use crate::tree::{Call, Recording};
use serde_json::json;
use std::rc::Rc;

const BUMP: &str = "fn bump(state, ev) { state.n += 1; state }\n";
const BUMP_TEN: &str = "fn bump(state, ev) { state.n += 10; state }\n";
const NONE: &str = "";

/// A compiled tree for `name`: one `p` (clicks go to `bump` when it has
/// handlers) whose text is `text`, a Rhai expression; `state` its declared
/// state; `uses` its methods.
fn tree(name: &str, props: &[&str], state: Value, text: &str, handlers: &str, uses: &[&str]) -> String {
    let p = if handlers.is_empty() {
        json!({ "el": "p", "children": [ { "text": { "expr": text } } ] })
    } else {
        json!({ "el": "p", "on": { "click": "bump" }, "children": [ { "text": { "expr": text } } ] })
    };
    json!({
        "name": name, "abi": "engine/1", "props": props, "state": state, "live": {}, "uses": uses,
        "handlers": sha256_hex(handlers), "tree": [ p ]
    })
    .to_string()
}

/// Version `version`'s manifest of `components` — `(name, tree, handlers,
/// uses)` — each component's files kept in `files` under that version.
fn manifest(version: &str, components: &[(&str, &str, &str, &[&str])], files: &mut Files) -> Manifest {
    let mut m = Manifest { abi: "engine/1".into(), contract: "unknown".into(), commit: "menger@0000000".into(), ..Manifest::default() };
    for (name, tree, handlers, uses) in components {
        files.put_version(version, name, tree, handlers);
        let entry = ComponentEntry { tree: sha256_hex(tree), handlers: sha256_hex(handlers), uses: uses.iter().map(|u| u.to_string()).collect() };
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
    let v1 = manifest("v1", &[("a-one", &a1, BUMP, &[]), ("b-two", &b, NONE, &[])], &mut files);
    app.provide(&files);
    app.load("v1", v1);
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
    let v2 = manifest("v2", &[("a-one", &a2, BUMP, &[]), ("b-two", &b, NONE, &[])], &mut files);
    app.provide(&files);
    let report = app.swap("v2", v2);
    assert_eq!(report, SwapReport { remounted: vec![a], failed: vec![] }, "only a-one's hash changed");
    assert_eq!(r.html(0), "<a-one><p>uno T-1: 2</p></a-one><b-two><p>two T-9</p></b-two>", "remounted, its state carried over");
    assert_eq!(app.element_of(a), a_el, "into the same element");
    assert_eq!(app.props_of(a), obj(json!({ "id": "T-1" })), "with the same props");
    assert!(touched(&r, at).iter().all(|n| !b_nodes.contains(n)), "b-two untouched: {:?}", r.calls_since(at));
    assert_eq!(app.drawn_from(a).0, "v2");
    assert_eq!(app.drawn_from(bm).0, "v1", "b-two still drawn from v1: its hashes did not change");

    // Another shape: the new initial state.
    let a3 = tree("a-one", &["id"], json!({ "n": 0, "extra": "" }), r#""tres " + id + ": " + state.n"#, BUMP, &[]);
    let v3 = manifest("v3", &[("a-one", &a3, BUMP, &[]), ("b-two", &b, NONE, &[])], &mut files);
    app.provide(&files);
    assert_eq!(app.swap("v3", v3).remounted, [a]);
    assert_eq!(app.state_of(a), obj(json!({ "n": 0, "extra": "" })), "a new shape starts from its initial state");
    assert_eq!(r.html(0), "<a-one><p>tres T-1: 0</p></a-one><b-two><p>two T-9</p></b-two>");

    // Only the handlers changed: remounted; the new fold runs.
    let a4 = tree("a-one", &["id"], json!({ "n": 0, "extra": "" }), r#""tres " + id + ": " + state.n"#, BUMP_TEN, &[]);
    let v4 = manifest("v4", &[("a-one", &a4, BUMP_TEN, &[]), ("b-two", &b, NONE, &[])], &mut files);
    app.provide(&files);
    assert_eq!(app.swap("v4", v4.clone()).remounted, [a]);
    r.fire(r.find_tag("p")[0], "click", json!({}));
    assert_eq!(r.html(0), "<a-one><p>tres T-1: 10</p></a-one><b-two><p>two T-9</p></b-two>");

    // The same hashes again: nothing.
    let at = r.mark();
    assert_eq!(app.swap("v5", v4), SwapReport::default());
    assert!(r.calls_since(at).is_empty());
}

/// The new tree is malformed: the old DOM stays, its state and nodes as
/// they were, the element carries `data-failed` with the version that
/// failed, and the console names the component and the version; handlers
/// refused (a call outside `uses`) likewise. At first mount, a refused
/// component is drawn as failed and the console names the method.
#[test]
fn a_failed_mount_keeps_the_last_good_version() {
    let (r, app) = app();
    let mut files = Files::default();
    let a1 = tree("a-one", &["id"], json!({ "n": 0 }), r#""one " + id + ": " + state.n"#, BUMP, &[]);
    let v1 = manifest("v1", &[("a-one", &a1, BUMP, &[])], &mut files);
    let v1_tree = v1.components["a-one"].tree.clone();
    app.provide(&files);
    app.load("v1", v1);
    let a = app.mount(&r.root(), "a-one", obj(json!({ "id": "T-1" }))).expect("mounts");
    r.fire(r.find_tag("p")[0], "click", json!({}));
    assert_eq!(r.html(0), "<a-one><p>one T-1: 1</p></a-one>");
    let inner = r.under(app.element_of(a));

    let at = r.mark();
    let broken = r#"{ "name": "a-one", "abi": "engine/1", "tree": [ { "el": "#;
    let v2 = manifest("v2", &[("a-one", broken, BUMP, &[])], &mut files);
    app.provide(&files);
    let report = app.swap("v2", v2);
    assert!(report.remounted.is_empty());
    assert_eq!(report.failed.len(), 1, "a-one failed: {report:?}");
    assert_eq!(report.failed[0].0, a);
    assert_eq!(r.html(0), r#"<a-one data-failed="v2"><p>one T-1: 1</p></a-one>"#, "the last good DOM, marked");
    assert_eq!(r.under(app.element_of(a)), inner, "the same nodes");
    assert!(r.calls_since(at).iter().all(|c| matches!(c, Call::SetAttr { name, .. } if name == "data-failed")), "nothing drawn but the mark: {:?}", r.calls_since(at));
    assert_eq!(app.state_of(a), obj(json!({ "n": 1 })));
    assert_eq!(app.drawn_from(a), ("v1".to_string(), v1_tree.clone(), sha256_hex(BUMP)), "still v1's");
    let said = app.console().join("\n");
    assert!(said.contains("a-one") && said.contains("v2"), "the console names the component and the version: {said}");

    // Handlers that call outside `uses`: refused, the old kept.
    let sneaky = "fn bump(state, ev) { call(\"tickets.delete\", #{ ids: [ev.props.id] }); state }\n";
    let a3 = tree("a-one", &["id"], json!({ "n": 0 }), r#""three " + id"#, sneaky, &["tickets.update"]);
    let v3 = manifest("v3", &[("a-one", &a3, sneaky, &["tickets.update"])], &mut files);
    app.provide(&files);
    let report = app.swap("v3", v3);
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].1.contains("tickets.delete"), "why names the method: {}", report.failed[0].1);
    assert_eq!(r.html(0), r#"<a-one data-failed="v3"><p>one T-1: 1</p></a-one>"#);
    // Still live on its last good: its fold runs.
    r.fire(r.find_tag("p")[0], "click", json!({}));
    assert_eq!(r.html(0), r#"<a-one data-failed="v3"><p>one T-1: 2</p></a-one>"#);

    // At first mount: drawn as failed, the console naming the method.
    let (r, app) = self::app();
    let mut files = Files::default();
    let v1 = manifest("v1", &[("c-bad", &a3, sneaky, &["tickets.update"])], &mut files);
    app.provide(&files);
    app.load("v1", v1);
    let refused = app.mount(&r.root(), "c-bad", obj(json!({ "id": "T-1" })));
    assert!(matches!(&refused, Err(SwapError::Failed { component, message }) if component == "c-bad" && message.contains("tickets.delete")), "{refused:?}");
    assert_eq!(r.html(0), r#"<c-bad data-failed="v1"></c-bad>"#, "drawn as failed");
    assert!(app.console().join("\n").contains("tickets.delete"));
}

/// A layout with two sections: `main` takes the manifest's default, `side`
/// is assigned a component of another version — the engine asks for that
/// version's files (`needs`), and draws it when they come; `slot_changed`
/// for `main` remounts it alone, in its element, its props kept.
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
    let mut v1 = manifest("v1", &[("page-shell", &layout, NONE, &[]), ("hello-card", &hello, BUMP, &[]), ("note-card", &note, NONE, &[])], &mut files);
    v1.slots.insert("page-shell".into(), BTreeMap::from([("main".to_string(), "hello-card".to_string()), ("side".to_string(), "note-card".to_string())]));
    app.provide(&files);
    app.load("v1", v1);
    app.set_assignments(vec![Assignment { layout: "page-shell".into(), slot: "side".into(), component: "hello-card".into(), version: "v0".into() }]);

    app.mount(&r.root(), "page-shell", obj(json!({ "id": "T-1" }))).expect("the layout mounts");
    assert_eq!(app.needs(), [("v0".to_string(), "hello-card".to_string())], "the engine asks for v0's hello-card");
    assert_eq!(
        r.html(0),
        concat!(
            "<page-shell><header>head</header>",
            r#"<div data-layout="page-shell" data-slot="main"><hello-card><p>hello T-1</p></hello-card></div>"#,
            r#"<div data-layout="page-shell" data-slot="side"></div>"#,
            "</page-shell>"
        ),
        "main: the default; side: waiting on v0's files"
    );
    let mut fetched = Files::default();
    fetched.put_version("v0", "hello-card", &old_hello, BUMP);
    app.provide(&fetched);
    assert!(app.needs().is_empty(), "nothing more asked");
    assert_eq!(
        r.html(0),
        concat!(
            "<page-shell><header>head</header>",
            r#"<div data-layout="page-shell" data-slot="main"><hello-card><p>hello T-1</p></hello-card></div>"#,
            r#"<div data-layout="page-shell" data-slot="side"><hello-card><p>old hello T-1</p></hello-card></div>"#,
            "</page-shell>"
        ),
        "side: its assignment, from v0"
    );
    let (main, side) = (app.section("page-shell", "main").expect("main"), app.section("page-shell", "side").expect("side"));
    assert_eq!(app.drawn_from(side).0, "v0");
    assert_eq!(app.drawn_from(main).0, "v1");
    assert_eq!(app.props_of(main), obj(json!({ "id": "T-1" })), "the slot's attributes are its props");

    let main_div = r.find_attr("data-slot", "main")[0];
    let keep: Vec<u32> = r.under(r.find_attr("data-slot", "side")[0]).into_iter().chain(r.find_tag("header")).collect();
    let at = r.mark();
    app.slot_changed(Assignment { layout: "page-shell".into(), slot: "main".into(), component: "note-card".into(), version: "v1".into() });
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

    // Cleared: back to the default.
    app.slot_reset("page-shell", "main");
    assert!(r.html(0).contains(r#"data-slot="main"><hello-card><p>hello T-1</p></hello-card></div>"#), "{}", r.html(0));
}

/// The section menu: the live bundle's components by name; for the
/// section's component, only the stored versions whose hash for it differs
/// from the live one's, newest first, the live one excluded.
#[test]
fn the_menu_lists_live_components_and_other_versions_newest_first() {
    let (r, app) = app();
    let mut files = Files::default();
    let layout = json!({
        "name": "page-shell", "abi": "engine/1", "props": [], "state": {}, "live": {}, "uses": [],
        "handlers": sha256_hex(NONE), "tree": [ { "slot": "main", "props": {} } ]
    })
    .to_string();
    let hello = tree("hello-card", &[], json!({}), r#""hello""#, NONE, &[]);
    let note = tree("note-card", &[], json!({}), r#""note""#, NONE, &[]);
    let mut v4 = manifest("v4", &[("page-shell", &layout, NONE, &[]), ("hello-card", &hello, NONE, &[]), ("note-card", &note, NONE, &[])], &mut files);
    v4.slots.insert("page-shell".into(), BTreeMap::from([("main".to_string(), "hello-card".to_string())]));
    app.provide(&files);
    app.load("v4", v4.clone());
    app.mount(&r.root(), "page-shell", Map::new()).expect("mounts");

    let entry = |text: &str| Some(ComponentEntry { tree: sha256_hex(text), handlers: sha256_hex(NONE), uses: vec![] });
    let live = v4.components["hello-card"].clone();
    let versions = vec![
        ("v4".to_string(), Some(live.clone())),  // the live one
        ("v3".to_string(), Some(live)),          // the same hash as live
        ("v2".to_string(), entry("two")),
        ("v1".to_string(), entry("one")),
        ("v0".to_string(), None),                // no hello-card in it
    ];
    let menu = app.menu("page-shell", "main", &versions);
    assert_eq!(menu.component, "hello-card");
    assert_eq!(menu.components, ["hello-card", "note-card", "page-shell"], "the live bundle's components by name");
    assert_eq!(menu.versions, ["v2", "v1"], "only differing hashes, newest first, live excluded");
}

/// UI-8: a declared prop the mount does not pass is there as null — the
/// route `#/` gives `board-layout` no `filter`, and its expressions read
/// `filter == ()` rather than fail on a name not found.
#[test]
fn a_declared_prop_not_passed_is_null() {
    let (r, app) = app();
    let mut files = Files::default();
    let t = tree("board-layout", &["filter"], json!({}), r#"if filter == () { "none" } else { filter }"#, NONE, &[]);
    let m = manifest("v1", &[("board-layout", &t, NONE, &[])], &mut files);
    app.provide(&files);
    app.load("v1", m);
    let root = r.root();
    let id = app.mount(&root, "board-layout", Map::new()).expect("mounted");
    assert_eq!(app.props_of(id), obj(json!({ "filter": null })));
    assert!(r.html(root).contains("none"), "drawn as none: {}", r.html(root));
    let id2 = app.mount(&root, "board-layout", obj(json!({ "filter": "status Ready" }))).expect("mounted");
    assert_eq!(app.props_of(id2), obj(json!({ "filter": "status Ready" })));
}

/// A component whose `live` reads `state.q` with `debounce_ms: 80`, and a
/// handler that sets `state.q`: typing runs the snapshot again once, 80 ms
/// after, with the new params — through eval_params and the wiring.
#[test]
fn a_handler_moves_a_lives_params_and_it_re_runs_after_its_debounce() {
    let (r, wire, clock) = (Recording::new(), FakeWire::new(), ManualClock::new());
    let app = App::new(&Runtime::new(), r.clone(), wire.clone(), clock.clone());
    let mut files = Files::default();
    let typed = "fn typed(state, ev) { state.q = ev.value; state }\n";
    let palette = json!({
        "name": "jump-palette", "abi": "engine/1", "props": [], "state": { "q": "" }, "uses": ["tickets.find"],
        "live": { "found": { "snapshot": "tickets.find", "params": { "query": { "expr": "state.q" }, "limit": 20 }, "changes_params": {}, "key": "id", "on": {}, "debounce_ms": 80 } },
        "handlers": sha256_hex(typed),
        "tree": [
            { "el": "input", "on": { "input": "typed" }, "children": [] },
            { "el": "p", "children": [ { "text": { "expr": "found.status" } } ] }
        ]
    })
    .to_string();
    let v1 = manifest("v1", &[("jump-palette", &palette, typed, &["tickets.find"])], &mut files);
    app.provide(&files);
    app.load("v1", v1);
    app.mount(&r.root(), "jump-palette", Map::new()).expect("mounts");
    assert_eq!(wire.opened_of("tickets.find").len(), 1, "the first snapshot at mount");
    assert_eq!(wire.opened_of("tickets.find")[0].params, json!({ "query": "", "limit": 20 }));
    assert!(wire.opened_of("tickets.watch").is_empty(), "a live without changes: no watch");
    wire.answer(wire.last_of("tickets.find"), [json!({ "type": "summary", "summary": { "id": "T-1" } })]);
    assert!(r.html(0).contains("<p>live</p>"), "{}", r.html(0));

    let input = r.find_tag("input")[0];
    r.fire(input, "input", json!({ "value": "kil" }));
    clock.advance(30);
    r.fire(input, "input", json!({ "value": "kiln" }));
    clock.advance(79);
    assert_eq!(wire.opened_of("tickets.find").len(), 1, "79 ms after the last keystroke: not yet");
    clock.advance(1);
    assert_eq!(wire.opened_of("tickets.find").len(), 2, "80 ms after: once");
    assert_eq!(wire.opened_of("tickets.find")[1].params, json!({ "query": "kiln", "limit": 20 }), "with the new params");
    assert!(r.html(0).contains("<p>resyncing</p>"), "the old rows kept while it runs: {}", r.html(0));
    wire.answer(wire.last_of("tickets.find"), []);
    assert!(r.html(0).contains("<p>live</p>"));
}
