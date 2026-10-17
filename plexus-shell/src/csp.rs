//! The Content-Security-Policy a page goes out under (menger's ACCESS-28).
//!
//! A page's policy is a template ([`ShellConfig::csp`](crate::ShellConfig::csp),
//! [`DEFAULT_CSP`] unless the app gives its own) with two placeholders, filled
//! per answer:
//!
//! - `{{SCRIPTS}}` — the hashes of the page's own inline `<script>` blocks
//!   ([`script_hashes`]): the entrypoint's loader and anything the app
//!   inlined, so no other inline script runs. A table's page is hashed when
//!   the table is read; a stamped page when the router is built.
//! - `{{HUB}}` — the hub's WebSocket origins ([`hub_sources`]): the host the
//!   page was asked for (its `Host`), at the hub's port, `ws:` and `wss:`.
//!   The socket is on its own port, so `'self'` does not reach it. And
//!   (menger's ACCESS-30) the page's own origin as `wss:` — behind a proxy
//!   that terminates TLS the socket is that origin's `/ws`, which `'self'`
//!   reaches only in browsers that read CSP3's rule for it.

use base64::Engine as _;
use sha2::{Digest, Sha256};

/// The policy a page gets when the app names none: its scripts are its own
/// files, the wasm, and its hashed inline blocks; it connects to itself and
/// its hub; images, fonts and styles are its own (styles inline too — a
/// framework writes `style` attributes); nothing frames it.
pub const DEFAULT_CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval' {{SCRIPTS}}; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; \
connect-src 'self' {{HUB}}; object-src 'none'; base-uri 'self'; frame-ancestors 'none'";

/// `template` filled for one answer: the page's script hashes and the hub's
/// origins as asked for at `host`.
pub fn page_policy(template: &str, scripts: &str, host: Option<&str>, ws_port: u16) -> String {
    let filled = template.replace("{{SCRIPTS}}", scripts).replace("{{HUB}}", &hub_sources(host, ws_port));
    // An empty placeholder leaves a double space; a policy reads the same either way.
    filled.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ").replace(" ;", ";")
}

/// `'sha256-…'` for each inline `<script>` block in `html` (one without a
/// `src`), space-separated, each once. HTML comments are skipped, so a
/// comment that mentions `<script>` neither adds a hash nor swallows the
/// block after it.
pub fn script_hashes(html: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while at < lower.len() {
        let next_comment = lower[at..].find("<!--").map(|i| at + i);
        let Some(open) = lower[at..].find("<script").map(|i| at + i) else { break };
        if let Some(c) = next_comment.filter(|c| *c < open) {
            at = lower[c..].find("-->").map_or(lower.len(), |e| c + e + 3);
            continue;
        }
        let Some(gt) = lower[open..].find('>').map(|i| open + i) else { break };
        let tag = &lower[open..gt];
        let body_at = gt + 1;
        let Some(close) = lower[body_at..].find("</script").map(|i| body_at + i) else { break };
        if !has_src(tag) {
            let h = format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(Sha256::digest(&html.as_bytes()[body_at..close])));
            if !out.contains(&h) {
                out.push(h);
            }
        }
        at = close + "</script".len();
    }
    out.join(" ")
}

/// Whether a `<script …` open tag (lowercased) names a `src`.
fn has_src(tag: &str) -> bool {
    tag.split(|c: char| c.is_ascii_whitespace()).any(|attr| attr == "src" || attr.starts_with("src="))
}

/// The hub's WebSocket origins for a page asked for at `host` (a `Host`
/// header: `name`, `name:port`, `[v6]:port`): `ws://name:port wss://name:port`,
/// then the page's own origin over TLS, `wss://<host as asked>` (ACCESS-30).
/// A missing or malformed `host` gives none — the page then reaches only
/// `'self'`.
pub fn hub_sources(host: Option<&str>, ws_port: u16) -> String {
    let Some(host) = host else { return String::new() };
    let Some(name) = host_name(host) else { return String::new() };
    let mut out = format!("ws://{name}:{ws_port} wss://{name}:{ws_port}");
    // The page's own origin: its name and the port it was asked at, if any
    // (digits only — anything else gives no source).
    let port = &host[name.len()..];
    let own = match port.strip_prefix(':') {
        None if port.is_empty() => Some(format!("wss://{name}")),
        Some(p) if !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()) => Some(format!("wss://{name}:{p}")),
        _ => None,
    };
    if let Some(own) = own.filter(|o| *o != format!("wss://{name}:{ws_port}")) {
        out.push(' ');
        out.push_str(&own);
    }
    out
}

/// The name part of a `Host` value, when it is only a name: letters,
/// digits, `.` and `-` (or a bracketed IPv6 literal) — never anything that
/// could end a source or a directive.
fn host_name(host: &str) -> Option<&str> {
    let name = if host.starts_with('[') {
        &host[..=host.find(']')?]
    } else {
        host.split(':').next()?
    };
    let ok = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'))
        && (name.starts_with('[') || !name.contains(['[', ']', ':']));
    ok.then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(s: &str) -> String {
        format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(Sha256::digest(s.as_bytes())))
    }

    /// Inline blocks are hashed; a `src` script, a comment mentioning a
    /// script and a repeated block are not (twice).
    #[test]
    fn only_the_inline_blocks_are_hashed() {
        let html = "<!-- not a <script> here --><script>a()</script>\
                    <script type=\"module\" src=\"/app/x.js\"></script>\
                    <SCRIPT type=\"module\">import x from '/app/x.js'</SCRIPT><script>a()</script>\
                    <script data-src=\"/y\">b()</script>";
        assert_eq!(script_hashes(html), [sha("a()"), sha("import x from '/app/x.js'"), sha("b()")].join(" "));
        assert_eq!(script_hashes("<p>no scripts</p>"), "");
        assert_eq!(script_hashes("<script>unclosed"), "", "an unclosed block is no block");
    }

    /// The hub's origins follow the name the page was asked for; anything
    /// that is not a plain name gives none.
    #[test]
    fn the_hub_is_reached_at_the_name_the_page_was_asked_for() {
        assert_eq!(hub_sources(Some("127.0.0.1:4456"), 4455), "ws://127.0.0.1:4455 wss://127.0.0.1:4455 wss://127.0.0.1:4456");
        assert_eq!(hub_sources(Some("menger.example"), 4455), "ws://menger.example:4455 wss://menger.example:4455 wss://menger.example");
        assert_eq!(hub_sources(Some("[::1]:4456"), 4455), "ws://[::1]:4455 wss://[::1]:4455 wss://[::1]:4456");
        // ACCESS-30: the page's own origin over TLS, port and all, once.
        assert_eq!(hub_sources(Some("menger.lan:8443"), 4455), "ws://menger.lan:4455 wss://menger.lan:4455 wss://menger.lan:8443");
        assert_eq!(hub_sources(Some("menger.lan:4455"), 4455), "ws://menger.lan:4455 wss://menger.lan:4455");
        assert_eq!(hub_sources(Some("menger.lan:44x"), 4455), "ws://menger.lan:4455 wss://menger.lan:4455");
        for bad in [None, Some(""), Some("a b"), Some("a;script-src *"), Some("x'y"), Some("[::1"), Some("a]b")] {
            assert_eq!(hub_sources(bad, 4455), "", "{bad:?}");
        }
    }

    /// The default policy, filled: its own scripts and its hub, nothing
    /// else; empty placeholders leave no gap.
    #[test]
    fn the_default_policy_is_filled_per_page() {
        let p = page_policy(DEFAULT_CSP, "'sha256-x'", Some("localhost:1"), 2);
        assert!(p.contains("script-src 'self' 'wasm-unsafe-eval' 'sha256-x';"), "{p}");
        assert!(p.contains("connect-src 'self' ws://localhost:2 wss://localhost:2 wss://localhost:1;"), "{p}");
        assert!(p.ends_with("frame-ancestors 'none'"), "{p}");
        let bare = page_policy(DEFAULT_CSP, "", None, 2);
        assert!(bare.contains("script-src 'self' 'wasm-unsafe-eval';") && bare.contains("connect-src 'self';"), "{bare}");
        assert!(!bare.contains("  "));
    }
}
