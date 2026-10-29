//! UI-7: a push compiles — over the socket, a spawned `menger` under the
//! fixture operator with its own ui file, the test signer and
//! `--ui-compiler dist/ui-compiler.wasm` (built once per test binary by
//! `scripts/ui-compiler-wasm.sh`, `support::ui_compiler_wasm`). Each source
//! is recompiled in the server's sealed compiler and compared with the tree
//! the bundle claims (or the tree filled in); each `uses` is checked against
//! the running `tickets` service through the seam. What a source compiles
//! to is known here from the compiler natively — the same bytes (UI-5).
//! The last test registers a reduced `tickets` (a probe exposing `list`
//! alone) in process, to show the lint reads the running service, never a
//! copy. Servers on ports the OS hands out, their files under
//! /tmp/claude-1000.

mod support;

use jsonrpsee::ws_client::WsClient;
use menger::login_access::BRANDING;
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use support::{call, compiled_push_params, connect, free_port, row_source, sha_hex, spawn_menger, ui_args, Component, Tree};

/// The component every test pushes (a component's name holds a hyphen).
const X: &str = "x-row";

struct Server {
    s: support::Spawned,
    shell: Option<u16>,
    _dir: tempfile::TempDir,
}

fn scratch(prefix: &str) -> tempfile::TempDir {
    let root = std::path::Path::new("/tmp/claude-1000");
    std::fs::create_dir_all(root).unwrap();
    tempfile::Builder::new().prefix(prefix).tempdir_in(root).unwrap()
}

/// The binary with `--ui-compiler`, a shell when `shell`.
async fn start_with(shell: bool) -> Server {
    let wasm = support::ui_compiler_wasm();
    let dir = scratch("menger-ui-compile-");
    let shell_port = if shell { Some(free_port().await) } else { None };
    let mut extra = match shell_port {
        Some(p) => {
            let empty = dir.path().join("no-dist");
            std::fs::create_dir_all(&empty).unwrap();
            vec!["--shell-port".to_string(), p.to_string(), "--app-dist".into(), empty.display().to_string()]
        }
        None => vec!["--no-shell".to_string()],
    };
    extra.extend(ui_args(&dir.path().join("ui.db")));
    extra.extend(["--ui-compiler".to_string(), wasm.display().to_string()]);
    let s = spawn_menger(free_port().await, &dir.path().join("t.db"), &extra).await;
    if let Some(p) = shell_port {
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(("127.0.0.1", p)).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    Server { s, shell: shell_port, _dir: dir }
}

async fn start() -> Server {
    start_with(false).await
}

/// The `version` events `versions` answers.
async fn versions(c: &WsClient) -> Vec<Value> {
    let evs = call(c, "ui.versions", json!({})).await;
    assert!(evs.iter().all(|e| e["type"] == "version"), "versions answers only version events: {evs:?}");
    evs
}

/// The running tickets service's hash, read off its schema.
async fn tickets_hash(c: &WsClient) -> String {
    let schema = call(c, "tickets.schema", json!({})).await;
    schema.first().and_then(|s| s["hash"].as_str()).unwrap_or_else(|| panic!("tickets.schema answers its hash: {schema:?}")).to_string()
}

/// One `pushed`; it.
fn pushed(evs: &[Value]) -> &Value {
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!(evs[0]["type"], "pushed", "{evs:?}");
    &evs[0]
}

/// One in-band refusal for `reason` naming the component `name`; it.
fn refused<'a>(evs: &'a [Value], reason: &str, name: &str) -> &'a Value {
    assert_eq!(evs.len(), 1, "{evs:?}");
    let e = &evs[0];
    assert_eq!((e["type"].as_str(), e["reason"].as_str()), (Some("refused"), Some(reason)), "{evs:?}");
    assert_eq!(e["name"], name, "the refusal names the component: {e}");
    assert!(e["message"].as_str().is_some_and(|m| !m.is_empty()), "it says why: {e}");
    e
}

#[tokio::test]
async fn a_push_recompiles_and_the_trees_must_match() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let right = support::compile(X, &row_source()).0.tree;
    // Edited by hand: a prop the source does not declare.
    let edited = right.replacen("\"props\": [", "\"props\": [\n    \"extra\",", 1);
    assert_ne!(edited, right, "the edit took");

    let (params, _) = compiled_push_params(1, "c-other", &[Component::compiled(X, &row_source()).tree(Tree::Given(edited))]);
    let evs = call(&admin, "ui.push", params).await;
    refused(&evs, "compile_mismatch", X);
    assert!(versions(&admin).await.is_empty(), "nothing stored");

    // The same bundle with the tree the source compiles to: pushed, as signed.
    let (params, bytes) = compiled_push_params(1, "c-other", &[Component::compiled(X, &row_source())]);
    let evs = call(&admin, "ui.push", params).await;
    let p = pushed(&evs);
    assert_eq!(p["version"], sha_hex(&bytes).as_str(), "a bundle whose trees match is stored as signed: {p}");
    assert_eq!((p["live"].as_bool(), p["new"].as_bool(), p["compiled"].as_bool()), (Some(true), Some(true), Some(true)), "{p}");
    assert_eq!(p["contract_matches"], false, "c-other is not the running tickets service's hash: {p}");
    assert_eq!(versions(&admin).await.len(), 1);
}

#[tokio::test]
async fn a_source_that_fails_to_compile_refuses_the_push() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    // `{#await}` is not in v1's subset (UI-4): refused at its line.
    let src = "<script>\n  prop(\"id\");\n</script>\n<div>\n{#await id}<p>{id}</p>{/await}\n</div>\n";
    let native = menger_ui_compiler::compile(X, src).expect_err("{#await} is refused natively");
    assert!(native.line >= 1, "a line in the file: {native:?}");

    let before = versions(&admin).await;
    let x = Component { name: X.into(), src: src.into(), tree: Tree::Absent, uses: vec![] };
    let (params, _) = compiled_push_params(1, "c1", &[x]);
    let evs = call(&admin, "ui.push", params).await;
    let e = refused(&evs, "compile_failed", X);
    assert_eq!(e["line"], native.line, "the line the compiler names: {e}");
    assert_eq!(versions(&admin).await, before, "versions unchanged");
}

#[tokio::test]
async fn trees_are_filled_in_when_absent() {
    let m = start_with(true).await;
    let admin = connect(m.s.port).await;
    let native = support::compile(X, &row_source()).0;
    let contract = tickets_hash(&admin).await;

    let (params, _) = compiled_push_params(1, &contract, &[Component::compiled(X, &row_source()).tree(Tree::Absent)]);
    let evs = call(&admin, "ui.push", params).await;
    let p = pushed(&evs);
    assert_eq!(p["compiled"], true, "{p}");
    assert_eq!(p["contract_matches"], true, "the manifest names the running tickets service's hash: {p}");
    let version = p["version"].as_str().expect("the stored version").to_string();
    let listed = versions(&admin).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!((listed[0]["hash"].as_str(), listed[0]["live"].as_bool()), (Some(version.as_str()), Some(true)), "{listed:?}");

    // The stored bundle holds the trees the server wrote.
    let shell = m.shell.unwrap();
    let token = support::mint_session(m.s.port).await;
    let cookie = format!("{}={token}", menger::login_names().session_cookie_for(&format!("http://127.0.0.1:{shell}")));
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let get = |file: &str| http.get(format!("http://127.0.0.1:{shell}/app/{version}/components/{file}")).header("cookie", cookie.clone()).send();
    let tree = get(&format!("{X}.json")).await.unwrap();
    assert_eq!(tree.status(), 200, "components/{X}.json is served");
    assert_eq!(tree.text().await.unwrap(), native.tree, "the compiled tree");
    let handlers = get(&format!("{X}.rhai")).await.unwrap();
    assert_eq!(handlers.status(), 200, "components/{X}.rhai is served");
    assert_eq!(handlers.text().await.unwrap(), native.handlers, "the compiled handlers");
}

#[tokio::test]
async fn a_method_outside_the_tickets_service_is_refused() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let frob = support::calling("tickets.frobnicate");
    let x = Component::compiled(X, &frob);
    assert_eq!(x.uses, ["tickets.frobnicate"], "the source's uses");
    let (params, _) = compiled_push_params(1, "c1", &[x]);
    let evs = call(&admin, "ui.push", params).await;
    let e = refused(&evs, "unknown_method", X);
    assert_eq!(e["method"], "tickets.frobnicate", "{e}");
    assert!(versions(&admin).await.is_empty(), "nothing stored");

    let x = Component::compiled(X, &row_source());
    assert_eq!(x.uses, ["tickets.list"], "the source's uses");
    let (params, _) = compiled_push_params(1, "c1", &[x]);
    pushed(&call(&admin, "ui.push", params).await);
}

// ── a reduced `tickets`: the probe exposes `list` and nothing else ────────

/// What the probe answers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProbeEvent {
    Listed { count: u32 },
    Error { message: String },
}

impl plexus_client::FromClientError for ProbeEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        ProbeEvent::Error { message: error.to_string() }
    }
}

/// A `tickets` with `list` alone — no `search`.
#[derive(Clone)]
pub struct TicketsActivation;

#[plexus_macros::activation(version = "0.1.0", description = "a reduced tickets: list alone")]
impl TicketsActivation {
    /// Lists nothing.
    #[plexus_macros::method]
    async fn list(&self) -> impl futures::Stream<Item = ProbeEvent> + Send + 'static {
        async_stream::stream! { yield ProbeEvent::Listed { count: 0 }; }
    }
}

/// The server as main builds it — the gate, the ui service with the
/// compiler — but `tickets` the probe; its port.
async fn start_probe(dir: &std::path::Path) -> u16 {
    let login = support::Login::new();
    let port = free_port().await;
    let backend = Arc::new(menger::auth_backend::MengerAuth::new(login.access.clone(), vec![], None, BRANDING.app));
    let signers = menger_ui_service::Signers::parse(&[support::public_hex(&support::UI_SIGNER)]).expect("the test signer");
    let compiler = menger_ui_service::Compiler::open(&support::ui_compiler_wasm()).expect("the module is taken");
    let ui = menger::UiSetup::open(&dir.join("ui.db"), signers).unwrap().compiling(Some(compiler));
    let hub = menger::build_rpc_ui_over(
        &dir.join("t.db"),
        None,
        Default::default(),
        vec![],
        login.access.clone(),
        backend,
        None,
        menger::store::BODY_MAX_DEFAULT,
        vec![],
        ui,
        |_, _| TicketsActivation,
    )
    .await
    .expect("the probe's server builds");
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = login;
        TransportServer::builder(hub, conv)
            .with_websocket(port)
            .with_session_validator(gate)
            .anonymous_view()
            .build()
            .await
            .expect("build server")
            .serve()
            .await
            .expect("serve");
    });
    port
}

#[tokio::test]
async fn the_lint_reads_the_running_service_not_a_copy() {
    let search = || Component::compiled(X, &support::calling("tickets.search"));
    assert_eq!(search().uses, ["tickets.search"], "the source's uses");

    // Where `tickets` has no `search`: refused.
    let dir = scratch("menger-ui-probe-");
    let port = start_probe(dir.path()).await;
    let probe = connect(port).await;
    let (params, _) = compiled_push_params(1, "c1", &[search()]);
    let e = refused(&call(&probe, "ui.push", params).await, "unknown_method", X).clone();
    assert_eq!(e["method"], "tickets.search", "{e}");
    // What it does have passes there.
    let (params, _) = compiled_push_params(2, "c1", &[Component::compiled(X, &row_source())]);
    pushed(&call(&probe, "ui.push", params).await);

    // On the full service the same bundle is accepted.
    let m = start().await;
    let admin = connect(m.s.port).await;
    let (params, _) = compiled_push_params(1, "c1", &[search()]);
    pushed(&call(&admin, "ui.push", params).await);
}
