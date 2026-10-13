//! THEME-3: a theme is a ticket's block — defined on create, amended,
//! refused (every validation named, nothing written), owned once, orphaned
//! and kept while used, removed when not, adopted; `extends`; the channel;
//! a reopen.

use super::super::*;
use crate::themes::{flatten, Takeover, ThemeReach};
use std::collections::BTreeSet;

fn ticket(epic: &str, body: &str) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {epic} ticket"),
        kind: Kind::Doc,
        severity: None,
        blocked_by: Vec::new(),
        parent: None,
        body: body.into(),
    }
}

fn block(name: &str, css: &str) -> String {
    format!("A theme.\n\n```theme {name}\n{css}\n```\n")
}

const PANTRY: &str = ":root { --accent: #d4572a; --font-display: \"Fraunces\", serif; }\n:root[data-theme=\"dark\"] { --accent: #f08a5d; }\n@import url(\"https://fonts.googleapis.com/css2?family=Fraunces\");\nm-code::part(frame) { border-radius: 14px; }";

fn names(store: &Store) -> Vec<String> {
    store.themes().into_iter().map(|t| t.name).collect()
}

/// Everything the themes channel said, drained.
fn heard(rx: &mut tokio::sync::broadcast::Receiver<ThemeChange>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(c) = rx.try_recv() {
        out.push(match c {
            ThemeChange::Changed(t) => format!("changed {} owner={} orphan={}", t.name, t.owner.as_deref().unwrap_or("-"), t.orphan),
            ThemeChange::Removed(n) => format!("removed {n}"),
            ThemeChange::Set(s) => format!("set {}", s.scope),
        });
    }
    out
}

#[tokio::test]
async fn a_block_defines_its_theme_owned_by_its_ticket_and_amends_move_it() {
    let store = Store::new();
    let mut rx = store.subscribe_themes();
    let t = store.create(ticket("PANTRY", &block("pantry", PANTRY))).await.unwrap();
    let th = store.theme("pantry").unwrap();
    assert_eq!(th.owner.as_deref(), Some(t.id.as_str()));
    assert!(!th.orphan);
    assert_eq!(th.light.get("--accent").map(String::as_str), Some("#d4572a"));
    assert_eq!(th.dark.get("--accent").map(String::as_str), Some("#f08a5d"));
    assert_eq!(th.imports, ["https://fonts.googleapis.com/css2?family=Fraunces"]);
    assert_eq!(th.rules, ["m-code::part(frame){border-radius:14px;}"]);
    assert_eq!(heard(&mut rx), ["changed pantry owner=PANTRY-1 orphan=false"]);
    // An amend that changes the block moves it; one that keeps it says nothing.
    store.amend(&t.id, None, Some(block("pantry", ":root { --accent: #b2401c; }"))).await.unwrap();
    assert_eq!(store.theme("pantry").unwrap().light.get("--accent").map(String::as_str), Some("#b2401c"));
    assert!(store.theme("pantry").unwrap().dark.is_empty());
    assert_eq!(heard(&mut rx), ["changed pantry owner=PANTRY-1 orphan=false"]);
    store.amend(&t.id, Some("retitled".into()), None).await.unwrap();
    store.amend(&t.id, None, Some(format!("{}\nmore prose\n", block("pantry", ":root { --accent: #b2401c; }")))).await.unwrap();
    assert!(heard(&mut rx).is_empty(), "the same block again is no move");
    // tickets.update takes the same path.
    store.update(&t.id, Update { body: Some(block("pantry", ":root { --accent: #123456; }")), ..Default::default() }).await.unwrap();
    assert_eq!(store.theme("pantry").unwrap().light.get("--accent").map(String::as_str), Some("#123456"));
}

/// THEME-3 criterion 2: every validation refuses the write in-band, naming
/// the block's line, and nothing is written — not the body, not the theme.
#[tokio::test]
async fn a_block_that_is_not_a_theme_refuses_the_write_naming_its_line() {
    let store = Store::new();
    let t = store.create(ticket("PANTRY", "plain")).await.unwrap();
    for (css, line, why) in [
        (":root {\n  --accent: red;\n  --nope: 1px;\n}", 3, "is not a token of the contract"),
        ("body { color: red; }", 1, "a theme's rules are :root"),
        (":root { color: red; }", 1, "a theme sets tokens there"),
        ("@import url(\"http://fonts.example/a.css\");", 1, "only an https URL"),
        ("@font-face {\n font-family: x;\n src: url(http://x.example/a.woff2);\n}", 3, "only https URLs"),
        ("m-code::part(frame) { width: expression(1); }", 1, "behaviour, not looks"),
    ] {
        let body = block("pantry", css);
        for err in [
            store.amend(&t.id, None, Some(body.clone())).await.unwrap_err().to_string(),
            store.create(ticket("PANTRY", &body)).await.unwrap_err().to_string(),
            store.update(&t.id, Update { body: Some(body.clone()), ..Default::default() }).await.unwrap_err().to_string(),
        ] {
            assert!(err.contains(&format!("```theme pantry: line {line}: ")) && err.contains(why), "{css}: {err}");
        }
        assert_eq!(store.get(&t.id).await.unwrap().body, "plain", "nothing written");
        assert_eq!(names(&store), ["default"]);
    }
    for (body, why) in [
        ("```theme\n:root{}\n```\n".to_string(), "names no theme"),
        ("```theme default\n:root{}\n```\n".to_string(), "`default` is the contract's"),
        (format!("{}\n{}", block("a", ":root{}"), block("a", ":root{}")), "a second ```theme block"),
    ] {
        let err = store.amend(&t.id, None, Some(body)).await.unwrap_err().to_string();
        assert!(err.contains(why), "{err}");
    }
    assert!(store.get("PANTRY-2").await.is_err(), "no id spent on a refused create");
}

#[tokio::test]
async fn a_theme_has_one_owner_and_extends_never_go_round() {
    let store = Store::new();
    let a = store.create(ticket("PANTRY", &block("pantry", ":root { --accent: #d4572a; }"))).await.unwrap();
    let b = store.create(ticket("OCEAN", "plain")).await.unwrap();
    let err = store.amend(&b.id, None, Some(block("pantry", ":root{}"))).await.unwrap_err().to_string();
    assert!(err.contains("defined by PANTRY-1's block") && err.contains("one owner"), "{err}");
    let err = store.define_theme("pantry", BTreeMap::new(), BTreeMap::new()).await.unwrap_err().to_string();
    assert!(err.contains("defined by PANTRY-1's ```theme block"), "{err}");
    // extends: a chain to default; a cycle is refused, named, whole.
    store.amend(&b.id, None, Some(format!("{}\n{}", block("sea extends=pantry", ":root[data-theme=dark] { --bg: #001; }"), block("deep extends=sea", ":root { --ink: #fff; }")))).await.unwrap();
    let all = store.themes();
    let deep = flatten("deep", |n| all.iter().find(|t| t.name == n));
    assert_eq!(deep.light.get("--accent").map(String::as_str), Some("#d4572a"), "from pantry, two up");
    assert_eq!(deep.dark.get("--bg").map(String::as_str), Some("#001"));
    assert_eq!(deep.light.get("--ink").map(String::as_str), Some("#fff"));
    let err = store.amend(&a.id, None, Some(block("pantry extends=deep", ":root{}"))).await.unwrap_err().to_string();
    assert!(err.contains("its extends go round — pantry → deep → sea → pantry"), "{err}");
    assert_eq!(store.theme("pantry").unwrap().extends, None, "nothing written");
}

/// COMP-3's orphans, for themes: a block gone leaves its theme an orphan,
/// kept while a ticket's frontmatter, a setting or a live theme's extends
/// uses it, removed by the write that leaves it unused; a block naming it
/// adopts it.
#[tokio::test]
async fn an_orphan_is_kept_while_used_removed_when_not_and_adopted_by_a_block() {
    let store = Store::new();
    let mut rx = store.subscribe_themes();
    let owner = store.create(ticket("PANTRY", &block("pantry", ":root { --accent: #d4572a; }"))).await.unwrap();
    let user = store.create(ticket("PANTRY", "---\ntheme: pantry\n---\nwears it")).await.unwrap();
    heard(&mut rx);
    // The block goes: an orphan, kept — a ticket's frontmatter names it.
    store.amend(&owner.id, None, Some("no block".into())).await.unwrap();
    let th = store.theme("pantry").unwrap();
    assert!(th.orphan && th.owner.is_none(), "{th:?}");
    assert_eq!(heard(&mut rx), ["changed pantry owner=- orphan=true"]);
    // A setting keeps it too, once the frontmatter lets go.
    store.set_theme("PANTRY", Some("pantry"), None, None).await.unwrap();
    store.amend(&user.id, None, Some("no longer".into())).await.unwrap();
    assert!(store.theme("pantry").is_ok(), "the setting keeps it");
    heard(&mut rx);
    // The setting lets go: nothing uses it — it goes in that write.
    store.set_theme("PANTRY", None, None, Some(Takeover::Off)).await.unwrap();
    assert!(store.theme("pantry").is_err());
    assert_eq!(heard(&mut rx), ["removed pantry", "set PANTRY"]);
    // A live theme's extends keeps an orphan; the orphan only an orphan
    // extends goes with it.
    let base = store.create(ticket("OCEAN", &format!("{}\n{}", block("base", ":root { --ink: #000; }"), block("mid extends=base", ":root{}")))).await.unwrap();
    let top = store.create(ticket("OCEAN", &block("top extends=mid", ":root{}"))).await.unwrap();
    store.amend(&base.id, None, Some("gone".into())).await.unwrap();
    assert!(store.theme("base").unwrap().orphan && store.theme("mid").unwrap().orphan, "top extends mid extends base");
    heard(&mut rx);
    store.amend(&top.id, None, Some("gone too".into())).await.unwrap();
    assert!(store.theme("top").is_err() && store.theme("mid").is_err() && store.theme("base").is_err());
    let mut said = heard(&mut rx);
    said.sort();
    assert_eq!(said, ["removed base", "removed mid", "removed top"]);
    // Adoption: an orphan, and a theme `define` made, are a block's to take.
    store.create(ticket("OCEAN", &block("sea", ":root{}"))).await.unwrap();
    store.set_theme("site", Some("sea"), None, None).await.unwrap();
    store.amend("OCEAN-3", None, Some("orphaned".into())).await.unwrap();
    let adopter = store.create(ticket("PANTRY", &block("sea", ":root { --bg: #eef; }"))).await.unwrap();
    let sea = store.theme("sea").unwrap();
    assert_eq!((sea.owner.as_deref(), sea.orphan), (Some(adopter.id.as_str()), false));
    store.define_theme("mint", BTreeMap::new(), BTreeMap::new()).await.unwrap();
    store.amend(&adopter.id, None, Some(format!("{}\n{}", block("sea", ":root{}"), block("mint", ":root { --accent: #0a7; }")))).await.unwrap();
    assert_eq!(store.theme("mint").unwrap().owner.as_deref(), Some(adopter.id.as_str()));
}

#[tokio::test]
async fn owners_orphans_and_references_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        let a = store.create(ticket("PANTRY", &block("pantry", PANTRY))).await.unwrap();
        store.create(ticket("PANTRY", &block("sea extends=pantry", ":root{}"))).await.unwrap();
        store.create(ticket("PANTRY", "---\nframe-theme: lost\n---\nnames a theme no one has")).await.unwrap();
        store.amend(&a.id, None, Some("gone".into())).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    let pantry = store.theme("pantry").unwrap();
    assert!(pantry.orphan && pantry.owner.is_none(), "kept: sea extends it");
    assert_eq!(pantry.rules, ["m-code::part(frame){border-radius:14px;}"]);
    let sea = store.theme("sea").unwrap();
    assert_eq!((sea.owner.as_deref(), sea.extends.as_deref()), (Some("PANTRY-2"), Some("pantry")));
    // The references came back: dropping sea's block leaves both unused.
    store.amend("PANTRY-2", None, Some("gone".into())).await.unwrap();
    assert_eq!(names(&store), ["default"]);
    let named: BTreeSet<String> = store.tickets.read().unwrap().themes.refs.keys().cloned().collect();
    assert_eq!(named, BTreeSet::from(["lost".to_string()]));
    // And the wire's shape.
    let ev = crate::themes::ThemesEvent::ThemeRemoved { name: "x".into() };
    assert_eq!(serde_json::to_value(&ev).unwrap(), serde_json::json!({"type": "theme_removed", "name": "x"}));
    let _ = ThemeReach::Ticket;
}
