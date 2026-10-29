//! UI-5's unit tests: what the declarations fill, what the subset refuses.

use super::{compile, Refusal};
use serde_json::{json, Value};

#[path = "../tests/common/cases.rs"]
mod cases;

const NAME: &str = "test-view";

fn tree(src: &str) -> Value {
    let out = compile(NAME, src).unwrap_or_else(|r| panic!("refused at line {}: {}\n{src}", r.line, r.message));
    serde_json::from_str(&out.tree).expect("the tree is JSON")
}

fn refusal(src: &str) -> Refusal {
    match compile(NAME, src) {
        Err(r) => r,
        Ok(c) => panic!("compiled what should be refused:\n{src}\n→ {}", c.tree),
    }
}

/// Refused on `line`, the message naming `found`.
fn refused_at(what: &str, src: &str, line: u32, found: &str) {
    let r = refusal(src);
    assert_eq!(r.line, line, "{what}: the line of `{found}` in:\n{src}\n(message: {})", r.message);
    assert!(r.message.contains(found), "{what}: the message names `{found}`: {}", r.message);
}

#[test]
fn uses_are_the_calls_and_the_live_methods() {
    let src = r#"<script>
  let rows = live(#{ snapshot: "tickets.list", params: #{ summary: true },
                     changes: "tickets.watch", key: "id", on: #{ created: "upsert:ticket" } });
  fn rename(state, ev) { call("tickets.update", #{ id: ev.props.id, title: ev.value }); state }
  // call("tickets.delete", #{ id: 1 }) — a comment calls nothing
  fn follow(state, ev) { call("tickets.watch", #{ id: ev.props.id }); state }
</script>
<ul>{#each rows as r (r.id)}<li>{r.title}</li>{/each}</ul>
"#;
    let t = tree(src);
    assert_eq!(t["uses"], json!(["tickets.list", "tickets.update", "tickets.watch"]), "sorted, deduplicated, no comment");

    // A method that is not a literal cannot be checked against `uses`.
    let src = "<script>\n  fn fetch(state, ev) {\n    let m = \"tickets.list\";\n    call(m, #{});\n    state\n  }\n</script>\n<p/>\n";
    refused_at("a non-literal call", src, 4, "call");
}

#[test]
fn props_and_state_come_from_the_declarations() {
    let src = r#"<script>
  prop("id");
  prop("title");
  state(#{ expanded: false, count: 0, label: "open", tags: ["a", "b"], inner: #{ deep: true } });
</script>
<p>{nowhere.declared} {title}</p>
"#;
    let t = tree(src);
    assert_eq!(t["name"], json!(NAME));
    assert_eq!(t["props"], json!(["id", "title"]));
    assert_eq!(
        t["state"],
        json!({ "count": 0, "expanded": false, "inner": { "deep": true }, "label": "open", "tags": ["a", "b"] })
    );
    assert_eq!(t["live"], json!({}), "no live declared: an empty map");
    assert_eq!(t["uses"], json!([]), "no call, no live: no uses");

    // No declarations at all: empty props and state, not missing ones.
    let bare = tree("<p>hello</p>\n");
    assert_eq!(bare["props"], json!([]));
    assert_eq!(bare["state"], json!({}));
}

#[test]
fn each_reads_a_live_collections_rows_and_nothing_else() {
    let src = r#"<script>
  prop("items");
  let rows = live(#{ snapshot: "tickets.list", changes: "tickets.watch", key: "id" });
</script>
{#each items as x}<p>{x}</p>{/each}
<p>{rows.status}</p>
{#each rows as r (r.id)}<p>{r.id}</p>{/each}
"#;
    let t = tree(src);
    assert_eq!(t["tree"][0]["each"], json!({ "expr": "items", "as": "x" }), "a prop stays itself; no key, none written");
    assert_eq!(t["tree"][1]["children"][0]["text"], json!({ "expr": "rows.status" }), "outside each, as written");
    assert_eq!(t["tree"][2]["each"], json!({ "expr": "rows.rows", "as": "r", "key": "r.id" }), "a bare live reads its rows");

    // The name is the only difference between two names on one source.
    let a = compile("row-a", src).expect("compiles");
    let b = compile("row-b", src).expect("compiles");
    assert_eq!(a.handlers, b.handlers);
    assert_ne!(a.tree, b.tree);
    assert_eq!(a.tree.replace("\"name\": \"row-a\"", "\"name\": \"row-b\""), b.tree);
}

#[test]
fn an_unknown_form_is_refused_naming_the_line() {
    for c in cases::unknown_forms().into_iter().chain(cases::line_offsets()) {
        refused_at(c.what, &c.src, c.line, c.found);
    }
}

#[test]
fn a_slot_is_a_named_section() {
    for c in cases::slot_refusals() {
        refused_at(c.what, &c.src, c.line, c.found);
    }
    // One name in both arms of an if/else is one section.
    let t = tree("{#if wide}<slot name=\"main\"/>{:else}<slot name=\"main\" compact=\"yes\"/>{/if}\n");
    assert_eq!(t["tree"][0]["then"][0], json!({ "slot": "main", "props": {} }));
    assert_eq!(t["tree"][0]["else"][0], json!({ "slot": "main", "props": { "compact": "yes" } }));
}

#[test]
fn a_handler_that_is_not_a_fold_is_refused() {
    for params in ["", "a", "a, b, c"] {
        let src = format!("<script>\n  fn spin({params}) {{ 1 }}\n</script>\n<p/>\n");
        let r = refusal(&src);
        assert!(r.message.contains("spin"), "fn spin({params}): the refusal names the function: {}", r.message);
        assert_eq!(r.line, 2, "fn spin({params}): its line");
    }
    let ok = compile(NAME, "<script>\n  fn spin(state, ev) { state }\n</script>\n<p on:click={spin}/>\n");
    assert!(ok.is_ok(), "fn spin(state, ev) is a fold: {ok:?}");
}

// Required behaviour beyond the testing surface's rows: a `live` missing any
// of snapshot / key (UI-10: changes may be absent — a one-shot read), and a
// prop or state declared twice, refused
// naming it.
#[test]
fn an_incomplete_live_or_a_declaration_twice_is_refused() {
    for missing in ["snapshot", "key"] {
        let mut fields = vec![r#"snapshot: "tickets.list""#, r#"changes: "tickets.watch""#, r#"key: "id""#];
        fields.retain(|f| !f.starts_with(missing));
        let src = format!("<script>\n  let rows = live(#{{ {} }});\n</script>\n<p/>\n", fields.join(", "));
        let r = refusal(&src);
        assert!(r.message.contains(missing), "a live without {missing}: {}", r.message);
        assert!(r.message.contains("rows"), "the refusal names the live: {}", r.message);
        assert_eq!(r.line, 2);
    }
    let r = refusal("<script>\n  prop(\"id\");\n  prop(\"id\");\n</script>\n<p/>\n");
    assert!(r.message.contains("id"), "a prop twice names it: {}", r.message);
    assert_eq!(r.line, 3, "the second declaration's line");
    let r = refusal("<script>\n  state(#{ a: 1 });\n  state(#{ b: 2 });\n</script>\n<p/>\n");
    assert!(r.message.contains("state"), "state twice names it: {}", r.message);
    assert_eq!(r.line, 3);
}

// UI-4, "The compiler's choices, pinned": a live holds snapshot, params,
// changes, changes_params, key and on; params, changes_params and on are
// always written, {} when absent.
#[test]
fn a_live_carries_its_changes_params() {
    let src = r#"<script>
  prop("epic");
  let rows = live(#{ snapshot: "tickets.list", params: #{ epic: epic, limit: 50 },
                     changes: "tickets.watch", changes_params: #{ epic: epic, summary: true }, key: "id" });
</script>
<p/>
"#;
    let t = tree(src);
    assert_eq!(t["live"]["rows"]["changes_params"], json!({ "epic": { "expr": "epic" }, "summary": true }));
    assert_eq!(t["live"]["rows"]["on"], json!({}), "on absent: {{}}");

    let bare = tree("<script>\n  let rows = live(#{ snapshot: \"tickets.list\", changes: \"tickets.watch\", key: \"id\" });\n</script>\n<p/>\n");
    for f in ["params", "changes_params", "on"] {
        assert_eq!(bare["live"]["rows"][f], json!({}), "{f} absent: {{}}");
    }

    let r = refusal("<script>\n  let rows = live(#{ snapshot: \"a.b\", changes: \"a.c\", key: \"id\", filter: #{} });\n</script>\n<p/>\n");
    assert!(r.message.contains("filter"), "a sixth field is refused, naming it: {}", r.message);
    assert_eq!(r.line, 2);
}

// ── UI-10: the second cut ─────────────────────────────────────────────────

/// `{@html expr}` → an `html` node holding the expression.
#[test]
fn html_is_a_node_of_its_own() {
    let t = tree("<script>\n  prop(\"body\");\n</script>\n<article>{@html markdown(body)}</article>\n");
    assert_eq!(t["tree"], json!([{ "children": [{ "html": { "expr": "markdown(body)" } }], "el": "article" }]));
    refused_at("an empty {@html}", "<p>{@html }</p>\n", 1, "@html");
    refused_at("another {@…}", "<p>{@debug x}</p>\n", 1, "@debug");
}

/// A `live` without `changes` is a one-shot read: no `changes` in the
/// tree, and only its snapshot in `uses`; `debounce_ms` is written when
/// given, a whole number of milliseconds.
#[test]
fn a_live_may_have_no_changes_and_a_debounce() {
    let src = r#"<script>
  state(#{ q: "" });
  let ext = live(#{ snapshot: "tickets.nav.extents", key: "epic", on: #{ extent: "upsert:." } });
  let found = live(#{ snapshot: "tickets.find", params: #{ query: state.q }, key: "id", debounce_ms: 80 });
</script>
<p>{ext.status} {found.status}</p>
"#;
    let t = tree(src);
    assert_eq!(t["live"]["ext"], json!({ "changes_params": {}, "key": "epic", "on": { "extent": "upsert:." }, "params": {}, "snapshot": "tickets.nav.extents" }), "no changes key at all");
    assert_eq!(t["live"]["found"]["debounce_ms"], json!(80));
    assert_eq!(t["live"]["found"]["params"], json!({ "query": { "expr": "state.q" } }), "a param reads the scope");
    assert!(t["live"]["ext"].get("debounce_ms").is_none(), "no debounce_ms unless given");
    assert_eq!(t["uses"], json!(["tickets.find", "tickets.nav.extents"]), "the snapshots alone");

    let bad = "<script>\n  let x = live(#{ snapshot: \"tickets.find\", key: \"id\", debounce_ms: \"80\" });\n</script>\n<p/>\n";
    refused_at("a debounce that is no number", bad, 2, "debounce_ms");
    let bad = "<script>\n  let x = live(#{ snapshot: \"tickets.find\", key: \"id\", debounce_ms: -1 });\n</script>\n<p/>\n";
    refused_at("a negative debounce", bad, 2, "debounce_ms");
}

/// `on:window-keydown={h}` → the event kind `window-keydown` on the
/// element, its handler checked as any other's.
#[test]
fn a_window_event_is_an_event_kind() {
    let src = "<script>\n  fn keyed(state, ev) { state }\n</script>\n<div on:window-keydown={keyed}><p>x</p></div>\n";
    let t = tree(src);
    assert_eq!(t["tree"][0]["on"], json!({ "window-keydown": "keyed" }));
    refused_at("a window event naming no fn", "<div on:window-keydown={nothing}></div>\n", 1, "nothing");
    // The component's root element only (UI-4): nested, refused at its line.
    let nested = "<script>\n  fn keyed(state, ev) { state }\n</script>\n<div>\n  <p on:window-keydown={keyed}>x</p>\n</div>\n";
    refused_at("a window event below the root", nested, 5, "window-keydown");
    let in_if = "<script>\n  fn keyed(state, ev) { state }\n</script>\n{#if true}\n  <div on:window-keydown={keyed}></div>\n{/if}\n";
    refused_at("a window event inside a block", in_if, 5, "window-keydown");
    // An element's own events stay where they are.
    let own = "<script>\n  fn keyed(state, ev) { state }\n</script>\n<div>\n  <input on:keydown={keyed}/>\n</div>\n";
    assert_eq!(tree(own)["tree"][0]["children"][0]["on"], json!({ "keydown": "keyed" }));
}
