//! COMPS-5: **the backfill** (`src/backfill.rs`) — every ```component
//! definition the store holds registered in plexus-ui by the server as it
//! starts. Bodies written straight to the store (as v1's were: never saved
//! by the UI, so plexus-ui holds none of them), and an orphan in v1's own
//! registry left in the file (`components`): the next start keeps each
//! framed one in `menger:v1`'s namespace — v1's custom element as
//! `element/1`, an `html` one as `html/1`, written by its ticket — refuses
//! an rhx one and a name someone authored on v2, and a start after it
//! changes nothing; a body changed with no save is kept again at the next.

mod support;

use serde_json::{json, Value};
use support::{call, connect_as, free_port, spawn_menger, ui_args, ADMIN};

const FILM: &str = "<template><b class=\"t\">-</b></template>\n<style>:host{display:block}</style>\n<script>\nthis.root.querySelector('.t').textContent = this.attr('label');\nthis.menger.on('theme', () => {});\n</script>\n";
const CARD: &str = "<div id=\"card\">a page</div>\n";
const HELLO: &str = "<script>\n  prop(\"words\");\n</script>\n<p class=\"x-hello\">{words}</p>\n";
const ORPHAN: &str = "<template><i>left by v1</i></template>";

fn body(defs: &[(&str, &str, &str)], uses: &str) -> String {
    let mut b = String::from("Defines:\n\n");
    for (head, _name, src) in defs {
        b.push_str(&format!("```component {head}\n{src}```\n\n"));
    }
    b.push_str(uses);
    b
}

async fn components(port: u16) -> Vec<Value> {
    let c = connect_as(&ADMIN, port).await;
    call(&c, "ui.components", json!({})).await.into_iter().filter(|e| e["type"] == "component").collect()
}

fn named<'a>(rows: &'a [Value], name: &str) -> Option<&'a Value> {
    rows.iter().find(|r| r["name"] == name)
}

#[tokio::test]
async fn the_start_registers_every_definition_once() {
    let dir = tempfile::tempdir().unwrap();
    let (db, ui_db, key) = (dir.path().join("t.db"), dir.path().join("ui.db"), dir.path().join("author.key"));
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&key, "ab".repeat(32)).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut args: Vec<String> = ui_args(&ui_db).into();
    args.extend(["--ui-author-key".to_string(), key.display().to_string()]);
    let port = free_port().await;

    // First start: an empty store, nothing to register. Then the bodies, written as v1's were, and a
    // name authored on v2 by the admin before its body is read.
    let (film, card, taken) = {
        let _s = spawn_menger(port, &db, &args).await;
        let c = connect_as(&ADMIN, port).await;
        let id = |evs: Vec<Value>| evs.iter().find(|e| e["type"] == "created").map(|e| e["ticket"]["id"].as_str().unwrap().to_string()).expect("created");
        let root = id(call(&c, "tickets.create", json!({ "epic": "BF", "title": "backfill", "kind": "scope" })).await);
        let mk = |title: &str, b: String| json!({ "epic": "BF", "title": title, "kind": "build", "parent": root, "body": b });
        let film = id(call(&c, "tickets.create", mk("the film", body(&[("x-film data-field=film", "x-film", FILM)], "<x-film label=\"Kiln\"></x-film>\n"))).await);
        let card = id(call(&c, "tickets.create", mk("a page", body(&[("x-card html", "x-card", CARD), ("x-hello", "x-hello", HELLO)], ""))).await);
        let taken = id(call(&c, "tickets.create", mk("taken", body(&[("x-taken", "x-taken", FILM)], ""))).await);
        let user = id(call(&c, "tickets.create", mk("uses the film", "<x-film label=\"Glaze\"></x-film>\n".into())).await);
        let authored = call(&c, "ui.author", json!({ "sources": [{ "name": "x-taken", "source": CARD, "profile": "html/1" }], "own": true })).await;
        assert_eq!(authored[0]["type"], "authored", "{authored:?}");
        let _ = user;
        (film, card, taken)
    };
    // v1's registry, left in the file: an orphan no body defines.
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS components (name TEXT PRIMARY KEY, source TEXT, owner TEXT, \"references\" TEXT NOT NULL, preview TEXT NOT NULL DEFAULT '[]') WITHOUT ROWID;").unwrap();
        conn.execute("INSERT INTO components (name, source, owner, \"references\") VALUES ('x-orphan', ?1, NULL, '[]'), ('x-film', 'older', 'ZZ-1', '[]')", [ORPHAN]).unwrap();
    }

    // Second start: the backfill.
    let first = {
        let _s = spawn_menger(port, &db, &args).await;
        components(port).await
    };
    let f = named(&first, "x-film").expect("x-film registered");
    assert_eq!(
        (f["profile"].as_str(), f["author"].as_str(), f["by"].as_str(), f["source"].as_str(), f["native"].as_bool()),
        (Some("element/1"), Some("menger:v1"), Some(film.as_str()), Some(FILM), Some(false)),
        "v1's custom element, from its body (not v1's registry's older copy): {f}"
    );
    let c = named(&first, "x-card").expect("x-card registered");
    assert_eq!((c["profile"].as_str(), c["by"].as_str()), (Some("html/1"), Some(card.as_str())), "{c}");
    let o = named(&first, "x-orphan").expect("v1's orphan registered");
    assert_eq!((o["profile"].as_str(), o["by"].as_str(), o["source"].as_str()), (Some("element/1"), Some("v1"), Some(format!("{ORPHAN}\n").as_str())), "{o}");
    assert!(named(&first, "x-hello").is_none(), "rhx is refused: compiled by its ticket's save");
    let t = named(&first, "x-taken").expect("x-taken");
    assert_ne!(t["author"].as_str(), Some("menger:v1"), "authored on v2 is never written over ({taken}): {t}");

    // Third start: nothing changes — every row as it was, `at` included.
    let again = {
        let _s = spawn_menger(port, &db, &args).await;
        components(port).await
    };
    assert_eq!(first, again, "a second backfill changes nothing");

    // A body changed with no save in the UI: kept again at the next start.
    {
        let _s = spawn_menger(port, &db, &args).await;
        let c = connect_as(&ADMIN, port).await;
        let moved = FILM.replace("-</b>", "--</b>");
        let r = call(&c, "tickets.amend", json!({ "id": film, "body": body(&[("x-film", "x-film", &moved)], "") })).await;
        assert!(r.iter().any(|e| e["type"] == "amended"), "{r:?}");
    }
    let last = {
        let _s = spawn_menger(port, &db, &args).await;
        components(port).await
    };
    let f2 = named(&last, "x-film").unwrap();
    assert!(f2["source"].as_str().unwrap().contains("--</b>") && f2["version"] != f["version"], "kept again at its new source: {f2}");
}
