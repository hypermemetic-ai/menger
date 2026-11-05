//! UI-3: `/app/*` from the ui store, over HTTP — a spawned `menger` with its
//! shell, under the fixture operator, its own ui file and the test signer.
//! The app's routes are behind a session (IDENTITY-19, unchanged): each
//! request carries the shell's session cookie, a session minted on the
//! socket (`support::mint_session`).
//!
//! Each server on ports the OS hands out.

mod support;

use serde_json::json;
use std::path::Path;
use support::{call, connect, connect_as, free_port, push_params, sha_hex, spawn_menger, ui_args, UI_SIGNER};

/// A key that holds a session and no preview.
const SOMEONE: [u8; 32] = [0x50; 32];

struct Served {
    _server: support::Spawned,
    _dir: tempfile::TempDir,
    ws: u16,
    shell: u16,
    http: reqwest::Client,
}

impl Served {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.shell)
    }

    /// The shell's session cookie carrying `token`.
    fn jar(&self, token: &str) -> String {
        format!("{}={token}", menger::login_names().session_cookie_for(&format!("http://127.0.0.1:{}", self.shell)))
    }

    async fn get(&self, path: &str, token: Option<&str>) -> reqwest::Response {
        let mut r = self.http.get(self.url(path));
        if let Some(t) = token {
            r = r.header("cookie", self.jar(t));
        }
        r.send().await.unwrap()
    }
}

async fn start() -> Served {
    start_with(None, &[]).await
}

/// UI-15: a menger on `ui_db` (none: its own), started with `more` flags
/// (`--ui-channel`, `--env`).
async fn start_with(ui_db: Option<&Path>, more: &[&str]) -> Served {
    let (ws, shell) = (free_port().await, free_port().await);
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let empty = dir.path().join("no-dist");
    std::fs::create_dir_all(&empty).unwrap();
    let mut extra = vec!["--shell-port".to_string(), shell.to_string(), "--app-dist".into(), empty.display().to_string()];
    extra.extend(ui_args(ui_db.unwrap_or(&dir.path().join("ui.db"))));
    extra.extend(more.iter().map(|s| s.to_string()));
    let server = spawn_menger(ws, &dir.path().join("t.db"), &extra).await;
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", shell)).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Served { _server: server, _dir: dir, ws, shell, http }
}

fn cache(r: &reqwest::Response) -> Option<String> {
    r.headers().get("cache-control").and_then(|v| v.to_str().ok()).map(str::to_string)
}

#[tokio::test]
async fn a_hashed_path_is_immutable_and_the_entrypoint_is_not() {
    let m = start().await;
    let admin = connect(m.ws).await;
    let pushed = call(&admin, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(pushed[0]["type"], "pushed", "{pushed:?}");
    let hash = sha_hex(&support::bundle(1));
    let token = support::mint_session(m.ws).await;

    let file = m.get(&format!("/app/{hash}/index.html"), Some(&token)).await;
    assert_eq!(file.status(), 200);
    assert_eq!(cache(&file).as_deref(), Some("public, max-age=31536000, immutable"), "a hashed path is immutable");
    assert_eq!(file.text().await.unwrap(), support::index_html(1), "the bundle's file, as pushed");
    let asset = m.get(&format!("/app/{hash}/app.js"), Some(&token)).await;
    assert_eq!((asset.status().as_u16(), cache(&asset).as_deref()), (200, Some("public, max-age=31536000, immutable")));
    assert_eq!(asset.text().await.unwrap(), support::asset_js(1));
    assert_eq!(m.get(&format!("/app/{hash}/nothing.js"), Some(&token)).await.status(), 404, "a file the bundle lacks");

    let entry = m.get("/app/", Some(&token)).await;
    assert_eq!(entry.status(), 200);
    assert_eq!(cache(&entry).as_deref(), Some("no-store"), "the entrypoint is never kept");
    let body = entry.text().await.unwrap();
    assert!(body.contains(&format!("version 1 on {}", m.ws)), "the live entrypoint, stamped with the socket's port: {body}");
    assert!(body.contains(&format!("at {hash}")), "UI-6: stamped with the served version's hash: {body}");
    assert!(!body.contains("{{"), "{body}");
}

#[tokio::test]
async fn a_preview_is_served_to_its_principal_alone() {
    let m = start().await;
    let admin = connect(m.ws).await;
    for n in [1, 2] {
        let pushed = call(&admin, "ui.push", push_params(n, &UI_SIGNER)).await;
        assert_eq!(pushed[0]["type"], "pushed", "{pushed:?}");
    }
    let one = sha_hex(&support::bundle(1));
    let set = call(&admin, "ui.preview", json!({ "version": one })).await;
    assert_eq!(set.len(), 1, "{set:?}");
    assert_eq!(set[0]["type"], "preview", "{set:?}");
    assert_eq!(set[0]["principal"], support::admin(), "{set:?}");
    assert_eq!(set[0]["version"], one.as_str(), "{set:?}");

    let mine = support::mint_session(m.ws).await;
    let body = m.get("/app/", Some(&mine)).await.text().await.unwrap();
    assert!(body.contains("version 1 on"), "the admin's session gets its preview: {body}");
    assert!(body.contains(&format!("at {one}")), "UI-6: the stamp names the version served, the preview's: {body}");

    // Anyone else — a session of a principal holding no preview — gets live.
    call(&admin, "auth.grant", json!({ "principal": support::principal_of(&SOMEONE), "role": "viewer", "scope": "*" })).await;
    let theirs = support::mint_session_for(&SOMEONE, m.ws).await;
    let body = m.get("/app/", Some(&theirs)).await.text().await.unwrap();
    assert!(body.contains("version 2 on"), "nobody else's page changes: {body}");
    // And with no session at all, the app's routes still ask for a sign-in.
    let anonymous = m.get("/app/", None).await;
    assert_eq!(anonymous.status(), 303, "no session: the login page");
    let to = anonymous.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    assert!(to.starts_with("/login?next="), "{to}");

    // `preview {}` clears it: the admin's page is live again.
    let cleared = call(&admin, "ui.preview", json!({})).await;
    assert_eq!(cleared.len(), 1, "{cleared:?}");
    assert_eq!(cleared[0]["type"], "preview", "{cleared:?}");
    assert_eq!(cleared[0]["principal"], support::admin(), "{cleared:?}");
    assert!(cleared[0]["version"].is_null(), "{cleared:?}");
    let body = m.get("/app/", Some(&mine)).await.text().await.unwrap();
    assert!(body.contains("version 2 on"), "cleared: live again: {body}");
}

#[tokio::test]
async fn no_live_version_is_the_placeholder() {
    let m = start().await;
    let token = support::mint_session(m.ws).await;
    let r = m.get("/app/", Some(&token)).await;
    assert_eq!(r.status(), 200);
    let body = r.text().await.unwrap();
    assert!(body.contains("menger v2") && body.contains("No frontend yet"), "the placeholder: {body}");
    assert!(body.contains(&format!("-P {}", m.ws)), "stamped as the placeholder is: {body}");
    assert!(!body.contains("preload"), "FRAME-16: the placeholder preloads nothing — there is no version to boot: {body}");
    let admin = connect(m.ws).await;
    assert!(call(&admin, "ui.versions", json!({})).await.is_empty(), "versions answers none");
}

/// UI-13 (owner, 2026-10-29: "I need a frontend on test"): `/` serves what
/// `/app/` serves — the live entrypoint, stamped, never kept — so a sign-in
/// that returns to `/` lands on the app; with no session it asks for one.
#[tokio::test]
async fn the_root_serves_what_app_serves() {
    let m = start().await;
    let admin = connect(m.ws).await;
    let pushed = call(&admin, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(pushed[0]["type"], "pushed", "{pushed:?}");
    let token = support::mint_session(m.ws).await;
    let root = m.get("/", Some(&token)).await;
    assert_eq!(root.status(), 200);
    assert_eq!(cache(&root).as_deref(), Some("no-store"), "the root is the entrypoint, never kept");
    let (at_root, at_app) = (root.text().await.unwrap(), m.get("/app/", Some(&token)).await.text().await.unwrap());
    assert!(at_root.contains(&format!("version 1 on {}", m.ws)), "the live entrypoint at /: {at_root}");
    assert_eq!(at_root, at_app, "/ and /app/ are one page");
    let anonymous = m.get("/", None).await;
    assert_eq!(anonymous.status(), 303, "no session: the login page");
}

/// A `<link …>`'s attributes: `name="value"`, `name='value'` or a bare `name`
/// (its value empty), names lower-cased.
fn link_attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag.trim_start_matches("<link").trim_end_matches('>').trim_end_matches('/');
    loop {
        rest = rest.trim_start();
        let Some(end) = rest.find(|c: char| c == '=' || c.is_whitespace()).or((!rest.is_empty()).then_some(rest.len())) else { break };
        let name = rest[..end].to_ascii_lowercase();
        rest = &rest[end..];
        if let Some(after) = rest.strip_prefix('=') {
            let q = after.chars().next().unwrap_or(' ');
            if q == '"' || q == '\'' {
                let close = after[1..].find(q).map(|i| i + 1).unwrap_or(after.len());
                out.push((name, after[1..close].to_string()));
                rest = after.get(close + 1..).unwrap_or("");
            } else {
                let close = after.find(char::is_whitespace).unwrap_or(after.len());
                out.push((name, after[..close].to_string()));
                rest = &after[close..];
            }
        } else if !name.is_empty() {
            out.push((name, String::new()));
        } else {
            break;
        }
    }
    out
}

/// FRAME-26: what a bundle's Svelte runtime stands for here (the server
/// serves its bytes and never runs them).
const RUNTIME_JS: &str = "export function mount() {}\n";
/// FRAME-30: a bundle's theme.css.
const THEME_CSS: &str = ":root {\n  --bg: #fbfaf7;\n  color-scheme: light;\n}\n";
/// A svelte/1 module beside it, importing it.
const MODULE_JS: &str = "import { mount as m } from \"./svelte-runtime.js\";\nexport function mount(t, o) { return m(t, o); }\n";

/// FRAME-16: menger's entrypoint (`ui/index.html`, as a push carries it)
/// names what the boot needs before its script runs, stamped with the
/// version it is served for — `<link rel="modulepreload">` of engine.js and
/// host.js, `<link rel="preload" as="fetch" crossorigin>` (empty or
/// `anonymous`) of engine.wasm, manifest.json and components.json
/// (`crossorigin`: a `fetch()` is a CORS request, and a preload without it
/// is not the one the fetch takes) — each before the script, each a file of
/// that version; and the script boots through host.js's `start(version)`,
/// not engine.js's `boot`. FRAME-26: and `<link rel="modulepreload">` of the
/// bundle's one Svelte runtime, `components/svelte-runtime.js` — always:
/// the route is a hash the server never sees, and menger's bundle always
/// holds svelte/1 components (the bundle here names it in `runtimes`).
/// FRAME-30: and `<link rel="stylesheet">` of the bundle's `theme.css`, the
/// tokens every component's sheet paints with — first, before the script.
/// UI-19: each link by the file's content URL (`/app/f/<sha256>.<ext>`),
/// the page's own references rewritten by the ui routes, beside the import
/// map (`plexus-ui-files`) the boot's imports resolve through; the
/// version's path still serves each.
#[tokio::test]
async fn the_entrypoint_preloads_the_boot() {
    let m = start().await;
    let admin = connect(m.ws).await;
    let index = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/index.html")).unwrap();
    let files = [
        ("index.html", index.as_str()),
        ("engine.js", "export default async function init() {}\nexport function boot() {}\n"),
        ("engine.wasm", "\0asm\u{1}\0\0\0"),
        ("host.js", "export function start() {}\n"),
        ("manifest.json", "{}\n"),
        ("components.json", "{}\n"),
        ("components/svelte-runtime.js", RUNTIME_JS),
        ("theme.css", THEME_CSS),
        // runtimes stands beside a svelte/1 component (the push refuses one alone).
        ("components/x-list.js", MODULE_JS),
        ("src/x-list.svelte", "<script module>\n  export const uses = [];\n</script>\n<p>x</p>\n"),
    ];
    let mut b = tar::Builder::new(Vec::new());
    for (name, body) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    let bytes = b.into_inner().unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&support::json_manifest("c1", "a1")).unwrap();
    manifest["components"] = json!({ "x-list": { "profile": "svelte/1", "module": sha_hex(MODULE_JS.as_bytes()), "uses": [] } });
    manifest["runtimes"] = json!({ "svelte/1": sha_hex(RUNTIME_JS.as_bytes()) });
    manifest["theme"] = json!(sha_hex(THEME_CSS.as_bytes()));
    let pushed = call(&admin, "ui.push", support::signed_params(&bytes, &manifest.to_string(), 1)).await;
    assert_eq!(pushed[0]["type"], "pushed", "{pushed:?}");
    let hash = sha_hex(&bytes);
    let token = support::mint_session(m.ws).await;
    let body = m.get("/app/", Some(&token)).await.text().await.unwrap();

    // UI-19: the module script — the files map (an import map) is a script too, first.
    let script = body.find("<script type=\"module\"").expect("the entrypoint has its script");
    let links: Vec<(usize, Vec<(String, String)>)> = body.match_indices("<link").map(|(at, _)| (at, link_attrs(&body[at..at + body[at..].find('>').unwrap() + 1]))).collect();
    let attr = |l: &[(String, String)], k: &str| l.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let shown: Vec<String> = links.iter().map(|(_, l)| format!("{l:?}")).collect();
    let boot = [
        ("modulepreload", None, "engine.js"),
        ("modulepreload", None, "host.js"),
        ("preload", Some("fetch"), "engine.wasm"),
        ("preload", Some("fetch"), "manifest.json"),
        ("preload", Some("fetch"), "components.json"),
        // FRAME-26: the bundle's one Svelte runtime, before the first module asks it.
        ("modulepreload", None, "components/svelte-runtime.js"),
        // FRAME-30: the theme's tokens.
        ("stylesheet", None, "theme.css"),
    ];
    for (rel, as_, file) in boot {
        // UI-19: each by its content URL — the same across versions while its bytes are.
        let body_of = files.iter().find(|(n, _)| *n == file).map(|(_, b)| *b).expect("a file of the bundle");
        let href = plexus_ui_service::routes::content_url(file, &sha_hex(body_of.as_bytes()));
        let Some((at, l)) = links.iter().find(|(_, l)| attr(l, "rel").as_deref() == Some(rel) && attr(l, "href").as_deref() == Some(href.as_str())) else {
            panic!("the entrypoint has no <link rel=\"{rel}\" href=\"{href}\">; its links: {shown:?}");
        };
        assert!(*at < script, "the {file} link comes before the script");
        if let Some(a) = as_ {
            assert_eq!(attr(l, "as").as_deref(), Some(a), "{file}'s preload is as=\"{a}\": {l:?}");
            assert!(matches!(attr(l, "crossorigin").as_deref(), Some("" | "anonymous")), "{file}'s preload carries crossorigin (empty or anonymous), as fetch() asks: {l:?}");
        }
        let r = m.get(&href, Some(&token)).await;
        assert_eq!(r.status(), 200, "{href} is a file of the version served");
        let r = m.get(&format!("/app/{hash}/{file}"), Some(&token)).await;
        assert_eq!(r.status(), 200, "{file} by the version's path still");
    }
    // UI-19: the import map of the version's files, before the script.
    let map = body.find("<script type=\"importmap\" id=\"plexus-ui-files\">").expect("the entrypoint carries its files map");
    assert!(map < script, "the files map comes before the module script");
    assert!(body[map..].contains(&format!("\"/app/{hash}/host.js\"")), "the map names host.js by the version's path: {body}");
    let code = &body[script..script + body[script..].find("</script>").expect("the script ends")];
    assert!(code.contains("host.js") && code.contains("start("), "the script boots through host.js's start(version): {code}");
    assert!(!code.contains(".boot("), "not through engine.js's boot: {code}");
    assert!(!body.contains("{{"), "every stamp filled: {body}");
}

/// STUDIO-2: the studio — the framework's own bundle, kept as `bundle:studio`
/// (pushed `live: false` under that name) — is served at `/studio/` beside
/// `/app/`, inside the session check, to a session holding `ui.author` on `*`
/// (admin, `editor` on `*`) alone — the gate's answer.
#[tokio::test]
async fn the_studio_is_mounted_for_the_admin_and_editors() {
    let m = start().await;
    let admin = connect(m.ws).await;
    let live = call(&admin, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(live[0]["type"], "pushed", "{live:?}");
    let mut studio = support::signed_params(&support::bundle(2), &json!({ "abi": "engine/1", "name": "studio", "contract": "unknown", "routes": {}, "components": {}, "slots": {} }).to_string(), 2);
    studio["live"] = json!(false);
    let kept = call(&admin, "ui.push", studio).await;
    assert_eq!((kept[0]["type"].as_str(), kept[0]["live"].as_bool()), (Some("pushed"), Some(false)), "{kept:?}");
    let two = sha_hex(&support::bundle(2));

    let mine = support::mint_session(m.ws).await;
    let page = m.get("/studio/", Some(&mine)).await;
    assert_eq!(page.status(), 200);
    let body = page.text().await.unwrap();
    assert!(body.contains("version 2 on") && body.contains(&format!("at {two}")), "the studio's own version, stamped: {body}");
    assert!(m.get("/app/", Some(&mine)).await.text().await.unwrap().contains("version 1 on"), "/app/ is still live");

    // A viewer is refused; an editor opens it; no session is the login page.
    call(&admin, "auth.grant", json!({ "principal": support::principal_of(&SOMEONE), "role": "viewer", "scope": "*" })).await;
    let viewer = support::mint_session_for(&SOMEONE, m.ws).await;
    assert_eq!(m.get("/studio/", Some(&viewer)).await.status(), 403, "a viewer");
    const EDITOR: [u8; 32] = [0x51; 32];
    call(&admin, "auth.grant", json!({ "principal": support::principal_of(&EDITOR), "role": "editor", "scope": "*" })).await;
    let editor = support::mint_session_for(&EDITOR, m.ws).await;
    assert_eq!(m.get("/studio/", Some(&editor)).await.status(), 200, "an editor");
    let anonymous = m.get("/studio/", None).await;
    assert_eq!(anonymous.status(), 303, "no session: the login page");
    // An editor under one ticket holds ui.author nowhere the studio writes.
    const NARROW: [u8; 32] = [0x52; 32];
    call(&admin, "tickets.create", json!({ "epic": "KILN", "kind": "scope", "title": "the kiln" })).await;
    let g = call(&admin, "auth.grant", json!({ "principal": support::principal_of(&NARROW), "role": "editor", "scope": "KILN" })).await;
    assert!(g.first().is_some_and(|e| e.get("error_kind").is_none()), "editor on KILN: {g:?}");
    let narrow = support::mint_session_for(&NARROW, m.ws).await;
    assert_eq!(m.get("/studio/", Some(&narrow)).await.status(), 403, "an editor on KILN alone");
    // STUDIO-6: the admin's own key, its session capped at editor@KILN-1 —
    // a ceiling the gate holds it to: refused too, the module with it.
    let capped = support::mint_session_with(&support::ADMIN, m.ws, json!({ "ceiling": ["editor@KILN-1"] })).await;
    assert_eq!(m.get("/studio/", Some(&capped)).await.status(), 403, "the admin capped at editor@KILN-1");
    assert_eq!(m.get("/studio/ui-compiler.wasm", Some(&capped)).await.status(), 403);
    assert_eq!(m.get("/studio/", Some(&mine)).await.status(), 200, "the admin uncapped");
}

/// UI-15: two mengers on one `ui.db` — one on channel `test`
/// (`--ui-channel test`), one on `prod` (the default from `--env prod`) —
/// as the two containers on the box share one store. A push on test leaves
/// prod's page as it was; `ui promote --to prod --from test`, a deployer's
/// on prod's socket, changes it in one write; `history` on each channel
/// shows its own moves; `versions` is one list, each row naming its
/// channels.
#[tokio::test]
async fn two_mengers_on_one_ui_db_serve_their_own_channels() {
    let shared = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let file = shared.path().join("ui.db");
    let test = start_with(Some(&file), &["--ui-channel", "test"]).await;
    let prod = start_with(Some(&file), &["--env", "prod"]).await;
    let (ta, pa) = (connect(test.ws).await, connect(prod.ws).await);
    let (tt, pt) = (support::mint_session(test.ws).await, support::mint_session(prod.ws).await);
    async fn page(m: &Served, tok: String) -> String {
        m.get("/app/", Some(&tok)).await.text().await.unwrap()
    }

    // A push on test: test's page, not prod's.
    let pushed = call(&ta, "ui.push", push_params(1, &UI_SIGNER)).await;
    assert_eq!(pushed[0]["type"], "pushed", "{pushed:?}");
    let v1 = sha_hex(&support::bundle(1));
    assert!(page(&test, tt.clone()).await.contains("version 1 on"), "test serves v1");
    assert!(page(&prod, pt.clone()).await.contains("No frontend yet"), "prod serves its placeholder still");
    let listed = call(&pa, "ui.versions", json!({})).await;
    assert_eq!(listed.len(), 1, "one row, as prod lists it: {listed:?}");
    assert_eq!((listed[0]["hash"].as_str(), listed[0]["live"].as_bool(), listed[0]["channels"].clone()), (Some(v1.as_str()), Some(false), json!(["test"])), "{listed:?}");

    // The promote, as a deployer on prod's socket (the pipeline key's role).
    const PIPELINE: [u8; 32] = [0x71; 32];
    let g = call(&pa, "auth.grant", json!({ "principal": support::principal_of(&PIPELINE), "role": "deployer", "scope": "*" })).await;
    assert!(g.first().is_some_and(|e| e.get("error_kind").is_none()), "deployer on *: {g:?}");
    let deployer = connect_as(&PIPELINE, prod.ws).await;
    let promoted = call(&deployer, "ui.promote", json!({ "to": "prod", "from": "test" })).await;
    assert_eq!(promoted.len(), 1, "{promoted:?}");
    assert_eq!((promoted[0]["type"].as_str(), promoted[0]["channel"].as_str(), promoted[0]["from"].as_str(), promoted[0]["version"].as_str()), (Some("promoted"), Some("prod"), Some("test"), Some(v1.as_str())), "{promoted:?}");
    assert!(promoted[0]["seq"].as_u64().is_some(), "a move: {promoted:?}");
    assert!(page(&prod, pt.clone()).await.contains("version 1 on"), "prod serves v1 now");
    assert!(page(&prod, pt.clone()).await.contains(&format!("at {v1}")), "stamped with the version served");
    let listed = call(&ta, "ui.versions", json!({})).await;
    assert_eq!((listed.len(), listed[0]["live"].as_bool(), listed[0]["channels"].clone()), (1, Some(true), json!(["prod", "test"])), "one row, named by both: {listed:?}");

    // Another push on test: prod's page as it was.
    let pushed = call(&ta, "ui.push", push_params(2, &UI_SIGNER)).await;
    assert_eq!(pushed[0]["type"], "pushed", "{pushed:?}");
    let v2 = sha_hex(&support::bundle(2));
    assert!(page(&test, tt.clone()).await.contains("version 2 on"));
    assert!(page(&prod, pt.clone()).await.contains("version 1 on"), "prod stays on v1");
    let live_on = |evs: &Vec<serde_json::Value>| evs.iter().find(|v| v["live"] == true).map(|v| v["hash"].as_str().unwrap().to_string());
    assert_eq!(live_on(&call(&ta, "ui.versions", json!({})).await), Some(v2.clone()), "live on test");
    assert_eq!(live_on(&call(&pa, "ui.versions", json!({})).await), Some(v1.clone()), "live on prod");
    assert_eq!(live_on(&call(&pa, "ui.versions", json!({ "channel": "test" })).await), Some(v2.clone()), "test's, asked of prod");

    // Each channel's history is its own.
    let hows = |evs: &Vec<serde_json::Value>| evs.iter().map(|m| format!("{}:{}", m["channel"].as_str().unwrap(), m["how"].as_str().unwrap())).collect::<Vec<_>>();
    assert_eq!(hows(&call(&pa, "ui.history", json!({})).await), vec!["prod:promote"]);
    assert_eq!(hows(&call(&ta, "ui.history", json!({})).await), vec!["test:push", "test:push"]);
    assert_eq!(hows(&call(&ta, "ui.history", json!({ "channel": "prod" })).await), vec!["prod:promote"], "prod's history, read on test");
    // The deployer pushes into no other channel, and promotes nothing a second time it would move.
    let same = call(&deployer, "ui.promote", json!({ "to": "prod", "from": "test" })).await;
    assert_eq!((same[0]["type"].as_str(), same[0]["version"].as_str()), (Some("promoted"), Some(v2.as_str())), "{same:?}");
    assert!(page(&prod, pt.clone()).await.contains("version 2 on"), "prod on v2 after the second promote");
    let again = call(&deployer, "ui.promote", json!({ "to": "prod", "from": "test" })).await;
    assert!(again[0]["seq"].is_null(), "nothing moved: {again:?}");
    let mut other = push_params(3, &UI_SIGNER);
    other["channel"] = json!("test");
    let refused = call(&deployer, "ui.push", other).await;
    assert_eq!((refused[0]["type"].as_str(), refused[0]["reason"].as_str()), (Some("refused"), Some("forbidden")), "a deployer on prod pushes into test's channel: {refused:?}");
}

/// V2-50 (main's TICKETS-36): the shell answers a link by uid,
/// `/t/<uid>[/…]`, and a path that names a ticket at the root — `/PERF-2`,
/// `/PLEXUS/PERF-2`, its full address — with `302` into the app's route for
/// it (`/app/#/t/<uid>/…`, `/app/#/ticket/<name>`), behind the login as
/// `/app/` is: signed out, the sign-in page first, the path kept as `next`.
/// Every other path answers as before — `/app/`, `/login`, `/ws`, a root
/// file, an unknown path.
#[tokio::test]
async fn a_path_that_names_a_ticket_redirects_into_the_app_behind_the_login() {
    let m = start().await;
    let token = support::mint_session(m.ws).await;
    let at = |r: &reqwest::Response| (r.status().as_u16(), r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string());
    let found = |to: &str| (302, to.to_string());
    let login = |next: &str| (303, format!("/login?next={next}"));

    // Signed in: into the app.
    for (path, to) in [
        ("/PERF-2", "/app/#/ticket/PERF-2"),
        ("/PLEXUS/PERF-2", "/app/#/ticket/PLEXUS/PERF-2"),
        ("/hypermemetic:PLEXUS/PERF-2", "/app/#/ticket/hypermemetic:PLEXUS/PERF-2"),
        ("/t/7k3mq2xa/x", "/app/#/t/7k3mq2xa/x"),
        ("/t/7k3mq2xa", "/app/#/t/7k3mq2xa"),
    ] {
        assert_eq!(at(&m.get(path, Some(&token)).await), found(to), "{path}, signed in");
    }
    // Signed out: the sign-in page first, as for /app/.
    assert_eq!(at(&m.get("/PERF-2", None).await), login("%2FPERF-2"));
    assert_eq!(at(&m.get("/t/7k3mq2xa/x", None).await), login("%2Ft%2F7k3mq2xa%2Fx"));
    assert_eq!(at(&m.get("/app/", None).await), login("%2Fapp%2F"));
    // Unchanged: the app, a path that names no ticket.
    assert_eq!(m.get("/app/", Some(&token)).await.status(), 200);
    for token in [Some(token.as_str()), None] {
        for path in ["/PERF", "/robots.txt", "/t/not-a-uid", "/7k3mq2xa", "/ws"] {
            assert_eq!(at(&m.get(path, token).await), (404, String::new()), "{path}, signed in: {}", token.is_some());
        }
    }
    // A write to a ticket's path is no redirect.
    let posted = m.http.post(m.url("/PERF-2")).header("cookie", m.jar(&token)).send().await.unwrap();
    assert_ne!(posted.status().as_u16(), 302, "only GET and HEAD are sent on");
}
