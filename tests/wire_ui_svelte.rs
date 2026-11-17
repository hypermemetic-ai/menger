//! NOTES-2: menger compiles the Svelte an editor authors — over the socket,
//! a spawned `menger` under the fixture operator with its own ui file, the
//! test signer, an author key and `--ui-svelte-compiler
//! dist/svelte-compiler.wasm` (plexus-ui's module, SV-2; built once per test
//! binary by its scripts/svelte-compiler-wasm.sh,
//! `support::svelte_compiler_wasm`). A `svelte/1` source sent to `ui.author`
//! is compiled in the service and kept; one that does not compile is refused
//! naming its line, nothing kept; a server started without the module
//! refuses `no_compiler`, as every menger did before; a file that is not the
//! module stops the start, naming the flag. Servers on ports the OS hands
//! out, their files under the test's own directory.

mod support;

use serde_json::{json, Value};
use support::{call, connect, free_port, login_args, push_params, spawn_menger, ui_args, UI_SIGNER};

/// A component as an agent writes one: its `uses`, a prop, a look on a token.
const HELLO: &str = "<script module>\n  export const uses = [];\n</script>\n\n<script>\n  let { name = \"you\" } = $props();\n</script>\n\n<p class=\"hello\">hello, {name}</p>\n\n<style>\n  .hello { color: var(--muted); }\n</style>\n";

/// The same, its `{#if}` never closed: line 9 is where Svelte's compiler stops.
const BROKEN: &str = "<script module>\n  export const uses = [];\n</script>\n\n<script>\n  let { name = \"you\" } = $props();\n</script>\n\n{#if name}<p>hello, {name}</p>\n";

struct Server {
    _s: support::Spawned,
    ws: u16,
    _dir: tempfile::TempDir,
}

/// A menger with an author key, and the Svelte compiler module when `svelte`.
async fn spawn(svelte: bool) -> Server {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let ws = free_port().await;
    let mut extra = vec!["--no-shell".to_string()];
    extra.extend(ui_args(&dir.path().join("ui.db")));
    extra.extend(["--ui-author-key".to_string(), dir.path().join("author.key").display().to_string()]);
    if svelte {
        extra.extend(["--ui-svelte-compiler".to_string(), support::svelte_compiler_wasm().display().to_string()]);
    }
    let s = spawn_menger(ws, &dir.path().join("t.db"), &extra).await;
    Server { _s: s, ws, _dir: dir }
}

fn author(name: &str, source: &str) -> Value {
    json!({ "sources": [{ "name": name, "source": source, "profile": "svelte/1" }] })
}

async fn versions(c: &jsonrpsee::ws_client::WsClient) -> usize {
    call(c, "ui.versions", json!({})).await.iter().filter(|e| e["type"] == "version").count()
}

/// With the module, an editor's `svelte/1` source is compiled in the service
/// and kept in the live bundle: `authored`, the version live, and the name
/// resolved as the live bundle's own component, in the page (`native`).
#[tokio::test]
async fn a_svelte_source_is_authored_with_the_module() {
    let m = spawn(true).await;
    let c = connect(m.ws).await;
    let live = call(&c, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(live[0]["type"], "pushed", "{live:?}");

    let made = call(&c, "ui.author", author("x-hello", HELLO)).await;
    assert_eq!(made[0]["type"], "authored", "{made:?}");
    assert_eq!((made[0]["live"].as_bool(), made[0]["new"].as_bool()), (Some(true), Some(true)), "{made:?}");
    assert_eq!(made[0]["components"], json!(["x-hello"]), "{made:?}");
    let version = made[0]["version"].as_str().expect("the authored version").to_string();

    let named = call(&c, "ui.components", json!({ "names": ["x-hello"] })).await;
    let x = named.iter().find(|e| e["type"] == "component").unwrap_or_else(|| panic!("x-hello resolves: {named:?}"));
    assert_eq!((x["from"].as_str(), x["native"].as_bool(), x["profile"].as_str(), x["version"].as_str()), (Some("bundle"), Some(true), Some("svelte/1"), Some(version.as_str())), "{x}");
    assert_eq!(versions(&c).await, 2, "the push and the authored version");
}

/// A source Svelte's compiler refuses is refused `compile_failed`, naming the
/// component and the line, and no version is kept.
#[tokio::test]
async fn a_source_that_does_not_compile_is_refused_naming_its_line() {
    let m = spawn(true).await;
    let c = connect(m.ws).await;
    assert_eq!(call(&c, "ui.push", push_params(1, &UI_SIGNER)).await[0]["type"], "pushed");

    let made = call(&c, "ui.author", author("x-broken", BROKEN)).await;
    assert_eq!((made[0]["type"].as_str(), made[0]["reason"].as_str(), made[0]["name"].as_str()), (Some("refused"), Some("compile_failed"), Some("x-broken")), "{made:?}");
    assert!(made[0]["line"].as_u64().is_some_and(|l| l >= 9), "its line: {made:?}");
    assert_eq!(versions(&c).await, 1, "nothing kept");
    assert!(call(&c, "ui.components", json!({ "names": ["x-broken"] })).await.iter().all(|e| e["type"] != "component"), "no such component");
}

/// Without the flag menger refuses as it always did: `no_compiler`.
#[tokio::test]
async fn without_the_module_svelte_is_refused_no_compiler() {
    let m = spawn(false).await;
    let c = connect(m.ws).await;
    assert_eq!(call(&c, "ui.push", push_params(1, &UI_SIGNER)).await[0]["type"], "pushed");

    let made = call(&c, "ui.author", author("x-hello", HELLO)).await;
    assert_eq!((made[0]["type"].as_str(), made[0]["reason"].as_str()), (Some("refused"), Some("no_compiler")), "{made:?}");
    assert_eq!(versions(&c).await, 1, "nothing kept");
}

/// A file that is not the module stops the start, naming the flag and the file.
#[tokio::test]
async fn a_file_that_is_not_the_module_stops_the_start() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let not = dir.path().join("not-a-module.wasm");
    std::fs::write(&not, b"not wasm").unwrap();
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_menger"));
    cmd.arg("--port").arg(free_port().await.to_string()).arg("--db-path").arg(dir.path().join("t.db")).arg("--data-dir").arg(dir.path()).arg("--no-shell");
    cmd.args(login_args()).args(ui_args(&dir.path().join("ui.db"))).arg("--ui-svelte-compiler").arg(&not);
    cmd.env("PLEXUS_NO_REGISTRY", "1").env_remove("MENGER_UI_SVELTE_COMPILER").stdin(std::process::Stdio::null());
    let out = tokio::task::spawn_blocking(move || cmd.output().expect("menger runs")).await.unwrap();
    assert!(!out.status.success(), "it does not start: {:?}", out.status);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("--ui-svelte-compiler") && said.contains("not-a-module.wasm"), "names the flag and the file: {said}");
}
