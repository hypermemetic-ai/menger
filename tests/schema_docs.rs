//! V2-11: the methods' `///` docs are the schema's descriptions — what
//! `synapse … --help` prints and what an agent reads before README — and
//! they say what v2 does.
//!
//! The schema is fetched as `synapse -s` fetches it: signed in as the
//! fixture operator (admin on `*`, so the view is every method), the root's
//! `menger.schema` names the services, and each one's `<path>.schema` (a
//! child's `tickets.commits.schema`, `ui.slots.schema`) its methods and its
//! children, walked to the leaves. The root's own methods and every
//! `schema` method are plexus-core's, not menger's `///`, and are not held
//! here; a child's stand-in in its parent's methods (`role: static_child`)
//! is the child, read at its own path.
//!
//! - [`every_description_says_what_v2_does`]: every description the schema
//!   carries — a service's and each child's, a method's (and its
//!   `long_description`, its examples' notes), and every `description`
//!   under its `params` and its `returns`, to any depth: a property's, an
//!   event's (a `oneOf` variant, named by its `type`), a type's in `$defs`
//!   with its properties and variants — says none of V2-8's gone words
//!   (`tickets-api/tests/cut.rs`'s `GONE_WORDS`) nor the schema's own
//!   ([`SCHEMA_WORDS`]: what went from the tickets service's surface, and
//!   the events it said them by). In the `ui` service (`ui`, `ui.slots`)
//!   `component` / `components` are its own word — the UI's components —
//!   and are allowed there alone.
//! - [`no_answer_carries_a_field_of_the_queue`]: no property of anything a
//!   method yields is named for the queue.
//! - [`the_ui_docs_say_who_may`]: each `ui` method's description names the
//!   role that holds it as the server declares it, and no other.
//! - [`the_tickets_docs_name_what_remains`]: `tickets.delete`, `archive` and
//!   `history` name what remains, and no component, theme, input or queue.
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
    "chats", "chat", "comments", "comment", "deployments", "deployment", "themes", "queue", "queues", "queued", "trunk",
    "Leptos", "sdk-codegen", "hub", "hubs", "mockup", "mockups",
];

/// The schema's own: what went from the tickets service's surface with
/// v2 — a ticket's inputs, its components, its assignees, media, notes and
/// the deploy webhook — and the names v1 said them by (each matched as a
/// whole identifier): its events, and the field `unqueued`.
const SCHEMA_WORDS: &[&str] = &[
    "inputs", "input", "components", "component", "assignees", "media", "notes", "note's", "webhook", "unqueued", "AssigneesChanged",
    "ComponentChanged", "ComponentRemoved", "InputChanged",
];

/// The words the `ui` service may say: its components are the UI's.
const UI_WORDS: &[&str] = &["components", "component"];

/// Fields of the queue, which no answer carries on v2.
const QUEUE_FIELDS: &[&str] = &["unqueued", "queued", "queue"];

/// Every service a v2 server registers, and their children, by path.
const PATHS: &[&str] = &[
    "auth", "epics", "members", "members.teams", "orgs", "session", "tickets", "tickets.commits", "tickets.filters",
    "tickets.nav", "tickets.revisions", "tickets.subtasks", "tickets.work", "ui", "ui.routes", "ui.slots",
];

/// Whether `word` occurs in `text` as an identifier of its own, ignoring
/// case (cut.rs's `names` over lower-cased text: `the queue`, `Chats` and
/// `InputChanged` do; `GitHub`, `HubTransport`, `unqueued` and
/// `InputChangedLater` do not).
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
    GONE_WORDS.iter().chain(SCHEMA_WORDS).filter(|w| !(ui && UI_WORDS.contains(w))).filter(|w| says(text, w)).copied().collect()
}

/// What a walk of a schema finds: each description by its name, and (under
/// a method's `returns`) each property by its name.
#[derive(Default)]
struct Found {
    descriptions: Vec<(String, String)>,
    fields: Vec<(String, String)>,
}

/// Walk `v`, named `name`, into `found`; `returns`: under what a method
/// yields. Names: a property `name.field`, an event (a `oneOf` variant with
/// a `type` const) `name→event`, a type in `$defs` `<Type>`; anything else
/// keeps the name it is under. Values (`default`, `const`, `enum`) are not
/// descriptions and are not read.
fn walk(v: &Value, name: &str, returns: bool, found: &mut Found) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                match (k.as_str(), x) {
                    ("description" | "long_description", Value::String(t)) => found.descriptions.push((name.to_string(), t.clone())),
                    ("properties", Value::Object(ps)) => {
                        for (p, s) in ps {
                            let n = format!("{name}.{p}");
                            if returns {
                                found.fields.push((n.clone(), p.clone()));
                            }
                            walk(s, &n, returns, found);
                        }
                    }
                    ("oneOf" | "anyOf" | "allOf", Value::Array(vs)) => {
                        for s in vs {
                            let n = match s["properties"]["type"]["const"].as_str() {
                                Some(c) if name.ends_with('→') => format!("{name}{c}"),
                                Some(c) => format!("{name}→{c}"),
                                None => name.to_string(),
                            };
                            walk(s, &n, returns, found);
                        }
                    }
                    ("$defs", Value::Object(ts)) => {
                        for (t, s) in ts {
                            walk(s, &format!("<{t}>"), returns, found);
                        }
                    }
                    ("examples", Value::Array(es)) => {
                        for e in es {
                            if let Some(t) = e["note"].as_str() {
                                found.descriptions.push((format!("{name} (example)"), t.to_string()));
                            }
                        }
                    }
                    ("default" | "const" | "enum", _) => {}
                    _ => walk(x, name, returns, found),
                }
            }
        }
        Value::Array(xs) => xs.iter().for_each(|x| walk(x, name, returns, found)),
        _ => {}
    }
}

/// Everything one method carries, walked: its own description as
/// `service.method`, its parameters' as `service.method.param`, what it
/// yields as `service.method→event.field`.
fn read_method(path: &str, method: &Value) -> Found {
    let name = format!("{path}.{}", method["name"].as_str().unwrap_or("?"));
    let mut found = Found::default();
    for (k, x) in method.as_object().into_iter().flatten() {
        match k.as_str() {
            "params" => walk(x, &name, false, &mut found),
            "returns" => walk(x, &format!("{name}→"), true, &mut found),
            "description" | "long_description" | "examples" => walk(&json!({ k.clone(): x.clone() }), &name, false, &mut found),
            _ => {}
        }
    }
    found
}

/// A service's own descriptions: its own, and each child's as its parent
/// lists it.
fn read_service(path: &str, schema: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(d) = schema["description"].as_str() {
        out.push((format!("{path} (service)"), d.to_string()));
    }
    for ch in schema["children"].as_array().into_iter().flatten() {
        if let Some(d) = ch["description"].as_str() {
            out.push((format!("{path}.{} (child)", ch["namespace"].as_str().unwrap_or("?")), d.to_string()));
        }
    }
    out
}

/// What is wrong with the services in `all` (path → its schema): each
/// description that says a gone word, as `name [words]: the description`,
/// once. A type in `$defs` is in many methods' schemas, and so is one
/// service's event enum: each is said once, with how many methods carry it
/// — a type as `<Type>`, an event of several methods as
/// `service.*→event.field`.
fn offences(all: &[(&str, Value)]) -> Vec<String> {
    let mut hits: BTreeMap<(String, String), (BTreeSet<&'static str>, BTreeSet<String>)> = BTreeMap::new();
    let mut note = |path: &str, carrier: String, name: String, text: String| {
        let words = gone_in(path, &text);
        if !words.is_empty() {
            // An answer's name without its method: `tickets.*→archived`.
            let name = match name.split_once('→') {
                Some((_, event)) if name.starts_with(&carrier) => format!("{}.*→{event}", path.split('.').next().unwrap_or(path)),
                _ => name,
            };
            let e = hits.entry((name, text)).or_default();
            e.0.extend(words);
            e.1.insert(carrier);
        }
    };
    for (path, schema) in all {
        for (name, text) in read_service(path, schema) {
            note(path, path.to_string(), name, text);
        }
        for m in methods(schema) {
            let carrier = format!("{path}.{}", m["name"].as_str().unwrap_or("?"));
            for (name, text) in read_method(path, &m).descriptions {
                note(path, carrier.clone(), name, text);
            }
        }
    }
    hits.into_iter()
        .map(|((name, text), (words, carriers))| {
            // One method's answer is named by that method.
            let name = match (carriers.len(), name.split_once(".*→")) {
                (1, Some((_, event))) => format!("{}→{event}", carriers.first().unwrap()),
                _ => name,
            };
            let many = if carriers.len() > 1 { format!(" (in {} methods)", carriers.len()) } else { String::new() };
            format!("{name} {:?}{many}: {}", words.into_iter().collect::<Vec<_>>(), text.replace('\n', " "))
        })
        .collect()
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
    let service = support::service(&db, &login).await;
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

/// One call's first item: a data item's content.
async fn one(c: &WsClient, method: &str) -> Value {
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", json!({})).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let item = tokio::time::timeout(Duration::from_secs(5), s.next()).await.unwrap_or_else(|_| panic!("{method}: no answer")).unwrap().unwrap();
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

/// A schema's methods: `schema` (plexus-core's) and a child's stand-in
/// (`role: static_child`, read at the child's own path) left out.
fn methods(schema: &Value) -> Vec<Value> {
    schema["methods"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m["name"] != "schema" && m["role"]["kind"] != "static_child")
        .collect()
}

fn method(all: &BTreeMap<String, Value>, path: &str, name: &str) -> Value {
    let schema = all.get(path).unwrap_or_else(|| panic!("no service {path}"));
    methods(schema).into_iter().find(|m| m["name"] == name).unwrap_or_else(|| panic!("no method {path}.{name}"))
}

/// A method's description, which must be there and say something.
fn description(m: &Value, name: &str) -> String {
    let d = m["description"].as_str().unwrap_or_else(|| panic!("{name}: no description in {m}"));
    assert!(!d.trim().is_empty(), "{name}: an empty description");
    d.to_string()
}

/// V2-11, acceptance 1: the schema, grepped for the gone words, matches
/// nothing — every description of every service (`tickets` and its
/// children, `members` and `teams`, `orgs`, `epics`, `session`, `auth`,
/// `ui` and `slots`), what its methods take and what they yield.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_description_says_what_v2_does() {
    let (port, _) = start_server().await;
    let c = support::connect(port).await;
    let all = schemas(&c).await;
    let have: BTreeSet<&str> = all.keys().map(String::as_str).collect();
    assert_eq!(have, PATHS.iter().copied().collect::<BTreeSet<_>>(), "the walk reached every service and child, and nothing else");
    let services: Vec<(&str, Value)> = all.iter().map(|(p, s)| (p.as_str(), s.clone())).collect();
    let mut read = BTreeSet::new();
    for (path, schema) in &services {
        for m in methods(schema) {
            let name = format!("{path}.{}", m["name"].as_str().unwrap());
            assert!(m["grant"].as_str().is_some_and(|g| !g.is_empty()), "{name}: a method with no grant: {m}");
            assert!(!read_method(path, &m).descriptions.is_empty(), "{name}: nothing read");
            read.insert(name);
        }
    }
    for m in ["tickets.watch", "ui.push", "tickets.archive", "ui.slots.set"] {
        assert!(read.contains(m), "{m} was not read: {read:?}");
    }
    let left = offences(&services);
    assert!(
        left.is_empty(),
        "V2-11: {} description(s) in the schema still say what v2 is not ({}, {}; `component` allowed in ui):\n  {}",
        left.len(),
        GONE_WORDS.join(", "),
        SCHEMA_WORDS.join(", "),
        left.join("\n  ")
    );
}

/// V2-11: the queue went, and with it every field that reported on it —
/// no property of anything a method yields (an event's, a type's in
/// `$defs`) is named [`QUEUE_FIELDS`].
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_answer_carries_a_field_of_the_queue() {
    let (port, _) = start_server().await;
    let c = support::connect(port).await;
    let all = schemas(&c).await;
    let mut left = BTreeSet::new();
    let mut fields = 0;
    for (path, schema) in &all {
        for m in methods(schema) {
            for (name, field) in read_method(path, &m).fields {
                fields += 1;
                if QUEUE_FIELDS.contains(&field.as_str()) {
                    left.insert(name);
                }
            }
        }
    }
    assert!(fields > 100, "read {fields} fields of answers: too few to be the surface");
    assert!(left.is_empty(), "V2-11: {} answer field(s) of the queue:\n  {}", left.len(), left.into_iter().collect::<Vec<_>>().join("\n  "));
}

/// Who may call each `ui` method, as its description must say it. The rule:
/// a method held by more than a viewer names its role — `Deployer only.`,
/// `Admin only.` — and no other; a viewer's read names no role, as the
/// tickets service's reads name none. Both ways: each role here is checked
/// against the roles the server declares (viewer if a viewer holds the
/// grant, else editor if an editor does — STUDIO-3 — else deployer if a
/// deployer does, else admin), so the table
/// cannot drift from the framework's (`../plexus-ui/service/src/activation.rs`).
const UI_ROLES: &[(&str, &str, &str)] = &[
    ("ui", "push", "deployer"),
    ("ui", "rollback", "admin"),
    ("ui", "preview", "admin"),
    ("ui.slots", "set", "admin"),
    ("ui.slots", "clear", "admin"),
    ("ui", "versions", "viewer"),
    ("ui", "watch", "viewer"),
    ("ui", "contract", "viewer"),
    ("ui.slots", "list", "viewer"),
    // STUDIO-3: routes as data, and a push the service signs for an editor.
    ("ui.routes", "list", "viewer"),
    ("ui.routes", "set", "editor"),
    ("ui.routes", "clear", "editor"),
    ("ui", "author", "editor"),
    // FRAME-48: an agent drives the studio.
    ("ui", "show", "editor"),
    ("ui", "sessions", "editor"),
    ("ui", "here", "viewer"),
    ("ui", "follow", "viewer"),
    // FRAME-52: named and tagged versions, and undo as a method.
    ("ui", "tag", "editor"),
    ("ui", "history", "viewer"),
    ("ui", "undo", "editor"),
    ("ui", "redo", "editor"),
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
        roles.get(role).is_some_and(|gs| gs.iter().any(|g| g == grant || *g == format!("{path}.{}", grant.rsplit('.').next().unwrap())))
    };
    let mut wrong = Vec::new();
    for (path, name, role) in UI_ROLES {
        let m = method(&all, path, name);
        let grant = m["grant"].as_str().unwrap_or_else(|| panic!("{path}.{name}: no grant in {m}"));
        let declared = if holds("viewer", path, grant) {
            "viewer"
        } else if holds("editor", path, grant) {
            "editor"
        } else if holds("deployer", path, grant) {
            "deployer"
        } else {
            "admin"
        };
        assert_eq!(declared, *role, "{path}.{name} (grant {grant}): the server declares it the {declared}'s; this table says {role}'s");
        let text = description(&m, &format!("{path}.{name}"));
        let named: Vec<&str> = ["admin", "deployer", "editor", "viewer"].into_iter().filter(|r| says(&text, r)).collect();
        let want: Vec<&str> = if *role == "viewer" { vec![] } else { vec![*role] };
        if named != want {
            wrong.push(format!("{path}.{name}: the {role}'s, but names {named:?}: {}", text.replace('\n', " ")));
        }
    }
    assert!(wrong.is_empty(), "V2-11: {} ui description(s) name the wrong role:\n  {}", wrong.len(), wrong.join("\n  "));
}

/// V2-11, acceptance (the table's third row): what `delete`, `archive` and
/// `history` take with them, or yield, is what remains — `delete` is
/// refused by children and blockers, `archive` takes a subtree, `history`
/// is transitions — and not a component, a theme, an input or the queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tickets_docs_name_what_remains() {
    let (port, _) = start_server().await;
    let c = support::connect(port).await;
    let all = schemas(&c).await;
    let gone = ["component", "components", "theme", "themes", "input", "inputs", "queue", "queued"];
    let mut wrong = Vec::new();
    for (name, remains) in [("delete", &["children", "blockers"][..]), ("archive", &["subtree"][..]), ("history", &["transition", "transitions"][..])] {
        let text = description(&method(&all, "tickets", name), &format!("tickets.{name}"));
        let named: Vec<&str> = gone.into_iter().filter(|w| says(&text, w)).collect();
        if !named.is_empty() {
            wrong.push(format!("tickets.{name} names {named:?}: {}", text.replace('\n', " ")));
        }
        let missing: Vec<&str> = remains.iter().copied().filter(|w| !says(&text, w)).collect();
        let needs_all = name == "delete";
        if (needs_all && !missing.is_empty()) || (!needs_all && missing.len() == remains.len()) {
            wrong.push(format!("tickets.{name} does not name {missing:?}: {}", text.replace('\n', " ")));
        }
    }
    assert!(wrong.is_empty(), "V2-11: {} tickets description(s) do not say what remains:\n  {}", wrong.len(), wrong.join("\n  "));
}

/// The matcher checks: a word of its own in any case matches, inside a
/// longer one it does not; `component` is the ui service's own word, in
/// `ui` and `ui.slots` alone; a method is held by everything it carries,
/// what it yields included, each named by its path.
#[test]
fn a_word_is_found_only_as_a_word_of_its_own() {
    for (text, word) in [
        ("Chats are the `chats` activation", "chats"),
        ("a queued one leaves the queue", "queue"),
        ("a hub, a Hub", "hub"),
        ("the hub-codegen generator", "hub"),
        ("an InputChanged by its ticket's", "InputChanged"),
        ("ComponentChanged / ComponentRemoved", "ComponentRemoved"),
        ("LabelsChanged / AssigneesChanged", "AssigneesChanged"),
        ("ORG-13's `naming` input asks", "input"),
        ("a comment's, a note's", "note's"),
        ("COMPONENTS", "components"),
    ] {
        assert!(says(text, word), "{word:?} should be found in {text:?}");
    }
    for (text, word) in [
        ("GitHub and github.com", "hub"),
        ("HubTransport::as_", "hub"),
        ("`unqueued` those it took out", "queued"),
        ("an InputChanged by its ticket's", "input"),
        ("an InputChangedLater", "InputChanged"),
        ("a footnote's", "note's"),
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
    assert_eq!(gone_in("ui", "a ComponentChanged"), ["ComponentChanged"], "v1's event is not the UI's component");

    // Everything a method carries is held, each by its path: its own
    // description, a parameter's, a type's in `$defs` (once, with how many
    // carry it), and what it yields — an event, its fields, a type's
    // properties and variants.
    let delete = json!({
        "name": "delete",
        "role": {"kind": "rpc"},
        "description": "Delete tickets, and their components.",
        "params": {
            "properties": { "ids": { "description": "The tickets." }, "body": { "$ref": "#/$defs/Body", "description": "A body." } },
            "$defs": { "Body": { "description": "A body — a ticket's, a comment's." } }
        },
        "returns": {
            "oneOf": [
                { "description": "Gone.", "properties": { "type": { "const": "deleted" }, "unqueued": { "description": "out of the queue" } } },
            ],
            "$defs": { "WatchEnd": { "description": "Why it ended.", "properties": { "reason": { "description": "a themes watch's" } } } }
        }
    });
    let mut get = delete.clone();
    get["name"] = json!("get");
    get["description"] = json!("Get a ticket.");
    get["returns"] = json!({});
    let stand_in = json!({ "name": "commits", "role": {"kind": "static_child"}, "description": "a component" });
    let tickets = json!({
        "description": "Tickets, and their inputs.",
        "children": [{ "namespace": "commits", "description": "a ticket's commits" }],
        "methods": [delete.clone(), get, stand_in]
    });
    assert_eq!(
        offences(&[("tickets", tickets)]),
        [
            "<Body> [\"comment\"] (in 2 methods): A body — a ticket's, a comment's.",
            "<WatchEnd>.reason [\"themes\"]: a themes watch's",
            "tickets (service) [\"inputs\"]: Tickets, and their inputs.",
            "tickets.delete→deleted.unqueued [\"queue\"]: out of the queue",
            "tickets.delete [\"components\"]: Delete tickets, and their components.",
        ],
        "a type once, with every method that carries it; what a method yields is read; a child's stand-in is not"
    );
    let fields: Vec<(String, String)> = read_method("tickets", &delete).fields;
    assert!(fields.contains(&("tickets.delete→deleted.unqueued".into(), "unqueued".into())), "{fields:?}");
    assert!(fields.contains(&("<WatchEnd>.reason".into(), "reason".into())), "{fields:?}");
    assert!(!fields.iter().any(|(_, f)| f == "ids"), "a parameter is not a field of an answer: {fields:?}");
    assert!(offences(&[("ui", json!({ "methods": [{"name": "set", "role": {"kind": "rpc"}, "description": "a component", "params": {}}] }))]).is_empty());
}
