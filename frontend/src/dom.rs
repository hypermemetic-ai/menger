//! The two browser primitives every module reaches for.
//!
//! MFE-9 deleted `net.rs` — the app's REST client and its hand-parsed SSE
//! reader — and the data plane moved to `session.rs` (one `WsTransport`,
//! the generated Plexus client). These two helpers were the only things in
//! `net.rs` that were never about the network: the `Window` handle and
//! `setTimeout`-as-a-future.

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

pub(crate) fn window() -> web_sys::Window {
    web_sys::window().expect("no window — this app is browser-only (CSR)")
}

/// setTimeout as a future — the reconnect backoff, and the "after the next
/// paint" hop several views use.
pub async fn sleep_ms(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        window()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("setTimeout");
    });
    let _ = JsFuture::from(p).await;
}

/// The Plexus WebSocket endpoint this page's server speaks on.
///
/// menger's shell substitutes the port into `<meta name="menger-ws-port">`
/// when it serves the app (src/shell.rs). Anything else serving the file
/// leaves the placeholder in place, and the default WS port is the honest
/// guess — a wrong one surfaces as `reconnecting…` plus the connect error
/// in the banner, not as silence.
pub(crate) fn ws_url() -> String {
    const DEFAULT_WS_PORT: &str = "4455";
    let loc = window().location();
    let host = loc.hostname().unwrap_or_else(|_| "127.0.0.1".into());
    let scheme = match loc.protocol().as_deref() {
        Ok("https:") => "wss",
        _ => "ws",
    };
    let port = window()
        .document()
        .and_then(|d| d.query_selector("meta[name=\"menger-ws-port\"]").ok().flatten())
        .and_then(|el| el.dyn_into::<web_sys::HtmlMetaElement>().ok())
        .map(|m| m.content())
        .filter(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty())
        .unwrap_or_else(|| DEFAULT_WS_PORT.to_string());
    format!("{scheme}://{host}:{port}")
}
