//! AUTH-22: the sign-in page's chrome — web/login.html filled from the
//! app's [`Branding`] and a body — and the headers every page goes out
//! with: `Cache-Control: no-store`, no framing, and a strict
//! Content-Security-Policy whose script and style sources are the hashes of
//! the page's own inline blocks (no third-party script, no inline script
//! that is not hashed).
//!
//! Nothing here knows an app or a session: [`Login`](crate::web_login::Login)
//! and [`SignInPage`](crate::signin::SignInPage) both draw through it,
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

/// A page and the Content-Security-Policy it goes out under.
#[derive(Debug, Clone)]
pub struct Page {
    pub html: String,
    pub csp: String,
    /// How many `<script` and `<style` the page may hold: the template's.
    blocks: usize,
}

impl Page {
    /// A page of plain HTML with no script or style of its own.
    pub fn plain(html: String) -> Self {
        Self { html, csp: csp_of(&[], &[]), blocks: 0 }
    }

    /// Whether the page holds exactly the blocks its policy was made for
    /// (AUTH-24 #7: a block that got in through an escaping bug is not
    /// whitelisted by its own hash — the page is refused).
    pub fn intact(&self) -> bool {
        count_blocks(&self.html) == self.blocks
    }
}

/// web/login.html cut once into text and `{{SLOT}}`s.
enum Seg {
    Text(&'static str),
    Slot(&'static str),
}

const TEMPLATE: &str = include_str!("../web/login.html");

fn segments() -> &'static [Seg] {
    static SEGS: std::sync::OnceLock<Vec<Seg>> = std::sync::OnceLock::new();
    SEGS.get_or_init(|| {
        let mut out = Vec::new();
        let mut rest = TEMPLATE;
        while let Some(i) = rest.find("{{") {
            match rest[i + 2..].find("}}") {
                Some(j) if rest[i + 2..i + 2 + j].bytes().all(|b| b.is_ascii_uppercase() || b == b'_') && j > 0 => {
                    out.push(Seg::Text(&rest[..i]));
                    out.push(Seg::Slot(&rest[i + 2..i + 2 + j]));
                    rest = &rest[i + 2 + j + 2..];
                }
                _ => {
                    out.push(Seg::Text(&rest[..i + 2]));
                    rest = &rest[i + 2..];
                }
            }
        }
        out.push(Seg::Text(rest));
        out
    })
}

/// The template's own `<script>` and `<style>` blocks, with only the
/// configuration slots they hold (`{{APP}}`, `{{DEV_COOKIE}}`) filled: what
/// the policy hashes — never the rendered response.
fn template_blocks(app: &str, dev_cookie: &str) -> (Vec<String>, Vec<String>) {
    let filled = TEMPLATE.replace("{{APP}}", app).replace("{{DEV_COOKIE}}", dev_cookie);
    (blocks(&filled, "<script>", "</script>"), blocks(&filled, "<style>", "</style>"))
}

/// The page, filled in one pass: every slot's value is inserted once and
/// never searched again (AUTH-24 #12). Text in `chrome` is escaped here;
/// `body` is HTML its maker escaped.
pub fn render(chrome: &Chrome, body: Body) -> Page {
    let facts: String = chrome.facts.iter().map(|(k, v)| format!("<dt>{}</dt><dd>{}</dd>", esc(k), esc(v))).collect();
    let motto = if chrome.branding.motto.is_empty() { String::new() } else { format!(r#"<p class="motto">{}</p>"#, esc(chrome.branding.motto)) };
    let alts = !body.alts.is_empty() || !body.extra.is_empty();
    let app = esc(chrome.branding.app);
    let dev_cookie = esc(&chrome.dev_cookie);
    let (mark, title, version) = (esc(chrome.branding.mark), esc(&chrome.title), esc(chrome.branding.version));
    let error = body.error.map(|e| format!(r#"<p class="error" role="alert">{}</p>"#, esc(&e))).unwrap_or_default();
    let or = if !body.main.is_empty() && alts { r#"<div class="or">or</div>"# } else { "" };
    let mut html = String::with_capacity(TEMPLATE.len() + body.main.len() + body.alts.len() + 1024);
    for seg in segments() {
        match seg {
            Seg::Text(t) => html.push_str(t),
            Seg::Slot(name) => html.push_str(match *name {
                "APP" => &app,
                "MARK" => &mark,
                "MOTTO" => &motto,
                "DEV_COOKIE" => &dev_cookie,
                "DEV" => &chrome.dev_toggle,
                "HEADING" => &body.heading,
                "FACTS" => &facts,
                "TITLE" => &title,
                "VERSION" => &version,
                "ERROR" => &error,
                "OR" => or,
                "PASSWORD" => &body.main,
                "BUTTONS" => &body.alts,
                "NOSTR" => &body.extra,
                _ => "",
            }),
        }
    }
    let (scripts, styles) = template_blocks(&app, &dev_cookie);
    Page { csp: csp_of(&scripts, &styles), blocks: scripts.len() + styles.len(), html }
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

/// A Content-Security-Policy admitting these script and style blocks and
/// nothing else of the kind: images its own or `data:` (the TOTP QR code),
/// `fetch` to its own origin only. `form-action` is left out on purpose: a
/// sign-in form's answer is a redirect to the identity backend and on to
/// the person's IdP, which `form-action` would block in browsers that apply
/// it to redirects.
fn csp_of(scripts: &[String], styles: &[String]) -> String {
    format!(
        "default-src 'none'; script-src {}; style-src {}; img-src 'self' data:; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'",
        or_none(&hash_all(scripts)),
        or_none(&hash_all(styles))
    )
}

fn hash_all(blocks: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in blocks {
        let h = format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(Sha256::digest(b.as_bytes())));
        if !out.contains(&h) {
            out.push(h);
        }
    }
    out
}

fn or_none(sources: &[String]) -> String {
    if sources.is_empty() {
        "'none'".into()
    } else {
        sources.join(" ")
    }
}

/// The contents of every block between `open` and `close`.
fn blocks(html: &str, open: &str, close: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(close) else { break };
        out.push(after[..j].to_string());
        rest = &after[j + close.len()..];
    }
    out
}

/// How many `<script` and `<style` openings a page holds, in any case.
fn count_blocks(html: &str) -> usize {
    let lower = html.to_ascii_lowercase();
    lower.matches("<script").count() + lower.matches("<style").count()
}

/// A page as a response: not stored, not framed, under its policy — or,
/// when it holds a block its policy was not made for, refused.
pub fn respond(status: StatusCode, page: Page) -> Response {
    if !page.intact() {
        tracing::error!("a page held a script or style block the template does not: refused (an escaping bug)");
        return respond(StatusCode::INTERNAL_SERVER_ERROR, Page::plain("<!doctype html><title>Error</title><p>Something went wrong. Go back and try again.</p>".into()));
    }
    let mut r = (status, Html(page.html)).into_response();
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Ok(v) = HeaderValue::from_str(&page.csp) {
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

    fn chrome() -> Chrome {
        Chrome {
            branding: Branding { app: "toy", version: "1", mark: "t", motto: "" },
            title: "toy <test>".into(),
            facts: vec![("instance".into(), "a&b".into())],
            dev_cookie: "toy_dev".into(),
            dev_toggle: String::new(),
        }
    }

    #[test]
    fn the_policy_is_the_template_s_never_the_response_s() {
        let p = render(&chrome(), Body { heading: heading("Sign in", "to continue"), ..Body::default() });
        assert!(p.intact());
        let (scripts, _) = template_blocks("toy", "toy_dev");
        assert_eq!(scripts.len(), 2);
        let h = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(scripts[0].as_bytes()));
        assert!(p.csp.contains(&h) && p.csp.contains("frame-ancestors 'none'") && !p.csp.contains("unsafe-inline"), "{}", p.csp);
        // AUTH-24 #7: an injected block is not whitelisted: the page is refused.
        let injected = render(&chrome(), Body { main: "<script>alert(document.cookie)</script>".into(), ..Body::default() });
        assert!(!injected.intact());
        let evil = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(b"alert(document.cookie)"));
        assert!(!injected.csp.contains(&evil));
        assert_eq!(respond(StatusCode::OK, injected).status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!render(&chrome(), Body { error: Some("<SCRIPT>x</SCRIPT>".into()), ..Body::default() }).html.contains("<SCRIPT>"), "escaped");
        assert!(Page::plain("<p>hi</p>".into()).csp.contains("script-src 'none'"));
    }

    #[test]
    fn the_page_is_filled_in_one_pass_and_escaped_once() {
        let p = render(&chrome(), Body { heading: heading("Sign in", "to continue"), error: Some("<b>no</b> {{TITLE}}".into()), main: "{{APP}}".into(), ..Body::default() });
        assert!(p.html.contains("<title>Sign in · toy &lt;test&gt;</title>"));
        assert!(p.html.contains("&lt;b&gt;no&lt;/b&gt; {{TITLE}}"), "a value is never searched for slots");
        assert!(p.html.contains(">{{APP}}<") || p.html.contains("{{APP}}"), "the body's text stays as given");
        assert!(p.html.contains("a&amp;b") && !p.html.contains("a&amp;amp;b"), "escaped once");
        for ph in ["{{TITLE}}</title>", "{{HEADING}}", "{{MOTTO}}", "{{BUTTONS}}"] {
            assert!(!p.html.contains(ph), "{ph}");
        }
    }

    #[test]
    fn the_template_s_blocks_hold_only_configuration_slots() {
        for b in blocks(TEMPLATE, "<script>", "</script>").into_iter().chain(blocks(TEMPLATE, "<style>", "</style>")) {
            let mut rest = b.as_str();
            while let Some(i) = rest.find("{{") {
                let j = rest[i..].find("}}").unwrap();
                let slot = &rest[i + 2..i + j];
                assert!(["APP", "DEV_COOKIE"].contains(&slot), "{slot}");
                rest = &rest[i + j..];
            }
        }
    }
}
