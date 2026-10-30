//! DIST-14: `menger` with no flags just works — on a fresh HOME and free
//! ports it prints a one-time link, the link signs a browser in once (as
//! the local admin, who reaches the hub), a second use is refused, and
//! `menger login` prints a fresh one. Never open mode: a server with a
//! provider serves no `once` link, and beyond loopback with none it refuses
//! to start, naming the flag. And prod is untouched: deploy/ names its
//! database and ports.

use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::WsClientBuilder;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ENV: &[&str] = &[
    "MENGER_DB_PATH",
    "MENGER_DATA_DIR",
    "MENGER_AUTH_PROVIDERS",
    "MENGER_ADMINS",
    "MENGER_IDENTITY",
    "MENGER_BIND",
    "MENGER_PUBLIC_URL",
    "MENGER_ENV",
    "XDG_DATA_HOME",
    "RUST_LOG",
];

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn menger(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_menger"));
    cmd.env("HOME", home).env("PLEXUS_NO_REGISTRY", "1").stdin(Stdio::null());
    for v in ENV {
        cmd.env_remove(v);
    }
    cmd
}

/// The binary with no flags but its ports, on `home`: the server and what
/// it printed up to and including `stop:`.
fn start(home: &Path, ws: u16, shell: u16, extra: &[&str]) -> (Server, Vec<String>) {
    let mut child = menger(home)
        .args(["--port", &ws.to_string(), "--shell-port", &shell.to_string()])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn menger");
    let out = child.stdout.take().unwrap();
    let server = Server(child);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let mut lines = Vec::new();
    while let Ok(l) = rx.recv_timeout(Duration::from_secs(60)) {
        let done = l.trim_start().starts_with("stop:");
        lines.push(l);
        if done {
            return (server, lines);
        }
    }
    panic!("menger printed no banner: {lines:?}");
}

/// `GET path` on the shell, with `cookie`: the whole answer.
async fn get(port: u16, path: &str, cookie: Option<&str>) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("the shell accepts");
    let c = cookie.map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n{c}\r\n").as_bytes()).await.unwrap();
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).await.unwrap();
    String::from_utf8_lossy(&resp).into_owned()
}

/// An answer's status code.
fn status(resp: &str) -> u16 {
    resp.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
}

fn path_of(link: &str) -> &str {
    &link[link.find("/login").expect("a /login link")..]
}

/// The session cookie an answer sets, as `name=value`.
fn session_cookie(resp: &str) -> Option<String> {
    resp.lines()
        .filter_map(|l| l.strip_prefix("set-cookie: ").or_else(|| l.strip_prefix("Set-Cookie: ")))
        .find(|c| c.starts_with("menger_session"))
        .map(|c| c.split(';').next().unwrap().to_string())
}

/// `tickets.health` over the socket carrying `cookie`, as the app's does.
async fn health(ws: u16, cookie: &str) -> Result<Value, String> {
    let mut h = jsonrpsee::ws_client::HeaderMap::new();
    h.insert("cookie", cookie.parse().unwrap());
    let client = WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{ws}")).await.map_err(|e| e.to_string())?;
    let mut p = ObjectParams::new();
    p.insert("method", "tickets.health").unwrap();
    p.insert("params", serde_json::json!({})).unwrap();
    let mut sub = client.subscribe::<Value, _>("menger.call", p, "menger.call_unsub").await.map_err(|e| e.to_string())?;
    let item = sub.next().await.expect("an item").map_err(|e| e.to_string())?;
    match item["type"].as_str() {
        Some("data") => Ok(item["content"].clone()),
        _ => Err(item.to_string()),
    }
}

#[tokio::test]
async fn a_first_run_prints_a_link_that_signs_in_once_and_menger_login_prints_another() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = dir.path();
    let (ws, shell) = (free_port(), free_port());
    let (_server, lines) = start(home, ws, shell, &[]);

    // What it prints, in this order.
    assert!(lines[0].starts_with("menger ") && lines[0].ends_with("— running"), "{lines:?}");
    let open = lines[1].trim_start();
    assert!(open.starts_with(&format!("open:  http://127.0.0.1:{shell}/login?once=")), "{lines:?}");
    assert!(open.contains("`menger login` for another"), "{open}");
    // DIST-17: under open, how an agent signs in.
    assert_eq!(lines[2].trim_start(), format!("agents: synapse -P {ws} _login menger --key menger-local --as menger-local"));
    assert_eq!(lines[3].trim_start(), "data:  ~/.local/share/menger/local.db");
    assert_eq!(lines[4].trim_start(), "stop:  Ctrl-C");
    let link = open.split_whitespace().nth(1).unwrap().to_string();

    // Its own data directory, the key 0600, never prod's tickets.db.
    let data = home.join(".local/share/menger");
    assert!(data.join("local.db").exists() && !data.join("tickets.db").exists());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(data.join("local.key")).unwrap().permissions().mode() & 0o777, 0o600);

    // Nobody is signed in before the link: the page says how to get one.
    let page = get(shell, "/login", None).await;
    assert!(page.contains("menger login"), "{page}");
    assert!(status(&get(shell, "/login/me", None).await) == 401);

    // The link signs the browser in, once.
    let first = get(shell, path_of(&link), None).await;
    assert!(status(&first) == 303, "{first}");
    let cookie = session_cookie(&first).expect("a session cookie");
    let me = get(shell, "/login/me", Some(&cookie)).await;
    assert!(status(&me) == 200 && me.contains("\"principal\":\"key:"), "{me}");
    // As the local admin, who reaches the hub.
    let h = health(ws, &cookie).await.expect("the session reaches the hub");
    assert_eq!(h["type"], "health", "{h}");
    // A second use is refused, and says how to get another.
    let again = get(shell, path_of(&link), None).await;
    assert!(status(&again) == 403, "{again}");
    assert!(session_cookie(&again).is_none());
    assert!(again.contains("already used or has expired") && again.contains("menger login"), "{again}");

    // `menger login` prints a fresh one, which works.
    let out = menger(home).arg("login").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let fresh = String::from_utf8(out.stdout).unwrap();
    let fresh = fresh.lines().next().unwrap();
    assert!(fresh.starts_with(&format!("http://127.0.0.1:{shell}/login?once=")) && fresh != link, "{fresh}");
    let second = get(shell, path_of(fresh), None).await;
    assert!(status(&second) == 303 && session_cookie(&second).is_some(), "{second}");
}

/// FRAME-21: the no-flags start keeps its sessions in its data directory —
/// `sessions.jsonl` beside `local.db`, the service's own (0600) — and the
/// cookie the link set is not in it.
#[tokio::test]
async fn a_first_run_keeps_its_sessions_in_its_data_dir() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = dir.path();
    let (ws, shell) = (free_port(), free_port());
    let (_server, lines) = start(home, ws, shell, &[]);
    let link = lines[1].trim_start().split_whitespace().nth(1).unwrap().to_string();
    let first = get(shell, path_of(&link), None).await;
    let cookie = session_cookie(&first).expect("a session cookie");
    let file = home.join(".local/share/menger/sessions.jsonl");
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    assert_eq!(meta.permissions().mode() & 0o777, 0o600, "the service's own file");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.is_empty(), "the sign-in is kept");
    assert!(!text.contains(cookie.split_once('=').unwrap().1), "the cookie itself is never at rest");
}

/// DIST-17: a local start links its key for synapse, says how to sign in,
/// `menger login --agent` says it again, an existing file is never
/// overwritten, and a provider's server links nothing and prints no line.
#[tokio::test]
async fn a_local_start_hands_its_key_to_synapse_and_never_overwrites() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = dir.path();
    let (ws, shell) = (free_port(), free_port());
    let line = format!("synapse -P {ws} _login menger --key menger-local --as menger-local");
    let link = home.join(".plexus/keys/menger-local.key");
    {
        let (_server, lines) = start(home, ws, shell, &[]);
        assert!(lines.iter().any(|l| l.trim_start() == format!("agents: {line}")), "{lines:?}");
        let key = home.join(".local/share/menger/local.key");
        assert_eq!(std::fs::read_link(&link).unwrap().canonicalize().unwrap(), key.canonicalize().unwrap(), "a link to the key, not a copy");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        let out = menger(home).args(["login", "--agent"]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), line);
    }
    // A second start changes nothing and says the same.
    let (ws2, shell2) = (free_port(), free_port());
    let target = std::fs::read_link(&link).unwrap();
    {
        let (_server, lines) = start(home, ws2, shell2, &[]);
        assert!(lines.iter().any(|l| l.contains(&format!("synapse -P {ws2} _login menger"))), "{lines:?}");
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
    }
    // Something else already there: left alone, said, no line.
    std::fs::remove_file(&link).unwrap();
    std::fs::write(&link, "someone else's").unwrap();
    {
        let (_server, lines) = start(home, free_port(), free_port(), &[]);
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "someone else's");
        assert!(lines.iter().any(|l| l.contains("is already there and is not this key")), "{lines:?}");
        assert!(!lines.iter().any(|l| l.contains("agents:")), "{lines:?}");
    }
    // A provider's server: no link, no line.
    let other = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let (_server, lines) = start(other.path(), free_port(), free_port(), &["--auth-provider", "key", "--admin", &format!("key:{}", "ab".repeat(32))]);
    assert!(!lines.iter().any(|l| l.contains("agents:")), "{lines:?}");
    assert!(!other.path().join(".plexus").exists());
}

#[tokio::test]
async fn menger_login_with_nothing_running_says_how_to_start_it() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let out = menger(dir.path()).arg("login").output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("isn't running") && err.contains("Start it with `menger`"), "{err}");
}

#[tokio::test]
async fn with_a_provider_no_once_link_is_served_and_beyond_loopback_with_none_it_refuses() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = dir.path();
    let (ws, shell) = (free_port(), free_port());
    let (_server, lines) = start(home, ws, shell, &["--auth-provider", "key", "--admin", &format!("key:{}", "ab".repeat(32))]);
    assert!(!lines[1].contains("once="), "a provider's server prints its address: {lines:?}");
    assert!(lines[1].trim_start().starts_with(&format!("open:  http://127.0.0.1:{shell}/")), "{lines:?}");
    // A token minted where the links would be names nothing here.
    let data = home.join(".local/share/menger");
    assert!(!data.join("local.key").exists(), "no local admin beside a provider");
    let link = menger_login_token(&data);
    let resp = get(shell, &format!("/login?once={link}"), None).await;
    assert!(session_cookie(&resp).is_none(), "{resp}");
    assert!(status(&resp) == 200, "{resp}");

    // Beyond loopback with no provider: refused, naming the flag.
    for extra in [&["--bind", "0.0.0.0"][..], &["--public-url", "https://tickets.example"][..]] {
        let out = menger(home).args(["--port", &free_port().to_string(), "--shell-port", &free_port().to_string()]).args(extra).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{extra:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(extra[0]) && err.contains("--auth-provider"), "{err}");
        assert!(!["ACCESS-", "DIST-"].iter().any(|id| err.contains(id)), "no ticket ids: {err}");
    }
}

/// A token in the shape the links take, written where they are kept.
fn menger_login_token(data: &Path) -> String {
    let once = menger::first_run::OnceLinks::new(data, "key:x".into());
    once.mint().unwrap()
}

/// Prod is untouched: every environment names its database (never the
/// default) and its ports, and the launcher passes the ports — so a change
/// of the default database or a port that falls back never moves prod.
#[test]
fn deploy_names_its_database_and_ports() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for env in ["deploy/env/prod.env", "deploy/env/test.env", "deploy/image/Dockerfile"] {
        let text = std::fs::read_to_string(root.join(env)).unwrap();
        let has = |k: &str| text.lines().any(|l| l.trim_start().starts_with(&format!("{k}=")));
        assert!(has("MENGER_DB_PATH") && has("MENGER_WS_PORT") && has("MENGER_SHELL_PORT"), "{env} names its database and ports");
        assert!(!text.contains("local.db"), "{env}");
    }
    let launch = std::fs::read_to_string(root.join("deploy/launch/run-menger")).unwrap();
    assert!(launch.contains("--port \"$MENGER_WS_PORT\"") && launch.contains("--shell-port \"$MENGER_SHELL_PORT\""), "{launch}");
}
