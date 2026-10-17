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

/// A session minted for [`ADMIN`] on the socket at `ws_port`
/// (`session.login`): its token — a bearer, the `access_token` cookie, or
/// the shell's session cookie.
pub async fn mint_session(ws_port: u16) -> String {
    use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::ObjectParams;
    let c = connect(ws_port).await;
    let mut p = ObjectParams::new();
    p.insert("method", "session.login").unwrap();
    p.insert("params", serde_json::json!({})).unwrap();
    let mut s: Subscription<serde_json::Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let item = s.next().await.expect("an answer").unwrap();
    item["content"]["token"].as_str().unwrap_or_else(|| panic!("session.login: {item}")).to_string()
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
