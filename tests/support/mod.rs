//! ACCESS-24: menger has no open mode, so every test server runs a login
//! provider — the `key` provider (IDENTITY-42), its `--admin` the fixture key
//! [`ADMIN`] — and every test client proves that key at the upgrade
//! (`Authorization: Bearer plexus-key.…`), as an agent does. What a test
//! once did with login off it does as the operator: admin on `*`.
//!
//! In process ([`hub`], [`gate`]): the hub as `main` builds it behind the
//! gate, and the upgrade's validator over the same session table, for a
//! test that wires its own `TransportServer`. A spawned binary
//! ([`LOGIN_ARGS`]): `--auth-provider key --admin <ADMIN's principal>`.
#![allow(dead_code)]

use ed25519_dalek::SigningKey;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use menger::login::{self, Provider};
use menger::sessions::{Gate, Sessions};
use menger_tickets_api::access::Access;
use plexus_auth_core::adapters::{key_principal, sign_key_proof};
use plexus_core::plexus::DynamicHub;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The fixture operator's Ed25519 seed: `--admin`, admin on `*`.
pub const ADMIN: [u8; 32] = [0x7e; 32];

/// A key's principal, `key:<hex>`.
pub fn principal_of(seed: &[u8; 32]) -> String {
    key_principal(SigningKey::from_bytes(seed).verifying_key().as_bytes()).to_string()
}

/// [`ADMIN`]'s principal.
pub fn admin() -> String {
    principal_of(&ADMIN)
}

/// The flags a spawned binary takes to run under the fixture operator.
pub fn login_args() -> [String; 4] {
    ["--auth-provider".into(), "key".into(), "--admin".into(), admin()]
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

/// A nonce no other proof in this process has used.
fn nonce() -> [u8; 32] {
    static N: AtomicU32 = AtomicU32::new(1);
    let mut n = [0u8; 32];
    n[..4].copy_from_slice(&N.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    n[4..8].copy_from_slice(&std::process::id().to_le_bytes());
    n[8..24].copy_from_slice(&std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos().to_le_bytes());
    n
}

/// A fresh key proof by `seed` naming the socket on `port` (one upgrade's).
pub fn proof_of(seed: &[u8; 32], port: u16) -> String {
    sign_key_proof(seed, &format!("ws://127.0.0.1:{port}"), "test", &nonce(), now())
}

/// `Authorization: Bearer <a fresh proof by ADMIN>` for the socket on `port`.
pub fn admin_headers(port: u16) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", proof_of(&ADMIN, port))).unwrap());
    h
}

/// A client signed in as [`ADMIN`] on `port`, waiting (5 s) for the server.
pub async fn connect(port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..100 {
        if let Ok(c) = WsClientBuilder::default().set_headers(admin_headers(port)).build(&url).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

/// The login a test server runs: one session table, the rule over it with
/// [`ADMIN`] as `--admin`.
pub struct Login {
    pub sessions: Arc<Sessions>,
    pub access: Access,
}

impl Login {
    pub fn new() -> Self {
        let sessions = Arc::new(Sessions::new(3600, &menger::login_names()));
        let table = menger::session_table(&sessions);
        let access = Access::with_login(table.clone(), vec![admin()]).minting(table);
        Self { sessions, access }
    }

    /// The upgrade's validator for the socket on `port`: a minted session,
    /// or a key proof naming that socket.
    pub fn gate(&self, port: u16) -> Arc<Gate> {
        let providers: Vec<Provider> = vec!["key".parse().unwrap()];
        let carriers = login::boundary_at(&providers, &login::socket_urls(port, None)).unwrap();
        Arc::new(Gate::new(self.sessions.clone(), carriers).noticing(menger::MengerAccess(self.access.clone())))
    }
}

impl Default for Login {
    fn default() -> Self {
        Self::new()
    }
}

/// The hub on `db_path` behind `login` (main's: the gate, `--admin`).
pub async fn hub(db_path: &Path, login: &Login) -> Arc<DynamicHub> {
    menger::build_rpc(db_path, login.access.clone()).await.expect("open database")
}

/// A session minted for [`ADMIN`] on the socket at `ws_port`: its token — a
/// bearer, the `access_token` cookie, or the shell's session cookie.
pub async fn mint_session(ws_port: u16) -> String {
    mint_session_for(&ADMIN, ws_port).await
}

/// A session minted for the key `seed` on the socket at `ws_port`. v2: by
/// `auth.login {flow: {session}}` — `session.login` went with the `session`
/// activation (IDENTITY-42's flow is the protocol's now).
pub async fn mint_session_for(seed: &[u8; 32], ws_port: u16) -> String {
    use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::ObjectParams;
    let c = connect_as(seed, ws_port).await;
    let mut p = ObjectParams::new();
    p.insert("method", "auth.login").unwrap();
    p.insert("params", serde_json::json!({ "flow": { "session": {} } })).unwrap();
    let mut s: Subscription<serde_json::Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let item = s.next().await.expect("an answer").unwrap();
    item["content"]["token"].as_str().unwrap_or_else(|| panic!("auth.login: {item}")).to_string()
}

/// A client signed in as the key `seed` on `port`, waiting (5 s) for the server.
pub async fn connect_as(seed: &[u8; 32], port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..100 {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", proof_of(seed, port))).unwrap());
        if let Ok(c) = WsClientBuilder::default().set_headers(h).build(&url).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

/// A call through `menger.call`: each data item's content; an error item
/// whole (a gate's refusal), and the stream ends there.
pub async fn call(c: &WsClient, method: &str, params: serde_json::Value) -> Vec<serde_json::Value> {
    use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::ObjectParams;
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    let mut s: Subscription<serde_json::Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let mut out = Vec::new();
    while let Ok(Some(Ok(item))) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
        match item["type"].as_str() {
            Some("data") => out.push(item["content"].clone()),
            Some("error") => {
                out.push(item);
                break;
            }
            _ => break,
        }
    }
    out
}

// ── UI-3: the ui service's fixtures ───────────────────────────────────────

/// The test signer's Ed25519 seed: its public key is `--ui-signer`.
pub const UI_SIGNER: [u8; 32] = [0x5a; 32];

/// A key that is no `--ui-signer`.
pub const NOT_A_SIGNER: [u8; 32] = [0x3c; 32];

/// `seed`'s Ed25519 public key, 64 hex digits (what `--ui-signer` takes).
pub fn public_hex(seed: &[u8; 32]) -> String {
    SigningKey::from_bytes(seed).verifying_key().as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// The flags beside [`login_args`] a ui test server takes: its own ui file,
/// and [`UI_SIGNER`] as the one signer.
pub fn ui_args(ui_db: &Path) -> [String; 4] {
    ["--ui-db-path".into(), ui_db.display().to_string(), "--ui-signer".into(), public_hex(&UI_SIGNER)]
}

/// Version `n`'s `index.html`: names `n`, the socket's port placeholder
/// and (UI-6) the served version's.
pub fn index_html(n: u32) -> String {
    format!("<!doctype html><title>v{n}</title><p>version {n} on {{{{WS_PORT}}}} at {{{{UI_VERSION}}}}</p>")
}

/// Version `n`'s one asset.
pub fn asset_js(n: u32) -> String {
    format!("console.log({n});")
}

/// A small bundle: a tar of `index.html` ([`index_html`]) and `app.js`
/// ([`asset_js`]).
pub fn bundle(n: u32) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for (name, body) in [("index.html", index_html(n)), ("app.js", asset_js(n))] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap()
}

/// The sha256 of `bytes`, hex — a version's name.
pub fn sha_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// `ui.push`'s params for version `n`, signed by `signer` over its digest.
pub fn push_params(n: u32, signer: &[u8; 32]) -> serde_json::Value {
    use base64::Engine;
    use ed25519_dalek::Signer;
    use sha2::Digest;
    let bytes = bundle(n);
    let digest = sha2::Sha256::digest(&bytes);
    let signature = SigningKey::from_bytes(signer).sign(&digest);
    let b64 = base64::engine::general_purpose::STANDARD;
    serde_json::json!({
        "bundle": b64.encode(&bytes),
        "manifest": json_manifest(&format!("c{n}"), &format!("a{n}")),
        "signature": b64.encode(signature.to_bytes()),
        "commit": format!("{n:07x}"),
    })
}

/// UI-9: a manifest as the contract writes it (UI-4's JSON) naming
/// `contract` and `abi`, with no routes, components or slots.
pub fn json_manifest(contract: &str, abi: &str) -> String {
    serde_json::json!({ "abi": abi, "contract": contract, "routes": {}, "components": {}, "slots": {} }).to_string()
}

/// A loopback port the OS just handed out, never handed to another test in
/// this process.
pub async fn free_port() -> u16 {
    static HANDED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let mut handed = HANDED.lock().unwrap();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}

/// A spawned `menger`. Dropping SIGKILLs it.
pub struct Spawned {
    child: std::process::Child,
    pub port: u16,
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The binary on `db` with its socket on `port`, under the fixture operator
/// ([`login_args`]) and `extra` (a shell port, [`ui_args`], …), waiting
/// until the socket answers.
pub async fn spawn_menger(port: u16, db: &Path, extra: &[String]) -> Spawned {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_menger"));
    cmd.arg("--port").arg(port.to_string()).arg("--db-path").arg(db).args(login_args()).args(extra);
    // Never the default data directory.
    cmd.arg("--data-dir").arg(db.parent().expect("a db in a directory"));
    for var in ["MENGER_DB_PATH", "MENGER_AUTH_PROVIDERS", "MENGER_ADMINS", "MENGER_UI_DB_PATH", "MENGER_UI_SIGNERS", "MENGER_UI_COMPILER", "RUST_LOG"] {
        cmd.env_remove(var);
    }
    cmd.env("PLEXUS_NO_REGISTRY", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut server = Spawned { child: cmd.spawn().expect("spawn menger"), port };
    for _ in 0..200 {
        if let Some(status) = server.child.try_wait().unwrap() {
            panic!("menger exited at start: {status}");
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return server;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("menger never answered on {port}");
}

/// A loopback proxy in front of the socket on `upstream` that signs each
/// upgrade in as [`ADMIN`] — an `Authorization: Bearer <fresh proof>` line
/// added to the request head — for a client that cannot send a header of
/// its own (plexus-client's `WsTransport::connect` takes a URL alone). Its
/// port.
pub async fn signing_proxy(upstream: u16) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if client.read(&mut byte).await.ok().filter(|n| *n == 1).is_none() {
                        return;
                    }
                    head.push(byte[0]);
                }
                let Ok(mut server) = tokio::net::TcpStream::connect(("127.0.0.1", upstream)).await else { return };
                let auth = format!("Authorization: Bearer {}\r\n\r\n", proof_of(&ADMIN, upstream));
                head.truncate(head.len() - 2);
                head.extend_from_slice(auth.as_bytes());
                if server.write_all(&head).await.is_err() {
                    return;
                }
                let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            });
        }
    });
    port
}

// ── UI-9: sections — bundles whose manifest declares components and slots ──

/// The layout every sections test fills: its one slot is [`SLOT`].
pub const LAYOUT: &str = "board-layout";

/// [`LAYOUT`]'s one section; the manifest's default for it is `ticket-list`.
pub const SLOT: &str = "main";

/// The slots most versions declare: `{ board-layout: { main: "ticket-list" } }`.
pub fn default_slots() -> serde_json::Value {
    serde_json::json!({ LAYOUT: { SLOT: "ticket-list" } })
}

/// Version `n`'s manifest (UI-4's JSON): `components` each named, and
/// `slots` as given (`{ layout: { slot: default component } }`).
pub fn sections_manifest(n: u32, components: &[&str], slots: &serde_json::Value) -> String {
    let components: serde_json::Map<String, serde_json::Value> = components
        .iter()
        .map(|c| (c.to_string(), serde_json::json!({ "tree": format!("{n}-{c}-tree"), "handlers": format!("{n}-{c}-rhai"), "uses": [] })))
        .collect();
    serde_json::json!({
        "abi": "engine/1", "contract": format!("c{n}"), "commit": format!("menger@{n:07x}"),
        "routes": { "#/": LAYOUT },
        "components": components,
        "slots": slots,
    })
    .to_string()
}

/// Component `c` of version `n`'s compiled tree, as the bundle holds it.
pub fn component_json(n: u32, c: &str) -> String {
    format!("{{\"name\":\"{c}\",\"v\":{n}}}")
}

/// Version `n` with sections: `index.html` ([`index_html`]), `manifest.json`
/// ([`sections_manifest`]) and each component's `components/<name>.json`
/// ([`component_json`]).
pub fn slotted_bundle(n: u32, components: &[&str], slots: &serde_json::Value) -> Vec<u8> {
    let mut files = vec![("index.html".to_string(), index_html(n)), ("manifest.json".to_string(), sections_manifest(n, components, slots))];
    files.extend(components.iter().map(|c| (format!("components/{c}.json"), component_json(n, c))));
    let mut b = tar::Builder::new(Vec::new());
    for (name, body) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap()
}

/// `ui.push`'s params for [`slotted_bundle`]`(n, components, slots)`, signed
/// by [`UI_SIGNER`]; the `manifest` param is the same JSON the bundle holds.
pub fn slotted_push_params(n: u32, components: &[&str], slots: &serde_json::Value) -> serde_json::Value {
    use base64::Engine;
    use ed25519_dalek::Signer;
    use sha2::Digest;
    let bytes = slotted_bundle(n, components, slots);
    let signature = SigningKey::from_bytes(&UI_SIGNER).sign(&sha2::Sha256::digest(&bytes));
    let b64 = base64::engine::general_purpose::STANDARD;
    serde_json::json!({
        "bundle": b64.encode(&bytes),
        "manifest": sections_manifest(n, components, slots),
        "signature": b64.encode(signature.to_bytes()),
        "commit": format!("{n:07x}"),
    })
}

/// An open call through `menger.call` — a watch held; read it with [`next_item`].
pub async fn subscribe(c: &WsClient, method: &str, params: serde_json::Value) -> jsonrpsee::core::client::Subscription<serde_json::Value> {
    use jsonrpsee::core::client::SubscriptionClientT;
    use jsonrpsee::core::params::ObjectParams;
    let mut p = ObjectParams::new();
    p.insert("method", method).unwrap();
    p.insert("params", params).unwrap();
    c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap()
}

/// The next item of an open call within `within`: a data item's content,
/// an error item whole; `None` when the stream ended or nothing came.
pub async fn next_item(s: &mut jsonrpsee::core::client::Subscription<serde_json::Value>, within: Duration) -> Option<serde_json::Value> {
    match tokio::time::timeout(within, s.next()).await {
        Ok(Some(Ok(item))) => match item["type"].as_str() {
            Some("data") => Some(item["content"].clone()),
            Some("error") => Some(item),
            _ => None,
        },
        _ => None,
    }
}

// ── UI-7: bundles with sources — the push compiles, compares and lints ────

/// `dist/ui-compiler.wasm`, built once per test binary by
/// `scripts/ui-compiler-wasm.sh` (UI-5) into this checkout's `dist/` — what
/// a test server takes as `--ui-compiler`.
pub fn ui_compiler_wasm() -> std::path::PathBuf {
    static BUILT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            let status = std::process::Command::new("bash")
                .arg(root.join("scripts/ui-compiler-wasm.sh"))
                .current_dir(root)
                .stdout(std::process::Stdio::null())
                .status()
                .expect("scripts/ui-compiler-wasm.sh runs");
            assert!(status.success(), "scripts/ui-compiler-wasm.sh failed: {status}");
            let path = root.join("dist/ui-compiler.wasm");
            assert!(path.is_file(), "{} was written", path.display());
            path
        })
        .clone()
}

/// What a bundle holds as a component's compiled tree.
#[derive(Debug, Clone)]
pub enum Tree {
    /// The tree the compiler makes of the source, and its handlers.
    Compiled,
    /// These bytes as `components/<name>.json` (the compiled handlers beside).
    Given(String),
    /// Neither `components/<name>.json` nor `.rhai`: the source alone.
    Absent,
}

/// One component of a bundle: `src/<name>.svelte`, its tree as [`Tree`]
/// says, and the `uses` its manifest entry names.
#[derive(Debug, Clone)]
pub struct Component {
    pub name: String,
    pub src: String,
    pub tree: Tree,
    pub uses: Vec<String>,
    /// These bytes as `components/<name>.rhai` in place of the compiled
    /// handlers (when the bundle holds a tree).
    pub rhai: Option<String>,
}

impl Component {
    /// `name` from `src`, its tree compiled, its `uses` what the compiler finds.
    pub fn compiled(name: &str, src: &str) -> Self {
        let uses = compile(name, src).tree_json()["uses"].as_array().map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Self { name: name.into(), src: src.into(), tree: Tree::Compiled, uses, rhai: None }
    }

    /// `name` from `src`, which need not compile: no tree, no uses.
    pub fn source(name: &str, src: &str) -> Self {
        Self { name: name.into(), src: src.into(), tree: Tree::Absent, uses: vec![], rhai: None }
    }

    pub fn tree(mut self, tree: Tree) -> Self {
        self.tree = tree;
        self
    }

    pub fn rhai(mut self, rhai: &str) -> Self {
        self.rhai = Some(rhai.into());
        self
    }

    pub fn uses(mut self, uses: &[&str]) -> Self {
        self.uses = uses.iter().map(|u| u.to_string()).collect();
        self
    }
}

/// The compiler's answer for `src` as `name`, natively — the same bytes the
/// module answers (UI-5).
pub struct Native(pub plexus_ui_compiler::Compiled);

impl Native {
    pub fn tree_json(&self) -> serde_json::Value {
        serde_json::from_str(&self.0.tree).expect("a compiled tree is JSON")
    }
}

/// [`Native`] for a source that compiles.
pub fn compile(name: &str, src: &str) -> Native {
    Native(plexus_ui_compiler::compile(name, src).unwrap_or_else(|r| panic!("{name} must compile natively: {r:?}")))
}

/// A component whose one handler calls `method` (its `uses`).
pub fn calling(method: &str) -> String {
    format!(
        "<script>\n  prop(\"id\");\n  fn open(state, ev) {{ call(\"{method}\", #{{ id: ev.props.id }}); state }}\n</script>\n<p class=\"row\" on:click={{open}}>{{id}}</p>\n"
    )
}

/// A source that compiles: [`calling`] `tickets.list`.
pub fn row_source() -> String {
    calling("tickets.list")
}

/// The manifest (UI-4's JSON) of `components`, its `contract` as given:
/// each component's `tree` and `handlers` the sha256 of the files the
/// bundle holds (of the compiled ones when it holds none), and its `uses`.
pub fn compiled_manifest(n: u32, contract: &str, components: &[Component]) -> String {
    let entries: serde_json::Map<String, serde_json::Value> = components
        .iter()
        .map(|c| {
            let native = plexus_ui_compiler::compile(&c.name, &c.src).ok();
            let tree = match (&c.tree, &native) {
                (Tree::Given(t), _) => sha_hex(t.as_bytes()),
                (_, Some(n)) => sha_hex(n.tree.as_bytes()),
                (_, None) => String::new(),
            };
            let handlers = match (&c.rhai, &native) {
                (Some(r), _) if !matches!(c.tree, Tree::Absent) => sha_hex(r.as_bytes()),
                (_, Some(n)) => sha_hex(n.handlers.as_bytes()),
                (_, None) => String::new(),
            };
            (c.name.clone(), serde_json::json!({ "tree": tree, "handlers": handlers, "uses": c.uses }))
        })
        .collect();
    serde_json::json!({
        "abi": "engine/1", "contract": contract, "commit": format!("menger@{n:07x}"),
        "routes": { "#/": components.first().map(|c| c.name.clone()).unwrap_or_default() },
        "components": entries,
        "slots": {},
    })
    .to_string()
}

/// Version `n` holding `components`: `index.html`, `manifest.json`
/// ([`compiled_manifest`]), each `src/<name>.svelte`, and the trees and
/// handlers each [`Tree`] says.
pub fn compiled_bundle(n: u32, contract: &str, components: &[Component]) -> Vec<u8> {
    let mut files = vec![("index.html".to_string(), index_html(n)), ("manifest.json".to_string(), compiled_manifest(n, contract, components))];
    for c in components {
        files.push((format!("src/{}.svelte", c.name), c.src.clone()));
        let native = plexus_ui_compiler::compile(&c.name, &c.src).ok();
        match (&c.tree, native) {
            (Tree::Absent, _) => {}
            (Tree::Given(t), n) => {
                files.push((format!("components/{}.json", c.name), t.clone()));
                if let Some(h) = c.rhai.clone().or(n.map(|n| n.handlers)) {
                    files.push((format!("components/{}.rhai", c.name), h));
                }
            }
            (Tree::Compiled, Some(n)) => {
                files.push((format!("components/{}.json", c.name), n.tree));
                files.push((format!("components/{}.rhai", c.name), c.rhai.clone().unwrap_or(n.handlers)));
            }
            (Tree::Compiled, None) => {}
        }
    }
    let mut b = tar::Builder::new(Vec::new());
    for (name, body) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap()
}

/// The test signer's signature over `bytes`, and `ui.push`'s params for them
/// with `manifest` — an upload built by hand.
pub fn signed_params(bytes: &[u8], manifest: &str, n: u32) -> serde_json::Value {
    use base64::Engine;
    use ed25519_dalek::Signer;
    use sha2::Digest;
    let signature = SigningKey::from_bytes(&UI_SIGNER).sign(&sha2::Sha256::digest(bytes));
    let b64 = base64::engine::general_purpose::STANDARD;
    serde_json::json!({ "bundle": b64.encode(bytes), "manifest": manifest, "signature": b64.encode(signature.to_bytes()), "commit": format!("{n:07x}") })
}

/// `ui.push`'s params for [`compiled_bundle`]`(n, contract, components)`,
/// signed by [`UI_SIGNER`]; the `manifest` param is the JSON the bundle
/// holds. With the bundle's bytes.
pub fn compiled_push_params(n: u32, contract: &str, components: &[Component]) -> (serde_json::Value, Vec<u8>) {
    compiled_push_params_with(n, contract, components, |m| m)
}

/// [`compiled_push_params`], the `manifest` param `param` of the JSON the
/// bundle holds — a param that disagrees with `manifest.json`.
pub fn compiled_push_params_with(n: u32, contract: &str, components: &[Component], param: impl FnOnce(serde_json::Value) -> serde_json::Value) -> (serde_json::Value, Vec<u8>) {
    use base64::Engine;
    use ed25519_dalek::Signer;
    use sha2::Digest;
    let bytes = compiled_bundle(n, contract, components);
    let signature = SigningKey::from_bytes(&UI_SIGNER).sign(&sha2::Sha256::digest(&bytes));
    let b64 = base64::engine::general_purpose::STANDARD;
    let params = serde_json::json!({
        "bundle": b64.encode(&bytes),
        "manifest": param(serde_json::from_str(&compiled_manifest(n, contract, components)).unwrap()).to_string(),
        "signature": b64.encode(signature.to_bytes()),
        "commit": format!("{n:07x}"),
    });
    (params, bytes)
}
