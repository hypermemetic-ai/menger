//! MENGER-14: **links in what people write** — a ticket body, a board note,
//! a comment — are followed by the app, never by the browser on its own.
//!
//! Every such link reaches [`follow`], from one of two places:
//!
//! - **a framed body** (src/frame.rs) — the frame's SDK takes the click (or
//!   Enter on a focused link, which the browser turns into a click; or a
//!   middle click), prevents it, and posts `menger:link { href }` — the
//!   `href` as written — over the frame's channel. The sandbox is exactly
//!   as strict as before (no `allow-same-origin`, nothing added): the click
//!   is handed over, nothing is loosened. The host trusts nothing of it but
//!   the string, and decides here;
//! - **the app's own page** — one delegated listener ([`install`]) takes a
//!   click on a link inside `.md` (an unframed body or note, a help text) or
//!   `[data-links]` (a comment's body, linkified by [`Linkified`]).
//!
//! **What a link is** ([`classify`], pure) — read the way the browser
//! reads it (the `url` crate is WHATWG's parser: tabs and newlines inside
//! dropped, case folded, backslashes as slashes, IDN hosts as punycode):
//!
//! | the link | is |
//! | -- | -- |
//! | `#/…` | an app route — the app goes there ([`Follow::Route`]) |
//! | `#id` | an anchor in the same text — scrolled to (a footnote) |
//! | `http(s)` on this page's origin, the app's path | an app route (its fragment) |
//! | `http(s)` on this page's origin, another path (`/vanilla`) | menger's own page — a new tab, no question |
//! | `http(s)` anywhere else | **external** — the leave dialog |
//! | `mailto:` | **external** — the dialog, always (no "don't ask again") |
//! | `javascript:`, `data:`, `file:`, `vbscript:`, any other scheme, or unreadable | **refused** — a short notice, nothing opened |
//!
//! **The leave dialog** ([`LeaveDialog`]): "This link leaves menger", the
//! ticket it came from, the full URL with its **registrable domain**
//! emphasised ([`registrable`] — the Public Suffix List, vendored at
//! `frontend/vendor/psl/`), an IDN host shown decoded *and* as punycode when
//! they differ, a `user@` before the host called out, and Open (a new tab,
//! `noopener,noreferrer`) / Cancel. Enter opens, Esc cancels, Tab stays in
//! the dialog, and focus goes back where it was. "Don't ask again for
//! <domain>" keeps the domain in this browser **per signed-in user**
//! ([`Trust`], localStorage `menger.links` — the app's settings are local
//! today, `menger.settings`), listed and revocable in `#/settings`. A
//! trusted domain opens at once only inside a viewer's gesture on the very
//! frame it came from (src/gesture.rs) — a body's script that posts a link
//! on its own gets the dialog.
//!
//! What decides — the classification, the domain, the dialog's words, the
//! trust record, what a click does — is pure and pinned by native tests;
//! the rest is DOM.

use std::cell::RefCell;
use std::collections::BTreeMap;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use url::{Host, Position, Url};

// ---------------------------------------------------------------------
// The registrable domain (the Public Suffix List)

/// libpsl's compiled list (vendor/psl/README.md).
const PSL: &[u8] = include_bytes!("../vendor/psl/public_suffix_list.dafsa");
/// Its header: 16 bytes, version 0 — ASCII (punycode) keys.
const PSL_HEADER: &[u8] = b".DAFSA@PSL_0   \n";
const PSL_EXCEPTION: u8 = 1;
const PSL_WILDCARD: u8 = 2;

fn psl_graph() -> &'static [u8] {
    PSL.strip_prefix(PSL_HEADER).unwrap_or(&[])
}

/// Chromium's `LookupStringInFixedSet` (libpsl's too): the flags `key` has
/// in the graph, or `None`. Every read is bounds-checked — a malformed graph
/// answers `None`, never panics.
fn dafsa_lookup(graph: &[u8], key: &[u8]) -> Option<u8> {
    if key.is_empty() {
        return None;
    }
    let at = |i: usize| graph.get(i).copied();
    let mut pos = Some(0usize);
    let mut offset = 0usize;
    let mut k = 0usize;
    while let Some(p) = pos {
        let b = at(p)?;
        let consumed = match b & 0x60 {
            0x60 => {
                offset += (usize::from(b & 0x1F) << 16) | (usize::from(at(p + 1)?) << 8) | usize::from(at(p + 2)?);
                3
            }
            0x40 => {
                offset += (usize::from(b & 0x1F) << 8) | usize::from(at(p + 1)?);
                2
            }
            _ => {
                offset += usize::from(b & 0x3F);
                1
            }
        };
        pos = if b & 0x80 != 0 { None } else { Some(p + consumed) };
        let mut consumed_char = false;
        if k != key.len() && at(offset)? & 0x80 == 0 {
            if at(offset)? != key[k] {
                continue;
            }
            consumed_char = true;
            offset += 1;
            k += 1;
            while at(offset)? & 0x80 == 0 && k != key.len() {
                if at(offset)? != key[k] {
                    return None;
                }
                k += 1;
                offset += 1;
            }
        }
        if k == key.len() {
            let v = at(offset)?;
            if v & 0xE0 == 0x80 {
                return Some(v & 0x0F);
            }
            if consumed_char {
                return None;
            }
            continue;
        }
        if at(offset)? != (key[k] | 0x80) {
            if consumed_char {
                return None;
            }
            continue;
        }
        k += 1;
        offset += 1;
        pos = Some(offset);
    }
    None
}

/// The registrable domain of `host` (ASCII, lowercase — punycode for an IDN,
/// as the URL parser gives it): the public suffix and one label more,
/// by the PSL's algorithm (an exception rule beats a wildcard; the longest
/// rule wins; a name no rule covers has the implicit `*`). A host that is
/// itself a public suffix (`github.io`, `localhost`), and an IP address,
/// is its own domain.
pub(crate) fn registrable(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.starts_with('[') || host.parse::<std::net::Ipv4Addr>().is_ok() {
        return host;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.iter().any(|l| l.is_empty()) {
        return host;
    }
    let graph = psl_graph();
    // Where the public suffix starts, in labels — the implicit `*` rule
    // (the last label) unless the list says otherwise.
    let mut suffix_at = labels.len() - 1;
    for i in 0..labels.len() {
        let candidate = labels[i..].join(".");
        if !candidate.is_ascii() {
            continue;
        }
        let Some(flags) = dafsa_lookup(graph, candidate.as_bytes()) else { continue };
        suffix_at = if flags & PSL_EXCEPTION != 0 {
            i + 1
        } else if flags & PSL_WILDCARD != 0 {
            i.saturating_sub(1)
        } else {
            i
        };
        break;
    }
    if suffix_at == 0 {
        host
    } else {
        labels[suffix_at - 1..].join(".")
    }
}

/// An ASCII host as a person reads it: IDN labels decoded. `None` when it
/// reads the same (or does not decode).
pub(crate) fn decoded(host: &str) -> Option<String> {
    if !host.split('.').any(|l| l.len() > 4 && l.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("xn--"))) {
        return None;
    }
    let (unicode, ok) = idna::domain_to_unicode(host);
    (ok.is_ok() && unicode != host).then_some(unicode)
}

// ---------------------------------------------------------------------
// What a link is

/// What [`classify`] makes of a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Follow {
    /// An app route: the hash to set (`#/ticket/MENGER-14`).
    Route(String),
    /// An anchor in the same text (a footnote): the element's id.
    Fragment(String),
    /// A page of menger's own origin outside the app (`/vanilla`): its URL.
    Menger(String),
    /// Somewhere else: the leave dialog's business.
    External(Box<Dest>),
    /// Never followed: what the notice says.
    Refused(String),
}

/// A web page or an email.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DestKind {
    Web,
    Mail,
}

/// An external destination, as the dialog shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dest {
    pub(crate) kind: DestKind,
    /// The URL as the browser will open it (serialized: ASCII, punycode).
    pub(crate) url: String,
    /// The URL in three pieces for the dialog: before the domain, the
    /// registrable domain itself (emphasised), after it.
    pub(crate) parts: (String, String, String),
    /// Web: the host (ASCII); mail: the first recipient's domain.
    pub(crate) host: String,
    /// The registrable domain (ASCII) — what "don't ask again" trusts.
    pub(crate) domain: String,
    /// The host decoded, when it is an IDN that reads differently.
    pub(crate) host_unicode: Option<String>,
    /// The domain decoded, likewise.
    pub(crate) domain_unicode: Option<String>,
    /// Web: what stands before the host — `user` or `user:…` (a password is
    /// never shown) — when the URL carries one.
    pub(crate) userinfo: Option<String>,
    /// Mail: the recipients, decoded.
    pub(crate) to: Vec<String>,
}

/// The app's own paths: where `#/…` routes live.
fn is_app_path(path: &str, page_path: &str) -> bool {
    path == page_path || matches!(path, "/" | "/app" | "/app/" | "/app/index.html" | "/index.html")
}

/// An app route's characters (the router reads the rest): ids, tags,
/// shortcodes, a route's query and its percent-escapes.
fn route_ok(route: &str) -> bool {
    route.len() <= 2048 && route.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:=&,%+!~*/?@".contains(c))
}

/// `#/…` as a hash to set — or refused, when it holds what no route does.
fn route(hash: &str) -> Follow {
    match hash.strip_prefix("#/") {
        Some(rest) if route_ok(rest) => Follow::Route(format!("#/{rest}")),
        _ => Follow::Refused("That link names no page of menger, so it was not followed.".into()),
    }
}

/// What a scheme the app never follows is told.
pub(crate) fn refused_words(scheme: &str) -> String {
    match scheme {
        "javascript" | "vbscript" => format!("Not followed: {scheme}: links run code, and menger never runs them."),
        "data" | "blob" => format!("Not followed: {scheme}: links carry their own page, which menger never opens."),
        "file" => "Not followed: file: links reach into your own disk.".into(),
        "" => "Not followed: menger could not read that link.".into(),
        s => format!("Not followed: {s}: is not a link menger opens — only http, https and mailto."),
    }
}

/// What `raw` (an `href` as written) is, on the page at `page` (the app's
/// own URL, `location.href`).
pub(crate) fn classify(raw: &str, page: &str) -> Follow {
    // The browser strips leading and trailing C0 controls and spaces.
    let raw = raw.trim_matches(|c: char| c <= ' ');
    if raw.len() > 8192 {
        return Follow::Refused("Not followed: that link is too long to be one.".into());
    }
    if raw.is_empty() {
        return Follow::Refused("Not followed: the link is empty.".into());
    }
    if raw.starts_with("#/") {
        return route(raw);
    }
    if let Some(id) = raw.strip_prefix('#') {
        let id = percent_encoding::percent_decode_str(id).decode_utf8_lossy().into_owned();
        return Follow::Fragment(id);
    }
    let Ok(page) = Url::parse(page) else { return Follow::Refused(refused_words("")) };
    let Ok(url) = Url::options().base_url(Some(&page)).parse(raw) else {
        return Follow::Refused(refused_words(""));
    };
    match url.scheme() {
        "http" | "https" => {
            if url.origin() == page.origin() && url.username().is_empty() && url.password().is_none() {
                if !is_app_path(url.path(), page.path()) || url.query().is_some() {
                    return Follow::Menger(url.to_string());
                }
                return match url.fragment() {
                    None | Some("") | Some("/") => Follow::Route("#/".into()),
                    Some(f) if f.starts_with('/') => route(&format!("#{f}")),
                    Some(f) => Follow::Fragment(percent_encoding::percent_decode_str(f).decode_utf8_lossy().into_owned()),
                };
            }
            web(&url).map_or_else(|| Follow::Refused(refused_words("")), |d| Follow::External(Box::new(d)))
        }
        "mailto" => Follow::External(Box::new(mail(&url))),
        other => Follow::Refused(refused_words(other)),
    }
}

/// A web destination's dialog fields.
fn web(url: &Url) -> Option<Dest> {
    let host = match url.host()? {
        Host::Domain(d) => d.to_string(),
        Host::Ipv4(a) => a.to_string(),
        Host::Ipv6(a) => format!("[{a}]"),
    };
    let domain = registrable(&host);
    let s = url.as_str();
    let (before, host_part, after) = (&url[..Position::BeforeHost], &url[Position::BeforeHost..Position::AfterHost], &url[Position::AfterHost..]);
    let sub = host_part.strip_suffix(domain.as_str()).unwrap_or("");
    let shown_domain = if sub.len() + domain.len() == host_part.len() { domain.clone() } else { host_part.to_string() };
    let userinfo = (!url.username().is_empty() || url.password().is_some()).then(|| {
        let user = percent_encoding::percent_decode_str(url.username()).decode_utf8_lossy().into_owned();
        if url.password().is_some() {
            format!("{user}:…")
        } else {
            user
        }
    });
    Some(Dest {
        kind: DestKind::Web,
        url: s.to_string(),
        parts: (format!("{before}{sub}"), shown_domain, after.to_string()),
        host_unicode: decoded(&host),
        domain_unicode: decoded(&domain),
        host,
        domain,
        userinfo,
        to: Vec::new(),
    })
}

/// A `mailto:`'s dialog fields: its recipients, and the first one's domain.
fn mail(url: &Url) -> Dest {
    let s = url.as_str().to_string();
    let to: Vec<String> = percent_encoding::percent_decode_str(url.path())
        .decode_utf8_lossy()
        .split(',')
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    let host = to
        .first()
        .and_then(|a| a.rsplit_once('@'))
        .map(|(_, h)| idna::domain_to_ascii(h).unwrap_or_else(|_| h.to_ascii_lowercase()))
        .unwrap_or_default();
    let domain = if host.is_empty() { String::new() } else { registrable(&host) };
    // The domain emphasised where the first recipient's host stands.
    let parts = (!host.is_empty())
        .then(|| s.to_ascii_lowercase().find(&format!("@{host}")))
        .flatten()
        .map(|i| {
            let end = i + 1 + host.len();
            let start = end - domain.len();
            (s[..start].to_string(), s[start..end].to_string(), s[end..].to_string())
        })
        .unwrap_or_else(|| (s.clone(), String::new(), String::new()));
    Dest {
        kind: DestKind::Mail,
        url: s,
        parts,
        host_unicode: decoded(&host),
        domain_unicode: decoded(&domain),
        host,
        domain,
        userinfo: None,
        to,
    }
}

// ---------------------------------------------------------------------
// Bare URLs (autolinks)

/// Where `text` holds a bare `http://` or `https://` URL — byte ranges, in
/// order — as GitHub's autolinks find them: starting a word, running to
/// whitespace or `<`, `>`, `"`, `` ` ``, and losing trailing punctuation
/// (`.`, `,`, `:`, `;`, `!`, `?`, `*`, `_`, `~`, `'`) and a `)` or `]` it
/// does not open.
pub(crate) fn bare_urls(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find("http") {
        let start = from + found;
        from = start + 4;
        let rest = &lower[start..];
        let scheme = if rest.starts_with("https://") {
            8
        } else if rest.starts_with("http://") {
            7
        } else {
            continue;
        };
        if text[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '/' || c == '@' || c == '.') {
            continue;
        }
        let run = text[start..]
            .char_indices()
            .find(|(_, c)| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '`'))
            .map_or(text.len(), |(i, _)| start + i);
        let mut end = run;
        loop {
            let s = &text[start..end];
            let Some(last) = s.chars().next_back() else { break };
            let trim = match last {
                '.' | ',' | ':' | ';' | '!' | '?' | '*' | '_' | '~' | '\'' => true,
                ')' => s.matches(')').count() > s.matches('(').count(),
                ']' => s.matches(']').count() > s.matches('[').count(),
                _ => false,
            };
            if !trim {
                break;
            }
            end -= last.len_utf8();
        }
        if text[start + scheme..end].chars().next().is_some_and(char::is_alphanumeric) {
            out.push((start, end));
            from = end;
        }
    }
    out
}

/// A piece of linkified text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(String),
    Link(String),
}

/// `text` as text and the bare URLs in it.
pub(crate) fn pieces(text: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut at = 0;
    for (s, e) in bare_urls(text) {
        if s > at {
            out.push(Piece::Text(text[at..s].to_string()));
        }
        out.push(Piece::Link(text[s..e].to_string()));
        at = e;
    }
    if at < text.len() {
        out.push(Piece::Text(text[at..].to_string()));
    }
    out
}

// ---------------------------------------------------------------------
// Trust ("don't ask again")

/// The localStorage key of the trust record.
pub(crate) const KEY: &str = "menger.links";
const VERSION: u32 = 1;
/// How many domains one user keeps.
pub(crate) const TRUST_MAX: usize = 500;

/// "Don't ask again", per user: `{"version":1,"trusted":{"<principal>":
/// ["example.com"]}}` — the principal `/login/me` names, or `""` on a
/// menger with no login. A record that is missing, corrupt or of another
/// version reads as empty; a domain that is not one is dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Trust {
    by_user: BTreeMap<String, Vec<String>>,
}

#[derive(Serialize, Deserialize)]
struct TrustRecord {
    version: u32,
    #[serde(default)]
    trusted: BTreeMap<String, Vec<String>>,
}

/// A domain as the record keeps it: ASCII, lowercase, letters, digits, `-`,
/// `.` (an IPv4 address too); never a public suffix alone, never empty.
fn domain_ok(d: &str) -> bool {
    !d.is_empty()
        && d.len() <= 253
        && d.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        && !d.starts_with('.')
        && !d.ends_with('.')
        && !d.contains("..")
}

impl Trust {
    pub(crate) fn decode(raw: Option<&str>) -> Self {
        let Some(record) = raw.and_then(|r| serde_json::from_str::<TrustRecord>(r).ok()) else {
            return Self::default();
        };
        if record.version != VERSION {
            return Self::default();
        }
        let by_user = record
            .trusted
            .into_iter()
            .map(|(user, list)| {
                let mut list: Vec<String> = list.into_iter().filter(|d| domain_ok(d)).collect();
                list.sort();
                list.dedup();
                list.truncate(TRUST_MAX);
                (user, list)
            })
            .filter(|(_, list)| !list.is_empty())
            .collect();
        Trust { by_user }
    }

    pub(crate) fn encode(&self) -> String {
        serde_json::to_string(&TrustRecord { version: VERSION, trusted: self.by_user.clone() }).expect("strings serialize")
    }

    /// The domains `user` trusts, sorted.
    pub(crate) fn domains(&self, user: &str) -> Vec<String> {
        self.by_user.get(user).cloned().unwrap_or_default()
    }

    pub(crate) fn with(mut self, user: &str, domain: &str) -> Self {
        if domain_ok(domain) {
            let list = self.by_user.entry(user.to_string()).or_default();
            if !list.iter().any(|d| d == domain) && list.len() < TRUST_MAX {
                list.push(domain.to_string());
                list.sort();
            }
        }
        self
    }

    pub(crate) fn without(mut self, user: &str, domain: &str) -> Self {
        if let Some(list) = self.by_user.get_mut(user) {
            list.retain(|d| d != domain);
            if list.is_empty() {
                self.by_user.remove(user);
            }
        }
        self
    }
}

// ---------------------------------------------------------------------
// What a click does

/// What the app does with a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    /// Set the hash.
    Route(String),
    /// Scroll to the element with this id, in the text the link is in.
    Scroll(String),
    /// Open a new tab (`noopener,noreferrer`).
    Tab(String),
    /// Ask first.
    Ask(Box<Dest>),
    /// Show a notice; open nothing.
    Notice(String),
    /// Nothing at all.
    Nothing,
}

/// What following `follow` does, for a user who trusts `trusted`, with or
/// without a viewer's gesture under way on the link's own frame (always
/// with one in the app's own page): a trusted web domain opens at once only
/// inside one — else it asks, like any other; a page of menger's own opens
/// only inside one. A `mailto:` always asks.
pub(crate) fn decide(follow: Follow, trusted: &[String], gesture: bool) -> Act {
    match follow {
        Follow::Route(h) => Act::Route(h),
        Follow::Fragment(id) => Act::Scroll(id),
        Follow::Menger(url) if gesture => Act::Tab(url),
        Follow::Menger(_) => Act::Nothing,
        Follow::External(d) if d.kind == DestKind::Web && gesture && trusted.contains(&d.domain) => Act::Tab(d.url),
        Follow::External(d) => Act::Ask(d),
        Follow::Refused(why) => Act::Notice(why),
    }
}

/// The ticket a `#/ticket/<ID>[?…]` route names.
pub(crate) fn route_ticket(hash: &str) -> Option<String> {
    let id = hash.strip_prefix("#/ticket/")?.split(['?', '/']).next()?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

/// The domain as the dialog names it: an IDN both decoded and as the
/// punycode the browser opens — never the look-alike letters alone.
pub(crate) fn shown_domain(dest: &Dest) -> String {
    match &dest.domain_unicode {
        Some(u) => format!("{u} ({})", dest.domain),
        None => dest.domain.clone(),
    }
}

/// Where a link was clicked, for the dialog's "From" line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct From {
    /// The ticket whose body, note or comment holds it.
    pub(crate) ticket: Option<String>,
    /// What held it: `body`, `board note`, `comment`, `help`.
    pub(crate) place: &'static str,
}

/// The dialog's "From" words (without the ticket, which is a link).
pub(crate) fn from_words(from: &From) -> String {
    match (&from.ticket, from.place) {
        (Some(_), "comment") => "From a comment on".into(),
        (Some(_), "board note") => "From the board's notes,".into(),
        (Some(_), _) => "From".into(),
        (None, "comment") => "From a comment".into(),
        (None, "board note") => "From the board's notes".into(),
        (None, "draft") => "From the draft being edited".into(),
        (None, _) => "From text in menger".into(),
    }
}

// ---------------------------------------------------------------------
// The browser side

/// A dialog up: the destination, and where the link was.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Asking {
    pub(crate) dest: Dest,
    pub(crate) from: From,
}

thread_local! {
    static ASKING: ArcRwSignal<Option<Asking>> = ArcRwSignal::new(None);
    static NOTICE: ArcRwSignal<Option<(u32, String)>> = ArcRwSignal::new(None);
    /// The current user's trusted domains, as the settings page lists them.
    static TRUSTED: ArcRwSignal<Vec<String>> = ArcRwSignal::new(Vec::new());
    /// Where focus was when the dialog opened (the link, or its frame).
    static RESTORE: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
    static NOTICES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn asking() -> ArcRwSignal<Option<Asking>> {
    ASKING.with(Clone::clone)
}

fn notice() -> ArcRwSignal<Option<(u32, String)>> {
    NOTICE.with(Clone::clone)
}

/// The current user's trusted domains — the settings page's list.
pub(crate) fn trusted() -> ArcRwSignal<Vec<String>> {
    TRUSTED.with(Clone::clone)
}

/// Who "don't ask again" is kept for: the signed-in principal, or `""`.
fn user() -> String {
    crate::signin::account().get_untracked().me.map(|m| m.principal).unwrap_or_default()
}

fn storage() -> Option<web_sys::Storage> {
    crate::dom::window().local_storage().ok().flatten()
}

fn load_trust() -> Trust {
    Trust::decode(storage().and_then(|s| s.get_item(KEY).ok().flatten()).as_deref())
}

fn store_trust(t: &Trust) -> bool {
    storage().is_some_and(|s| s.set_item(KEY, &t.encode()).is_ok())
}

/// Reads the record into [`trusted`] (the user may have changed).
pub(crate) fn refresh_trusted() {
    trusted().set(load_trust().domains(&user()));
}

/// Keeps `domain` for this user ("don't ask again"). Whether it was kept.
fn trust(domain: &str) -> bool {
    let next = load_trust().with(&user(), domain);
    let ok = store_trust(&next);
    trusted().set(next.domains(&user()));
    ok
}

/// Forgets `domain` for this user (settings' "ask again").
pub(crate) fn forget(domain: &str) {
    let next = load_trust().without(&user(), domain);
    let _ = store_trust(&next);
    trusted().set(next.domains(&user()));
}

/// A new tab, with neither opener nor referrer.
fn open_tab(url: &str) {
    let _ = crate::dom::window().open_with_url_and_target_and_features(url, "_blank", "noopener,noreferrer");
}

fn show_notice(text: String) {
    let n = NOTICES.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    notice().set(Some((n, text)));
    wasm_bindgen_futures::spawn_local(async move {
        crate::dom::sleep_ms(6000).await;
        let sig = notice();
        if sig.with_untracked(|v| v.as_ref().is_some_and(|(m, _)| *m == n)) {
            sig.set(None);
        }
    });
}

/// Follows `href` (as written), clicked in `from`: `gesture` — a viewer's
/// gesture is under way on the link's own frame (always true in the app's
/// own page); `holder` — the element the link's text is in (for an anchor
/// in it); `restore` — where focus goes back after the dialog.
pub(crate) fn follow(href: &str, from: From, gesture: bool, holder: Option<web_sys::Element>, restore: Option<web_sys::HtmlElement>) {
    let w = crate::dom::window();
    let page = w.location().href().unwrap_or_default();
    match decide(classify(href, &page), &load_trust().domains(&user()), gesture) {
        Act::Route(hash) => {
            if w.location().hash().unwrap_or_default() != hash {
                let _ = w.location().set_hash(&hash);
            }
        }
        Act::Scroll(id) => {
            let Some(holder) = holder else { return };
            let Ok(list) = holder.query_selector_all("[id]") else { return };
            if let Some(el) = (0..list.length())
                .filter_map(|i| list.get(i).and_then(|n| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(n).ok()))
                .find(|el| el.id() == id)
            {
                el.scroll_into_view();
            }
        }
        Act::Tab(url) => open_tab(&url),
        Act::Ask(dest) => {
            RESTORE.with(|r| *r.borrow_mut() = restore);
            asking().set(Some(Asking { dest: *dest, from }));
        }
        Act::Notice(text) => show_notice(text),
        Act::Nothing => {}
    }
}

/// The app's own links: a click (or a middle click) on `a[href]` inside
/// `.md` or `[data-links]` — what people wrote, painted in the app — is
/// [`follow`]ed instead. A click a handler already took (a wave reference
/// revealing its card) is left alone. Called once by the App.
pub(crate) fn install() {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let Some(doc) = crate::dom::window().document() else { return };
    let cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
        if ev.default_prevented() {
            return;
        }
        let middle = ev.type_() == "auxclick";
        if (middle && ev.button() != 1) || (!middle && ev.button() != 0) {
            return;
        }
        let Some(a) = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("a[href]").ok().flatten())
        else {
            return;
        };
        let Some(holder) = a.closest(".md, [data-links]").ok().flatten() else { return };
        let href = a.get_attribute("href").unwrap_or_default();
        ev.prevent_default();
        let ticket = holder
            .closest("[data-ticket]")
            .ok()
            .flatten()
            .and_then(|el| el.get_attribute("data-ticket"))
            .filter(|t| !t.is_empty());
        let place = match holder.get_attribute("data-links").as_deref() {
            Some("comment") => "comment",
            _ if holder.closest("#waves").ok().flatten().is_some() => "board note",
            _ => "body",
        };
        // An unframed body on a ticket's page is that ticket's.
        let ticket = ticket.or_else(|| {
            (place == "body").then(|| route_ticket(&crate::dom::window().location().hash().unwrap_or_default())).flatten()
        });
        let restore = a.dyn_into::<web_sys::HtmlElement>().ok();
        follow(&href, From { ticket, place }, true, Some(holder), restore);
    });
    let _ = doc.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
    let _ = doc.add_event_listener_with_callback("auxclick", cb.as_ref().unchecked_ref());
    cb.forget();
    refresh_trusted();
}

fn close() {
    asking().set(None);
    if let Some(el) = RESTORE.with(|r| r.borrow_mut().take()) {
        let _ = el.focus();
    }
}

/// Tab and Shift+Tab stay inside the dialog.
fn trap(shift: bool) {
    use wasm_bindgen::JsCast;
    let Some(doc) = crate::dom::window().document() else { return };
    let Ok(Some(dialog)) = doc.query_selector("#leave") else { return };
    let Ok(list) = dialog.query_selector_all("a[href], button:not([disabled]), input:not([disabled])") else { return };
    let items: Vec<web_sys::HtmlElement> = (0..list.length()).filter_map(|i| list.get(i).and_then(|n| n.dyn_into().ok())).collect();
    if items.is_empty() {
        return;
    }
    let cur = doc.active_element().and_then(|a| items.iter().position(|el| el.is_same_node(Some(&a))));
    let n = items.len();
    let next = match (cur, shift) {
        (Some(c), false) => (c + 1) % n,
        (Some(c), true) => (c + n - 1) % n,
        (None, false) => 0,
        (None, true) => n - 1,
    };
    let _ = items[next].focus();
}

/// The leave dialog and the refusal notice — mounted once beside the app's
/// columns.
#[component]
pub(crate) fn LeaveDialog() -> impl IntoView {
    let ask = asking();
    let note = notice();
    view! {
        {move || ask.get().map(dialog_view)}
        {move || note.get().map(|(_, text)| view! {
            <div id="link-notice" class="lnotice" role="status">
                <span>{text}</span>
                <button class="link" aria-label="dismiss" on:click=move |_| notice().set(None)>"×"</button>
            </div>
        })}
    }
}

fn dialog_view(a: Asking) -> impl IntoView {
    let Asking { dest, from } = a;
    let keep = RwSignal::new(false);
    let web = dest.kind == DestKind::Web;
    let url = dest.url.clone();
    let domain = dest.domain.clone();
    let open = move || {
        if web && keep.get_untracked() {
            trust(&domain);
        }
        let url = url.clone();
        asking().set(None);
        RESTORE.with(|r| r.borrow_mut().take());
        open_tab(&url);
    };
    let open_click = open.clone();
    let open_btn = NodeRef::<leptos::html::Button>::new();
    open_btn.on_load(|el: web_sys::HtmlButtonElement| {
        let _ = el.focus();
    });
    let on_key = move |ev: web_sys::KeyboardEvent| {
        match ev.key().as_str() {
            "Escape" => {
                ev.prevent_default();
                close();
            }
            "Tab" => {
                ev.prevent_default();
                trap(ev.shift_key());
            }
            "Enter" => {
                // A button takes its own Enter (Cancel cancels).
                let on_button = ev
                    .target()
                    .and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok())
                    .is_some_and(|el| el.tag_name().eq_ignore_ascii_case("button") || el.tag_name().eq_ignore_ascii_case("a"));
                if !on_button {
                    ev.prevent_default();
                    open();
                }
            }
            _ => {}
        }
        ev.stop_propagation();
    };
    let shown_domain = shown_domain(&dest);
    let (before, dom_part, after) = dest.parts.clone();
    let from_line = {
        let words = from_words(&from);
        let ticket = from.ticket.clone();
        view! {
            <p class="lfrom" id="leave-from">
                {words}
                {ticket.map(|t| view! { " " <a class="code" href=format!("#/ticket/{t}") on:click=move |_| close()>{t.clone()}</a> })}
            </p>
        }
    };
    let what = if web {
        view! { <p class="lsite">"It opens a page on " <strong class="ldomain" id="leave-domain">{shown_domain.clone()}</strong> " in a new tab."</p> }.into_any()
    } else {
        let to = dest.to.join(", ");
        view! {
            <p class="lsite">"It writes an email, in your mail app, to " <strong class="ldomain" id="leave-domain">{if to.is_empty() { "no one named".to_string() } else { to }}</strong> "."</p>
        }
        .into_any()
    };
    let idn = dest.host_unicode.clone().map(|u| {
        let ascii = dest.host.clone();
        view! {
            <p class="lwarn" id="leave-idn">
                "Its name is written with letters beyond ASCII: " <code>{u}</code> " is " <code>{ascii}</code>
                " — the same name to a browser. Letters from different alphabets can look alike; make sure it is the site you mean."
            </p>
        }
    });
    let user = dest.userinfo.clone().map(|u| {
        let host = dest.host_unicode.clone().unwrap_or_else(|| dest.host.clone());
        view! {
            <p class="lwarn" id="leave-user">
                "Before the host it carries " <code>{format!("{u}@")}</code> " — a user name, not the site. The site is "
                <strong>{host}</strong> "."
            </p>
        }
    });
    let trust_row = web.then(|| {
        let d = shown_domain.clone();
        view! {
            <label class="ltrust">
                <input type="checkbox" id="leave-trust" prop:checked=move || keep.get() on:change=move |ev| keep.set(event_target_checked(&ev)) />
                " Don't ask again for " <strong>{d}</strong>
            </label>
        }
    });
    let mark_title = if web { "the registrable domain — the part one owner controls" } else { "the recipient's domain" };
    view! {
        <div class="overlay" on:click=move |_| close()>
            <div
                id="leave"
                class="dialog"
                role="dialog"
                aria-modal="true"
                aria-labelledby="leave-h"
                aria-describedby="leave-url"
                data-kind=if web { "web" } else { "mail" }
                data-domain=dest.domain.clone()
                on:click=|ev| ev.stop_propagation()
                on:keydown=on_key
            >
                <h2 id="leave-h">"This link leaves menger"</h2>
                {from_line}
                {what}
                <p class="lurl" id="leave-url"><code>{before}<mark title=mark_title>{dom_part}</mark>{after}</code></p>
                {user}
                {idn}
                {trust_row}
                <div class="sacts">
                    <button data-act="cancel" on:click=move |_| close()>"Cancel"</button>
                    <button class="primary" data-act="open" node_ref=open_btn on:click=move |_| open_click()>"Open"</button>
                </div>
            </div>
        </div>
    }
}

/// `text` with its bare URLs as links — a comment's body. The holder must
/// carry `data-links` for the app's listener to take them.
#[component]
pub(crate) fn Linkified(#[prop(into)] text: String) -> impl IntoView {
    pieces(&text)
        .into_iter()
        .map(|p| match p {
            Piece::Text(t) => t.into_any(),
            Piece::Link(u) => {
                let href = u.clone();
                view! { <a class="ulink" href=href rel="noopener noreferrer">{u}</a> }.into_any()
            }
        })
        .collect_view()
}

/// The settings page's group: the domains this user opens without asking,
/// each with "ask again".
#[component]
pub(crate) fn TrustGroup() -> impl IntoView {
    refresh_trusted();
    let list = trusted();
    view! {
        <fieldset class="group" id="settings-links">
            <legend>"Links"</legend>
            <p class="now">
                "A link in a ticket, a note or a comment that leaves menger asks before it opens. These sites open without asking — kept in this browser, for "
                {move || crate::signin::account().get().who().unwrap_or_else(|| "you".into())} ":"
            </p>
            {move || {
                let domains = list.get();
                if domains.is_empty() {
                    view! { <p class="hint" id="links-none">"None — every link that leaves menger asks first."</p> }.into_any()
                } else {
                    view! {
                        <ul class="ltrusted" id="links-trusted">
                            {domains.into_iter().map(|d| {
                                let shown = decoded(&d).map(|u| format!("{u} ({d})")).unwrap_or_else(|| d.clone());
                                let gone = d.clone();
                                view! {
                                    <li data-domain=d.clone()>
                                        <code>{shown}</code>
                                        <button class="link" data-act="forget" title="ask again before opening a link to it" on:click=move |_| forget(&gone)>"ask again"</button>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any()
                }
            }}
        </fieldset>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "http://127.0.0.1:4456/#/ticket/MENGER-14";

    fn ext(raw: &str) -> Dest {
        match classify(raw, PAGE) {
            Follow::External(d) => *d,
            other => panic!("{raw}: {other:?}"),
        }
    }

    fn refused(raw: &str) -> String {
        match classify(raw, PAGE) {
            Follow::Refused(w) => w,
            other => panic!("{raw}: {other:?}"),
        }
    }

    /// The vendored list is the one the README names, and reads.
    #[test]
    fn the_vendored_list_has_its_header_and_answers() {
        assert!(PSL.starts_with(PSL_HEADER), "libpsl's DAFSA, version 0");
        assert_eq!(PSL.len(), 55_336, "the README's size");
        let g = psl_graph();
        assert_eq!(dafsa_lookup(g, b"com"), Some(4));
        assert_eq!(dafsa_lookup(g, b"co.uk"), Some(4));
        assert_eq!(dafsa_lookup(g, b"github.io"), Some(8), "the private section");
        assert_eq!(dafsa_lookup(g, b"ck"), Some(4 | PSL_WILDCARD), "*.ck");
        assert_eq!(dafsa_lookup(g, b"www.ck"), Some(4 | PSL_EXCEPTION), "!www.ck");
        assert_eq!(dafsa_lookup(g, b"example.com"), None);
        assert_eq!(dafsa_lookup(g, b"co"), Some(4));
        assert_eq!(dafsa_lookup(g, b""), None);
        assert_eq!(dafsa_lookup(g, b"zzzzzzzzzz"), None);
        assert_eq!(dafsa_lookup(&[], b"com"), None, "an empty graph answers nothing");
        assert_eq!(dafsa_lookup(&[0x7F], b"com"), None, "a truncated one neither");
    }

    /// The registrable domain, by the list's own test vectors
    /// (publicsuffix.org/list/test.txt, `checkPublicSuffix`), and the
    /// private section's names.
    #[test]
    fn the_registrable_domain_is_the_public_suffix_and_one_label() {
        for (host, want) in [
            ("com", "com"),
            ("example.com", "example.com"),
            ("www.example.com", "example.com"),
            ("a.b.example.com", "example.com"),
            ("uk.com", "uk.com"),
            ("example.uk.com", "example.uk.com"),
            ("b.example.uk.com", "example.uk.com"),
            ("test.ac", "test.ac"),
            ("mm", "mm"),
            ("c.mm", "c.mm"),
            ("b.c.mm", "b.c.mm"),
            ("a.b.c.mm", "b.c.mm"),
            ("jp", "jp"),
            ("test.jp", "test.jp"),
            ("www.test.jp", "test.jp"),
            ("ac.jp", "ac.jp"),
            ("test.ac.jp", "test.ac.jp"),
            ("www.test.ac.jp", "test.ac.jp"),
            ("kyoto.jp", "kyoto.jp"),
            ("test.kyoto.jp", "test.kyoto.jp"),
            ("ide.kyoto.jp", "ide.kyoto.jp"),
            ("b.ide.kyoto.jp", "b.ide.kyoto.jp"),
            ("a.b.ide.kyoto.jp", "b.ide.kyoto.jp"),
            ("c.kobe.jp", "c.kobe.jp"),
            ("b.c.kobe.jp", "b.c.kobe.jp"),
            ("a.b.c.kobe.jp", "b.c.kobe.jp"),
            ("city.kobe.jp", "city.kobe.jp"),
            ("www.city.kobe.jp", "city.kobe.jp"),
            ("ck", "ck"),
            ("test.ck", "test.ck"),
            ("b.test.ck", "b.test.ck"),
            ("a.b.test.ck", "b.test.ck"),
            ("www.ck", "www.ck"),
            ("www.www.ck", "www.ck"),
            ("us", "us"),
            ("test.us", "test.us"),
            ("www.test.us", "test.us"),
            ("ak.us", "ak.us"),
            ("test.ak.us", "test.ak.us"),
            ("www.test.ak.us", "test.ak.us"),
            ("k12.ak.us", "k12.ak.us"),
            ("test.k12.ak.us", "test.k12.ak.us"),
            ("www.test.k12.ak.us", "test.k12.ak.us"),
            ("xn--85x722f.com.cn", "xn--85x722f.com.cn"),
            ("xn--85x722f.xn--55qx5d.cn", "xn--85x722f.xn--55qx5d.cn"),
            ("www.xn--85x722f.xn--55qx5d.cn", "xn--85x722f.xn--55qx5d.cn"),
            ("shishi.xn--55qx5d.cn", "shishi.xn--55qx5d.cn"),
            ("xn--55qx5d.cn", "xn--55qx5d.cn"),
            ("xn--85x722f.xn--fiqs8s", "xn--85x722f.xn--fiqs8s"),
            ("www.xn--85x722f.xn--fiqs8s", "xn--85x722f.xn--fiqs8s"),
            // The private section: one owner per name under these.
            ("github.io", "github.io"),
            ("alice.github.io", "alice.github.io"),
            ("x.alice.github.io", "alice.github.io"),
            ("bucket.s3.amazonaws.com", "bucket.s3.amazonaws.com"),
            ("www.example.co.uk", "example.co.uk"),
            ("xn--pple-43d.com", "xn--pple-43d.com"),
            ("www.xn--pple-43d.com", "xn--pple-43d.com"),
            // No rule: the implicit `*`.
            ("localhost", "localhost"),
            ("a.b.notatld", "b.notatld"),
            ("127.0.0.1", "127.0.0.1"),
            ("[::1]", "[::1]"),
            ("Example.COM.", "example.com"),
        ] {
            assert_eq!(registrable(host), want, "{host}");
        }
    }

    #[test]
    fn app_routes_and_anchors_stay_in_the_app() {
        assert_eq!(classify("#/ticket/MENGER-10", PAGE), Follow::Route("#/ticket/MENGER-10".into()));
        assert_eq!(classify("  #/waves/PLEXUS?sc=is:RETURNS ", PAGE), Follow::Route("#/waves/PLEXUS?sc=is:RETURNS".into()));
        assert_eq!(classify("#/inputs", PAGE), Follow::Route("#/inputs".into()));
        assert_eq!(classify("#/ticket/<script>", PAGE), Follow::Refused("That link names no page of menger, so it was not followed.".into()));
        assert_eq!(classify("#1", PAGE), Follow::Fragment("1".into()));
        assert_eq!(classify("#caf%C3%A9", PAGE), Follow::Fragment("café".into()));
        // The app's own origin and path: its fragment is the route.
        assert_eq!(classify("http://127.0.0.1:4456/#/ticket/MENGER-2", PAGE), Follow::Route("#/ticket/MENGER-2".into()));
        assert_eq!(classify("http://127.0.0.1:4456/app/#/ticket/MENGER-2", PAGE), Follow::Route("#/ticket/MENGER-2".into()));
        assert_eq!(classify("/app/#/ticket/MNG-2", PAGE), Follow::Route("#/ticket/MNG-2".into()));
        assert_eq!(classify("HTTP://127.0.0.1:4456/", PAGE), Follow::Route("#/".into()));
        // menger, not the app: its own page in a new tab.
        assert_eq!(classify("/vanilla", PAGE), Follow::Menger("http://127.0.0.1:4456/vanilla".into()));
        assert_eq!(classify("http://127.0.0.1:4456/vanilla#x", PAGE), Follow::Menger("http://127.0.0.1:4456/vanilla#x".into()));
        // Another port is another origin: the test environment leaves.
        assert_eq!(ext("http://127.0.0.1:4451/#/ticket/MENGER-10").domain, "127.0.0.1");
        assert_eq!(ext("http://localhost:4456/").domain, "localhost");
    }

    #[test]
    fn a_web_link_names_its_registrable_domain() {
        let d = ext("https://www.example.co.uk/a/b?c=1#d");
        assert_eq!(d.kind, DestKind::Web);
        assert_eq!(d.url, "https://www.example.co.uk/a/b?c=1#d");
        assert_eq!(d.host, "www.example.co.uk");
        assert_eq!(d.domain, "example.co.uk");
        assert_eq!(d.parts, ("https://www.".into(), "example.co.uk".into(), "/a/b?c=1#d".into()));
        assert_eq!((d.host_unicode, d.userinfo), (None, None));
        // Scheme-relative, and the browser's backslashes.
        assert_eq!(ext("//evil.example.com/x").url, "http://evil.example.com/x");
        assert_eq!(ext("https:\\\\evil.com\\x").url, "https://evil.com/x");
        assert_eq!(ext("HTTPS://EVIL.COM").url, "https://evil.com/");
        // A port stays after the domain.
        assert_eq!(ext("https://a.github.io:8443/p").parts, ("https://".into(), "a.github.io".into(), ":8443/p".into()));
        assert_eq!(ext("http://[::1]:9/").domain, "[::1]");
    }

    /// IDN: the punycode the browser opens, the letters a person reads.
    #[test]
    fn an_idn_host_is_shown_decoded_and_as_punycode() {
        let d = ext("https://xn--pple-43d.com/login");
        assert_eq!(d.domain, "xn--pple-43d.com");
        assert_eq!(d.host_unicode.as_deref(), Some("\u{0430}pple.com"), "a Cyrillic а");
        assert_eq!(d.domain_unicode.as_deref(), Some("\u{0430}pple.com"));
        assert_eq!(d.parts.1, "xn--pple-43d.com");
        assert_eq!(shown_domain(&d), "\u{0430}pple.com (xn--pple-43d.com)", "never the look-alike alone");
        assert_eq!(shown_domain(&ext("https://www.example.com/")), "example.com");
        // Written in Unicode, it is opened as punycode all the same.
        let u = ext("https://\u{0430}pple.com/");
        assert_eq!(u.url, "https://xn--pple-43d.com/");
        assert_eq!(u.domain, "xn--pple-43d.com");
        // A plain ASCII name has nothing to decode.
        assert_eq!(ext("https://apple.com").host_unicode, None);
        // Percent-escapes in a host decode before IDNA, as the browser does.
        assert_eq!(ext("https://ex%61mple.com/").domain, "example.com");
        assert_eq!(decoded("www.xn--bcher-kva.example"), Some("www.bücher.example".into()));
    }

    /// `user@host`: the site is the host.
    #[test]
    fn userinfo_before_the_host_is_called_out() {
        let d = ext("https://paypal.com@evil.example/");
        assert_eq!(d.domain, "evil.example");
        assert_eq!(d.userinfo.as_deref(), Some("paypal.com"));
        assert_eq!(d.parts, ("https://paypal.com@".into(), "evil.example".into(), "/".into()));
        let p = ext("https://me:secret@www.example.com/");
        assert_eq!(p.userinfo.as_deref(), Some("me:…"), "a password is never shown");
        assert!(!p.parts.0.is_empty());
        // Even on menger's own origin, a user name makes it a stranger.
        assert!(matches!(classify("http://x@127.0.0.1:4456/#/ticket/A-1", PAGE), Follow::External(_)));
    }

    #[test]
    fn mailto_goes_through_the_dialog_with_its_recipients() {
        let d = ext("mailto:Someone@Mail.Example.co.uk?subject=hi");
        assert_eq!(d.kind, DestKind::Mail);
        assert_eq!(d.to, ["Someone@Mail.Example.co.uk"]);
        assert_eq!(d.domain, "example.co.uk");
        assert_eq!(d.parts.1.to_ascii_lowercase(), "example.co.uk");
        assert!(d.parts.0.ends_with("Mail.") && d.parts.2 == "?subject=hi", "{:?}", d.parts);
        let two = ext("mailto:a@x.example,b%40y.example");
        assert_eq!(two.to, ["a@x.example", "b@y.example"]);
        assert_eq!(two.domain, "x.example");
        let none = ext("mailto:");
        assert_eq!((none.domain.as_str(), none.to.len()), ("", 0));
    }

    /// Only http(s) and mailto are followed — however the scheme is spelled.
    #[test]
    fn executable_local_and_unknown_schemes_are_refused() {
        assert!(refused("javascript:alert(1)").contains("javascript:"));
        assert!(refused("JaVaScRiPt:alert(1)").contains("javascript:"));
        assert!(refused(" \u{1}javascript:alert(1)").contains("javascript:"), "leading controls stripped");
        assert!(refused("java\tscript:alert(1)").contains("javascript:"), "a tab inside dropped");
        assert!(refused("java\nscript:alert(1)").contains("javascript:"), "a newline inside dropped");
        assert!(refused("vbscript:msgbox").contains("vbscript:"));
        assert!(refused("data:text/html,<script>alert(1)</script>").contains("data:"));
        assert!(refused("file:///etc/passwd").contains("file:"));
        assert!(refused("blob:http://x/1").contains("blob:"));
        assert!(refused("ftp://example.com/").contains("ftp:"));
        assert!(refused("intent://x#Intent;end").contains("intent:"));
        assert!(refused("").contains("empty"));
        assert!(refused(&format!("https://e.com/{}", "a".repeat(9000))).contains("too long"));
        // Percent-escaping a scheme makes it no scheme: a path of menger's.
        assert_eq!(classify("jav%61script:alert(1)", PAGE), Follow::Menger("http://127.0.0.1:4456/jav%61script:alert(1)".into()));
        // `https:` with no slashes, on an https page, is relative — on an
        // http page it is the host.
        assert_eq!(ext("https:evil.com").url, "https://evil.com/");
        assert_eq!(classify("https:x", "https://m.example/"), Follow::Menger("https://m.example/x".into()));
        assert_eq!(classify("http://", PAGE), Follow::Refused(refused_words("")));
    }

    #[test]
    fn a_click_asks_unless_the_domain_is_trusted_and_a_gesture_is_under_way() {
        let trusted = vec!["example.com".to_string()];
        let follow = |raw: &str| classify(raw, PAGE);
        assert_eq!(decide(follow("https://www.example.com/x"), &trusted, true), Act::Tab("https://www.example.com/x".into()));
        assert!(matches!(decide(follow("https://www.example.com/x"), &trusted, false), Act::Ask(_)), "no gesture: ask");
        assert!(matches!(decide(follow("https://example.org/"), &trusted, true), Act::Ask(_)));
        // Trusting example.com trusts no look-alike, nor the parent suffix.
        assert!(matches!(decide(follow("https://example.com.evil.org/"), &trusted, true), Act::Ask(_)));
        assert!(matches!(decide(follow("https://xn--exmple-cua.com/"), &trusted, true), Act::Ask(_)));
        // One github.io page trusted is not every one.
        let gh = vec!["alice.github.io".to_string()];
        assert_eq!(decide(follow("https://alice.github.io/p"), &gh, true), Act::Tab("https://alice.github.io/p".into()));
        assert!(matches!(decide(follow("https://mallory.github.io/p"), &gh, true), Act::Ask(_)));
        // mailto always asks.
        assert!(matches!(decide(follow("mailto:a@example.com"), &trusted, true), Act::Ask(_)));
        assert_eq!(decide(follow("#/ticket/A-1"), &[], false), Act::Route("#/ticket/A-1".into()));
        assert_eq!(decide(follow("#fn1"), &[], false), Act::Scroll("fn1".into()));
        assert_eq!(decide(follow("/vanilla"), &[], true), Act::Tab("http://127.0.0.1:4456/vanilla".into()));
        assert_eq!(decide(follow("/vanilla"), &[], false), Act::Nothing);
        assert!(matches!(decide(follow("javascript:1"), &trusted, true), Act::Notice(_)));
    }

    #[test]
    fn the_trust_record_is_per_user_and_round_trips() {
        let t = Trust::default().with("", "example.com").with("", "a.github.io").with("npub1x", "example.org").with("", "example.com");
        assert_eq!(t.domains(""), ["a.github.io", "example.com"]);
        assert_eq!(t.domains("npub1x"), ["example.org"], "each user their own");
        assert_eq!(Trust::decode(Some(&t.encode())), t);
        assert_eq!(
            t.encode(),
            r#"{"version":1,"trusted":{"":["a.github.io","example.com"],"npub1x":["example.org"]}}"#
        );
        let gone = t.clone().without("", "example.com").without("npub1x", "example.org");
        assert_eq!(gone.domains(""), ["a.github.io"]);
        assert_eq!(gone.domains("npub1x"), Vec::<String>::new());
        // Anything else reads as nothing trusted; a non-domain is dropped.
        for raw in [None, Some(""), Some("{"), Some(r#"{"version":2,"trusted":{"":["e.com"]}}"#)] {
            assert_eq!(Trust::decode(raw), Trust::default(), "{raw:?}");
        }
        let odd = Trust::decode(Some(r#"{"version":1,"trusted":{"":["E.com","e.com","","..","javascript:x","e.com"]}}"#));
        assert_eq!(odd.domains(""), ["e.com"]);
        assert_eq!(Trust::default().with("", "Bad Domain").domains(""), Vec::<String>::new());
    }

    #[test]
    fn bare_urls_are_found_as_github_finds_them() {
        let found = |t: &str| bare_urls(t).into_iter().map(|(s, e)| t[s..e].to_string()).collect::<Vec<_>>();
        assert_eq!(found("see https://example.com."), ["https://example.com"]);
        assert_eq!(found("(https://example.com/a_(b))"), ["https://example.com/a_(b)"]);
        assert_eq!(found("(see https://example.com/x)"), ["https://example.com/x"]);
        assert_eq!(found("http://a.example/?q=1&r=2, then"), ["http://a.example/?q=1&r=2"]);
        assert_eq!(found("two: https://a.example https://b.example/p!"), ["https://a.example", "https://b.example/p"]);
        assert_eq!(found("<https://a.example>"), ["https://a.example"]);
        assert_eq!(found("HTTPS://A.EXAMPLE/"), ["HTTPS://A.EXAMPLE/"]);
        assert_eq!(found("https://xn--pple-43d.com/x"), ["https://xn--pple-43d.com/x"]);
        assert_eq!(found("https://аpple.com/"), ["https://аpple.com/"]);
        for none in ["https://", "https:// x", "xhttps://a.example", "file:///x", "javascript:alert(1)", "httpx://a", "/http://a.example"] {
            assert!(found(none).is_empty(), "{none}");
        }
        assert_eq!(
            pieces("a https://e.example b"),
            [Piece::Text("a ".into()), Piece::Link("https://e.example".into()), Piece::Text(" b".into())]
        );
        assert_eq!(pieces("plain"), [Piece::Text("plain".into())]);
    }

    #[test]
    fn the_dialog_says_where_the_link_was() {
        let t = |ticket: Option<&str>, place| from_words(&From { ticket: ticket.map(str::to_string), place });
        assert_eq!(t(Some("MENGER-14"), "body"), "From");
        assert_eq!(t(Some("MENGER-14"), "comment"), "From a comment on");
        assert_eq!(t(None, "draft"), "From the draft being edited");
        assert_eq!(t(None, "help"), "From text in menger");
        assert_eq!(route_ticket("#/ticket/MENGER-14?field=x"), Some("MENGER-14".into()));
        assert_eq!(route_ticket("#/ticket/<b>"), None);
        assert_eq!(route_ticket("#/waves/MENGER"), None);
    }
}
