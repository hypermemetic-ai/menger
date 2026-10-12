//! AUTH-22: the sign-in page's chrome — web/login.html filled from the
//! app's [`Branding`] and a body — and the headers every page goes out
//! with: `Cache-Control: no-store`, no framing, and a strict
//! Content-Security-Policy whose script and style sources are the hashes of
//! the page's own inline blocks (no third-party script, no inline script
//! that is not hashed).
//!
//! Nothing here knows an app or a session: [`Login`](crate::web_login::Login)
//! and [`WorkOsPage`](crate::workos_page::WorkOsPage) both draw through it,
//! and so can a central sign-in service.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use base64::Engine as _;
use sha2::{Digest, Sha256};

use crate::access::Branding;

/// What the page says around its body: the app, the instance, the side
/// column's facts, and (IDENTITY-35) the dev sign-in's toggle.
#[derive(Debug, Clone)]
pub struct Chrome {
    pub branding: Branding,
    /// The instance's title (the tab and the brand line).
    pub title: String,
    /// The side column's `<dt>`/`<dd>` pairs.
    pub facts: Vec<(String, String)>,
    /// The dev sign-in's cookie name (the toggle's script writes it).
    pub dev_cookie: String,
    /// The dev sign-in's toggle, already HTML.
    pub dev_toggle: String,
}

/// The parts of the page's main column, each already HTML (escaped by its
/// maker).
#[derive(Debug, Clone, Default)]
pub struct Body {
    pub heading: String,
    pub error: Option<String>,
    /// The first block (a form).
    pub main: String,
    /// The other ways in, below an "or" when `main` is not empty.
    pub alts: String,
    /// Content after the ways in (nostr's button).
    pub extra: String,
}

/// The page, filled. Text in `chrome` is escaped here; `body` is HTML.
pub fn render(chrome: &Chrome, body: Body) -> String {
    let facts: String = chrome.facts.iter().map(|(k, v)| format!("<dt>{}</dt><dd>{}</dd>", esc(k), esc(v))).collect();
    let motto = if chrome.branding.motto.is_empty() {
        String::new()
    } else {
        format!(r#"<p class="motto">{}</p>"#, esc(chrome.branding.motto))
    };
    let alts = !body.alts.is_empty() || !body.extra.is_empty();
    include_str!("../web/login.html")
        .replace("{{APP}}", &esc(chrome.branding.app))
        .replace("{{MARK}}", &esc(chrome.branding.mark))
        .replace("{{MOTTO}}", &motto)
        .replace("{{DEV_COOKIE}}", &esc(&chrome.dev_cookie))
        .replace("{{DEV}}", &chrome.dev_toggle)
        .replace("{{HEADING}}", &body.heading)
        .replace("{{FACTS}}", &facts)
        .replace("{{TITLE}}", &esc(&chrome.title))
        .replace("{{VERSION}}", &esc(chrome.branding.version))
        .replace("{{ERROR}}", &body.error.map(|e| format!(r#"<p class="error" role="alert">{}</p>"#, esc(&e))).unwrap_or_default())
        .replace("{{OR}}", if !body.main.is_empty() && alts { r#"<div class="or">or</div>"# } else { "" })
        .replace("{{PASSWORD}}", &body.main)
        .replace("{{BUTTONS}}", &body.alts)
        .replace("{{NOSTR}}", &body.extra)
}

/// A heading and a line beneath it.
pub fn heading(title: &str, sub: &str) -> String {
    format!(
        r#"<h1>{}</h1>
      <p class="sub">{}</p>"#,
        esc(title),
        esc(sub)
    )
}

/// The page's Content-Security-Policy: nothing but itself. Scripts and
/// styles are the page's own inline blocks, by hash; images are its own or
/// `data:` (the TOTP QR code); `fetch` goes only to its own origin.
/// `form-action` is left out on purpose: a sign-in form's answer is a
/// redirect to WorkOS and on to the person's IdP, which `form-action`
/// would block in browsers that apply it to redirects.
pub fn csp(html: &str) -> String {
    let scripts = hashes(html, "<script>", "</script>");
    let styles = hashes(html, "<style>", "</style>");
    format!(
        "default-src 'none'; script-src {}; style-src {}; img-src 'self' data:; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'",
        or_none(&scripts),
        or_none(&styles)
    )
}

fn or_none(sources: &[String]) -> String {
    if sources.is_empty() {
        "'none'".into()
    } else {
        sources.join(" ")
    }
}

/// `'sha256-…'` of every block between `open` and `close`.
fn hashes(html: &str, open: &str, close: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(close) else { break };
        let digest = Sha256::digest(&after.as_bytes()[..j]);
        let h = format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(digest));
        if !out.contains(&h) {
            out.push(h);
        }
        rest = &after[j + close.len()..];
    }
    out
}

/// A page as a response: not stored, not framed, its CSP.
pub fn respond(status: StatusCode, html: String) -> Response {
    let csp = csp(&html);
    let mut r = (status, Html(html)).into_response();
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Ok(v) = HeaderValue::from_str(&csp) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    r
}

/// HTML-escaped text.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_names_exactly_the_page_s_own_blocks() {
        let html = "<style>a{}</style><script>x()</script><script>x()</script>";
        let p = csp(html);
        let h = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(b"x()"));
        assert!(p.contains(&format!("script-src 'sha256-{h}';")), "{p}");
        assert!(p.contains("frame-ancestors 'none'") && p.contains("default-src 'none'") && !p.contains("unsafe-inline"));
        assert!(csp("<p>no script</p>").contains("script-src 'none'"));
    }

    #[test]
    fn the_filled_page_is_branded_and_escaped() {
        let chrome = Chrome {
            branding: Branding { app: "toy", version: "1", mark: "t", motto: "" },
            title: "toy <test>".into(),
            facts: vec![("instance".into(), "a&b".into())],
            dev_cookie: "toy_dev".into(),
            dev_toggle: String::new(),
        };
        let html = render(&chrome, Body { heading: heading("Sign in", "to continue"), error: Some("<b>no</b>".into()), ..Body::default() });
        assert!(html.contains("<title>Sign in · toy &lt;test&gt;</title>"));
        assert!(html.contains("&lt;b&gt;no&lt;/b&gt;") && html.contains("a&amp;b"));
        for ph in ["{{APP}}", "{{TITLE}}", "{{HEADING}}", "{{ERROR}}", "{{MOTTO}}", "{{PASSWORD}}", "{{BUTTONS}}"] {
            assert!(!html.contains(ph), "{ph}");
        }
        assert!(!html.contains("class=\"motto\""));
    }
}
