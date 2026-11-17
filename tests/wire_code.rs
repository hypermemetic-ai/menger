//! CODE-2: `code` over the real protocol — a real TransportServer, a real
//! WebSocket client, the `menger.call` envelope, the service as `main`
//! builds it, its reads of GitHub sent to a local stand-in
//! (`CodeSetup::github_at`, what the hidden `--code-github-at` flag sets):
//! `code.open` → `code.fetch` → `code.tree` → `code.read`, the fetch's
//! bytes counted against the fetcher on `code.bytes`, and a viewer refused
//! `code.fetch` by the gate, typed.

use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use plexus_rpc::core::plexus::DynamicHub;
use plexus_rpc::transport::TransportServer;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod support;

/// An editor on `A`.
const EDITOR: [u8; 32] = [0xc1; 32];
/// A viewer on `A`.
const VIEWER: [u8; 32] = [0xc2; 32];

const REPO: &str = "acme/kiln";

/// The commit `main` is at.
fn commit() -> String {
    format!("9c41d07e{}", "1".repeat(32))
}

fn pkt(payload: &str) -> String {
    format!("{:04x}{payload}", payload.len() + 4)
}

/// The listing GitHub answers: `main` at [`commit`], the default branch.
fn listing() -> Vec<u8> {
    let c = commit();
    let mut t = pkt("# service=git-upload-pack\n") + "0000";
    t += &pkt(&format!("{c} HEAD\0multi_ack symref=HEAD:refs/heads/main agent=git/stand-in\n"));
    t += &pkt(&format!("{c} refs/heads/main\n"));
    (t + "0000").into_bytes()
}

/// The archive GitHub answers for [`commit`]: the pax header naming it,
/// then `kiln-9c41d07/` holding a README and `src/main.rs`.
fn archive() -> Vec<u8> {
    let c = commit();
    let mut tar = tar::Builder::new(Vec::new());
    let body = format!(" comment={c}\n");
    let rec = format!("{}{body}", body.len() + 2);
    let mut g = tar::Header::new_ustar();
    g.set_path("pax_global_header").unwrap();
    g.set_entry_type(tar::EntryType::XGlobalHeader);
    g.set_size(rec.len() as u64);
    g.set_cksum();
    tar.append(&g, rec.as_bytes()).unwrap();
    for (path, bytes) in [("kiln-9c41d07/", None), ("kiln-9c41d07/README.md", Some(&b"# kiln\n"[..])), ("kiln-9c41d07/src/", None), ("kiln-9c41d07/src/main.rs", Some(&b"fn main() {}\n"[..]))] {
        let mut h = tar::Header::new_ustar();
        h.set_mtime(1_790_000_000);
        h.set_mode(0o664);
        match bytes {
            Some(b) => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_size(b.len() as u64);
                tar.append_data(&mut h, path, b).unwrap();
            }
            None => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_size(0);
                tar.append_data(&mut h, path, std::io::empty()).unwrap();
            }
        }
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &tar.into_inner().unwrap()).unwrap();
    gz.finish().unwrap()
}

/// The stand-in for github.com and codeload.github.com (an archive by its
/// full hash or a short one, as codeload answers): every request asked.
async fn stand_in() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let log = log.clone();
            tokio::spawn(async move {
                let (mut buf, mut got) = (vec![0u8; 8192], Vec::new());
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&got).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                log.lock().unwrap().push(head.clone());
                let (status, mime, body) = if path == format!("/{REPO}.git/info/refs?service=git-upload-pack") {
                    (200, "application/x-git-upload-pack-advertisement", listing())
                } else if path.strip_prefix(&format!("/{REPO}/tar.gz/")).is_some_and(|c| c.len() >= 7 && commit().starts_with(c)) {
                    (200, "application/x-gzip", archive())
                } else {
                    (404, "text/plain", b"no".to_vec())
                };
                let out = format!("HTTP/1.1 {status} X\r\ncontent-type: {mime}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len());
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.write_all(&body).await;
            });
        }
    });
    (at, asked)
}

/// A server on a throwaway database, as `main` builds it, GitHub at `github`.
async fn start(github: SocketAddr) -> u16 {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_path = dir.path().join("tickets.db");
    let port = support::free_port().await;
    let login = support::Login::new();
    let access = login.access.clone();
    let backend = Arc::new(menger::auth_backend::MengerAuth::new(access.clone(), Vec::new(), None, "menger"));
    let ui = menger::UiSetup::beside(&db_path).unwrap();
    let media = menger::MediaSetup::beside(&db_path).unwrap();
    let quota = menger::QuotaSetup::beside(&db_path).unwrap();
    let code = menger::CodeSetup::beside(&db_path).unwrap().github_at(Some(github));
    let service = menger::build_rpc_services(&db_path, None, Default::default(), Vec::new(), access, backend, None, menger::store::BODY_MAX_DEFAULT, ui, media, menger::NotesSetup::beside(&db_path).unwrap(), quota, code, menger::LogsSetup::beside(&db_path).unwrap(), Vec::new(), |store, access| {
        menger::TicketsActivation::with_access(store, access)
    })
    .await
    .unwrap();
    let gate = login.gate(port);
    let conv = move |arc: Arc<DynamicHub>| menger::calls::rpc_module(arc, Default::default()).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(service, conv).with_websocket(port).with_session_validator(gate).anonymous_view().build().await.expect("build").serve().await.expect("serve");
    });
    port
}

async fn connect_key(seed: &[u8; 32], port: u16) -> WsClient {
    let url = format!("ws://127.0.0.1:{port}");
    for _ in 0..100 {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", support::proof_of(seed, port))).unwrap());
        if let Ok(c) = WsClientBuilder::default().set_headers(h).build(&url).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never came up on {url}");
}

async fn one(c: &WsClient, method: &str, params: Value) -> Value {
    let mut all = support::call(c, method, params).await;
    assert_eq!(all.len(), 1, "{method}: {all:?}");
    all.remove(0)
}

/// open → fetch → tree → read over the wire, against the stand-in; the
/// fetched bytes on the editor's `code.bytes`; a second open says it is
/// kept and asks GitHub nothing new; a viewer reads, and is refused
/// `code.fetch` by the gate.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repository_is_opened_fetched_and_read_over_the_wire() {
    let (github, asked) = stand_in().await;
    let port = start(github).await;
    let admin = support::connect(port).await;
    let made = one(&admin, "tickets.create", json!({"epic": "A", "title": "a", "kind": "scope"})).await;
    assert_eq!(made["type"], "created", "{made}");
    for (seed, role) in [(&EDITOR, "editor"), (&VIEWER, "viewer")] {
        let g = one(&admin, "auth.grant", json!({ "principal": support::principal_of(seed), "role": role, "scope": "A" })).await;
        assert!(g.get("error_kind").is_none(), "{role}: {g}");
    }
    let (editor, viewer) = (connect_key(&EDITOR, port).await, connect_key(&VIEWER, port).await);
    let c = commit();

    let opened = one(&editor, "code.open", json!({"repo": "Acme/Kiln", "refname": "main"})).await;
    assert_eq!(
        opened,
        json!({"type": "opened", "repo": REPO, "commit": c, "ref": "main", "tip": true, "committed_unix": null, "kept": false, "files": null, "bytes": null})
    );
    // A viewer may not fetch: the gate's refusal, typed.
    let no = one(&viewer, "code.fetch", json!({"repo": REPO, "commit": c})).await;
    assert_eq!((no["type"].as_str(), no["error_kind"].as_str(), no["grant"].as_str()), (Some("error"), Some("forbidden"), Some("code.fetch")), "{no}");
    let fetched = one(&editor, "code.fetch", json!({"repo": REPO, "commit": &c[..7]})).await;
    assert_eq!(fetched, json!({"type": "fetched", "repo": REPO, "commit": c, "files": 2, "bytes": 20, "new_bytes": 20}));
    let usage = one(&editor, "quota.usage", json!({})).await;
    let code = usage["sources"].as_array().unwrap().iter().find(|s| s["source"] == "code.bytes").unwrap_or_else(|| panic!("{usage}"));
    assert_eq!(code["used"], 20, "the fetch counts against the fetcher: {code}");

    let again = one(&viewer, "code.open", json!({"repo": REPO})).await;
    assert_eq!((again["kept"].as_bool(), again["files"].as_u64(), again["committed_unix"].as_i64()), (Some(true), Some(2), Some(1_790_000_000)), "{again}");
    let before = asked.lock().unwrap().len();
    let tree = one(&viewer, "code.tree", json!({"repo": REPO, "commit": c})).await;
    assert_eq!(
        tree,
        json!({"type": "tree", "repo": REPO, "commit": c, "path": "", "truncated": false, "entries": [
            {"path": "README.md", "kind": "file", "bytes": 7},
            {"path": "src", "kind": "dir", "bytes": 0},
            {"path": "src/main.rs", "kind": "file", "bytes": 13},
        ]})
    );
    let read = one(&viewer, "code.read", json!({"repo": REPO, "commit": &c[..8], "file": "/src/main.rs"})).await;
    assert_eq!(read, json!({"type": "file", "path": "src/main.rs", "bytes": 13, "lines": 1, "text": "fn main() {}\n"}));
    assert_eq!(one(&viewer, "code.read", json!({"repo": REPO, "commit": c, "file": "nope"})).await, json!({"type": "not_found", "path": "nope"}));
    assert_eq!(asked.lock().unwrap().len(), before, "tree and read ask GitHub nothing");
    let kept = support::call(&viewer, "code.kept", json!({})).await;
    assert_eq!((kept[0]["type"].as_str(), kept[0]["fetched_by"].as_str()), (Some("kept"), Some(support::principal_of(&EDITOR).as_str())), "{kept:?}");
    assert_eq!(kept[1]["type"], "store");

    // A repository that is not GitHub's owner/name: refused before anything is asked.
    let bad = one(&viewer, "code.open", json!({"repo": "git.example.org/a/b"})).await;
    assert_eq!(bad["type"], "error", "{bad}");
    assert!(bad.to_string().contains("bad_repo"), "{bad}");
    assert_eq!(asked.lock().unwrap().len(), before);
    // Forget is the admin's.
    let no = one(&editor, "code.forget", json!({"repo": REPO, "commit": c})).await;
    assert_eq!(no["error_kind"], "forbidden", "{no}");
    assert_eq!(one(&admin, "code.forget", json!({"repo": REPO, "commit": c})).await, json!({"type": "forgotten", "repo": REPO, "commit": c, "freed": 20}));

    // Nothing that names a caller, a ticket or the tracker went to GitHub.
    for head in asked.lock().unwrap().iter() {
        let lower = head.to_ascii_lowercase();
        assert!(!lower.contains("authorization") && !lower.contains("cookie") && !lower.contains("key:") && lower.contains("user-agent: menger-code/1"), "{head}");
    }
}
