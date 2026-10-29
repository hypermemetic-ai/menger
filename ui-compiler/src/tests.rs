//! UI-5's unit tests: what the declarations fill, what the subset refuses.

use super::{compile, Refusal};
use serde_json::{json, Value};

fn tree(src: &str) -> Value {
    let out = compile(src).unwrap_or_else(|r| panic!("refused at line {}: {}", r.line, r.message));
    serde_json::from_str(&out.tree).expect("the tree is JSON")
}

fn refusal(src: &str) -> Refusal {
    match compile(src) {
        Err(r) => r,
        Ok(c) => panic!("compiled what should be refused:\n{src}\n→ {}", c.tree),
    }
}

/// Refused on `line`, the message naming `found`.
fn refused_at(src: &str, line: u32, found: &str) {
    let r = refusal(src);
    assert_eq!(r.line, line, "the line of `{found}` in:\n{src}\n(message: {})", r.message);
    assert!(r.message.contains(found), "the message names `{found}`: {}", r.message);
}

#[test]
fn uses_are_the_calls_and_the_live_methods() {
    let src = r#"<script>
  let rows = live(#{ snapshot: "tickets.list", params: #{ summary: true },
                     changes: "tickets.watch", key: "id", on: #{ created: "upsert:ticket" } });
  fn rename(state, ev) { call("tickets.update", #{ id: ev.id, title: ev.value }); state }
  fn retitle(state, ev) { call("tickets.update", #{ id: ev.id, title: "x" }); state }
</script>
<ul>{#each rows as r (r.id)}<li>{r.title}</li>{/each}</ul>
"#;
    let t = tree(src);
    assert_eq!(t["uses"], json!(["tickets.list", "tickets.update", "tickets.watch"]), "sorted, deduplicated");
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
fn an_unknown_form_is_refused_naming_the_line() {
    let head = "<script>\n  prop(\"id\");\n</script>\n\n"; // the template starts on line 5
    refused_at(&format!("{head}<div>\n{{#await load}}<p>…</p>{{/await}}\n</div>\n"), 6, "{#await");
    refused_at(&format!("{head}<div>\n  <input bind:value={{name}}/>\n</div>\n"), 6, "bind:value");
    // A bare <slot> (Svelte's default slot) is not the sections form: a slot
    // names its section (`<slot name="main" …/>`, UI-4's sections).
    refused_at(&format!("{head}<div>\n\n  <slot/>\n</div>\n"), 7, "<slot");
    refused_at(&format!("{head}<div>\n{{#if open}}\n  <p>x</p>\n</div>\n"), 6, "{#if");
    refused_at(&format!("{head}<section>\n  <frobnicate>x</frobnicate>\n</section>\n"), 6, "frobnicate");
    refused_at(&format!("{head}{{#key id}}<p/>{{/key}}\n"), 5, "{#key");
    refused_at(&format!("{head}<p>\n  {{@html body}}\n</p>\n"), 6, "{@html");
}

#[test]
fn a_handler_that_is_not_a_fold_is_refused() {
    for params in ["", "a", "a, b, c"] {
        let src = format!("<script>\n  fn spin({params}) {{ 1 }}\n</script>\n<p/>\n");
        let r = refusal(&src);
        assert!(r.message.contains("spin"), "fn spin({params}): the refusal names the function: {}", r.message);
        assert_eq!(r.line, 2, "fn spin({params}): its line");
    }
    let ok = compile("<script>\n  fn spin(state, ev) { state }\n</script>\n<p on:click={spin}/>\n");
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
