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
/// ACCESS-30: a page served over https is behind a proxy that terminates
/// TLS for one origin, so the socket is on that origin too — `wss://<the
/// page's host>/ws`, which the proxy routes to the WebSocket port (README
/// "Behind https"). The app never builds `wss://host:<port>`.
///
/// Over http (loopback, the test environment, a dev server, the LAN until
/// it has TLS) it is as before: menger's shell substitutes the port into
/// `<meta name="menger-ws-port">` when it serves the app (src/shell.rs).
/// Anything else serving the file leaves the placeholder in place, and the
/// default WS port is the honest guess — a wrong one surfaces as
/// `reconnecting…` plus the connect error in the banner, not as silence.
pub(crate) fn ws_url() -> String {
    let loc = window().location();
    let port = window()
        .document()
        .and_then(|d| d.query_selector("meta[name=\"menger-ws-port\"]").ok().flatten())
        .and_then(|el| el.dyn_into::<web_sys::HtmlMetaElement>().ok())
        .map(|m| m.content());
    ws_url_for(
        loc.protocol().as_deref().unwrap_or("http:"),
        &loc.host().unwrap_or_default(),
        &loc.hostname().unwrap_or_else(|_| "127.0.0.1".into()),
        port.as_deref(),
    )
}

/// ACCESS-30: [`ws_url`] for a page at `protocol` (`https:`, `http:`),
/// `host` (its name and port as the address bar has them) and `hostname`,
/// with the shell's stamped port, if any.
pub(crate) fn ws_url_for(protocol: &str, host: &str, hostname: &str, port: Option<&str>) -> String {
    const DEFAULT_WS_PORT: &str = "4455";
    if protocol == "https:" && !host.is_empty() {
        return format!("wss://{host}/ws");
    }
    let port = port.filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())).unwrap_or(DEFAULT_WS_PORT);
    format!("ws://{hostname}:{port}")
}

#[cfg(test)]
mod tests {
    use super::ws_url_for;

    /// ACCESS-30: over https the socket is the page's own origin at `/ws`,
    /// port and all; over http it is the stamped port at the page's name.
    #[test]
    fn over_https_the_socket_is_on_the_page_s_own_origin() {
        assert_eq!(ws_url_for("https:", "app.menger.sh", "app.menger.sh", Some("4455")), "wss://app.menger.sh/ws");
        assert_eq!(ws_url_for("https:", "menger.lan:8443", "menger.lan", Some("4455")), "wss://menger.lan:8443/ws");
        assert_eq!(ws_url_for("http:", "127.0.0.1:4456", "127.0.0.1", Some("4455")), "ws://127.0.0.1:4455");
        assert_eq!(ws_url_for("http:", "192.168.8.117:4451", "192.168.8.117", Some("4450")), "ws://192.168.8.117:4450");
        // The placeholder unstamped, or nothing: the default port.
        assert_eq!(ws_url_for("http:", "x:1", "x", Some("{{WS_PORT}}")), "ws://x:4455");
        assert_eq!(ws_url_for("http:", "x:1", "x", None), "ws://x:4455");
    }
}
