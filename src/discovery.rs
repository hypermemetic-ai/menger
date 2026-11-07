//! V2-64: MCP discovery from a domain — an agent that knows only the host
//! finds `/mcp` by the standard (SEP-2127, MCP Server Cards, and the AI
//! Catalog it leans on; the extension's `docs/discovery.md`):
//!
//! - `GET /.well-known/ai-catalog.json` ([`CATALOG_TYPE`]): one entry, this
//!   service's Server Card;
//! - `GET /mcp/server-card` ([`CARD_TYPE`]): the card — name, version,
//!   description, the `/mcp` remote; never the tools (the protocol lists
//!   those once connected);
//! - `GET /llms.txt`: the same in prose, for an agent that reads text.
//!
//! Public — no session, no credential, nothing in them the README does not
//! say — with CORS `*`, an hour's cache and an `ETag` that `If-None-Match`
//! answers `304`. main.rs mounts them with `/mcp` (on the shell, after the
//! session check), so they exist exactly when it does; the card's path is
//! under the bridge's `nest_service("/mcp")`, and this route, being exact,
//! is matched before it.

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// The AI Catalog's well-known URI (RFC 8615).
pub const CATALOG_PATH: &str = "/.well-known/ai-catalog.json";
/// `<streamable-http-url>/server-card`, the location MCP reserves.
pub const CARD_PATH: &str = "/mcp/server-card";
/// The llms.txt convention: Markdown for a language model to read.
pub const LLMS_PATH: &str = "/llms.txt";

pub const CATALOG_TYPE: &str = "application/ai-catalog+json";
pub const CARD_TYPE: &str = "application/mcp-server-card+json";
pub const LLMS_TYPE: &str = "text/plain; charset=utf-8";
/// The one schema URI a Server Card may name (`schema.ts`'s pattern).
pub const CARD_SCHEMA: &str = "https://static.modelcontextprotocol.io/schemas/v1/server-card.schema.json";

/// At most 100 characters (the card's `description`).
pub const DESCRIPTION: &str = "Tickets, plans and documents: a plan is a program whose leaves are prose.";
pub const WEBSITE: &str = "https://menger.sh";
pub const REPOSITORY: &str = "https://github.com/hypermemetic-ai/menger";

/// The three documents for the service at `public`
/// (`https://app.menger.sh`): the catalog, the card, llms.txt.
pub struct Documents {
    pub catalog: String,
    pub card: String,
    pub llms: String,
}

/// `oauth`: `/mcp` is an OAuth 2.1 protected resource
/// (`--mcp-authorization-server`), which llms.txt says how to use.
pub fn documents(public: &str, oauth: bool) -> Documents {
    let public = public.trim_end_matches('/');
    let host = host_of(public);
    let mcp = format!("{public}/mcp");
    let card_url = format!("{public}{CARD_PATH}");
    let card = serde_json::json!({
        "$schema": CARD_SCHEMA,
        "name": server_name(&host),
        "version": env!("CARGO_PKG_VERSION"),
        "title": "menger",
        "description": DESCRIPTION,
        "websiteUrl": WEBSITE,
        "repository": { "url": REPOSITORY, "source": "github" },
        "remotes": [{ "type": "streamable-http", "url": mcp }],
    });
    let catalog = serde_json::json!({
        "specVersion": "1.0",
        "host": { "displayName": "menger", "identifier": host, "documentationUrl": REPOSITORY },
        "entries": [{
            "identifier": catalog_identifier(&host),
            "displayName": "menger",
            "type": CARD_TYPE,
            "url": card_url,
        }],
    });
    let sign_in = if oauth {
        format!(
            "Sign-in is OAuth 2.1: a request without a token is answered 401 naming \
             {public}/.well-known/oauth-protected-resource/mcp, and an MCP client signs \
             in at the authorization server it names, in the browser, with the same \
             account as the menger app."
        )
    } else {
        "Sign-in is a Bearer token: an agent's session (synapse's `_login`), or a token \
         one of the server's sign-in providers admits."
            .to_string()
    };
    let llms = format!(
        "# menger\n\n\
         > {DESCRIPTION}\n\n\
         menger is an MCP server (Streamable HTTP) at {mcp}. {sign_in} Every menger \
         method the caller may call is a tool, and each call runs as the caller.\n\n\
         ## Connect\n\n\
         - [MCP endpoint]({mcp}): add it as a remote HTTP server, e.g. \
         `claude mcp add --transport http menger {mcp}`\n\
         - [Server card]({card_url}): SEP-2127, `{CARD_TYPE}`\n\
         - [AI catalog]({public}{CATALOG_PATH}): `{CATALOG_TYPE}`\n\
         - [Source]({REPOSITORY})\n"
    );
    Documents { catalog: pretty(&catalog), card: pretty(&card), llms }
}

/// The routes serving [`documents`].
pub fn router(public: &str, oauth: bool) -> Router {
    let d = documents(public, oauth);
    Router::new()
        .route(CATALOG_PATH, served(Doc::new(d.catalog, CATALOG_TYPE)))
        .route(CARD_PATH, served(Doc::new(d.card, CARD_TYPE)))
        .route(LLMS_PATH, served(Doc::new(d.llms, LLMS_TYPE)))
}

/// The host of `public`, lower case and without its port (an `urn:air`
/// segment holds no colon); the URL itself when it has none.
fn host_of(public: &str) -> String {
    url::Url::parse(public)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_ascii_lowercase()))
        .unwrap_or_else(|| public.to_ascii_lowercase())
}

/// The card's `name`: the host in reverse-DNS order, `/menger`
/// (`app.menger.sh` → `sh.menger.app/menger`), each character outside
/// `[a-zA-Z0-9.-]` a `-` — so each deployment names itself.
fn server_name(host: &str) -> String {
    let ns: Vec<String> = host
        .split('.')
        .rev()
        .map(|label| label.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect())
        .collect();
    format!("{}/menger", ns.join("."))
}

/// The entry's identifier, `urn:air:{publisher}:mcp:menger` — the host as
/// the publisher, as the AI Catalog recommends for one that may be listed
/// elsewhere (menger.sh's catalog lists app.menger.sh's card under it).
pub fn catalog_identifier(host: &str) -> String {
    format!("urn:air:{}:mcp:menger", host.replace(':', "-"))
}

fn pretty(v: &serde_json::Value) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("a JSON value");
    s.push('\n');
    s
}

/// A document with its type and entity tag, fixed at start.
struct Doc {
    body: String,
    kind: &'static str,
    etag: HeaderValue,
}

impl Doc {
    fn new(body: String, kind: &'static str) -> Arc<Doc> {
        let digest = Sha256::digest(body.as_bytes());
        let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
        let etag = HeaderValue::from_str(&format!("\"{hex}\"")).expect("hex is a header value");
        Arc::new(Doc { body, kind, etag })
    }

    /// `If-None-Match` names this version (or `*`).
    fn matches(&self, headers: &HeaderMap) -> bool {
        let tag = self.etag.to_str().unwrap_or_default();
        headers
            .get_all(header::IF_NONE_MATCH)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .map(|t| t.trim().trim_start_matches("W/"))
            .any(|t| t == "*" || t == tag)
    }
}

/// GET (and HEAD) answer the document, or 304; OPTIONS the preflight.
fn served(doc: Arc<Doc>) -> axum::routing::MethodRouter {
    let d = doc.clone();
    get(move |headers: HeaderMap| {
        let doc = d.clone();
        async move {
            if doc.matches(&headers) {
                return (StatusCode::NOT_MODIFIED, common(&doc)).into_response();
            }
            let mut h = common(&doc);
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static(doc.kind));
            (StatusCode::OK, h, doc.body.clone()).into_response()
        }
    })
    .options(move || {
        let doc = doc.clone();
        async move { (StatusCode::NO_CONTENT, common(&doc)).into_response() }
    })
}

/// What every answer carries: CORS for a browser-based client, an hour's
/// cache, the entity tag (`docs/discovery.md`, "CORS Requirements" and
/// "Caching").
fn common(doc: &Doc) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET"));
    h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("Content-Type, If-None-Match"));
    h.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("ETag"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=3600"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::ETAG, doc.etag.clone());
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn json(s: &str) -> Value {
        serde_json::from_str(s).expect("JSON")
    }

    #[test]
    fn the_card_is_what_the_schema_asks() {
        let d = documents("https://app.menger.sh/", true);
        let card = json(&d.card);
        assert_eq!(card["$schema"], CARD_SCHEMA);
        let name = card["name"].as_str().unwrap();
        assert_eq!(name, "sh.menger.app/menger");
        let (ns, n) = name.split_once('/').unwrap();
        assert!(!n.contains('/') && ns.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'), "{name}");
        assert_eq!(card["version"], env!("CARGO_PKG_VERSION"));
        let desc = card["description"].as_str().unwrap();
        assert!(!desc.is_empty() && desc.chars().count() <= 100, "{desc}");
        assert_eq!(card["remotes"], serde_json::json!([{ "type": "streamable-http", "url": "https://app.menger.sh/mcp" }]));
        assert!(card.get("tools").is_none(), "a card lists no primitives");
    }

    #[test]
    fn the_catalog_lists_the_card() {
        let d = documents("https://app-test.menger.sh", true);
        let cat = json(&d.catalog);
        assert_eq!(cat["specVersion"], "1.0");
        assert_eq!(cat["host"]["displayName"], "menger");
        let entries = cat["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["identifier"], "urn:air:app-test.menger.sh:mcp:menger");
        assert_eq!(entries[0]["type"], CARD_TYPE);
        assert_eq!(entries[0]["url"], "https://app-test.menger.sh/mcp/server-card");
    }

    /// menger.sh's static catalog (landing/, the edge's) lists prod's card
    /// exactly as prod's own catalog does — the identifier preserved, as the
    /// AI Catalog asks of a catalog that lists another's entry.
    #[test]
    fn menger_sh_lists_prods_card_as_prod_does() {
        let landing = json(include_str!("../landing/.well-known/ai-catalog.json"));
        let prod = json(&documents("https://app.menger.sh", true).catalog);
        assert_eq!(landing["specVersion"], "1.0");
        assert_eq!(landing["entries"], prod["entries"]);
        let llms = include_str!("../landing/llms.txt");
        assert!(llms.starts_with("# menger\n") && llms.contains(&format!("> {DESCRIPTION}")));
        assert!(llms.contains("https://app.menger.sh/mcp/server-card"));
    }

    #[test]
    fn a_port_or_an_address_names_no_colon() {
        let d = documents("http://127.0.0.1:4456", false);
        assert_eq!(json(&d.card)["name"], "1.0.0.127/menger");
        assert_eq!(json(&d.catalog)["entries"][0]["identifier"], "urn:air:127.0.0.1:mcp:menger");
        assert_eq!(json(&d.card)["remotes"][0]["url"], "http://127.0.0.1:4456/mcp");
        assert_eq!(server_name("::1"), "--1/menger");
    }

    #[test]
    fn llms_txt_says_how_to_connect() {
        let oauth = documents("https://app.menger.sh", true).llms;
        assert!(oauth.starts_with("# menger\n"));
        assert!(oauth.contains("https://app.menger.sh/mcp"));
        assert!(oauth.contains("/.well-known/oauth-protected-resource/mcp"));
        assert!(oauth.contains("https://app.menger.sh/mcp/server-card"));
        let bearer = documents("https://menger.example", false).llms;
        assert!(!bearer.contains("oauth-protected-resource") && bearer.contains("Bearer"));
    }

    /// The routes beside a stand-in for the MCP bridge (`nest_service` at
    /// `/mcp` answering 401, as the real one does without a token): the
    /// exact card route wins, `/mcp` is still the bridge's, and the answers
    /// carry CORS, caching and an entity tag `If-None-Match` honours.
    #[tokio::test]
    async fn served_beside_the_bridge() {
        let bridge = Router::new().nest_service(
            "/mcp",
            axum::routing::any(|| async { StatusCode::UNAUTHORIZED }),
        );
        let app = router("http://127.0.0.1:1", true).merge(bridge);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let c = reqwest::Client::new();
        let url = |p: &str| format!("http://{at}{p}");

        let card = c.get(url(CARD_PATH)).header("accept", CARD_TYPE).send().await.unwrap();
        assert_eq!(card.status(), 200);
        let h = card.headers().clone();
        assert_eq!(h["content-type"], CARD_TYPE);
        assert_eq!(h["access-control-allow-origin"], "*");
        assert_eq!(h["cache-control"], "public, max-age=3600");
        let etag = h["etag"].to_str().unwrap().to_string();
        assert_eq!(json(&card.text().await.unwrap())["remotes"][0]["url"], "http://127.0.0.1:1/mcp");

        let again = c.get(url(CARD_PATH)).header("if-none-match", &etag).send().await.unwrap();
        assert_eq!(again.status(), 304);
        assert_eq!(again.headers()["etag"], etag.as_str());

        let cat = c.get(url(CATALOG_PATH)).send().await.unwrap();
        assert_eq!((cat.status().as_u16(), cat.headers()["content-type"].to_str().unwrap()), (200, CATALOG_TYPE));
        let llms = c.get(url(LLMS_PATH)).send().await.unwrap();
        assert_eq!((llms.status().as_u16(), llms.headers()["content-type"].to_str().unwrap()), (200, LLMS_TYPE));
        let pre = c.request(reqwest::Method::OPTIONS, url(CARD_PATH)).send().await.unwrap();
        assert_eq!(pre.status(), 204);
        assert_eq!(pre.headers()["access-control-allow-headers"], "Content-Type, If-None-Match");

        assert_eq!(c.post(url("/mcp")).send().await.unwrap().status(), 401, "/mcp is the bridge's");
        assert_eq!(c.get(url("/mcp/other")).send().await.unwrap().status(), 401, "and so is the rest under it");
    }
}
