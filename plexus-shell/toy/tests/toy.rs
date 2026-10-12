//! SCAFFOLD-5: a tiny hub and an embedded two-file app, through
//! plexus-shell's LocalServer on free ports — `/` is the stamped page, the
//! app's asset is served byte-exact, the WebSocket answers a call, and a
//! shutdown releases both ports.

use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::core::params::ObjectParams;
use jsonrpsee::ws_client::WsClientBuilder;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn get(port: u16, path: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("the shell accepts");
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes()).await.unwrap();
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).await.unwrap();
    String::from_utf8_lossy(&resp).into_owned()
}

#[tokio::test]
async fn the_toy_serves_its_embedded_app_and_its_hub() {
    let keys: Vec<_> = plexus_shell_toy::APP.iter().map(|f| f.0).collect();
    assert_eq!(keys, ["app.js", "index.html"], "build.rs embedded app/");
    let server = plexus_shell_toy::start().await.expect("start");
    let (ws, shell) = (server.ws_port(), server.shell_port());
    assert!(ws != 0 && shell != 0 && ws != shell);

    let page = get(shell, "/").await;
    assert!(page.starts_with("HTTP/1.0 200") || page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("<title>toy</title>"), "{page}");
    assert!(page.contains(&format!("content=\"{ws}\"")), "stamped with the socket's port: {page}");
    assert!(page.contains("<body>hello from plexus-shell</body>"), "an app's own placeholder: {page}");
    assert!(page.to_ascii_lowercase().contains("x-frame-options: deny"), "hardened: {page}");
    let js = get(shell, "/app/app.js").await;
    assert!(js.contains("text/javascript") && js.contains("toy.call"), "{js}");
    assert!(js.contains("{text: \"hi\"}") || js.contains("text: \"hi\""), "byte-exact, not stamped: {js}");
    assert!(get(shell, "/app/../Cargo.toml").await.contains("404"));
    assert!(get(shell, "/app/nothing.js").await.contains("404"));

    let client = WsClientBuilder::default().build(server.ws_url()).await.expect("the socket");
    let mut p = ObjectParams::new();
    p.insert("method", "echo.say").unwrap();
    p.insert("params", serde_json::json!({ "text": "hi" })).unwrap();
    let mut sub = client.subscribe::<Value, _>("toy.call", p, "toy.call_unsub").await.expect("subscribe");
    let item = sub.next().await.expect("an item").expect("ok");
    assert_eq!(item["type"], "data", "{item}");
    assert_eq!(item["content"], "echo: hi", "{item}");

    server.shutdown().await;
    for port in [ws, shell] {
        tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap_or_else(|e| panic!("port {port} released: {e}"));
    }
}
