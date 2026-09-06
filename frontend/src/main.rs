//! MFE-3: the live board — a read-and-status surface. Every ticket
//! renders as a row (ordered by epic, then sequence) from one initial
//! fetch plus `tickets.watch` SSE updates — no polling. Each row carries
//! a status control (the closed seven-value vocabulary; the server stays
//! the validator); in-band `error` events on the app's own calls surface
//! in a visible banner. There is deliberately NO creation affordance
//! (owner, 2026-09-05): tickets are created by agents/CLI/API — the
//! browser reads and changes status.
//!
//! MFE-7: the ticket page. Hash-routed (`/app/#/ticket/{id}` — the
//! deep-link contract recorded in MFE-1): the route never reaches the
//! server, so a cold load rides the existing `/app/` index serving with
//! no SPA fallback, and dist assets can never collide with a route. The
//! page shows every wire-exposed field, renders the body as sanitized
//! markdown (src/md.rs), resolves blockers via `tickets.blockers`, and
//! stays live through the same watch stream as the board: `get` and
//! `blockers` results upsert into the id-keyed row store and the page
//! renders THROUGH those signals, so a status change arriving from
//! anywhere updates the open page — and a `created` event can resolve a
//! dangling blocker in place.
//!
//! Reactivity is signal-grained: each row owns an inner `RwSignal<Ticket>`,
//! so an incoming event updates the affected row's text in place — the
//! row list signal only fires on insert/remove, and untouched rows keep
//! their DOM identity across every update.
//!
//! AGS-12: the persistent left nav (src/nav.rs) — manifest's sidebar
//! pattern, tickets grouped under their epics — sits beside every view
//! and renders through the same row store, so it is live by construction.

//! AGS-14: ```mermaid fences in a body render as SVG. md.rs emits an
//! inert placeholder per fence; once the body's div is in the DOM the
//! page hands it to src/mermaid.rs, which lazily loads the vendored
//! renderer (served by menger, fetched only when a body has a fence) and
//! draws in strict mode, painting failures inline.

mod md;
mod nav;

mod mermaid;
mod net;

use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

/// The closed status vocabulary (src/store.rs `Status`) — the legal UI
/// choices. The server remains the validator; anything else it receives
/// comes back as an in-band `error` event.
const STATUSES: [&str; 7] = [
    "Pending",
    "Ready",
    "Blocked",
    "Complete",
    "Idea",
    "Epic",
    "Superseded",
];

/// Mirror of the server's ticket JSON (src/store.rs). Enums arrive as
/// plain strings so unknown future variants degrade to text, not panics.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct Ticket {
    id: String,
    epic: String,
    seq: u32,
    pub(crate) title: String,
    pub(crate) status: String,
    /// AGS-13: scope | execution | build | gap | spike.
    pub(crate) kind: String,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    unlocks: Vec<String>,
    /// Markdown body — rendered on the ticket page (MFE-7).
    #[serde(default)]
    body: String,
    #[serde(default)]
    created_unix: i64,
    updated_unix: i64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Health {
    version: String,
    ticket_count: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum Conn {
    Connecting,
    Live,
    Reconnecting,
}

/// One board row. `id`/`epic`/`seq` are immutable for a ticket's
/// lifetime (they define its identity and sort position), so they live
/// outside the signal; everything mutable renders through `ticket`.
#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) epic: String,
    pub(crate) seq: u32,
    pub(crate) ticket: RwSignal<Ticket>,
}

/// Fine-grained upsert. An existing ticket updates its row's inner
/// signal in place — the list signal never fires, `<For>` never re-runs,
/// and every other row's DOM survives untouched. A new ticket inserts at
/// its (epic, seq) position, which is stable because those fields never
/// change.
fn upsert(rows: RwSignal<Vec<Row>>, t: Ticket) {
    let existing = rows.with_untracked(|v| v.iter().find(|r| r.id == t.id).map(|r| r.ticket));
    match existing {
        Some(sig) => {
            if sig.with_untracked(|old| *old != t) {
                sig.set(t);
            }
        }
        None => rows.update(|v| {
            let pos = v.partition_point(|r| (r.epic.as_str(), r.seq) < (t.epic.as_str(), t.seq));
            let row = Row {
                id: t.id.clone(),
                epic: t.epic.clone(),
                seq: t.seq,
                ticket: RwSignal::new(t),
            };
            v.insert(pos, row);
        }),
    }
}

/// Reconcile a full `list` snapshot into the rows without rebuilding
/// them: upsert every ticket, drop rows the snapshot no longer contains
/// (nothing deletes tickets today, but a reconnect must converge on the
/// server's truth regardless). Unchanged rows keep their DOM even across
/// a reconnect.
fn reconcile(rows: RwSignal<Vec<Row>>, snapshot: Vec<Ticket>) {
    let keep: std::collections::HashSet<String> = snapshot.iter().map(|t| t.id.clone()).collect();
    let stale = rows.with_untracked(|v| v.iter().any(|r| !keep.contains(&r.id)));
    if stale {
        rows.update(|v| v.retain(|r| keep.contains(&r.id)));
    }
    for t in snapshot {
        upsert(rows, t);
    }
}

fn parse_ticket(ev: &serde_json::Value) -> Option<Ticket> {
    serde_json::from_value(ev.get("ticket")?.clone()).ok()
}

/// First in-band failure in a unary response's events, as a message.
/// `error` and `not_found` both mean "the call did not do what was
/// asked" — the server is the validator and this is how it says no.
fn first_error(events: &[serde_json::Value]) -> Option<String> {
    events.iter().find_map(|e| {
        match e.get("type").and_then(|t| t.as_str()) {
            Some("error") => Some(
                e.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error")
                    .to_string(),
            ),
            Some("not_found") => Some(format!(
                "{}: not found",
                e.get("id").and_then(|i| i.as_str()).unwrap_or("?")
            )),
            _ => None,
        }
    })
}

/// One connected session: subscribe first (so nothing created between
/// list and watch is missed — events are idempotent upserts by id), then
/// snapshot via unary calls, then pump the stream until it dies. Err ends
/// the session; the caller loop reconnects. In-band errors on the app's
/// own snapshot calls surface through `last_error`.
async fn session(
    rows: RwSignal<Vec<Row>>,
    health: RwSignal<Option<Health>>,
    conn: RwSignal<Conn>,
    last_error: RwSignal<Option<String>>,
) -> Result<(), String> {
    let mut watch = net::open_watch(&json!({})).await?;

    let health_events = net::call("health", &json!({})).await?;
    if let Some(msg) = first_error(&health_events) {
        last_error.set(Some(format!("health: {msg}")));
    }
    if let Some(h) = health_events
        .first()
        .and_then(|e| serde_json::from_value::<Health>(e.clone()).ok())
    {
        health.set(Some(h));
    }

    let list_events = net::call("list", &json!({})).await?;
    if let Some(msg) = first_error(&list_events) {
        last_error.set(Some(format!("list: {msg}")));
    }
    reconcile(rows, list_events.iter().filter_map(parse_ticket).collect());
    conn.set(Conn::Live);

    loop {
        let ev = watch.next_event().await?;
        match ev.get("type").and_then(|t| t.as_str()) {
            // Every mutation event carries the full updated ticket.
            Some("created") | Some("status_changed") | Some("unlocks_changed") => {
                if let Some(t) = parse_ticket(&ev) {
                    let created = ev["type"] == "created";
                    upsert(rows, t);
                    if created {
                        health.update(|h| {
                            if let Some(h) = h {
                                h.ticket_count += 1;
                            }
                        });
                    }
                }
            }
            _ => {} // future event kinds: ignore, stay live
        }
    }
}

/// The app's one mutation: `set_status`, fired from a row's select. The
/// select is left showing the user's choice while the call is in flight;
/// the authoritative update arrives back through `watch`. Any failure —
/// transport-level or an in-band `error`/`not_found` event — surfaces in
/// the banner and snaps the select back to the last server-confirmed
/// status.
fn request_set_status(
    id: String,
    chosen: String,
    revert_to: String,
    sel: web_sys::HtmlSelectElement,
    last_error: RwSignal<Option<String>>,
) {
    spawn_local(async move {
        let failure = match net::call("set_status", &json!({"id": id, "status": chosen})).await {
            Ok(events) => first_error(&events),
            Err(e) => Some(e),
        };
        if let Some(msg) = failure {
            last_error.set(Some(format!("set_status {id} -> {chosen}: {msg}")));
            sel.set_value(&revert_to);
        }
    });
}

// ---------------------------------------------------------------------
// Routing (MFE-7). The deep-link contract this ticket owns, recorded in
// MFE-1: a ticket's page is `/app/#/ticket/{id}`.

#[derive(Clone, PartialEq)]
pub(crate) enum Route {
    Board,
    Ticket(String),
}

fn parse_route(hash: &str) -> Route {
    let path = hash.strip_prefix('#').unwrap_or(hash);
    let path = path.strip_prefix('/').unwrap_or(path);
    match path.split_once('/') {
        Some(("ticket", id)) if !id.is_empty() => Route::Ticket(id.to_string()),
        _ => Route::Board,
    }
}

fn current_route() -> Route {
    parse_route(&net::window().location().hash().unwrap_or_default())
}

/// The route as a signal: seeded from the URL (deep links / cold loads),
/// updated on every hashchange — link clicks, row clicks, and the
/// browser's back/forward all funnel through the hash.
fn use_route() -> RwSignal<Route> {
    let route = RwSignal::new(current_route());
    let cb = Closure::<dyn FnMut()>::new(move || route.set(current_route()));
    net::window()
        .add_event_listener_with_callback("hashchange", cb.as_ref().unchecked_ref())
        .expect("hashchange listener");
    cb.forget(); // lives as long as the app
    route
}

fn navigate_to_ticket(id: &str) {
    let _ = net::window().location().set_hash(&format!("/ticket/{id}"));
}

/// Unix seconds, human-readable: "2026-09-05T12:34:56Z (1788690896)".
/// js_sys::Date so no chrono rides into the wasm.
fn fmt_unix(unix: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(unix as f64 * 1000.0));
    let iso: String = date.to_iso_string().into();
    let iso = iso
        .strip_suffix(".000Z")
        .map(|s| format!("{s}Z"))
        .unwrap_or(iso);
    format!("{iso} ({unix})")
}

// ---------------------------------------------------------------------
// Views

/// The board (MFE-3), unchanged in behavior; MFE-7 adds click-through:
/// the id cell is a real link (middle-click friendly) and a click
/// anywhere else on the row — except the status control — navigates to
/// the ticket page.
#[component]
fn Board(rows: RwSignal<Vec<Row>>, last_error: RwSignal<Option<String>>) -> impl IntoView {
    view! {
        <table>
            <thead>
                <tr>
                    <th>"id"</th><th>"title"</th><th>"kind"</th>
                    <th>"status"</th><th>"severity"</th><th>"blocked by"</th>
                </tr>
            </thead>
            <tbody id="board">
                <For
                    each=move || rows.get()
                    // id alone: mutations re-render fields inside the
                    // row; the <tr> itself is never replaced.
                    key=|row| row.id.clone()
                    children=move |row: Row| {
                        let ticket = row.ticket;
                        let id = row.id.clone();
                        let nav_id = row.id.clone();
                        let href_id = row.id.clone();
                        view! {
                            <tr
                                data-ticket-id=row.id.clone()
                                on:click=move |ev| {
                                    // The select (and its options) keep their own
                                    // behavior; the id link handles itself.
                                    let hit_control = ev
                                        .target()
                                        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                        .and_then(|el| el.closest("select, option, a").ok().flatten())
                                        .is_some();
                                    if !hit_control {
                                        navigate_to_ticket(&nav_id);
                                    }
                                }
                            >
                                <td class="id">
                                    <a href=format!("#/ticket/{href_id}")>{row.id.clone()}</a>
                                </td>
                                <td>{move || ticket.with(|t| t.title.clone())}</td>
                                <td class="kind">{move || ticket.with(|t| t.kind.clone())}</td>
                                <td class="status">
                                    <select
                                        class="status"
                                        prop:value=move || ticket.with(|t| t.status.clone())
                                        on:change=move |ev| {
                                            let Some(sel) = ev
                                                .target()
                                                .and_then(|t| t.dyn_into::<web_sys::HtmlSelectElement>().ok())
                                            else {
                                                return;
                                            };
                                            let chosen = sel.value();
                                            let current = ticket.with_untracked(|t| t.status.clone());
                                            if chosen != current {
                                                request_set_status(id.clone(), chosen, current, sel, last_error);
                                            }
                                        }
                                    >
                                        {STATUSES
                                            .iter()
                                            .map(|s| {
                                                let s = *s;
                                                view! {
                                                    <option
                                                        value=s
                                                        selected=move || ticket.with(|t| t.status == s)
                                                    >
                                                        {s}
                                                    </option>
                                                }
                                            })
                                            .collect_view()}
                                    </select>
                                </td>
                                <td>{move || ticket.with(|t| t.severity.clone().unwrap_or_default())}</td>
                                <td>{move || ticket.with(|t| t.blocked_by.join(" "))}</td>
                            </tr>
                        }
                    }
                />
            </tbody>
        </table>
    }
}

/// One entry of the blockers/unlocks lists. Present in the store → a
/// live link (status and title re-render on watch events). Absent: for
/// blockers, once the `blockers` call has resolved (`dangling_when`),
/// the id is a dangling forward reference — visually distinct, unlinked;
/// until then (and for unlocks entries the snapshot hasn't carried yet)
/// a plain link. A `created` watch event upserts the store, so a
/// dangling entry resolves live while the page is open.
fn dag_entry(
    id: String,
    rows: RwSignal<Vec<Row>>,
    dangling_when: Option<RwSignal<bool>>,
) -> impl IntoView {
    let found = {
        let id = id.clone();
        Memo::new(move |_| rows.with(|v| v.iter().find(|r| r.id == id).map(|r| r.ticket)))
    };
    let attr_id = id.clone();
    view! {
        <li data-dag-id=attr_id>
            {move || match found.get() {
                Some(t) => {
                    let href = format!("#/ticket/{id}");
                    view! {
                        <a href=href>{id.clone()}</a>
                        <span class="bstatus">{move || t.with(|x| x.status.clone())}</span>
                        <span class="btitle">{move || t.with(|x| x.title.clone())}</span>
                    }
                        .into_any()
                }
                None if dangling_when.is_some_and(|loaded| loaded.get()) => {
                    view! {
                        <span class="dangling">{id.clone()}" (dangling — no such ticket yet)"</span>
                    }
                        .into_any()
                }
                None => {
                    let href = format!("#/ticket/{id}");
                    view! { <a href=href>{id.clone()}</a> }.into_any()
                }
            }}
        </li>
    }
}

/// The full ticket render: every wire-exposed field. The body is
/// markdown, sanitized in md.rs; a leading YAML frontmatter block is
/// collapsed behind a <details>, never painted raw. Body/blockers/
/// unlocks render through memos so a status change re-renders the status
/// text without rebuilding those sections (an open frontmatter fold
/// survives a live update).
fn ticket_view(
    ticket: RwSignal<Ticket>,
    rows: RwSignal<Vec<Row>>,
    blockers_loaded: RwSignal<bool>,
) -> impl IntoView {
    let body_raw = Memo::new(move |_| ticket.with(|t| t.body.clone()));
    let blocked_by = Memo::new(move |_| ticket.with(|t| t.blocked_by.clone()));
    let unlocks_ids = Memo::new(move |_| ticket.with(|t| t.unlocks.clone()));

    let blockers = move || {
        let ids = blocked_by.get();
        if ids.is_empty() {
            view! { <p class="none">"none"</p> }.into_any()
        } else {
            view! {
                <ul id="blockers">
                    {ids
                        .into_iter()
                        .map(|bid| dag_entry(bid, rows, Some(blockers_loaded)))
                        .collect_view()}
                </ul>
            }
                .into_any()
        }
    };
    let unlocks = move || {
        let ids = unlocks_ids.get();
        if ids.is_empty() {
            view! { <p class="none">"none"</p> }.into_any()
        } else {
            view! {
                <ul id="unlocks">
                    {ids
                        .into_iter()
                        .map(|uid| dag_entry(uid, rows, None))
                        .collect_view()}
                </ul>
            }
                .into_any()
        }
    };
    let body = move || {
        let raw = body_raw.get();
        if raw.trim().is_empty() {
            return view! { <p class="none">"(no body)"</p> }.into_any();
        }
        let (fm, md_src) = md::split_frontmatter(&raw);
        let fm_view = fm.map(|f| {
            let f = f.to_string();
            view! {
                <details class="frontmatter">
                    <summary>"frontmatter"</summary>
                    <pre>{f}</pre>
                </details>
            }
        });
        let md::Rendered { html, diagrams } = md::render_body(md_src);
        // AGS-14: once this div is in the DOM, hand its mermaid
        // placeholders to the renderer. `on_load` fires once per mounted
        // div, and a body re-render makes a new div (and a new hook), so
        // navigation never re-renders a stale one; a diagram-free body
        // never touches the renderer at all.
        let md_node = NodeRef::<leptos::html::Div>::new();
        if diagrams > 0 {
            md_node.on_load(move |el: web_sys::HtmlDivElement| mermaid::mount(&el));
        }
        view! {
            {fm_view}
            <div class="md" node_ref=md_node inner_html=html></div>
        }
            .into_any()
    };

    view! {
        <article class="ticket">
            <h2>
                <span class="id">{move || ticket.with(|t| t.id.clone())}</span>
                {move || ticket.with(|t| t.title.clone())}
            </h2>
            <dl class="meta">
                <div>
                    <dt>"status"</dt>
                    <dd id="t-status" class="status">{move || ticket.with(|t| t.status.clone())}</dd>
                </div>
                <div><dt>"kind"</dt><dd id="t-kind" class="kind">{move || ticket.with(|t| t.kind.clone())}</dd></div>
                <div>
                    <dt>"severity"</dt>
                    <dd>{move || ticket.with(|t| t.severity.clone().unwrap_or_else(|| "—".into()))}</dd>
                </div>
                <div><dt>"epic"</dt><dd>{move || ticket.with(|t| t.epic.clone())}</dd></div>
                <div><dt>"seq"</dt><dd>{move || ticket.with(|t| t.seq)}</dd></div>
                <div><dt>"created"</dt><dd>{move || ticket.with(|t| fmt_unix(t.created_unix))}</dd></div>
                <div><dt>"updated"</dt><dd>{move || ticket.with(|t| fmt_unix(t.updated_unix))}</dd></div>
            </dl>
            <section class="dag">
                <h3>"blocked by"</h3>
                {blockers}
                <h3>"unlocks"</h3>
                {unlocks}
            </section>
            <section class="bodysec">
                <h3>"body"</h3>
                {body}
            </section>
        </article>
    }
}

/// The ticket page (MFE-7). `get` + `blockers` fetch on entry and upsert
/// into the shared row store; the page renders THROUGH the store's
/// signals, so the watch stream keeps it live exactly like the board —
/// no data path of its own. A `not_found` answer shows the not-found
/// state; if the id comes into existence later (watch `created`), the
/// store lookup wins and the page upgrades itself.
#[component]
fn TicketPage(
    id: String,
    rows: RwSignal<Vec<Row>>,
    last_error: RwSignal<Option<String>>,
) -> impl IntoView {
    let not_found = RwSignal::new(false);
    let blockers_loaded = RwSignal::new(false);

    {
        let id = id.clone();
        spawn_local(async move {
            match net::call("get", &json!({ "id": id })).await {
                Ok(events) => {
                    for ev in &events {
                        match ev.get("type").and_then(|t| t.as_str()) {
                            Some("ticket") => {
                                if let Some(t) = parse_ticket(ev) {
                                    upsert(rows, t);
                                }
                            }
                            Some("not_found") => not_found.set(true),
                            Some("error") => {
                                let msg = ev
                                    .get("message")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("unknown error");
                                last_error.set(Some(format!("get {id}: {msg}")));
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => last_error.set(Some(format!("get {id}: {e}"))),
            }
            // Resolve blockers (the DAG position). Resolved blockers
            // upsert into the shared store — their statuses render live
            // from it; an id the call did not resolve renders as
            // dangling once the flag below is set. `not_found` here is
            // the same fact `get` already surfaced.
            match net::call("blockers", &json!({ "id": id })).await {
                Ok(events) => {
                    for ev in &events {
                        if ev.get("type").and_then(|t| t.as_str()) == Some("blocker") {
                            if let Some(t) = parse_ticket(ev) {
                                upsert(rows, t);
                            }
                        }
                    }
                }
                Err(e) => last_error.set(Some(format!("blockers {id}: {e}"))),
            }
            blockers_loaded.set(true);
        });
    }

    let ticket = {
        let id = id.clone();
        Memo::new(move |_| rows.with(|v| v.iter().find(|r| r.id == id).map(|r| r.ticket)))
    };

    let id_view = id.clone();
    view! {
        <div id="ticket-page">
            <p class="crumbs"><a href="#/">"← board"</a></p>
            {move || match ticket.get() {
                Some(t) => ticket_view(t, rows, blockers_loaded).into_any(),
                None if not_found.get() => {
                    let id = id_view.clone();
                    view! {
                        <div id="notfound">
                            <h2>{id}" — not found"</h2>
                            <p>
                                "No ticket has this id. "
                                <a href="#/">"Back to the board"</a>
                                "."
                            </p>
                        </div>
                    }
                        .into_any()
                }
                None => {
                    view! { <p class="loading">"loading "{id_view.clone()}"…"</p> }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn App() -> impl IntoView {
    let rows = RwSignal::new(Vec::<Row>::new());
    let health = RwSignal::new(None::<Health>);
    let conn = RwSignal::new(Conn::Connecting);
    let last_error = RwSignal::new(None::<String>);
    let route = use_route();

    // The whole data plane lives in this one task; ~1s retry keeps a
    // server restart to a few seconds of "reconnecting…" (MFE-2 criterion 3).
    // It runs regardless of route — the ticket page's liveness is this
    // same stream.
    spawn_local(async move {
        loop {
            let err = session(rows, health, conn, last_error)
                .await
                .expect_err("session loop only returns on error");
            leptos::logging::log!("session ended: {err}; reconnecting");
            conn.set(Conn::Reconnecting);
            net::sleep_ms(1000).await;
        }
    });

    let conn_view = move || match conn.get() {
        Conn::Connecting => ("", "connecting…"),
        Conn::Live => ("live", "live · SSE"),
        Conn::Reconnecting => ("reconnecting", "reconnecting…"),
    };
    let health_view = move || {
        health
            .get()
            .map(|h| format!("menger v{} · {} tickets", h.version, h.ticket_count))
    };

    // AGS-12: the nav's hidden/shown state, persisted. The nav stays
    // mounted either way (a class hides it), so its rows keep their DOM
    // identity across hide/reveal and the content column reflows.
    let nav_hidden = RwSignal::new(nav::load_hidden());
    Effect::new(move |_| nav::save_hidden(nav_hidden.get()));

    view! {
        <div id="app" class:nonav=move || nav_hidden.get()>
            <nav::Nav rows route hidden=nav_hidden />
            <div class="content">
                <header>
                    {move || {
                        nav_hidden.get().then(|| view! {
                            <button
                                class="nav-reveal"
                                title="show the nav"
                                on:click=move |_| nav_hidden.set(false)
                            >
                                "☰"
                            </button>
                        })
                    }}
                    <h1><a href="#/">"menger"</a></h1>
                    <span class="tag">
                        {move || match route.get() {
                            Route::Board => "the live board (MFE-3)",
                            Route::Ticket(_) => "ticket (MFE-7)",
                        }}
                    </span>
                    <span id="conn" class=move || conn_view().0>{move || conn_view().1}</span>
                </header>
                <main>
                    <div id="health">{health_view}</div>
                    {move || {
                        last_error.get().map(|msg| view! {
                            <div id="error" role="alert">
                                <span class="msg">{msg}</span>
                                <button on:click=move |_| last_error.set(None)>"dismiss"</button>
                            </div>
                        })
                    }}
                    {move || match route.get() {
                        Route::Board => view! { <Board rows last_error /> }.into_any(),
                        Route::Ticket(id) => view! { <TicketPage id rows last_error /> }.into_any(),
                    }}
                </main>
            </div>
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
