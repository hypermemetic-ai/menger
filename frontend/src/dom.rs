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

/// APP-53: what the page scrolls in. On a wide window that is the content
/// column (`#app .content`, beside the nav); on a phone (≤ 820 px) it is the
/// document itself, because a mobile browser hides its own bars only when
/// the document scrolls. Which one is read off the column's computed
/// `overflow-y` — the stylesheet decides, and every reader agrees with it.
pub(crate) struct Scroller {
    el: web_sys::Element,
    doc: bool,
}

impl Scroller {
    /// The page's scroller, from the content column.
    pub(crate) fn page() -> Option<Self> {
        let content = window().document()?.query_selector("#app .content").ok().flatten()?;
        Self::of(&content)
    }

    /// The page's scroller, from an element inside the content column.
    pub(crate) fn around(el: &web_sys::Element) -> Option<Self> {
        Self::of(&el.closest(".content").ok().flatten()?)
    }

    /// CHAT-29: a chat's message list (`.pmsgs`) — itself where it scrolls,
    /// the document where the stylesheet lets it run (the full-screen chat
    /// on a phone).
    pub(crate) fn list(el: &web_sys::Element) -> Option<Self> {
        Self::of(el)
    }

    /// Whether this is the document's scroll.
    pub(crate) fn is_doc(&self) -> bool {
        self.doc
    }

    fn of(content: &web_sys::Element) -> Option<Self> {
        let scrolls = window()
            .get_computed_style(content)
            .ok()
            .flatten()
            .and_then(|st| st.get_property_value("overflow-y").ok())
            .is_none_or(|o| o != "visible");
        if scrolls {
            return Some(Self { el: content.clone(), doc: false });
        }
        let doc = window().document()?;
        let el = doc.scrolling_element().or_else(|| doc.document_element())?;
        Some(Self { el, doc: true })
    }

    pub(crate) fn element(&self) -> &web_sys::Element {
        &self.el
    }

    pub(crate) fn top(&self) -> f64 {
        f64::from(self.el.scroll_top())
    }

    pub(crate) fn set_top(&self, y: f64) {
        self.el.set_scroll_top(y.round() as i32);
    }

    /// Scroll by `dy` px.
    pub(crate) fn by(&self, dy: f64) {
        self.set_top(self.top() + dy);
    }

    pub(crate) fn scroll_height(&self) -> f64 {
        f64::from(self.el.scroll_height())
    }

    pub(crate) fn client_height(&self) -> f64 {
        f64::from(self.el.client_height())
    }

    /// Where the scroller shows the page, in the viewport: its top and
    /// height. The document's is the viewport itself (its bounding rect
    /// moves with the scroll, so it is not asked).
    pub(crate) fn view(&self) -> (f64, f64) {
        if self.doc {
            (0.0, self.client_height())
        } else {
            let r = self.el.get_bounding_client_rect();
            (r.top(), r.height())
        }
    }
}

/// APP-53: listen for the page's scroll wherever it happens — the content
/// column's (a wide window) and the window's (a phone) — passively;
/// [`unlisten_page_scroll`] takes it back.
pub(crate) fn listen_page_scroll(f: &js_sys::Function) {
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_passive(true);
    if let Some(c) = window().document().and_then(|d| d.query_selector("#app .content").ok().flatten()) {
        let _ = c.add_event_listener_with_callback_and_add_event_listener_options("scroll", f, &opts);
    }
    let _ = window().add_event_listener_with_callback_and_add_event_listener_options("scroll", f, &opts);
}

pub(crate) fn unlisten_page_scroll(f: &js_sys::Function) {
    if let Some(c) = window().document().and_then(|d| d.query_selector("#app .content").ok().flatten()) {
        let _ = c.remove_event_listener_with_callback("scroll", f);
    }
    let _ = window().remove_event_listener_with_callback("scroll", f);
}
