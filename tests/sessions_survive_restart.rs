//! FRAME-21: a restart signs no one out. A browser signed in (the one-time
//! link's cookie, `menger_session_<port>`) and an agent's session token
//! (`auth.login {flow: {session}}`, as `session login` mints) still stand
//! after the service is killed and started again on the same data directory
//! and ports — the cookie admitted at `/` and on the socket, the token
//! answering `session.whoami` — because the login's session table is kept
//! beside the database, named for it (`local.db` → `local.sessions.jsonl`;
//! 0600, each id's sha256 only).

mod support;

use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use std::path::Path;
use support::{admin, call, free_port, mint_session, spawn_local};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// `GET path` on the shell, with `cookie`: the whole answer.
async fn get(port: u16, path: &str, cookie: Option<&str>) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("the shell accepts");
    let c = cookie.map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n{c}\r\n").as_bytes()).await.unwrap();
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).await.unwrap();
    String::from_utf8_lossy(&resp).into_owned()
}

fn status(resp: &str) -> u16 {
    resp.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
}

fn location(resp: &str) -> Option<&str> {
    resp.lines().find_map(|l| l.strip_prefix("location: ").or_else(|| l.strip_prefix("Location: "))).map(str::trim)
}

/// The session cookie an answer sets, as `name=value`.
fn session_cookie(resp: &str) -> Option<String> {
    resp.lines()
        .filter_map(|l| l.strip_prefix("set-cookie: ").or_else(|| l.strip_prefix("Set-Cookie: ")))
        .find(|c| c.starts_with("menger_session"))
        .map(|c| c.split(';').next().unwrap().to_string())
}

/// The one-time link in a local start's banner, as a path on the shell.
fn once_path(banner: &[String], shell: u16) -> String {
    let open = banner.iter().map(|l| l.trim_start()).find(|l| l.starts_with("open:")).unwrap_or_else(|| panic!("no open: line in {banner:?}"));
    let link = open.split_whitespace().nth(1).unwrap();
    assert!(link.starts_with(&format!("http://127.0.0.1:{shell}/login?once=")), "the shell on its own port again: {banner:?}");
    link[link.find("/login").unwrap()..].to_string()
}

/// A socket opened with `headers`, or why not (a refused upgrade: 401).
async fn socket(ws: u16, name: &'static str, value: &str) -> Result<WsClient, String> {
    let mut h = HeaderMap::new();
    h.insert(name, HeaderValue::from_str(value).unwrap());
    WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{ws}")).await.map_err(|e| e.to_string())
}

/// What the browser's cookie and the agent's token each still reach.
async fn both_stand(ws: u16, shell: u16, cookie: &str, local: &str, token: &str, when: &str) {
    let page = get(shell, "/", Some(cookie)).await;
    assert!(
        !(status(&page) == 303 && location(&page).is_some_and(|l| l.starts_with("/login"))),
        "{when}: the browser's cookie is admitted at / — not sent to the login page:\n{page}"
    );
    let me = get(shell, "/login/me", Some(cookie)).await;
    assert!(status(&me) == 200 && me.contains(&format!("\"principal\":\"{local}\"")), "{when}: /login/me with the cookie is the local admin {local}:\n{me}");
    // On the socket the cookie is the local admin too — not the anonymous
    // view, which `tickets.health` alone would not tell apart.
    let c = socket(ws, "cookie", cookie).await.unwrap_or_else(|e| panic!("{when}: the cookie opens the socket: {e}"));
    let who = call(&c, "session.whoami", serde_json::json!({})).await;
    let first = who.first().unwrap_or_else(|| panic!("{when}: session.whoami on the cookie answered nothing"));
    assert_eq!((first["type"].as_str(), first["principal"].as_str()), (Some("identity"), Some(local)), "{when}: session.whoami on the cookie: {first}");
    let c = socket(ws, "authorization", &format!("Bearer {token}")).await.unwrap_or_else(|e| panic!("{when}: the agent's token opens the socket: {e}"));
    let who = call(&c, "session.whoami", serde_json::json!({})).await;
    let first = who.first().unwrap_or_else(|| panic!("{when}: session.whoami answered nothing"));
    assert_eq!((first["type"].as_str(), first["principal"].as_str()), (Some("identity"), Some(admin().as_str())), "{when}: session.whoami on the token: {first}");
}

#[tokio::test]
async fn a_browser_and_an_agent_stay_signed_in_across_a_restart() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let (home, data) = (dir.path().join("home"), dir.path().join("data"));
    std::fs::create_dir_all(&home).unwrap();
    let (ws, shell) = (free_port().await, free_port().await);
    // The local start, the fixture key an admin beside the local one.
    let extra = ["--admin".to_string(), admin()];
    // The local admin: the key the first start makes in the data directory.
    let local = || menger::first_run::local_admin(&data).expect("the local admin's key");

    let (cookie, local, token) = {
        let (_server, banner) = spawn_local(&home, &data, ws, shell, &extra);
        let first = get(shell, &once_path(&banner, shell), None).await;
        assert_eq!(status(&first), 303, "{first}");
        let cookie = session_cookie(&first).expect("the link signs the browser in");
        assert!(cookie.starts_with(&format!("menger_session_{shell}=")), "{cookie}");
        let local = local();
        let token = mint_session(ws).await;
        assert!(token.starts_with("mgs_"), "{token}");
        both_stand(ws, shell, &cookie, &local, &token, "before the restart").await;
        (cookie, local, token)
        // Dropped: SIGKILL — no shutdown hook runs, so what was minted was
        // kept before it was answered.
    };

    let (_server, banner) = spawn_local(&home, &data, ws, shell, &extra);
    let _ = once_path(&banner, shell);
    both_stand(ws, shell, &cookie, &local, &token, "after the restart").await;

    // Where it was kept: the service's own file, beside the database, which
    // a read of opens nothing.
    sessions_file_holds_no_credential(&data.join("local.sessions.jsonl"), &[cookie.split_once('=').unwrap().1, &token]);
}

/// `path` is 0600 and holds none of `ids`.
fn sessions_file_holds_no_credential(path: &Path, ids: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(meta.permissions().mode() & 0o777, 0o600, "{}", path.display());
    let text = std::fs::read_to_string(path).unwrap();
    for id in ids {
        assert!(!text.contains(id), "{} holds a credential", path.display());
    }
}
