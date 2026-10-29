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
// of snapshot / changes / key, and a prop or state declared twice, refused
// naming it.
#[test]
fn an_incomplete_live_or_a_declaration_twice_is_refused() {
    for missing in ["snapshot", "changes", "key"] {
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
