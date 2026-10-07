//! COMP-4: framed bodies — a body that holds HTML renders in a sandboxed
//! frame, lavish-axi's way (COMP-1 decisions 1 and 4).
//!
//! A body md.rs calls framed ([`crate::md::is_framed`]) is not painted into
//! the page: [`Frame`] renders it as one `<iframe sandbox=SANDBOX>` — no
//! `allow-same-origin`, so the frame's origin is opaque: its scripts run,
//! but cannot read the app's DOM, storage or cookies, nor reach its
//! WebSocket — whose `srcdoc` ([`srcdoc`]) is the body's markup
//! ([`crate::md::render_body_framed`]: raw HTML passed through, `{wave: ID}`
//! already placed by the host) under a head that carries:
//!
//! - `<base target="_blank">` — a link opens a new top-level tab (the
//!   sandbox's popups escape it);
//! - `<style id="menger-theme">` — the app's theme tokens ([`THEME_TOKENS`],
//!   read off the app's `:root` as painted now) and `color-scheme`, so the
//!   first paint is in the app's theme;
//! - `#menger-init` — the frame's channel, the SDK version, the theme, and
//!   where the vendored mermaid bundle is and the strict config it runs
//!   under (src/mermaid.rs);
//! - the SDK shell, inline: `frontend/sdk/menger-sdk.js` (also served at
//!   [`SDK_URL`]), whose header is the protocol's reference — base
//!   typography, theme tokens that follow a flip, mermaid drawn inside, the
//!   document's height reported, `window.menger = { version }`.
//!
//! The host half of the handshake is here: one `message` listener for every
//! frame ([`install_listener`]) accepts a `menger:*` message only from a
//! frame's own `contentWindow` carrying that frame's channel; it answers
//! `menger:ready` with the current theme and sizes the frame to each
//! `menger:size` (no inner scrollbar). [`retheme_all`] — called by the theme
//! toggle when the painted mode changes — posts the new tokens to every
//! frame. A body that changes (an `amended`, a keystroke in the editor's
//! preview, a wave reference placed) loads — once it has held still for
//! [`RERENDER_AFTER_MS`] — into a second, invisible frame beside the one on
//! screen, which is replaced only once the new one reports `settled` (its
//! diagrams drawn) — so the page never flashes.
//!
//! What decides — the srcdoc, the theme block, the init block, which
//! messages count — is pure and pinned by native tests; the rest is DOM.

use std::cell::Cell;

use js_sys::{Object, Reflect};
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;

use crate::{dom, mermaid};

/// The SDK shell's version — `window.menger.version` inside every frame
/// (pinned equal to the script's own `VERSION` by a native test).
pub(crate) const SDK_VERSION: &str = "0.1.0";

/// Where the app serves the SDK shell (index.html copy-files
/// `sdk/menger-sdk.js` into `dist/sdk/`). The frames do not fetch it — it is
/// inlined — but it is the app's published copy of what they run.
pub(crate) const SDK_URL: &str = "/app/sdk/menger-sdk.js";

/// The shell, inlined into every srcdoc.
const SDK_JS: &str = include_str!("../sdk/menger-sdk.js");

/// The frame's sandbox (COMP-1 decision 1, after lavish-axi's): scripts,
/// forms, and popups that escape the sandbox — never `allow-same-origin`,
/// which would hand the body the app's origin.
pub(crate) const SANDBOX: &str = "allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox";

/// The class every frame's `<iframe>` wears — what the listener and
/// [`retheme_all`] look frames up by.
const FRAME_CLASS: &str = "mframe-doc";

/// A re-rendered frame that never reports `settled` replaces the one on
/// screen after this long anyway.
const PROMOTE_AFTER_MS: i32 = 3000;

/// A body that changes while a frame is on screen re-renders once it has
/// held still this long — a burst of keystrokes in the editor's preview is
/// one new frame, not one per key. A first frame loads at once.
const RERENDER_AFTER_MS: i32 = 150;

/// The height a frame may claim (the SDK caps it too).
const MAX_HEIGHT: f64 = 50_000.0;

/// COMP-1 pin 10: the theme tokens a frame receives — every custom property
/// of index.html's light `:root` block (pinned equal to it by a native
/// test in main.rs), sent with the values the app paints now.
pub(crate) const THEME_TOKENS: [&str; 27] = [
    "--bg", "--panel", "--card", "--ink", "--muted", "--line", "--accent", "--hover", "--row-sel", "--sel",
    "--chip-bg", "--chip-fg", "--chip-line", "--tree-line", "--live", "--err-bg", "--err-fg", "--err-line",
    "--scrim", "--shadow", "--kind-scope", "--kind-build", "--kind-execution", "--kind-gap", "--kind-spike",
    "--mono", "--sans",
];

/// The painted mode, as a frame's `color-scheme` and `data-theme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
    }

    /// From the root's computed `color-scheme` (index.html says `light`, or
    /// `dark` in both dark blocks).
    pub(crate) fn from_color_scheme(css: &str) -> Self {
        if css.split_whitespace().any(|w| w == "dark") {
            Scheme::Dark
        } else {
            Scheme::Light
        }
    }
}

/// What one frame's head is made of.
pub(crate) struct Shell<'a> {
    pub(crate) channel: &'a str,
    pub(crate) scheme: Scheme,
    /// `(token, value)` — [`THEME_TOKENS`] as painted.
    pub(crate) tokens: &'a [(String, String)],
    /// The vendored mermaid bundle, absolute (the frame's origin is opaque;
    /// a classic script loads across origins).
    pub(crate) mermaid_url: &'a str,
}

/// A token name the frame will take: `--` and lowercase ASCII words.
fn token_name_ok(name: &str) -> bool {
    name.strip_prefix("--")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
}

/// A token value that cannot end its declaration, rule or `<style>` early.
/// The values are the app's own stylesheet's, so this only ever drops a
/// value nobody wrote.
fn token_value_ok(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains(['<', '>', '{', '}', ';', '\\'])
}

/// The frame's first theme: `:root { color-scheme; --token: value; … }`.
pub(crate) fn theme_css(scheme: Scheme, tokens: &[(String, String)]) -> String {
    let mut css = format!(":root{{color-scheme:{}", scheme.as_str());
    for (name, value) in tokens {
        if token_name_ok(name) && token_value_ok(value) {
            css.push(';');
            css.push_str(name);
            css.push(':');
            css.push_str(value.trim());
        }
    }
    css.push('}');
    css
}

/// The init block the SDK reads (`#menger-init`): JSON with every `<`
/// written as `<`, so nothing in it can close its `<script>`.
pub(crate) fn init_json(shell: &Shell) -> String {
    let init = serde_json::json!({
        "channel": shell.channel,
        "version": SDK_VERSION,
        "scheme": shell.scheme.as_str(),
        "mermaid": {
            "url": shell.mermaid_url,
            // The SDK sets `theme` per draw, from the scheme.
            "config": mermaid::config_json("default"),
        },
    });
    init.to_string().replace('<', "\\u003c")
}

/// A framed body's whole document: the head that makes it menger's (see
/// the module doc), then the body's markup as `render_body_framed` gave it.
pub(crate) fn srcdoc(body_html: &str, shell: &Shell) -> String {
    let mut doc = String::with_capacity(body_html.len() + SDK_JS.len() + 2048);
    doc.push_str("<!doctype html><html lang=\"en\" data-theme=\"");
    doc.push_str(shell.scheme.as_str());
    doc.push_str("\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    doc.push_str("<base target=\"_blank\">");
    doc.push_str("<style id=\"menger-theme\">");
    doc.push_str(&theme_css(shell.scheme, shell.tokens));
    doc.push_str("</style><script type=\"application/json\" id=\"menger-init\">");
    doc.push_str(&init_json(shell));
    // The inlined shell says which one it is and where the app publishes it.
    doc.push_str("</script><script id=\"menger-sdk\" data-version=\"");
    doc.push_str(SDK_VERSION);
    doc.push_str("\" data-src=\"");
    doc.push_str(SDK_URL);
    doc.push_str("\">");
    doc.push_str(SDK_JS);
    doc.push_str("</script></head><body>");
    doc.push_str(body_html);
    doc.push_str("</body></html>");
    doc
}

/// A message the host acts on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Msg {
    Ready,
    /// The document's height in CSS px (clamped), and whether its first
    /// paint is complete.
    Size { height: u32, settled: bool },
}

/// Read a `menger:*` message's kind and fields; anything else is ignored.
pub(crate) fn parse_msg(kind: &str, height: Option<f64>, settled: Option<bool>) -> Option<Msg> {
    match kind {
        "menger:ready" => Some(Msg::Ready),
        "menger:size" => {
            let h = height.filter(|h| h.is_finite())?;
            Some(Msg::Size { height: h.clamp(0.0, MAX_HEIGHT).ceil() as u32, settled: settled.unwrap_or(false) })
        }
        _ => None,
    }
}

// -- the DOM half ----------------------------------------------------------

thread_local! {
    static NEXT_CHANNEL: Cell<u32> = const { Cell::new(0) };
    static LISTENING: Cell<bool> = const { Cell::new(false) };
}

fn next_channel() -> String {
    NEXT_CHANNEL.with(|n| {
        let v = n.get().wrapping_add(1);
        n.set(v);
        format!("f{v}")
    })
}

/// The theme painted now: the root's `color-scheme`, and each token's value.
fn read_theme() -> (Scheme, Vec<(String, String)>) {
    let w = dom::window();
    let style = w
        .document()
        .and_then(|d| d.document_element())
        .and_then(|root| w.get_computed_style(&root).ok().flatten());
    let Some(style) = style else {
        return (Scheme::Light, Vec::new());
    };
    let scheme = Scheme::from_color_scheme(&style.get_property_value("color-scheme").unwrap_or_default());
    let tokens = THEME_TOKENS
        .iter()
        .filter_map(|name| {
            let v = style.get_property_value(name).ok()?;
            let v = v.trim();
            (!v.is_empty()).then(|| (name.to_string(), v.to_string()))
        })
        .collect();
    (scheme, tokens)
}

fn set(obj: &Object, key: &str, value: &JsValue) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), value);
}

/// `menger:theme` for the frame on `channel`, as painted now.
fn theme_message(channel: &str) -> JsValue {
    let (scheme, tokens) = read_theme();
    let msg = Object::new();
    set(&msg, "type", &"menger:theme".into());
    set(&msg, "channel", &channel.into());
    set(&msg, "scheme", &scheme.as_str().into());
    let t = Object::new();
    for (name, value) in tokens {
        set(&t, &name, &value.into());
    }
    set(&msg, "tokens", &t);
    msg.into()
}

fn post(frame: &web_sys::HtmlIFrameElement, msg: &JsValue) {
    if let Some(w) = frame.content_window() {
        let _ = w.post_message(msg, "*");
    }
}

/// Every frame in the document.
fn frames() -> Vec<web_sys::HtmlIFrameElement> {
    let Some(list) = dom::window()
        .document()
        .and_then(|d| d.query_selector_all(&format!("iframe.{FRAME_CLASS}")).ok())
    else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|i| list.get(i))
        .filter_map(|n| n.dyn_into::<web_sys::HtmlIFrameElement>().ok())
        .collect()
}

/// COMP-1 pin 10: the painted mode changed — every frame takes the new
/// tokens (and redraws its diagrams). Called by the theme toggle beside
/// `mermaid::rerender_all`. Nothing here can throw.
pub(crate) fn retheme_all() {
    for f in frames() {
        if let Some(channel) = f.get_attribute("data-channel") {
            post(&f, &theme_message(&channel));
        }
    }
}

/// The frame on screen in `frame`'s container becomes `frame`: the old one
/// (and any other) goes, the new one stops being invisible.
fn promote(frame: &web_sys::HtmlIFrameElement) {
    let _ = frame.set_attribute("data-state", "live");
    let Some(parent) = frame.parent_element() else { return };
    let Ok(list) = parent.query_selector_all(":scope > iframe") else { return };
    for i in 0..list.length() {
        if let Some(el) = list.get(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) {
            if !Object::is(&el, frame) {
                el.remove();
            }
        }
    }
}

fn has_live_sibling(frame: &web_sys::HtmlIFrameElement) -> bool {
    frame
        .parent_element()
        .and_then(|p| p.query_selector(":scope > iframe[data-state=\"live\"]").ok().flatten())
        .is_some()
}

fn on_message(ev: web_sys::MessageEvent) {
    let data = ev.data();
    if !data.is_object() {
        return;
    }
    let field = |k: &str| Reflect::get(&data, &JsValue::from_str(k)).ok();
    let Some(kind) = field("type").and_then(|v| v.as_string()).filter(|k| k.starts_with("menger:")) else {
        return;
    };
    let Some(source) = ev.source() else { return };
    let Some(channel) = field("channel").and_then(|v| v.as_string()) else { return };
    // The sender must be a frame's own window, speaking on that frame's
    // channel — never another window, and never a stale document.
    let Some(frame) = frames().into_iter().find(|f| f.content_window().is_some_and(|w| Object::is(&w, &source))) else {
        return;
    };
    if frame.get_attribute("data-channel").as_deref() != Some(channel.as_str()) {
        return;
    }
    match parse_msg(&kind, field("height").and_then(|v| v.as_f64()), field("settled").and_then(|v| v.as_bool())) {
        Some(Msg::Ready) => post(&frame, &theme_message(&channel)),
        Some(Msg::Size { height, settled }) => {
            let _ = web_sys::HtmlElement::style(&frame).set_property("height", &format!("{height}px"));
            // A first frame shows at once; a re-render waits until drawn.
            if frame.get_attribute("data-state").as_deref() == Some("pending") && (settled || !has_live_sibling(&frame)) {
                promote(&frame);
            }
        }
        None => {}
    }
}

/// The one `message` listener every frame's SDK talks to; installed by the
/// first [`Frame`], for the app's lifetime.
fn install_listener() {
    if LISTENING.with(|l| l.replace(true)) {
        return;
    }
    let cb = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(on_message);
    let _ = dom::window().add_event_listener_with_callback("message", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Load `body_html` into a new frame in `container`, beside the one on
/// screen (if any), invisible until [`promote`]d.
fn load(container: &web_sys::Element, body_html: &str) {
    let w = dom::window();
    let Some(doc) = w.document() else { return };
    let channel = next_channel();
    let (scheme, tokens) = read_theme();
    let mermaid_url = format!("{}{}", w.location().origin().unwrap_or_default(), mermaid::ASSET_URL);
    let html = srcdoc(body_html, &Shell { channel: &channel, scheme, tokens: &tokens, mermaid_url: &mermaid_url });
    // A re-render still loading is superseded by this one.
    if let Ok(list) = container.query_selector_all(":scope > iframe[data-state=\"pending\"]") {
        for i in 0..list.length() {
            if let Some(el) = list.get(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) {
                el.remove();
            }
        }
    }
    let Ok(el) = doc.create_element("iframe") else { return };
    // Sandboxed before it has a document: the first navigation is too.
    let _ = el.set_attribute("sandbox", SANDBOX);
    let _ = el.set_attribute("class", FRAME_CLASS);
    let _ = el.set_attribute("data-channel", &channel);
    let _ = el.set_attribute("data-state", "pending");
    let _ = el.set_attribute("data-sdk", SDK_VERSION);
    let _ = el.set_attribute("title", "ticket body");
    let Ok(frame) = el.dyn_into::<web_sys::HtmlIFrameElement>() else { return };
    frame.set_srcdoc(&html);
    let _ = container.append_child(&frame);
    spawn_local(async move {
        dom::sleep_ms(PROMOTE_AFTER_MS).await;
        if frame.is_connected() && frame.get_attribute("data-state").as_deref() == Some("pending") {
            promote(&frame);
        }
    });
}

/// COMP-4: a framed body — `html` (a framed body's markup) in a sandboxed
/// frame with the SDK shell, re-rendered without a flash whenever it
/// changes. The frontmatter is the caller's: it stays outside.
#[component]
pub(crate) fn Frame(#[prop(into)] html: Signal<String>) -> impl IntoView {
    install_listener();
    let host = NodeRef::<leptos::html::Div>::new();
    // The newest body asked for: a debounced re-render loads only if it is
    // still the newest when its wait ends (and the frame still mounted).
    let asked = StoredValue::new(0u32);
    Effect::new(move |_| {
        let body = html.get();
        let Some(el) = host.get() else { return };
        let n = asked.with_value(|n| n.wrapping_add(1));
        asked.set_value(n);
        if el.query_selector(":scope > iframe").ok().flatten().is_none() {
            load(&el, &body);
            return;
        }
        spawn_local(async move {
            dom::sleep_ms(RERENDER_AFTER_MS).await;
            if asked.try_with_value(|v| *v == n) == Some(true) {
                load(&el, &body);
            }
        });
    });
    view! { <div class="mframe" node_ref=host></div> }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn shell_tokens() -> Vec<(String, String)> {
        vec![
            ("--bg".into(), "#fbfbf9".into()),
            ("--ink".into(), " #1f2328".into()),
            ("--sans".into(), "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, sans-serif".into()),
        ]
    }

    /// COMP-1 decision 1: lavish-axi's sandbox, minus downloads — and never
    /// the same origin.
    #[test]
    fn the_sandbox_runs_scripts_but_never_shares_the_apps_origin() {
        let flags: Vec<&str> = SANDBOX.split_whitespace().collect();
        assert_eq!(flags, ["allow-scripts", "allow-forms", "allow-popups", "allow-popups-to-escape-sandbox"]);
        assert!(!SANDBOX.contains("same-origin") && !SANDBOX.contains("top-navigation"));
    }

    /// The srcdoc: the theme first (no flash of the wrong one), links to a
    /// new tab, the init block, the SDK inline before the body, then the
    /// body's markup as given.
    #[test]
    fn the_srcdoc_is_the_shell_then_the_body() {
        let tokens = shell_tokens();
        let shell = Shell { channel: "f7", scheme: Scheme::Dark, tokens: &tokens, mermaid_url: "http://h:1/app/vendor/m.js" };
        let doc = srcdoc("<details><summary>s</summary><p>x</p></details>", &shell);
        assert!(doc.starts_with("<!doctype html><html lang=\"en\" data-theme=\"dark\"><head><meta charset=\"utf-8\">"), "{doc}");
        let at = |needle: &str| doc.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at("<base target=\"_blank\">") < at("<style id=\"menger-theme\">"));
        assert!(doc.contains("<style id=\"menger-theme\">:root{color-scheme:dark;--bg:#fbfbf9;--ink:#1f2328;--sans:-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, sans-serif}</style>"), "{doc}");
        assert!(at("id=\"menger-init\"") < at(&format!("var VERSION = \"{SDK_VERSION}\"")));
        assert!(at("var VERSION") < at("<body>"));
        assert!(doc.ends_with("<body><details><summary>s</summary><p>x</p></details></body></html>"), "{doc}");
        // The SDK is the one script before the body, whole, and labelled.
        assert!(doc.contains(&format!("<script id=\"menger-sdk\" data-version=\"{SDK_VERSION}\" data-src=\"{SDK_URL}\">{SDK_JS}</script></head>")));
    }

    /// The init block parses back to what the SDK reads, and nothing in it
    /// can close its script element.
    #[test]
    fn the_init_block_is_json_that_cannot_close_its_script() {
        let tokens = shell_tokens();
        let shell = Shell { channel: "f1", scheme: Scheme::Light, tokens: &tokens, mermaid_url: "http://h/</script><script>alert(1)//" };
        let json = init_json(&shell);
        assert!(!json.contains('<'), "{json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["channel"], "f1");
        assert_eq!(v["version"], SDK_VERSION);
        assert_eq!(v["scheme"], "light");
        assert_eq!(v["mermaid"]["url"], "http://h/</script><script>alert(1)//");
        // The app's strict mermaid posture rides along unchanged.
        assert_eq!(v["mermaid"]["config"]["securityLevel"], "strict");
        assert_eq!(v["mermaid"]["config"]["htmlLabels"], false);
    }

    /// A token the frame cannot take whole is dropped, never spliced.
    #[test]
    fn the_theme_block_takes_only_well_formed_tokens() {
        let tokens: Vec<(String, String)> = [
            ("--bg", "#101014"),
            ("--x", "red}body{display:none"),
            ("--y", "</style><script>1</script>"),
            ("--z", "a;b"),
            ("color", "red"),
            ("--Upper", "red"),
            ("--empty", "  "),
            ("--ok-2", "rgba(0, 0, 0, .55)"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(theme_css(Scheme::Light, &tokens), ":root{color-scheme:light;--bg:#101014;--ok-2:rgba(0, 0, 0, .55)}");
    }

    #[test]
    fn the_scheme_is_read_off_the_painted_color_scheme() {
        assert_eq!(Scheme::from_color_scheme("dark"), Scheme::Dark);
        assert_eq!(Scheme::from_color_scheme(" light "), Scheme::Light);
        assert_eq!(Scheme::from_color_scheme("light dark"), Scheme::Dark);
        assert_eq!(Scheme::from_color_scheme("normal"), Scheme::Light);
        assert_eq!(Scheme::from_color_scheme(""), Scheme::Light);
    }

    /// Only the protocol's two inbound kinds count; a size is a finite,
    /// clamped height.
    #[test]
    fn the_host_reads_ready_and_size_and_nothing_else() {
        assert_eq!(parse_msg("menger:ready", None, None), Some(Msg::Ready));
        assert_eq!(parse_msg("menger:size", Some(312.2), Some(true)), Some(Msg::Size { height: 313, settled: true }));
        assert_eq!(parse_msg("menger:size", Some(40.0), None), Some(Msg::Size { height: 40, settled: false }));
        assert_eq!(parse_msg("menger:size", Some(-5.0), Some(false)), Some(Msg::Size { height: 0, settled: false }));
        assert_eq!(parse_msg("menger:size", Some(1e12), Some(true)), Some(Msg::Size { height: 50_000, settled: true }));
        assert_eq!(parse_msg("menger:size", Some(f64::NAN), Some(true)), None);
        assert_eq!(parse_msg("menger:size", None, Some(true)), None);
        assert_eq!(parse_msg("menger:theme", None, None), None, "outbound only");
        assert_eq!(parse_msg("lavish:queuePrompt", None, None), None);
    }

    /// The shell's three names agree — the version the host stamps, the
    /// script's own, and `window.menger.version` — and the app serves the
    /// script it inlines (index.html copy-files it to `SDK_URL`). The
    /// script is safe to inline: nothing in it ends a `<script>`.
    #[test]
    fn the_sdk_shell_is_versioned_served_and_inlinable() {
        assert!(SDK_JS.contains(&format!("var VERSION = \"{SDK_VERSION}\";")), "the script's VERSION");
        assert!(SDK_JS.contains("Object.freeze({ version: VERSION })"), "window.menger = {{ version }}");
        let lower = SDK_JS.to_ascii_lowercase();
        assert!(!lower.contains("</script") && !lower.contains("<!--"), "inlinable");
        let name = SDK_URL.rsplit('/').next().unwrap();
        assert!(SDK_URL.starts_with("/app/sdk/"), "{SDK_URL}");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(dir.join("sdk").join(name).is_file());
        let index = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(
            index.contains(&format!("<link data-trunk rel=\"copy-file\" href=\"sdk/{name}\" data-target-path=\"sdk\" />")),
            "index.html copy-files the SDK into dist/sdk/"
        );
        // Every message the script sends or takes is one the protocol names.
        for kind in ["menger:ready", "menger:size", "menger:theme"] {
            assert!(SDK_JS.contains(&format!("\"{kind}\"")), "{kind}");
        }
        // Base typography paints through the app's tokens only.
        for token in ["var(--ink)", "var(--sans)", "var(--mono)", "var(--panel)", "var(--line)", "var(--accent)"] {
            assert!(SDK_JS.contains(token), "{token}");
        }
        assert!(!SDK_JS.contains("#fff") && !SDK_JS.contains("rgb("), "no color literal in the shell");
    }
}
