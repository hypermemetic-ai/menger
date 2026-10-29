//! V2-11: the methods' `///` docs are the schema's descriptions — what
//! `synapse … --help` prints and what an agent reads before README — and
//! they say what v2 does.
//!
//! The schema is fetched as `synapse -s` fetches it: signed in as the
//! fixture operator (admin on `*`, so the view is every method), the root's
//! `menger.schema` names the services, and each one's `<path>.schema` (a
//! child's `tickets.commits.schema`, `ui.slots.schema`) its methods and its
//! children, walked to the leaves. The root's own methods (`call`, `schema`)
//! are plexus-core's, not menger's `///`, and are not held here.
//!
//! - [`every_description_says_what_v2_does`]: every method's description and
//!   every parameter's — the property's own and each parameter type's in
//!   the method's `params.$defs` — says none of V2-8's gone words
//!   (`tickets-api/tests/cut.rs`'s `GONE_WORDS`) nor the schema's own
//!   ([`SCHEMA_WORDS`]: what went from the tickets service's surface). In
//!   the `ui` service (`ui`, `ui.slots`) `component` / `components` are
//!   its own word — the UI's components — and are allowed there alone.
//!   What a method yields (`returns`) is not held here.
//! - [`the_ui_docs_say_who_may`]: each `ui` method's description names the
//!   role that holds it as the server declares it, and no other.
//! - [`the_tickets_docs_name_what_remains`]: `tickets.delete`, `archive` and
//!   `history` name no component, theme, input or the queue.
//! - [`a_word_is_found_only_as_a_word_of_its_own`]: the matcher's check.
//!
//! Its own throwaway database and a free port; prod and test are never
//! touched.

mod support;
use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::WsClient;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

/// V2-8's words (`tickets-api/tests/cut.rs`'s `GONE_WORDS`, copied): v1's
/// services, its queue, its mockups, its browser toolchain and the old
/// word for a service.
const GONE_WORDS: &[&str] = &[
    "chats",
    "chat",
    "comments",
    "comment",
    "deployments",
    "deployment",
    "themes",
    "queue",
    "queues",
    "queued",
    "trunk",
    "Leptos",
    "sdk-codegen",
    "hub",
    "hubs",
    "mockup",
    "mockups",
];

/// The schema's own: what went from the tickets service's surface with
/// v2 — a ticket's inputs, its components, its assignees, media, notes and
/// the deploy webhook.
const SCHEMA_WORDS: &[&str] = &["inputs", "input", "components", "component", "assignees", "media", "notes", "webhook"];

/// The words the `ui` service may say: its components are the UI's.
const UI_WORDS: &[&str] = &["components", "component"];

/// Whether `word` occurs in `text` as an identifier of its own, ignoring
/// case (cut.rs's `names` over lower-cased text: `the queue` and `Chats`
/// do; `GitHub`, `HubTransport` and `unqueued` do not).
fn says(text: &str, word: &str) -> bool {
    let (text, word) = (text.to_ascii_lowercase(), word.to_ascii_lowercase());
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(&word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

/// The gone words `text` says, in the service at `path` (`ui`, `ui.slots`:
/// the UI's components allowed).
fn gone_in(path: &str, text: &str) -> Vec<&'static str> {
    let ui = path == "ui" || path.starts_with("ui.");
    GONE_WORDS
        .iter()
        .chain(SCHEMA_WORDS)
        .filter(|w| !(ui && UI_WORDS.contains(w)))
        .filter(|w| says(text, w))
        .copied()
        .collect()
}

/// Every description a method carries that this file holds: its own
/// (`service.method`) and each parameter's (`service.method.param`); and,
/// apart, each parameter type's in its `params.$defs`, by type name.
fn descriptions(path: &str, method: &Value) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let name = format!("{path}.{}", method["name"].as_str().unwrap_or("?"));
    let mut own = Vec::new();
    if let Some(d) = method["description"].as_str() {
        own.push((name.clone(), d.to_string()));
    }
    let params = &method["params"];
    if let Some(props) = params["properties"].as_object() {
        for (p, schema) in props {
            if let Some(d) = schema["description"].as_str() {
                own.push((format!("{name}.{p}"), d.to_string()));
            }
        }
    }
    let mut types = Vec::new();
    if let Some(defs) = params["$defs"].as_object() {
        for (ty, schema) in defs {
            if let Some(d) = schema["description"].as_str() {
                types.push((ty.clone(), d.to_string()));
            }
        }
    }
    (own, types)
}

/// What is wrong with the methods of the services in `all` (path →
/// methods): each description that says a gone word, as `name [words]:
/// the description`. A parameter type's is said once, as `<Type>` with the
/// methods whose parameters carry it (one type is in many methods' schemas).
fn offences(all: &[(&str, Vec<Value>)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut types: BTreeMap<(String, String), (Vec<&'static str>, Vec<String>)> = BTreeMap::new();
    for (path, methods) in all {
        for m in methods {
            let (own, tys) = descriptions(path, m);
            for (name, text) in own {
                let words = gone_in(path, &text);
                if !words.is_empty() {
                    out.push(format!("{name} {words:?}: {}", text.replace('\n', " ")));
                }
            }
            for (ty, text) in tys {
                let words = gone_in(path, &text);
                if !words.is_empty() {
                    types
                        .entry((ty, text))
                        .or_insert_with(|| (words, Vec::new()))
                        .1
                        .push(format!("{path}.{}", m["name"].as_str().unwrap_or("?")));
                }
            }
        }
    }
    for ((ty, text), (words, used)) in types {
        out.push(format!("<{ty}> {words:?} (a parameter type of {}): {}", used.join(", "), text.replace('\n', " ")));
    }
    out
}

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// A server as the wire reference starts one: the service as `main` builds
/// it, behind the gate, on a throwaway database. Also its declared roles.
async fn start_server() -> (u16, BTreeMap<String, Vec<String>>) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db = dir.path().join("tickets.db");
    let port = free_port().await;
    let login = support::Login::new();
    let service = support::hub(&db, &login).await;
    let roles = service.declared_roles();
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(service, conv)
            .with_websocket(port)
            .with_session_validator(gate)
            .anonymous_view()
            .credential_cookies(["access_token"])
            .build()
            .await
            .expect("build server")
            .serve()
            .await
            .expect("serve");
    });
    (port, roles)
}

/// One call's first item: a data item's content, an error item whole.
async fn one(c: &WsClient, method: &str) -> Value {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", json!({})).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let item = tokio::time::timeout(Duration::from_secs(5), s.next())
        .await
        .unwrap_or_else(|_| panic!("{method}: no answer"))
        .unwrap()
        .unwrap();
    assert_eq!(item["type"], "data", "{method}: {item}");
    item["content"].clone()
}

/// Every service's schema by its path (`tickets`, `tickets.commits`, `ui`,
/// `ui.slots`, …), walked from the root's children; the root itself not.
async fn schemas(c: &WsClient) -> BTreeMap<String, Value> {
    let child_paths = |path: Option<&str>, schema: &Value| -> Vec<String> {
        schema["children"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|ch| {
                let ns = ch["namespace"].as_str().unwrap_or_else(|| panic!("a child's namespace: {ch}"));
                path.map_or(ns.to_string(), |p| format!("{p}.{ns}"))
            })
            .collect()
    };
    let mut todo = child_paths(None, &one(c, "menger.schema").await);
    let mut out = BTreeMap::new();
    while let Some(path) = todo.pop() {
        let schema = one(c, &format!("{path}.schema")).await;
        todo.extend(child_paths(Some(&path), &schema));
        out.insert(path, schema);
    }
    out
}

/// A schema's methods, the `schema` method (plexus-core's) and a child's
/// stand-in left out.
fn methods(schema: &Value) -> Vec<Value> {
    schema["methods"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m["name"] != "schema" && m.get("grant").is_some_and(|g| !g.is_null()))
        .collect()
}

fn method<'a>(all: &'a BTreeMap<String, Value>, path: &str, name: &str) -> Value {
    let schema = all.get(path).unwrap_or_else(|| panic!("no service {path}"));
    methods(schema).into_iter().find(|m| m["name"] == name).unwrap_or_else(|| panic!("no method {path}.{name}"))
}

/// V2-11, acceptance 1: the schema, grepped for the gone words, matches
/// nothing — every method's description and every parameter's, of every
/// service (`tickets` and its children, `members`, `orgs`, `epics`,
/// `session`, `auth`, `ui` and `slots`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_description_says_what_v2_does() {
    let (port, _) = start_server().await;
    let c = support::connect(port).await;
    let all = schemas(&c).await;
    let want: BTreeSet<&str> = ["tickets", "members", "orgs", "epics", "session", "auth", "ui", "ui.slots", "tickets.commits"].into();
    let have: BTreeSet<&str> = all.keys().map(String::as_str).collect();
    assert!(want.is_subset(&have), "the walk reached {have:?}");
    let services: Vec<(&str, Vec<Value>)> = all.iter().map(|(path, schema)| (path.as_str(), methods(schema))).collect();
    let seen: usize = services.iter().map(|(_, ms)| ms.len()).sum();
    let left = offences(&services);
    assert!(seen > 80, "the walk read {seen} methods: too few to be the surface");
    assert!(
        left.is_empty(),
        "V2-11: {} description(s) in the schema still say what v2 is not ({}, {}; `component` allowed in ui):\n  {}",
        left.len(),
        GONE_WORDS.join(", "),
        SCHEMA_WORDS.join(", "),
        left.join("\n  ")
    );
}

/// Who may call each `ui` method, as its description must say it. The rule:
/// a method held by more than a viewer names its role — `Deployer only.`,
/// `Admin only.` — and no other; a viewer's read names no role, as the
/// tickets service's reads name none. Both ways: each role here is checked
/// against the roles the server declares (viewer if a viewer holds the
/// grant, else deployer if a deployer does, else admin), so the table
/// cannot drift from `ui-service/src/activation.rs`.
const UI_ROLES: &[(&str, &str, &str)] = &[
    ("ui", "push", "deployer"),
    ("ui", "rollback", "admin"),
    ("ui", "preview", "admin"),
    ("ui.slots", "set", "admin"),
    ("ui.slots", "clear", "admin"),
    ("ui", "versions", "viewer"),
    ("ui", "watch", "viewer"),
    ("ui.slots", "list", "viewer"),
];

/// V2-11, acceptance (the table's second row): `ui push --help` says the
/// deployer's, `rollback`, `preview`, `slots set` / `clear` the admin's,
/// `versions`, `watch`, `slots list` no role (a viewer's).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ui_docs_say_who_may() {
    let (port, roles) = start_server().await;
    let c = support::connect(port).await;
    let all = schemas(&c).await;
    // MACRO-20: a child's grant is declared under its path (`ui.slots.list`),
    // its schema says it without the parent (`slots.list`).
    let holds = |role: &str, path: &str, grant: &str| {
        roles
            .get(role)
            .is_some_and(|gs| gs.iter().any(|g| g == grant || *g == format!("{path}.{}", grant.rsplit('.').next().unwrap())))
    };
    let mut wrong = Vec::new();
    for (path, name, role) in UI_ROLES {
        let m = method(&all, path, name);
        let grant = m["grant"].as_str().unwrap_or_else(|| panic!("{path}.{name}: no grant in {m}"));
        let declared = if holds("viewer", path, grant) {
            "viewer"
        } else if holds("deployer", path, grant) {
            "deployer"
        } else {
            "admin"
        };
        assert_eq!(declared, *role, "{path}.{name} (grant {grant}): the server declares it the {declared}'s; this table says {role}'s");
        let text = m["description"].as_str().unwrap_or_default();
        let named: Vec<&str> = ["admin", "deployer", "viewer"].into_iter().filter(|r| says(text, r)).collect();
        let want: Vec<&str> = if *role == "viewer" { vec![] } else { vec![*role] };
        if named != want {
            wrong.push(format!("{path}.{name}: the {role}'s, but names {named:?}: {}", text.replace('\n', " ")));
        }
    }
    assert!(wrong.is_empty(), "V2-11: {} ui description(s) name the wrong role:\n  {}", wrong.len(), wrong.join("\n  "));
}

/// V2-11, acceptance (the table's third row): what `delete`, `archive` and
/// `history` take with them, or yield, is children, blockers, shortcodes,
/// revisions — not a component, a theme, an input or the queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tickets_docs_name_what_remains() {
    let (port, _) = start_server().await;
    let c = support::connect(port).await;
    let all = schemas(&c).await;
    let words = ["component", "components", "theme", "themes", "input", "inputs", "queue", "queued"];
    let mut wrong = Vec::new();
    for name in ["delete", "archive", "history"] {
        let text = method(&all, "tickets", name)["description"].as_str().unwrap_or_default().to_string();
        let named: Vec<&str> = words.into_iter().filter(|w| says(&text, w)).collect();
        if !named.is_empty() {
            wrong.push(format!("tickets.{name} {named:?}: {}", text.replace('\n', " ")));
        }
    }
    assert!(wrong.is_empty(), "V2-11: {} tickets description(s) name what went:\n  {}", wrong.len(), wrong.join("\n  "));
}

/// The matcher checks: a word of its own in any case matches, inside a
/// longer one it does not; `component` is the ui service's own word, in
/// `ui` and `ui.slots` alone; a method is held by its description, its
/// parameters' and its parameter types'.
#[test]
fn a_word_is_found_only_as_a_word_of_its_own() {
    for (text, word) in [
        ("Chats are the `chats` activation", "chats"),
        ("a queued one leaves the queue", "queue"),
        ("a hub, a Hub", "hub"),
        ("the hub-codegen generator", "hub"),
        ("an InputChanged by its ticket's; component changes always", "component"),
        ("ORG-13's `naming` input asks", "input"),
        ("a comment's, a note's", "comment"),
        ("COMPONENTS", "components"),
    ] {
        assert!(says(text, word), "{word:?} should be found in {text:?}");
    }
    for (text, word) in [
        ("GitHub and github.com", "hub"),
        ("HubTransport::as_", "hub"),
        ("`unqueued` those it took out", "queued"),
        ("an InputChanged by its ticket's", "input"),
        ("chatter", "chat"),
        ("the data-theme attribute", "themes"),
        ("deployed to test", "deployment"),
        ("ComponentName", "component"),
    ] {
        assert!(!says(text, word), "{word:?} should not be found in {text:?}");
    }

    // `component` is the ui service's word, and only its.
    assert_eq!(gone_in("ui", "Fill a section with a component"), Vec::<&str>::new());
    assert_eq!(gone_in("ui.slots", "the components of a version"), Vec::<&str>::new());
    assert_eq!(gone_in("tickets", "a component it defines"), ["component"]);
    assert_eq!(gone_in("uikit", "a component"), ["component"], "the allowance is the ui service, not a prefix");
    assert_eq!(gone_in("ui", "a component's comment on the queue"), ["comment", "queue"], "only `component` is allowed in ui");

    // A method's description, its parameters' and its parameter types' are held.
    let m = json!({
        "name": "delete",
        "description": "Delete tickets, and their components.",
        "params": {
            "properties": { "ids": { "description": "The tickets." }, "body": { "$ref": "#/$defs/Body", "description": "A note." } },
            "$defs": { "Body": { "description": "A body — a ticket's, a comment's." } }
        },
        "returns": { "description": "the queue (not held: a yield)" }
    });
    let mut get = m.clone();
    get["name"] = json!("get");
    get["description"] = json!("Get a ticket.");
    assert_eq!(
        offences(&[("tickets", vec![m, get])]),
        [
            "tickets.delete [\"components\"]: Delete tickets, and their components.",
            "<Body> [\"comment\"] (a parameter type of tickets.delete, tickets.get): A body — a ticket's, a comment's.",
        ],
        "a parameter type is said once, with every method that carries it"
    );
    assert!(offences(&[("ui", vec![json!({"name": "set", "description": "a component", "params": {}})])]).is_empty());
}
