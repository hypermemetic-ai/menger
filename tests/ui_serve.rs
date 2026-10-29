//! UI-3: `/app/*` from the ui store, over HTTP — a spawned `menger` with its
//! shell, under the fixture operator, its own ui file and the test signer.
//! The app's routes are behind a session (IDENTITY-19, unchanged): each
//! request carries the shell's session cookie, a session minted on the
//! socket (`support::mint_session`).
//!
//! Each server on ports the OS hands out.

mod support;

use serde_json::json;
use support::{call, connect, free_port, push_params, sha_hex, spawn_menger, ui_args, UI_SIGNER};

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
    let (ws, shell) = (free_port().await, free_port().await);
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let empty = dir.path().join("no-dist");
    std::fs::create_dir_all(&empty).unwrap();
    let mut extra = vec!["--shell-port".to_string(), shell.to_string(), "--app-dist".into(), empty.display().to_string()];
    extra.extend(ui_args(&dir.path().join("ui.db")));
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
