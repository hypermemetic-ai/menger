//! TRANSPORT-15: menger serves `/mcp` through the mounted bridge, behind the
//! login — the request's Bearer (a key proof, as synapse signs in) names
//! the caller to the gate; no credential is the anonymous view; a stranger's
//! key sees no write it holds no grant for.

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use plexus_core::plexus::{DynamicHub, SessionValidator};
use plexus_transport::plexus_transport_mcp::{router, McpBridge};
use plexus_transport::{McpConfig, ServiceUpstream, TransportServer};
use rmcp::model::CallToolRequestParams;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{json, Value};

/// A key with no grants: admitted by the `key` provider, a signed-in nobody.
const STRANGER: [u8; 32] = [0x11; 32];

async fn free_port() -> u16 {
    tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port()
}

/// A session minted for the key `seed` on the socket at `ws_port`
/// (`session.login`, as `support::mint_session` does for the operator):
/// its token, what synapse's `_login` keeps.
async fn mint_session_for(seed: &[u8; 32], ws_port: u16) -> String {
    use jsonrpsee::core::client::{Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::ObjectParams;
    use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClientBuilder};
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", support::proof_of(seed, ws_port))).unwrap());
    let c = WsClientBuilder::default().set_headers(h).build(format!("ws://127.0.0.1:{ws_port}")).await.expect("the key is admitted");
    let mut p = ObjectParams::new();
    p.insert("method", "session.login").unwrap();
    p.insert("params", json!({})).unwrap();
    let mut s: Subscription<Value> = c.subscribe("menger.call", p, "menger.call_unsub").await.unwrap();
    let item = s.next().await.expect("an answer").unwrap();
    item["content"]["token"].as_str().unwrap_or_else(|| panic!("session.login: {item}")).to_string()
}

/// menger on a free WebSocket port, and `/mcp` as `main` mounts it on the
/// shell: the bridge over the service and the login's gate, served here on
/// a port of its own (the shell's other routes are not what is under test).
async fn start() -> (u16, SocketAddr) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let login = support::Login::new();
    let service = support::hub(&dir.path().join("t.db"), &login).await;
    let port = free_port().await;
    let gate = login.gate(port);
    let validator: Arc<dyn SessionValidator> = login.gate(port);
    let bridge = McpBridge::new(ServiceUpstream::new(service.clone(), Some(validator)), McpConfig::at("/mcp"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mcp = listener.local_addr().unwrap();
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

type Client = rmcp::service::RunningService<rmcp::RoleClient, ()>;

/// An MCP client at `mcp`, its Bearer the session minted for the key `seed`
/// on the WebSocket at `ws_port` — what synapse's `_login` keeps — or none.
/// A key proof itself is one upgrade's credential: MCP carries the header on
/// every request, and a replayed proof is refused (see `a_key_proof_is_one_request_s`).
async fn client(mcp: SocketAddr, seed: Option<&[u8; 32]>, ws_port: u16) -> Client {
    let mut config = StreamableHttpClientTransportConfig::with_uri(format!("http://{mcp}/mcp"));
    if let Some(seed) = seed {
        config = config.auth_header(mint_session_for(seed, ws_port).await);
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
    let r = reqwest13::Client::default()
        .post(format!("http://{mcp}/mcp"))
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .bearer_auth("plexus-key.not-a-proof")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(r.headers()["www-authenticate"].to_str().unwrap(), "Bearer realm=\"plexus\", error=\"invalid_token\"");
}
