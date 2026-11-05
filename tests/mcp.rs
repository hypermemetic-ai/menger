//! TRANSPORT-15, V2-43: menger serves `/mcp` through the mounted bridge,
//! behind the login — the request's Bearer (a session, as synapse's `_login`
//! keeps) names the caller to the gate; no credential is the anonymous view;
//! a stranger's key sees no write it holds no grant for. v2 (V2-43): every
//! service v2 registers is there, and with an authorization server (TRANSPORT-24,
//! the environment's AuthKit) `/mcp` is a protected resource.

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use plexus_core::plexus::{DynamicHub, SessionValidator};
use plexus_transport::plexus_transport_mcp::{router, McpBridge, OAuthConfig};
use plexus_transport::{McpConfig, ServiceUpstream, TransportServer};
use rmcp::model::CallToolRequestParams;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{json, Value};

/// A key with no grants: admitted by the `key` provider, a signed-in nobody.
const STRANGER: [u8; 32] = [0x11; 32];

/// The authorization server a protected `/mcp` names (app-prod.env's).
const AUTHKIT: &str = "https://manageable-editor-54.authkit.app";

/// menger on a free WebSocket port, and `/mcp` as `main` mounts it on the
/// shell: the bridge over the service and the login's gate, served here on
/// a port of its own (the shell's other routes are not what is under test).
/// `oauth`: the authorization servers that make it a protected resource.
async fn start_with(oauth: &[&str]) -> (u16, SocketAddr) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let login = support::Login::new();
    let service = support::service(&dir.path().join("t.db"), &login).await;
    let port = support::free_port().await;
    let gate = login.gate(port);
    let validator: Arc<dyn SessionValidator> = login.gate(port);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mcp = listener.local_addr().unwrap();
    let mut config = McpConfig::at("/mcp");
    if !oauth.is_empty() {
        config.oauth = Some(OAuthConfig { resource: format!("http://{mcp}/mcp"), authorization_servers: oauth.iter().map(|s| s.to_string()).collect(), scopes: None });
    }
    let bridge = McpBridge::new(ServiceUpstream::new(service.clone(), Some(validator)), config);
    tokio::spawn(async move {
        axum::serve(listener, router(bridge)).await.unwrap();
    });
    let conv = |arc: Arc<DynamicHub>| DynamicHub::arc_into_rpc_module(arc).map_err(|e| anyhow::anyhow!("{e}"));
    tokio::spawn(async move {
        let _keep = dir;
        TransportServer::builder(service, conv)
            .with_websocket(port)
            .with_session_validator(gate)
            .anonymous_view()
            .credential_cookies(["access_token"])
            .without_registry()
            .build()
            .await
            .unwrap()
            .serve()
            .await
            .unwrap();
    });
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(mcp).await.is_ok() && tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return (port, mcp);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the servers never came up");
}

async fn start() -> (u16, SocketAddr) {
    start_with(&[]).await
}

type Client = rmcp::service::RunningService<rmcp::RoleClient, ()>;

/// An MCP client at `mcp`, its Bearer the session minted for the key `seed`
/// on the WebSocket at `ws_port` — what synapse's `_login` keeps — or none.
/// A key proof itself is one upgrade's credential: MCP carries the header on
/// every request, and a replayed proof is refused (see `a_key_proof_is_one_request_s`).
async fn client(mcp: SocketAddr, seed: Option<&[u8; 32]>, ws_port: u16) -> Client {
    let mut config = StreamableHttpClientTransportConfig::with_uri(format!("http://{mcp}/mcp"));
    if let Some(seed) = seed {
        config = config.auth_header(support::mint_session_for(seed, ws_port).await);
    }
    ().serve(StreamableHttpClientTransport::with_client(reqwest13::Client::default(), config)).await.unwrap_or_else(|e| panic!("initialize: {e}"))
}

async fn names(c: &Client) -> Vec<String> {
    let mut names: Vec<String> = c.peer().list_all_tools().await.unwrap().into_iter().map(|t| t.name.to_string()).collect();
    names.sort();
    names
}

async fn call(c: &Client, name: &str, args: Value) -> rmcp::model::CallToolResult {
    let params = CallToolRequestParams::new(name.to_string()).with_arguments(args.as_object().cloned().unwrap_or_default());
    c.peer().call_tool(params).await.expect("the call is answered")
}

fn text(r: &rmcp::model::CallToolResult) -> String {
    r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect::<Vec<_>>().join("")
}

/// An `initialize` posted by hand, with `bearer` as the credential if any.
async fn initialize(mcp: SocketAddr, bearer: Option<&str>) -> reqwest13::Response {
    let mut r = reqwest13::Client::default()
        .post(format!("http://{mcp}/mcp"))
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json");
    if let Some(b) = bearer {
        r = r.bearer_auth(b);
    }
    r.body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#)
        .send()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_operator_lists_the_tickets_tools_creates_and_reads_a_ticket() {
    let (ws, mcp) = start().await;
    let admin = client(mcp, Some(&support::ADMIN), ws).await;
    let tools = names(&admin).await;
    for expected in ["tickets.get", "tickets.list", "tickets.create", "tickets.set_status", "tickets.health"] {
        assert!(tools.contains(&expected.to_string()), "{expected} among {tools:?}");
    }
    let created = call(&admin, "tickets.create", json!({"epic": "MCP", "title": "over MCP", "kind": "scope"})).await;
    assert_ne!(created.is_error, Some(true), "{}", text(&created));
    let id = created.structured_content.as_ref().and_then(|s| s["items"][0]["ticket"]["id"].as_str()).map(str::to_string).expect("the created ticket's id");
    assert_eq!(id, "MCP-1");
    let got = call(&admin, "tickets.get", json!({"id": "MCP-1"})).await;
    assert_ne!(got.is_error, Some(true), "{}", text(&got));
    assert!(text(&got).contains("over MCP"), "{}", text(&got));
}

/// V2-43: `/mcp` is v2's whole surface — every service it registers, not
/// the tickets service alone.
#[tokio::test(flavor = "multi_thread")]
async fn the_operator_sees_every_v2_service() {
    let (ws, mcp) = start().await;
    let admin = client(mcp, Some(&support::ADMIN), ws).await;
    let tools = names(&admin).await;
    for service in ["tickets", "members", "orgs", "ui", "media", "docs", "quota", "auth"] {
        assert!(tools.iter().any(|t| t.starts_with(&format!("{service}."))), "{service}.* among {tools:?}");
    }
    let versions = call(&admin, "ui.versions", json!({})).await;
    assert_ne!(versions.is_error, Some(true), "{}", text(&versions));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stranger_s_key_sees_no_write_and_cannot_call_one() {
    let (ws, mcp) = start().await;
    let stranger = client(mcp, Some(&STRANGER), ws).await;
    let tools = names(&stranger).await;
    assert!(!tools.contains(&"tickets.set_status".to_string()), "a write it holds no grant for: {tools:?}");
    assert!(!tools.contains(&"tickets.create".to_string()), "{tools:?}");
    let refused = stranger
        .peer()
        .call_tool(CallToolRequestParams::new("tickets.set_status").with_arguments(json!({"id": "MCP-1", "status": "Ready"}).as_object().cloned().unwrap()))
        .await;
    assert!(refused.is_err(), "the write is refused");
}

#[tokio::test(flavor = "multi_thread")]
async fn nobody_sees_the_door_only() {
    let (ws, mcp) = start().await;
    let nobody = client(mcp, None, ws).await;
    let tools = names(&nobody).await;
    assert!(tools.contains(&"auth.login".to_string()), "{tools:?}");
    assert!(tools.iter().all(|t| t.starts_with("auth.") || t == "session.providers"), "the door only: {tools:?}");
}

/// A key proof opens one upgrade; presented on every MCP request it is
/// refused the second time, so the session is the Bearer over HTTP.
#[tokio::test(flavor = "multi_thread")]
async fn a_key_proof_is_one_request_s() {
    let (ws, mcp) = start().await;
    let config = StreamableHttpClientTransportConfig::with_uri(format!("http://{mcp}/mcp")).auth_header(support::proof_of(&support::ADMIN, ws));
    let attempt = ().serve(StreamableHttpClientTransport::with_client(reqwest13::Client::default(), config)).await;
    assert!(attempt.is_err(), "the proof is spent by initialize; the initialized notification is refused");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_credential_is_a_401() {
    let (_ws, mcp) = start().await;
    let r = initialize(mcp, Some("plexus-key.not-a-proof")).await;
    assert_eq!(r.status(), 401);
    assert_eq!(r.headers()["www-authenticate"].to_str().unwrap(), "Bearer realm=\"plexus\", error=\"invalid_token\"");
}

/// TRANSPORT-24, V2-43: with an authorization server (the environment's
/// AuthKit), no token is a 401 naming RFC 9728's document, the document
/// names that server, and a forged token is a 401 too.
#[tokio::test(flavor = "multi_thread")]
async fn a_protected_mcp_sends_a_tokenless_client_to_authkit() {
    let (_ws, mcp) = start_with(&[AUTHKIT]).await;
    let r = initialize(mcp, None).await;
    assert_eq!(r.status(), 401);
    let challenge = r.headers()["www-authenticate"].to_str().unwrap().to_string();
    let doc_url = format!("http://{mcp}/.well-known/oauth-protected-resource/mcp");
    assert_eq!(challenge, format!("Bearer resource_metadata=\"{doc_url}\""));
    let doc: Value = serde_json::from_str(&reqwest13::Client::default().get(&doc_url).send().await.unwrap().text().await.unwrap()).unwrap();
    assert_eq!(doc["resource"], json!(format!("http://{mcp}/mcp")), "{doc}");
    assert_eq!(doc["authorization_servers"], json!([AUTHKIT]), "{doc}");
    let forged = initialize(mcp, Some("eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ4In0.bm90LWEtc2lnbmF0dXJl")).await;
    assert_eq!(forged.status(), 401);
    assert!(forged.headers()["www-authenticate"].to_str().unwrap().contains("error=\"invalid_token\""));
}
