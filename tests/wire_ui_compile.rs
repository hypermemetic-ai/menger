//! UI-7: a push compiles — over the socket, a spawned `menger` under the
//! fixture operator with its own ui file, the test signer and
//! `--ui-compiler dist/ui-compiler.wasm` (built once per test binary by
//! `../plexus-ui/scripts/ui-compiler-wasm.sh`, `support::ui_compiler_wasm`). Each source
//! is recompiled in the server's sealed compiler and compared with the tree
//! the bundle claims (or the tree filled in); each `uses` is checked against
//! the running `tickets` service through the seam. What a source compiles
//! to is known here from the compiler natively — the same bytes (UI-5).
//! In process (the builder main uses, `menger::build_rpc_ui_over`): a
//! reduced `tickets` (a probe exposing `list` alone), to show the lint reads
//! the running service, never a copy; and stub compiler modules — one that
//! answers a fixed tree, one that never answers — to show the push compares
//! against the sandbox's answer, never a native compile. One server runs
//! with no `--ui-compiler`: the lint and the contract check still run.
//! Servers on ports the OS hands out, their files under /tmp/claude-1000.
//!
//! SV-6: rhx is switched off; these are rhx's own tests, so every server
//! here is started `--ui-allow-rhx` (in process, `UiSetup::allowing_rhx`)
//! — but `rhx_is_refused_without_ui_allow_rhx`, which starts one without
//! it: an rhx push refused `rhx_disabled` naming the component, a svelte/1
//! one taken.

mod support;

use jsonrpsee::ws_client::WsClient;
use menger::login_access::BRANDING;
use plexus_rpc::core::plexus::DynamicHub;
use plexus_rpc::transport::TransportServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use support::{call, compiled_push_params, compiled_push_params_with, connect, free_port, row_source, sha_hex, spawn_menger, ui_args, Component, Tree};

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

/// The binary with `--ui-compiler` (when `compiler`), a shell when `shell`.
async fn spawn(compiler: bool, shell: bool) -> Server {
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
    if compiler {
        extra.extend(["--ui-compiler".to_string(), support::ui_compiler_wasm().display().to_string()]);
    }
    // SV-6: these are rhx's own tests — rhx is switched off unless allowed.
    extra.push("--ui-allow-rhx".to_string());
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

async fn start_with(shell: bool) -> Server {
    spawn(true, shell).await
}

async fn start() -> Server {
    start_with(false).await
}

/// The versions as `versions` lists them: (hash, live) each.
async fn listed(c: &WsClient) -> Vec<(String, bool)> {
    versions(c).await.iter().map(|v| (v["hash"].as_str().unwrap_or_default().to_string(), v["live"] == true)).collect()
}

/// Pushes a good version first — `row_source` as `x-row`, the trees
/// compiled — and answers what `versions` lists after it.
async fn a_good_version(c: &WsClient) -> Vec<(String, bool)> {
    let (params, _) = compiled_push_params(100, "c-good", &[Component::compiled(X, &row_source())]);
    pushed(&call(c, "ui.push", params).await);
    let l = listed(c).await;
    assert_eq!(l.len(), 1, "{l:?}");
    l
}

/// The shell's GET of `/app/<version>/components/<file>` as the admin: status and text.
async fn served(m: &Server, version: &str, file: &str) -> (u16, String) {
    let shell = m.shell.expect("a shell");
    let token = support::mint_session(m.s.port).await;
    let cookie = format!("{}={token}", menger::login_names().session_cookie_for(&format!("http://127.0.0.1:{shell}")));
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let r = http.get(format!("http://127.0.0.1:{shell}/app/{version}/components/{file}")).header("cookie", cookie).send().await.unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
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

    // The handlers edited by hand, the tree right: refused the same way.
    let (params, _) = compiled_push_params(1, "c-other", &[Component::compiled(X, &row_source()).rhai("fn open(state, ev) { state }\n")]);
    let evs = call(&admin, "ui.push", params).await;
    refused(&evs, "compile_mismatch", X);
    assert!(versions(&admin).await.is_empty(), "nothing stored");

    // The same bundle with the tree the source compiles to: pushed, as signed.
    let (params, bytes) = compiled_push_params(1, "c-other", &[Component::compiled(X, &row_source())]);
    let evs = call(&admin, "ui.push", params).await;
    let p = pushed(&evs);
    assert_eq!(p["version"], sha_hex(&bytes).as_str(), "a bundle whose trees match is stored as signed: {p}");
    assert_eq!(p["uploaded"], p["version"], "the two hashes equal: {p}");
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
    let native = plexus_ui_compiler::compile(X, src).expect_err("{#await} is refused natively");
    assert!(native.line >= 1, "a line in the file: {native:?}");

    let before = a_good_version(&admin).await;
    let (params, _) = compiled_push_params(1, "c1", &[Component::source(X, src)]);
    let evs = call(&admin, "ui.push", params).await;
    let e = refused(&evs, "compile_failed", X);
    assert_eq!(e["line"], native.line, "the line the compiler names: {e}");
    assert_eq!(listed(&admin).await, before, "versions unchanged, the good one still live");
}

#[tokio::test]
async fn trees_are_filled_in_when_absent() {
    let m = start_with(true).await;
    let admin = connect(m.s.port).await;
    let native = support::compile(X, &row_source()).0;
    let contract = tickets_hash(&admin).await;

    let (params, upload) = compiled_push_params(1, &contract, &[Component::compiled(X, &row_source()).tree(Tree::Absent)]);
    let evs = call(&admin, "ui.push", params).await;
    let p = pushed(&evs);
    assert_eq!(p["compiled"], true, "{p}");
    assert_eq!(p["uploaded"], sha_hex(&upload).as_str(), "uploaded is the upload's sha256: {p}");
    assert_ne!(p["version"], p["uploaded"], "the stored bytes hold the trees the upload did not: {p}");
    assert_eq!(p["contract_matches"], true, "the manifest names the running tickets service's hash: {p}");
    let version = p["version"].as_str().expect("the stored version").to_string();
    let listed = versions(&admin).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!((listed[0]["hash"].as_str(), listed[0]["live"].as_bool()), (Some(version.as_str()), Some(true)), "{listed:?}");

    // The stored bundle holds the trees the server wrote.
    assert_eq!(served(&m, &version, &format!("{X}.json")).await, (200, native.tree), "components/{X}.json: the compiled tree");
    assert_eq!(served(&m, &version, &format!("{X}.rhai")).await, (200, native.handlers), "components/{X}.rhai: the compiled handlers");
}

#[tokio::test]
async fn a_method_outside_the_tickets_service_is_refused() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let before = a_good_version(&admin).await;
    // Outside the service: a method it lacks, one its child lacks, another service's.
    for method in ["tickets.frobnicate", "tickets.nav.frobnicate", "chats.post"] {
        let x = Component::compiled(X, &support::calling(method));
        assert_eq!(x.uses, [method], "the source's uses");
        let (params, _) = compiled_push_params(1, "c1", &[x]);
        let evs = call(&admin, "ui.push", params).await;
        let e = refused(&evs, "unknown_method", X);
        assert_eq!(e["method"], method, "{e}");
        assert_eq!(listed(&admin).await, before, "versions unchanged after {method}");
    }

    // A method under a child of the service counts.
    let x = Component::compiled(X, &support::calling("tickets.nav.tree"));
    let (params, _) = compiled_push_params(2, "c1", &[x]);
    pushed(&call(&admin, "ui.push", params).await);

    let x = Component::compiled(X, &row_source());
    assert_eq!(x.uses, ["tickets.list"], "the source's uses");
    let (params, _) = compiled_push_params(1, "c1", &[x]);
    pushed(&call(&admin, "ui.push", params).await);
}

#[tokio::test]
async fn a_manifest_that_disagrees_with_the_compiled_tree_is_refused() {
    let m = start().await;
    let admin = connect(m.s.port).await;
    let before = a_good_version(&admin).await;
    let x = || Component::compiled(X, &row_source());
    let entry = |m: &mut Value| m["components"][X].take();

    // The param's uses differ from manifest.json's (which agrees with the tree).
    let (params, _) = compiled_push_params_with(1, "c1", &[x()], |mut m| {
        let mut e = entry(&mut m);
        e["uses"] = json!(["tickets.get"]);
        m["components"][X] = e;
        m
    });
    refused(&call(&admin, "ui.push", params).await, "manifest_mismatch", X);
    assert_eq!(listed(&admin).await, before, "versions unchanged");

    // Both manifests agree, and both claim uses the compiled tree does not have.
    let (params, _) = compiled_push_params(1, "c1", &[x().uses(&["tickets.get"])]);
    refused(&call(&admin, "ui.push", params).await, "manifest_mismatch", X);
    assert_eq!(listed(&admin).await, before, "versions unchanged");

    // The param's tree hash, then its handlers hash, not the compiled files'.
    for field in ["tree", "handlers"] {
        let (params, _) = compiled_push_params_with(1, "c1", &[x()], |mut m| {
            let mut e = entry(&mut m);
            e[field] = json!("0".repeat(64));
            m["components"][X] = e;
            m
        });
        refused(&call(&admin, "ui.push", params).await, "manifest_mismatch", X);
        assert_eq!(listed(&admin).await, before, "versions unchanged after the {field} hash");
    }
}

#[tokio::test]
async fn without_a_compiler_the_lint_and_the_contract_still_run() {
    let m = spawn(false, true).await;
    let admin = connect(m.s.port).await;
    let contract = tickets_hash(&admin).await;

    // The lint reads the manifest's uses.
    let (params, _) = compiled_push_params(1, &contract, &[Component::compiled(X, &support::calling("tickets.frobnicate"))]);
    let e = refused(&call(&admin, "ui.push", params).await, "unknown_method", X).clone();
    assert_eq!(e["method"], "tickets.frobnicate", "{e}");
    assert!(versions(&admin).await.is_empty(), "nothing stored");

    // An edited tree is stored as given: nothing compiles it.
    let right = support::compile(X, &row_source()).0.tree;
    let edited = right.replacen("\"props\": [", "\"props\": [\n    \"extra\",", 1);
    let (params, bytes) = compiled_push_params(1, &contract, &[Component::compiled(X, &row_source()).tree(Tree::Given(edited.clone()))]);
    let evs = call(&admin, "ui.push", params).await;
    let p = pushed(&evs);
    assert_eq!((p["compiled"].as_bool(), p["contract_matches"].as_bool()), (Some(false), Some(true)), "{p}");
    assert_eq!((p["version"].as_str(), p["uploaded"].as_str()), (Some(sha_hex(&bytes).as_str()), Some(sha_hex(&bytes).as_str())), "stored as signed: {p}");
    let version = p["version"].as_str().unwrap().to_string();
    assert_eq!(served(&m, &version, &format!("{X}.json")).await, (200, edited), "the tree as given");
}

// ── in process: the builder main uses, a module of the test's own ─────────

/// UI-5's exports, `compile` answering `answer` whatever it is asked.
fn answering(answer: &str) -> Vec<u8> {
    let wat = format!(
        r#"(module
  (memory (export "memory") 1)
  (data (i32.const 0) "{escaped}")
  (func (export "alloc") (param i32) (result i32) (i32.const 4096))
  (func (export "free") (param i32 i32))
  (func (export "compile") (param i32 i32 i32 i32) (result i64) (i64.const {len})))"#,
        escaped = answer.replace('\\', "\\\\").replace('"', "\\\""),
        len = answer.len(),
    );
    wat::parse_str(&wat).expect("the answering module parses")
}

/// UI-5's exports, `compile` looping for ever.
fn looping() -> Vec<u8> {
    wat::parse_str(
        r#"(module
  (memory (export "memory") 1)
  (func (export "alloc") (param i32) (result i32) (i32.const 4096))
  (func (export "free") (param i32 i32))
  (func (export "compile") (param i32 i32 i32 i32) (result i64) (loop $again (br $again)) (i64.const 0)))"#,
    )
    .expect("the looping module parses")
}

#[tokio::test]
async fn the_push_compares_against_the_sandbox_never_a_native_compile() {
    // A module whose every answer is one fixed tree: a bundle compiled natively disagrees.
    let fixed = r#"{"ok":{"tree":"{}\n","handlers":""}}"#;
    let dir = scratch("menger-ui-stub-");
    let port = start_inproc(dir.path(), |store, access| menger::TicketsActivation::with_access(store, access), answering(fixed)).await;
    let admin = connect(port).await;
    let (params, _) = compiled_push_params(1, "c1", &[Component::compiled(X, &row_source())]);
    refused(&call(&admin, "ui.push", params).await, "compile_mismatch", X);
    assert!(versions(&admin).await.is_empty(), "nothing stored");
}

#[tokio::test]
async fn a_compiler_that_never_answers_fails_the_push_within_2s() {
    let dir = scratch("menger-ui-loop-");
    let port = start_inproc(dir.path(), |store, access| menger::TicketsActivation::with_access(store, access), looping()).await;
    let admin = connect(port).await;
    let (params, _) = compiled_push_params(1, "c1", &[Component::compiled(X, &row_source())]);
    let start = std::time::Instant::now();
    let evs = call(&admin, "ui.push", params).await;
    let took = start.elapsed();
    refused(&evs, "compile_failed", X);
    assert!(took < Duration::from_secs(2), "answered in {took:?}, not within 2 s");
    assert!(versions(&admin).await.is_empty(), "the store unchanged");
}

// ── a reduced `tickets`: the probe exposes `list` and nothing else ────────

/// What the probe answers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProbeEvent {
    Listed { count: u32 },
    Error { message: String },
}

impl plexus_rpc::client::FromClientError for ProbeEvent {
    fn from_client_error(error: plexus_rpc::client::ClientError) -> Self {
        ProbeEvent::Error { message: error.to_string() }
    }
}

/// A `tickets` with `list` alone — no `search`.
#[derive(Clone)]
pub struct TicketsActivation;

#[plexus_rpc::macros::activation(version = "0.1.0", description = "a reduced tickets: list alone")]
impl TicketsActivation {
    /// Lists nothing.
    #[plexus_rpc::macros::method]
    async fn list(&self) -> impl futures::Stream<Item = ProbeEvent> + Send + 'static {
        async_stream::stream! { yield ProbeEvent::Listed { count: 0 }; }
    }
}

/// The server as main builds it — the gate, the ui service with the
/// compiler — but `tickets` the probe; its port.
async fn start_probe(dir: &std::path::Path) -> u16 {
    let module = std::fs::read(support::ui_compiler_wasm()).expect("the module");
    start_inproc(dir, |_, _| TicketsActivation, module).await
}

/// The server as main builds it — the gate, the ui service compiling in
/// `module` — with `tickets` what `make` registers; its port.
async fn start_inproc<T, F>(dir: &std::path::Path, make: F, module: Vec<u8>) -> u16
where
    T: plexus_rpc::core::plexus::Activation + plexus_rpc::core::plexus::ChildRouter + Clone + 'static,
    F: FnOnce(Arc<menger::store::Store>, menger_tickets_api::access::Access) -> T,
{
    let login = support::Login::new();
    let port = free_port().await;
    let backend = Arc::new(menger::auth_backend::MengerAuth::new(login.access.clone(), vec![], None, BRANDING.app));
    let signers = plexus_ui_service::Signers::parse(&[support::public_hex(&support::UI_SIGNER)]).expect("the test signer");
    let compiler = plexus_ui_service::Compiler::new(&module).expect("the module is taken");
    let ui = menger::UiSetup::open(&dir.join("ui.db"), signers).unwrap().compiling(Some(compiler)).allowing_rhx(true);
    let service = menger::build_rpc_ui_over(
        &dir.join("t.db"),
        None,
        Default::default(),
        vec![],
        login.access.clone(),
        backend,
        None,
        menger::store::BODY_MAX_DEFAULT,
        ui,
        make,
    )
    .await
    .expect("the probe's server builds");
    let gate = login.gate(port);
    let conv = |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = login;
        TransportServer::builder(service, conv)
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

/// Acceptance 2's measurement (not a gate: timings on a shared machine):
/// ten components, sources only — UI-5's golden `ticket-row` under ten
/// names — pushed in one call; the time from the call to `pushed`.
/// `cargo test -p menger --test wire_ui_compile ten_components -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn ten_components_timing() {
    let golden = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ticket-row.rhx")).expect("UI-5's golden (a copy of ../plexus-ui's compiler/tests/golden/ticket-row.rhx)");
    let m = start().await;
    let admin = connect(m.s.port).await;
    let contract = tickets_hash(&admin).await;
    let mut runs = Vec::new();
    for n in 1..=5u32 {
        // Each run its own sources (a comment differs), so each is a new version.
        let components: Vec<Component> =
            (0..10).map(|i| Component::compiled(&format!("row-{i}"), &format!("<!-- run {n} -->\n{golden}")).tree(Tree::Absent)).collect();
        let (params, _) = compiled_push_params(n, &contract, &components);
        let start = std::time::Instant::now();
        let evs = call(&admin, "ui.push", params).await;
        let took = start.elapsed();
        let p = pushed(&evs);
        assert_eq!((p["compiled"].as_bool(), p["contract_matches"].as_bool()), (Some(true), Some(true)), "{p}");
        runs.push(took);
    }
    println!("ten components, sources only, call → pushed: {runs:?}");
}

/// SV-6: without `--ui-allow-rhx` (the default) a push carrying an rhx/1
/// component is refused `rhx_disabled`, naming it, nothing kept; a bundle of
/// svelte/1 modules is taken.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rhx_is_refused_without_ui_allow_rhx() {
    let dir = scratch("menger-ui-rhx-off-");
    let mut extra = vec!["--no-shell".to_string()];
    extra.extend(ui_args(&dir.path().join("ui.db")));
    extra.extend(["--ui-compiler".to_string(), support::ui_compiler_wasm().display().to_string()]);
    let s = spawn_menger(free_port().await, &dir.path().join("t.db"), &extra).await;
    let admin = connect(s.port).await;
    let (params, _) = compiled_push_params(1, "unknown", &[Component::compiled(X, &row_source())]);
    let e = refused(&call(&admin, "ui.push", params).await, "rhx_disabled", X).clone();
    assert!(e["message"].as_str().is_some_and(|m| m.contains("rhx/1")), "{e}");
    assert!(listed(&admin).await.is_empty(), "nothing kept");
    let (params, _) = support::module_push_params(2, "unknown", X, &["tickets.list"]);
    assert_eq!(pushed(&call(&admin, "ui.push", params).await)["live"], true);
}
