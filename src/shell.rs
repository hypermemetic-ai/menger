//! Day-one web shell.
//!
//! menger's rule (since 0.04): frontend and backend are online from the
//! first commit, so every noun is visible the day it is written. This
//! serves one embedded page that speaks the Plexus wire protocol directly
//! over WebSocket — no polling, no build step. The page is the seed the
//! reactive framework grows from (MNG-5/MNG-6).

use axum::response::Html;
use axum::routing::get;
use axum::Router;

pub async fn serve(port: u16, ws_port: u16) -> anyhow::Result<()> {
    let html = include_str!("../web/index.html").replace("{{WS_PORT}}", &ws_port.to_string());
    let app = Router::new().route("/", get(move || std::future::ready(Html(html.clone()))));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("Shell:     http://127.0.0.1:{port}/");
    axum::serve(listener, app).await?;
    Ok(())
}
