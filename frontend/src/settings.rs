//! MFE-21: `#/settings` — the app's settings; the first is the backend's
//! WebSocket URL.
//!
//! Until now the app talked to one backend: the page's own server, the host
//! it was loaded from at the port the shell stamps into
//! `<meta name="menger-ws-port">` ([`dom::ws_url`]). The settings page lets it
//! talk to another menger — the test environment, a throwaway — from the same
//! page.
//!
//! # The record
//!
//! [`Settings`] is one typed, versioned record in localStorage under [`KEY`]:
//!
//! ```text
//! menger.settings = {"version":1,"ws_url":"ws://127.0.0.1:4472","recent":["ws://127.0.0.1:4472"]}
//! ```
//!
//! `ws_url` is the override (`null`: this page's server); `recent` the last
//! [`RECENT_MAX`] URLs saved, newest first. A record that is missing, does not
//! parse, or carries another `version` reads as the defaults — no override —
//! and so does any stored URL that does not validate. Every read and write
//! goes through web-sys's `catch` bindings (`local_storage`, `get_item`,
//! `set_item` return `Result`), so a browser that refuses storage leaves the
//! app on its defaults rather than failing to load.
//!
//! # Connecting
//!
//! [`effective_ws_url`] is what `session` connects to: the override when one
//! is set, else this page's server as before. It is fixed for the page's
//! lifetime ([`backend`]): a session's caches, views and holders belong to one
//! backend, so a new URL takes effect by reloading the app — Save and reset
//! both reload — never by a reconnect switching servers under live views.
//! While the effective URL is not this page's server, the header's status says
//! which one it is ([`conn_text`]).
//!
//! # Test
//!
//! Test opens a temporary connection of its own to the candidate URL — a
//! `WsTransport` and the typed `TicketsClient`, as the session uses — asks
//! `health`, shows `✓ menger <version> · <n> tickets` or the error, and drops
//! the connection. Saving a URL that has not passed Test asks to confirm.
//!
//! What decides — validation, the record's round trip, the effective URL, the
//! recent list, the header's words — is pure and pinned by native tests.

use std::cell::OnceCell;
use std::rc::Rc;

use futures::future::{self, Either};
use leptos::prelude::*;
use menger_tickets_api::{TicketsClient, TicketsEvent};
use plexus_client::WsTransport;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

use crate::{dom, session};

/// The localStorage key the whole record lives under.
pub(crate) const KEY: &str = "menger.settings";
/// The record's version. A record of any other version reads as the
/// defaults.
pub(crate) const VERSION: u32 = 1;
/// How many saved URLs the page offers as one-click fills.
pub(crate) const RECENT_MAX: usize = 5;
/// How long Test waits for a connection and an answer.
const TEST_TIMEOUT_MS: i32 = 8_000;

// ---------------------------------------------------------------------
// Validation

/// A backend URL as the app accepts it: `ws://` or `wss://`, a host (a name,
/// an IPv4 address, or an IPv6 address in brackets) and an optional port —
/// nothing else. `Ok` is the URL normalized (trimmed, scheme and host
/// lowercased, a trailing `/` dropped), so two spellings of one server
/// compare equal; `Err` is what to tell the user.
pub(crate) fn validate(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("enter a ws:// or wss:// URL".into());
    }
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| "a WebSocket URL begins with ws:// or wss://".to_string())?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "ws" && scheme != "wss" {
        return Err(format!("{scheme}:// is not a WebSocket scheme — use ws:// or wss://"));
    }
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.contains(['/', '?', '#', '@', '\\']) || rest.chars().any(char::is_whitespace) {
        return Err("only a host and an optional port — no path, query, user or spaces".into());
    }
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (addr, after) = v6.split_once(']').ok_or_else(|| "an IPv6 host needs its closing ]".to_string())?;
        if addr.is_empty() || !addr.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') || !addr.contains(':') {
            return Err(format!("[{addr}] is not an IPv6 address"));
        }
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':').ok_or_else(|| "a port follows the host after a :".to_string())?),
        };
        (format!("[{}]", addr.to_ascii_lowercase()), port)
    } else {
        let (host, port) = match rest.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (rest, None),
        };
        if port.is_some_and(|p| p.contains(':')) {
            return Err("an IPv6 host goes in brackets: ws://[::1]:4455".into());
        }
        if host.is_empty() {
            return Err("the URL needs a host".into());
        }
        let label_ok = |l: &str| {
            !l.is_empty()
                && l.len() <= 63
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        };
        if host.len() > 253 || !host.split('.').all(label_ok) {
            return Err(format!("{host} is not a host name or address"));
        }
        (host.to_ascii_lowercase(), port)
    };
    let port = match port {
        None => None,
        Some(p) => match p.parse::<u16>() {
            Ok(n) if n > 0 && p.chars().all(|c| c.is_ascii_digit()) => Some(n),
            _ if p.is_empty() => return Err("a port after the : — or no :".into()),
            _ => return Err(format!("{p} is not a port (1–65535)")),
        },
    };
    Ok(match port {
        Some(p) => format!("{scheme}://{host}:{p}"),
        None => format!("{scheme}://{host}"),
    })
}

// ---------------------------------------------------------------------
// The record

/// The app's settings: the typed record behind [`KEY`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Settings {
    /// The backend's WebSocket URL when it is not this page's server
    /// (`None`: the page's server — the default).
    pub(crate) ws_url: Option<String>,
    /// The last URLs saved, newest first, at most [`RECENT_MAX`].
    pub(crate) recent: Vec<String>,
}

/// The record as stored. Its own type, so the stored shape is one place.
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    #[serde(default)]
    ws_url: Option<String>,
    #[serde(default)]
    recent: Vec<String>,
}

/// Only the version — read first, so a record of another version reads as
/// the defaults whatever else it holds.
#[derive(Deserialize)]
struct Head {
    version: u32,
}

impl Settings {
    /// What `raw` (the stored string, if any) says. Missing, corrupt, or
    /// another version: the defaults. A stored URL that does not validate is
    /// dropped (a hand edit, a rule tightened later); the recent list keeps
    /// its valid, distinct entries, at most [`RECENT_MAX`].
    pub(crate) fn decode(raw: Option<&str>) -> Self {
        let Some(raw) = raw else { return Self::default() };
        match serde_json::from_str::<Head>(raw) {
            Ok(Head { version: VERSION }) => {}
            _ => return Self::default(),
        }
        let Ok(record) = serde_json::from_str::<Record>(raw) else {
            return Self::default();
        };
        let ws_url = record.ws_url.and_then(|u| validate(&u).ok());
        let mut recent = Vec::new();
        for url in record.recent.iter().filter_map(|u| validate(u).ok()) {
            if !recent.contains(&url) && recent.len() < RECENT_MAX {
                recent.push(url);
            }
        }
        Settings { ws_url, recent }
    }

    /// The stored string.
    pub(crate) fn encode(&self) -> String {
        serde_json::to_string(&Record {
            version: VERSION,
            ws_url: self.ws_url.clone(),
            recent: self.recent.clone(),
        })
        .expect("a record of strings serializes")
    }

    /// The URL the app connects to: the override, else `page` (this page's
    /// server).
    pub(crate) fn effective(&self, page: &str) -> String {
        self.ws_url
            .as_deref()
            .and_then(|u| validate(u).ok())
            .unwrap_or_else(|| page.to_string())
    }

    /// The record after `url` is saved from a page served by `page`: the
    /// override (none, when `url` is the page's own server) and `url` first
    /// among the recent ones. The page's own server is never kept there — it
    /// is always offered, labelled.
    pub(crate) fn saved(mut self, url: &str, page: &str) -> Self {
        if url == page {
            self.ws_url = None;
        } else {
            self.ws_url = Some(url.to_string());
            self.recent.retain(|u| u != url);
            self.recent.insert(0, url.to_string());
            self.recent.truncate(RECENT_MAX);
        }
        self
    }

    /// The record after "reset to this page's server": no override; the
    /// recent list stays.
    pub(crate) fn reset(mut self) -> Self {
        self.ws_url = None;
        self
    }

    /// The one-click fills: this page's server first (`true`: label it),
    /// then the recent URLs that are not it.
    pub(crate) fn fills(&self, page: &str) -> Vec<(String, bool)> {
        std::iter::once((page.to_string(), true))
            .chain(self.recent.iter().filter(|u| *u != page).map(|u| (u.clone(), false)))
            .collect()
    }
}

/// The header's status words (`#conn`). With no override they are exactly
/// what they were — `live · WS`, `connecting…`, `reconnecting…`; with one,
/// the URL follows the state: `live · ws://127.0.0.1:4450`.
pub(crate) fn conn_text(state: &str, live: bool, over: Option<&str>) -> String {
    match over {
        Some(url) => format!("{state} · {url}"),
        None if live => format!("{state} · WS"),
        None => state.to_string(),
    }
}

/// `✓ menger 0.5.0 · 233 tickets`.
pub(crate) fn health_line(version: &str, tickets: u64) -> String {
    let noun = if tickets == 1 { "ticket" } else { "tickets" };
    format!("✓ menger {version} · {tickets} {noun}")
}

// ---------------------------------------------------------------------
// The browser side

/// localStorage, or `None` where the browser refuses it (a privacy mode,
/// blocked site data). `local_storage` is a `catch` binding: a getter that
/// throws comes back as `Err`, not as an exception.
fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

/// The stored record, or the defaults.
pub(crate) fn load() -> Settings {
    Settings::decode(storage().and_then(|s| s.get_item(KEY).ok().flatten()).as_deref())
}

/// Writes the record. `Err` says why it could not be kept.
fn store(settings: &Settings) -> Result<(), String> {
    let s = storage().ok_or_else(|| "this browser refuses storage here, so the setting cannot be kept".to_string())?;
    s.set_item(KEY, &settings.encode())
        .map_err(|_| "the browser refused to store the setting".to_string())
}

/// Which backend this page's app talks to, fixed when it is first asked
/// (the session's first connect): `url` is the effective URL, `page` this
/// page's server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Backend {
    pub(crate) url: String,
    pub(crate) page: String,
}

impl Backend {
    /// The URL when it is not this page's server — what the header marks.
    pub(crate) fn over(&self) -> Option<&str> {
        (self.url != self.page).then_some(self.url.as_str())
    }
}

thread_local! {
    static BACKEND: OnceCell<Backend> = const { OnceCell::new() };
}

/// See [`Backend`]. Read once per page: a change takes a reload.
pub(crate) fn backend() -> Backend {
    BACKEND.with(|b| {
        b.get_or_init(|| {
            let page = dom::ws_url();
            Backend { url: load().effective(&page), page }
        })
        .clone()
    })
}

/// What `session` connects to: the override when set, else this page's
/// server ([`dom::ws_url`]) as before.
pub(crate) fn effective_ws_url() -> String {
    backend().url
}

/// Starts the app again on whatever the record now says — a clean session.
fn reload() {
    let _ = dom::window().location().reload();
}

/// What Test found.
#[derive(Clone, Debug, PartialEq)]
enum Probe {
    Testing,
    Ok { version: String, tickets: u64 },
    Failed(String),
}

/// Test: a temporary connection to `url`, `health` over the typed client,
/// and the connection dropped — on an answer, an error, or the timeout.
async fn probe(url: String) -> Probe {
    let attempt = async {
        let transport = WsTransport::connect(&url, session::HUB)
            .await
            .map_err(|e| format!("cannot connect to {url}: {e}"))?;
        let client = TicketsClient::new(Rc::new(transport));
        let events = client.health_all().await.map_err(|e| format!("health: {}", session::describe(&e)))?;
        for ev in events {
            match ev {
                TicketsEvent::Health { version, ticket_count, .. } => {
                    return Ok(Probe::Ok { version, tickets: ticket_count })
                }
                TicketsEvent::Error { message } => return Err(format!("health: {message}")),
                _ => {}
            }
        }
        Err("health: no answer".to_string())
        // `client` drops here, and with it the only handle on the
        // temporary socket: it closes.
    };
    let timeout = dom::sleep_ms(TEST_TIMEOUT_MS);
    match future::select(Box::pin(attempt), Box::pin(timeout)).await {
        Either::Left((Ok(found), _)) => found,
        Either::Left((Err(e), _)) => Probe::Failed(e),
        Either::Right(_) => Probe::Failed(format!("no answer from {url} in {} s", TEST_TIMEOUT_MS / 1000)),
    }
}

/// `#/settings`.
#[component]
pub(crate) fn SettingsPage() -> impl IntoView {
    let Backend { url, page } = backend();
    let overridden = url != page;
    let fills = load().fills(&page);
    // The field opens on the effective URL.
    let text = RwSignal::new(url.clone());
    let valid = Memo::new(move |_| text.with(|t| validate(t)));
    // The last Test: the URL it asked, and what it found.
    let tested = RwSignal::new(None::<(String, Probe)>);
    // Why Save could not keep the setting.
    let problem = RwSignal::new(None::<String>);

    let run_test = move || {
        let Ok(candidate) = valid.get_untracked() else { return };
        tested.set(Some((candidate.clone(), Probe::Testing)));
        spawn_local(async move {
            let found = probe(candidate.clone()).await;
            // The page may have gone, or the field moved on to another Test.
            let current = tested.try_with_untracked(|t| matches!(t, Some((u, Probe::Testing)) if *u == candidate));
            if current == Some(true) {
                tested.try_set(Some((candidate, found)));
            }
        });
    };

    // What Test found for the URL in the field (none once it is edited).
    let shown = move || match (valid.get(), tested.get()) {
        (Ok(candidate), Some((u, found))) if u == candidate => Some(found),
        _ => None,
    };

    let page_save = page.clone();
    let save = move |_| {
        let Ok(candidate) = valid.get_untracked() else { return };
        let passed = tested.with_untracked(|t| matches!(t, Some((u, Probe::Ok { .. })) if *u == candidate));
        if candidate != page_save && !passed {
            let ask = format!("{candidate} has not passed Test. Save it and reconnect anyway?");
            if !dom::window().confirm_with_message(&ask).unwrap_or(false) {
                return;
            }
        }
        let next = load().saved(&candidate, &page_save);
        match store(&next) {
            Ok(()) => reload(),
            // Nothing to keep: the page's own server is the default.
            Err(_) if next.ws_url.is_none() => reload(),
            Err(e) => problem.set(Some(e)),
        }
    };
    let reset = move |_| {
        // With storage refused there was no override to clear.
        let _ = store(&load().reset());
        reload();
    };

    let page_note = page.clone();
    let now = if overridden {
        view! {
            <p class="now" id="ws-now">
                "This app talks to " <code>{url.clone()}</code> " — saved in this browser, in place of this page's server ("
                <code>{page_note}</code> ")."
            </p>
        }
        .into_any()
    } else {
        view! {
            <p class="now" id="ws-now">"This app talks to " <code>{url.clone()}</code> " — this page's server."</p>
        }
        .into_any()
    };

    let fill_views = fills
        .into_iter()
        .map(|(u, is_page)| {
            let fill = u.clone();
            let data = u.clone();
            view! {
                <li>
                    <button class="fill" data-url=data title="put it in the field" on:click=move |_| {
                        text.set(fill.clone());
                        problem.set(None);
                    }>{u}</button>
                    {is_page.then(|| view! { <span class="lbl">" (this page's server)"</span> })}
                </li>
            }
        })
        .collect_view();

    view! {
        <section id="settings">
            <h2>"Settings"</h2>
            <fieldset class="group" id="settings-backend">
                <legend>"Backend"</legend>
                {now}
                <div class="field">
                    <label for="ws-url">"WebSocket URL"</label>
                    <div class="control">
                        <div class="line">
                            <input
                                id="ws-url"
                                type="text"
                                spellcheck="false"
                                autocomplete="off"
                                placeholder="ws://127.0.0.1:4455"
                                aria-invalid=move || valid.with(|v| v.is_err()).then_some("true")
                                aria-describedby="ws-status"
                                prop:value=move || text.get()
                                on:input=move |ev| {
                                    text.set(event_target_value(&ev));
                                    problem.set(None);
                                }
                                on:keydown=move |ev: web_sys::KeyboardEvent| {
                                    if ev.key() == "Enter" {
                                        ev.prevent_default();
                                        run_test();
                                    }
                                }
                            />
                            <button
                                id="ws-test"
                                title="connect to it, ask health, disconnect"
                                disabled=move || valid.with(|v| v.is_err()) || matches!(shown(), Some(Probe::Testing))
                                on:click=move |_| run_test()
                            >
                                "Test"
                            </button>
                        </div>
                        <p class="status" id="ws-status" aria-live="polite">
                            {move || match valid.get() {
                                Err(why) => view! { <span class="invalid" id="ws-invalid">{why}</span> }.into_any(),
                                Ok(_) => match shown() {
                                    None => view! { <span class="hint">"Test it before saving, or save it as it is."</span> }.into_any(),
                                    Some(Probe::Testing) => view! { <span class="probe testing" id="ws-probe">"testing…"</span> }.into_any(),
                                    Some(Probe::Ok { version, tickets }) => {
                                        view! { <span class="probe ok" id="ws-probe">{health_line(&version, tickets)}</span> }.into_any()
                                    }
                                    Some(Probe::Failed(e)) => {
                                        view! { <span class="probe failed" id="ws-probe">{format!("✗ {e}")}</span> }.into_any()
                                    }
                                },
                            }}
                        </p>
                        <div class="line actions">
                            <button
                                id="ws-save"
                                class="primary"
                                disabled=move || valid.with(|v| v.is_err())
                                title="keep this URL in this browser and start the app again on it"
                                on:click=save
                            >
                                "Save & reconnect"
                            </button>
                            <button
                                id="ws-reset"
                                class="link"
                                disabled=!overridden
                                title=if overridden { "forget the saved URL and start the app again on this page's server" } else { "already this page's server" }
                                on:click=reset
                            >
                                "reset to this page's server"
                            </button>
                        </div>
                        {move || problem.get().map(|p| view! { <p class="invalid" id="ws-problem" role="alert">{p}</p> })}
                    </div>
                </div>
                <div class="recent">
                    <span class="lbl">"recent:"</span>
                    <ul>{fill_views}</ul>
                </div>
            </fieldset>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "ws://127.0.0.1:4455";

    /// MFE-21 acceptance 1: `ws://` or `wss://`, a host, an optional port —
    /// normalized; anything else an error.
    #[test]
    fn validation_accepts_a_scheme_a_host_and_a_port_only() {
        for (raw, want) in [
            ("ws://127.0.0.1:4450", "ws://127.0.0.1:4450"),
            ("  ws://127.0.0.1:4450  ", "ws://127.0.0.1:4450"),
            ("ws://127.0.0.1:4450/", "ws://127.0.0.1:4450"),
            ("wss://menger.example.org", "wss://menger.example.org"),
            ("WS://LocalHost:4455", "ws://localhost:4455"),
            ("ws://localhost", "ws://localhost"),
            ("ws://my-box.lan:1", "ws://my-box.lan:1"),
            ("ws://host:65535", "ws://host:65535"),
            ("ws://[::1]:4455", "ws://[::1]:4455"),
            ("ws://[FE80::1]", "ws://[fe80::1]"),
        ] {
            assert_eq!(validate(raw).as_deref(), Ok(want), "{raw}");
        }
        for raw in [
            "",
            "   ",
            "127.0.0.1:4455",
            "http://127.0.0.1:4455",
            "https://menger.example.org",
            "ws://",
            "ws://:4455",
            "ws://127.0.0.1:",
            "ws://127.0.0.1:0",
            "ws://127.0.0.1:65536",
            "ws://127.0.0.1:44a5",
            "ws://127.0.0.1:+445",
            "ws://127.0.0.1:4455/ws",
            "ws://127.0.0.1:4455?x=1",
            "ws://127.0.0.1:4455#x",
            "ws://user@127.0.0.1:4455",
            "ws://my host:4455",
            "ws://bad_host:4455",
            "ws://-lead.example:4455",
            "ws://a..b:4455",
            "ws://::1:4455",
            "ws://[::1",
            "ws://[]:4455",
            "ws://[::1]4455",
            "ws://[zz::1]:4455",
        ] {
            assert!(validate(raw).is_err(), "{raw:?} should not validate: {:?}", validate(raw));
        }
        // The error says what is wrong, in words.
        assert!(validate("http://x").unwrap_err().contains("ws://"));
        assert!(validate("ws://x:99999").unwrap_err().contains("port"));
        assert!(validate("ws://x/path").unwrap_err().contains("path"));
    }

    /// MFE-21 acceptance 1: the record round-trips through its stored
    /// string; missing, corrupt and other-version records read as the
    /// defaults.
    #[test]
    fn the_record_round_trips_and_anything_else_reads_as_the_defaults() {
        let s = Settings {
            ws_url: Some("ws://127.0.0.1:4450".into()),
            recent: vec!["ws://127.0.0.1:4450".into(), "ws://127.0.0.1:4460".into()],
        };
        let raw = s.encode();
        assert_eq!(
            raw,
            r#"{"version":1,"ws_url":"ws://127.0.0.1:4450","recent":["ws://127.0.0.1:4450","ws://127.0.0.1:4460"]}"#
        );
        assert_eq!(Settings::decode(Some(&raw)), s);
        let none = Settings::default();
        assert_eq!(Settings::decode(Some(&none.encode())), none);
        assert_eq!(none.encode(), r#"{"version":1,"ws_url":null,"recent":[]}"#);

        for raw in [
            None,
            Some(""),
            Some("not json"),
            Some("{"),
            Some("[]"),
            Some("null"),
            Some("42"),
            Some(r#"{"ws_url":"ws://127.0.0.1:4450"}"#),
            Some(r#"{"version":"1","ws_url":"ws://127.0.0.1:4450"}"#),
            Some(r#"{"version":0,"ws_url":"ws://127.0.0.1:4450"}"#),
            Some(r#"{"version":2,"ws_url":"ws://127.0.0.1:4450"}"#),
            Some(r#"{"version":1,"ws_url":42}"#),
            Some(r#"{"version":1,"recent":"ws://127.0.0.1:4450"}"#),
        ] {
            assert_eq!(Settings::decode(raw), Settings::default(), "{raw:?}");
        }
        // Fields default; a URL that does not validate is dropped; the recent
        // list keeps its valid, distinct entries, normalized, at most five.
        assert_eq!(Settings::decode(Some(r#"{"version":1}"#)), Settings::default());
        let odd = Settings::decode(Some(
            r#"{"version":1,"ws_url":"http://nope","recent":["ws://a:1","bad","WS://A:1/","ws://b:2","ws://c:3","ws://d:4","ws://e:5","ws://f:6"],"later":true}"#,
        ));
        assert_eq!(odd.ws_url, None);
        assert_eq!(odd.recent, ["ws://a:1", "ws://b:2", "ws://c:3", "ws://d:4", "ws://e:5"]);
    }

    /// MFE-21 acceptance 1: the effective URL is the override when one is
    /// set, else the page's server — exactly as before.
    #[test]
    fn the_effective_url_is_the_override_else_the_page_s_server() {
        assert_eq!(Settings::default().effective(PAGE), PAGE);
        let over = Settings { ws_url: Some("ws://127.0.0.1:4450".into()), recent: vec![] };
        assert_eq!(over.effective(PAGE), "ws://127.0.0.1:4450");
        // An override that does not validate is no override.
        let bad = Settings { ws_url: Some("http://127.0.0.1:4450".into()), recent: vec![] };
        assert_eq!(bad.effective(PAGE), PAGE);
        // Through storage: what was saved is what is connected to.
        let saved = Settings::default().saved("ws://127.0.0.1:4472", PAGE);
        assert_eq!(Settings::decode(Some(&saved.encode())).effective(PAGE), "ws://127.0.0.1:4472");
        assert_eq!(Settings::decode(Some(&saved.reset().encode())).effective(PAGE), PAGE);
        assert_eq!(Settings::decode(None).effective(PAGE), PAGE);
    }

    /// Save puts the URL first among the recent ones (five at most, no
    /// repeats); saving the page's own server is no override; reset clears
    /// the override and keeps the list.
    #[test]
    fn save_and_reset_move_the_override_and_the_recent_list() {
        let mut s = Settings::default();
        for port in 4460..4467 {
            s = s.saved(&format!("ws://127.0.0.1:{port}"), PAGE);
        }
        assert_eq!(s.ws_url.as_deref(), Some("ws://127.0.0.1:4466"));
        assert_eq!(s.recent.len(), RECENT_MAX);
        assert_eq!(s.recent[0], "ws://127.0.0.1:4466");
        assert_eq!(s.recent[4], "ws://127.0.0.1:4462");
        let s = s.saved("ws://127.0.0.1:4463", PAGE);
        assert_eq!(s.recent[..2], ["ws://127.0.0.1:4463".to_string(), "ws://127.0.0.1:4466".to_string()]);
        assert_eq!(s.recent.len(), RECENT_MAX);
        let back = s.clone().saved(PAGE, PAGE);
        assert_eq!(back.ws_url, None);
        assert_eq!(back.recent, s.recent, "the page's own server is not a recent entry");
        let reset = s.clone().reset();
        assert_eq!(reset.ws_url, None);
        assert_eq!(reset.recent, s.recent);
    }

    /// The fills: the page's server first, labelled; then the recent ones
    /// that are not it.
    #[test]
    fn the_fills_offer_the_page_s_server_then_the_recent_ones() {
        let s = Settings { ws_url: None, recent: vec!["ws://127.0.0.1:4450".into(), PAGE.into()] };
        assert_eq!(s.fills(PAGE), vec![(PAGE.to_string(), true), ("ws://127.0.0.1:4450".to_string(), false)]);
        assert_eq!(Settings::default().fills(PAGE), vec![(PAGE.to_string(), true)]);
    }

    /// The header: unchanged with no override; the URL after the state with
    /// one, in every state.
    #[test]
    fn the_header_names_an_overridden_backend_in_every_state() {
        assert_eq!(conn_text("live", true, None), "live · WS");
        assert_eq!(conn_text("connecting…", false, None), "connecting…");
        assert_eq!(conn_text("reconnecting…", false, None), "reconnecting…");
        let url = Some("ws://127.0.0.1:4450");
        assert_eq!(conn_text("live", true, url), "live · ws://127.0.0.1:4450");
        assert_eq!(conn_text("reconnecting…", false, url), "reconnecting… · ws://127.0.0.1:4450");
        assert_eq!(conn_text("connecting…", false, url), "connecting… · ws://127.0.0.1:4450");
        let b = Backend { url: PAGE.into(), page: PAGE.into() };
        assert_eq!(b.over(), None);
        let b = Backend { url: "ws://127.0.0.1:4450".into(), page: PAGE.into() };
        assert_eq!(b.over(), Some("ws://127.0.0.1:4450"));
    }

    #[test]
    fn the_health_line_reads_like_the_ticket() {
        assert_eq!(health_line("0.5.0", 233), "✓ menger 0.5.0 · 233 tickets");
        assert_eq!(health_line("0.5.0", 1), "✓ menger 0.5.0 · 1 ticket");
    }
}
