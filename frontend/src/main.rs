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
//!
//! AGS-16: the ticket menu (src/menu.rs). Every element that renders a
//! ticket carries `data-ticket-id`; one document-level listener turns a
//! right-click on any of them into the same menu (Open / Open in new tab
//! / Copy code / Set status ▸), mounted once by the App.
//!
//! AGS-17: light and dark mode (src/theme.rs). index.html's palette is
//! two value sets under one set of token names; a header toggle cycles
//! system → light → dark by stamping `data-theme` on the root, persisted
//! client-side, and every view re-colors through the tokens with no
//! re-render — only the drawn mermaid diagrams are redrawn.

//! AGS-20: children are visible. A ticket carries `parent`/`children`
//! (AGS-19's model); the nav nests a ticket's children under its row at
//! any depth, across epics, with sub-shortcode epics nesting under their
//! parent scope (src/nav.rs); the ticket page grows a parent crumb and a
//! `children` list beneath the DAG; a board row marks that it decomposes.
//! Nothing new is fetched — `children_changed` rides the same watch
//! stream, so a child born anywhere appears nested without a reload.
//!
//! AGS-18: the app is drivable from the keyboard alone (src/keys.rs): the
//! nav is a WAI-ARIA tree with a roving tabindex, board rows take focus
//! and open on Enter, `/` (or Ctrl/⌘+K) opens a jump palette and `?` a
//! cheat sheet — both `role="dialog"` overlays that trap Tab and give
//! focus back. Mouse behavior is untouched; the keyboard is an addition,
//! not a mode.

mod keys;
mod md;
mod menu;
mod nav;

mod mermaid;
mod net;
mod theme;

use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

/// The closed status vocabulary (src/store.rs `Status`) — the legal UI
/// choices. The server remains the validator; anything else it receives
/// comes back as an in-band `error` event.
pub(crate) const STATUSES: [&str; 7] = [
    "Pending",
    "Ready",
    "Blocked",
    "Complete",
    "Idea",
    "Epic",
    "Superseded",
];

/// AGS-15: the classes that make an element wear a status as color —
/// `st` (the paint rule) plus `st-<status>` (a row of index.html's status
/// table). Every place a status appears (nav tag, board control, ticket
/// page, blockers pills) puts these on the element that shows the word,
/// so the word stays present and the color is the table's. A status
/// outside the table paints nothing extra and stays legible text.
pub(crate) fn st_class(status: &str) -> String {
    format!("st st-{}", status.to_ascii_lowercase())
}

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
    /// AGS-19/20: the ticket this one is an output of (any epic). The
    /// server omits it when unset, and it is immutable for a ticket's
    /// lifetime — there is no re-parent — which is what lets the nav's
    /// forest be a memo over the row *list* (src/nav.rs).
    #[serde(default)]
    pub(crate) parent: Option<String>,
    /// AGS-19/20: the tickets whose `parent` is this one, in birth order,
    /// derived by the server and kept live by `children_changed`.
    #[serde(default)]
    pub(crate) children: Vec<String>,
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
            // Every mutation event carries the full updated ticket —
            // AGS-19's `children_changed` carries the full *parent*, which
            // is how a page and a nav that never re-fetch learn that a
            // child was born (AGS-20).
            Some("created") | Some("status_changed") | Some("unlocks_changed")
            | Some("children_changed") => {
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

/// The app's one mutation: `set_status`, fired from a row's select or
/// the ticket menu (AGS-16) — one call path. The caller's control is left
/// showing the user's choice while the call is in flight; the
/// authoritative update arrives back through `watch`. Any failure —
/// transport-level or an in-band `error`/`not_found` event — surfaces in
/// the banner and runs `on_failure` (the board snaps its select back to
/// the last server-confirmed status; the menu has nothing to revert —
/// its chips follow the server).
pub(crate) fn request_set_status(
    id: String,
    chosen: String,
    last_error: RwSignal<Option<String>>,
    on_failure: impl FnOnce() + 'static,
) {
    spawn_local(async move {
        let failure = match net::call("set_status", &json!({"id": id, "status": chosen})).await {
            Ok(events) => first_error(&events),
            Err(e) => Some(e),
        };
        if let Some(msg) = failure {
            last_error.set(Some(format!("set_status {id} -> {chosen}: {msg}")));
            on_failure();
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

pub(crate) fn navigate_to_ticket(id: &str) {
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
    // AGS-18: rows are focusable (roving tabindex — every row renders
    // with -1 and exactly one is promoted to 0), so j/k move a real focus
    // and Enter opens what is focused. The status <select> keeps its own
    // place in the tab order, after the row.
    Effect::new(move |_| {
        rows.with(|v| v.len());
        spawn_local(async {
            net::sleep_ms(0).await;
            keys::ensure_board_tab_stop();
        });
    });
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
                                tabindex="-1"
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
                                    // AGS-20: no `parent` column — just the
                                    // mark that this row decomposes, and how
                                    // far (the nav is where the tree is).
                                    {move || {
                                        ticket
                                            .with(|t| {
                                                let n = t.children.len();
                                                (n > 0)
                                                    .then(|| {
                                                        let word = if n == 1 { "child" } else { "children" };
                                                        view! {
                                                            <span class="haskids" title=format!("{n} {word}")>
                                                                {format!("▸{n}")}
                                                            </span>
                                                        }
                                                    })
                                            })
                                    }}
                                </td>
                                <td>{move || ticket.with(|t| t.title.clone())}</td>
                                <td class="kind">{move || ticket.with(|t| t.kind.clone())}</td>
                                <td class="status">
                                    <select
                                        // The control is the chip: it wears the
                                        // server-confirmed status's color (AGS-15).
                                        class=move || format!("status {}", ticket.with(|t| st_class(&t.status)))
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
                                                request_set_status(id.clone(), chosen, last_error, move || {
                                                    sel.set_value(&current)
                                                });
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
                                                        class=st_class(s)
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
/// dangling entry resolves live while the page is open. The entry carries
/// `data-ticket-id` (the AGS-16 menu's attachment) exactly while it names
/// a ticket — a dangling reference is not one.
fn dag_entry(
    id: String,
    rows: RwSignal<Vec<Row>>,
    dangling_when: Option<RwSignal<bool>>,
) -> impl IntoView {
    let found = {
        let id = id.clone();
        Memo::new(move |_| rows.with(|v| v.iter().find(|r| r.id == id).map(|r| r.ticket)))
    };
    let dangling = move || found.get().is_none() && dangling_when.is_some_and(|loaded| loaded.get());
    let attr_id = id.clone();
    let menu_id = id.clone();
    view! {
        <li data-dag-id=attr_id data-ticket-id=move || (!dangling()).then(|| menu_id.clone())>
            {move || match found.get() {
                Some(t) => {
                    let href = format!("#/ticket/{id}");
                    view! {
                        <a href=href>{id.clone()}</a>
                        <span class=move || format!("bstatus {}", t.with(|x| st_class(&x.status)))>
                            {move || t.with(|x| x.status.clone())}
                        </span>
                        <span class="btitle">{move || t.with(|x| x.title.clone())}</span>
                    }
                        .into_any()
                }
                None if dangling() => {
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

/// A ticket's ancestors, root first, from the row store (AGS-20). Stops
/// at the first id the store does not hold — a deep link can be on screen
/// before the snapshot lands — and at a repeat, so a hand-edited parent
/// cycle cannot spin the crumb.
fn ancestors(rows: RwSignal<Vec<Row>>, id: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur = id.to_string();
    rows.with(|v| {
        while let Some(row) = v.iter().find(|r| r.id == cur) {
            let Some(p) = row.ticket.with(|t| t.parent.clone()) else { break };
            if p == id || chain.contains(&p) {
                break;
            }
            chain.push(p.clone());
            cur = p;
        }
    });
    chain.reverse();
    chain
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
    // AGS-20: the ticket's own decomposition. `children` is the server's
    // derived list, so a child born anywhere arrives over `watch`
    // (`children_changed` carries this whole parent) and the list grows
    // with no re-fetch.
    let children_ids = Memo::new(move |_| ticket.with(|t| t.children.clone()));
    let parent_id = Memo::new(move |_| ticket.with(|t| t.parent.clone()));

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
    let children = move || {
        let ids = children_ids.get();
        if ids.is_empty() {
            view! { <p class="none">"none"</p> }.into_any()
        } else {
            view! {
                <ul id="children">
                    {ids
                        .into_iter()
                        .map(|cid| dag_entry(cid, rows, None))
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

    // The id never changes for a row's lifetime (see `Row`), so the
    // header's menu attachment is static.
    let header_id = ticket.with_untracked(|t| t.id.clone());
    view! {
        <article class="ticket">
            <h2 data-ticket-id=header_id>
                <span class="id">{move || ticket.with(|t| t.id.clone())}</span>
                {move || ticket.with(|t| t.title.clone())}
            </h2>
            <dl class="meta">
                <div>
                    <dt>"status"</dt>
                    <dd id="t-status" class=move || format!("status {}", ticket.with(|t| st_class(&t.status)))>
                        {move || ticket.with(|t| t.status.clone())}
                    </dd>
                </div>
                <div><dt>"kind"</dt><dd id="t-kind" class="kind">{move || ticket.with(|t| t.kind.clone())}</dd></div>
                <div>
                    <dt>"severity"</dt>
                    <dd>{move || ticket.with(|t| t.severity.clone().unwrap_or_else(|| "—".into()))}</dd>
                </div>
                // AGS-20: the ticket this one is an output of, as a link
                // (the crumb above walks the whole chain).
                <div>
                    <dt>"parent"</dt>
                    <dd id="t-parent">
                        {move || match parent_id.get() {
                            Some(p) => {
                                let (href, code) = (format!("#/ticket/{p}"), p.clone());
                                view! { <a class="crumb" data-ticket-id=p href=href>{code}</a> }
                                    .into_any()
                            }
                            None => view! { "—" }.into_any(),
                        }}
                    </dd>
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
            // AGS-20: decomposition, beneath the DAG — the DAG is what
            // this ticket waits on and frees; this is what it is made of.
            <section class="dag kidsec">
                <h3>"children"</h3>
                {children}
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

    // AGS-20: the crumb is the ancestor chain — the decomposition read
    // downward, root first — resolved through the same row store, so a
    // parent that arrives later fills itself in.
    let crumbs = {
        let id = id.clone();
        move || {
            ancestors(rows, &id)
                .into_iter()
                .map(|a| {
                    let (href, code) = (format!("#/ticket/{a}"), a.clone());
                    view! {
                        <span class="sep" aria-hidden="true">" / "</span>
                        <a class="crumb" data-ticket-id=a href=href>{code}</a>
                    }
                })
                .collect_view()
        }
    };

    let id_view = id.clone();
    view! {
        <div id="ticket-page">
            <p class="crumbs"><a href="#/">"← board"</a>{crumbs}</p>
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

    // AGS-18: one window-level key listener for the app's lifetime, and
    // the overlay it opens. Both are additive — every existing pointer
    // path is untouched.
    let overlay = RwSignal::new(keys::Overlay::None);
    keys::install(overlay);

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
                    // AGS-17: the theme toggle (system → light → dark).
                    <theme::Toggle />
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
            // AGS-16: mounted once, outside every ticket element, so a
            // right-click inside the menu itself never re-targets it.
            <menu::TicketMenu rows last_error />
            <keys::Overlays rows overlay />
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[cfg(test)]
mod tests {
    use super::{st_class, STATUSES};

    fn index_html() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/index.html")).unwrap()
    }

    /// The app's stylesheet: what sits between the shell's <style> tags.
    fn app_css() -> String {
        let html = index_html();
        let start = html.find("<style>").expect("<style>") + "<style>".len();
        let end = html.find("</style>").expect("</style>");
        html[start..end].to_string()
    }

    /// AGS-15: every status in the closed vocabulary has a row in
    /// index.html's status table (a bg and an fg), and the class the
    /// views put on a chip names exactly that row. A new status without
    /// a row would paint as plain text; this fails first. AGS-17: and a
    /// row in the dark table, under both dark mechanisms.
    #[test]
    fn every_status_has_a_row_in_the_status_table() {
        let css = app_css();
        for s in STATUSES {
            let class = st_class(s);
            let row = class.strip_prefix("st ").unwrap();
            assert_eq!(row, format!("st-{}", s.to_lowercase()));
            for prefix in ["", &format!("{DARK_ATTR} "), &format!("{DARK_MEDIA} ")] {
                let line = css
                    .lines()
                    .find(|l| l.trim_start().starts_with(&format!("{prefix}.{row} ")))
                    .unwrap_or_else(|| panic!("no status-table row for {s} ({prefix}.{row})"));
                assert!(
                    line.contains("--st-bg: #") && line.contains("--st-fg: #"),
                    "{s}: {line}"
                );
            }
        }
        assert_eq!(st_class("Blocked"), "st st-blocked");
        let scan = scan(&css);
        let want: std::collections::BTreeSet<String> =
            STATUSES.iter().map(|s| s.to_ascii_lowercase()).collect();
        for (table, rows) in [
            ("light", &scan.st_light),
            ("dark (attribute)", &scan.st_dark_attr),
            ("dark (media)", &scan.st_dark_media),
        ] {
            let got: std::collections::BTreeSet<String> = rows.iter().map(|r| r.0.clone()).collect();
            assert_eq!(got, want, "the {table} status table names exactly the vocabulary");
            assert_eq!(rows.len(), STATUSES.len(), "the {table} status table: one row per status");
        }
    }

    /// True if `line` holds a color literal: `#` + 3/4/6/8 hex digits as a
    /// whole token (an id selector like `#board` or `#error` is not one —
    /// its letters are not all hex, or it runs on into more word
    /// characters), an `rgb(`/`rgba(`/`hsl(` function, or a named color.
    fn has_color_literal(line: &str) -> bool {
        let bytes = line.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b != b'#' {
                continue;
            }
            let run = bytes[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            let next = bytes.get(i + 1 + run).copied();
            let ends_token = !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
            if matches!(run, 3 | 4 | 6 | 8) && ends_token {
                return true;
            }
        }
        let lower = line.to_ascii_lowercase();
        if ["rgb(", "rgba(", "hsl(", "hsla(", "color("].iter().any(|f| lower.contains(f)) {
            return true;
        }
        // The named colors a scaffold reaches for; `transparent`, `inherit`,
        // `none`, and `currentColor` are not palette choices.
        let named = [
            "white", "black", "red", "green", "blue", "gray", "grey", "yellow", "orange",
            "purple", "pink", "silver", "navy", "teal", "maroon", "olive", "lime", "aqua",
        ];
        // A hyphen is part of the word: `white-space` names no color.
        lower
            .split(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
            .any(|w| named.contains(&w))
    }

    /// The two dark selectors (AGS-17): the attribute the toggle stamps,
    /// and the media-query guard that a stored "light" opts out of.
    const DARK_ATTR: &str = ":root[data-theme=\"dark\"]";
    const DARK_MEDIA: &str = ":root:not([data-theme=\"light\"])";
    const MEDIA_DARK: &str = "@media (prefers-color-scheme: dark) {";

    /// The stylesheet, sorted into the places a color may be named — the
    /// light `:root` block, the two dark blocks, the light status table,
    /// the dark status table (under both mechanisms) — and everything
    /// else that names one (`strays`). Status rows are `(status, body)`;
    /// block declarations are kept verbatim (trimmed) so the two dark
    /// copies can be compared line by line.
    #[derive(Default, Debug)]
    struct Scan {
        light: Vec<String>,
        dark_attr: Vec<String>,
        dark_media: Vec<String>,
        st_light: Vec<(String, String)>,
        st_dark_attr: Vec<(String, String)>,
        st_dark_media: Vec<(String, String)>,
        strays: Vec<String>,
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Block {
        None,
        Light,
        DarkAttr,
        DarkMedia,
    }

    /// A status row's `(status, normalized body)` from what follows `.st-`.
    fn status_row(rest: &str) -> (String, String) {
        let name = rest.split(|c: char| c == ' ' || c == '{').next().unwrap_or_default();
        let body = rest[name.len()..].split_whitespace().collect::<Vec<_>>().join(" ");
        (name.to_string(), body)
    }

    fn scan(css: &str) -> Scan {
        let mut out = Scan::default();
        let mut block = Block::None;
        let mut in_media = false;
        for line in css.lines() {
            let t = line.trim();
            if block != Block::None {
                if t == "}" {
                    block = Block::None;
                } else {
                    match block {
                        Block::Light => out.light.push(t.to_string()),
                        Block::DarkAttr => out.dark_attr.push(t.to_string()),
                        Block::DarkMedia => out.dark_media.push(t.to_string()),
                        Block::None => unreachable!(),
                    }
                }
                continue;
            }
            if t == MEDIA_DARK {
                assert!(!in_media, "nested prefers-color-scheme media blocks");
                in_media = true;
                continue;
            }
            if in_media && t == "}" {
                in_media = false;
                continue;
            }
            if t == ":root {" {
                block = Block::Light;
                continue;
            }
            if t == format!("{DARK_ATTR} {{") {
                assert!(!in_media, "the attribute block must not sit inside the media query");
                block = Block::DarkAttr;
                continue;
            }
            if t == format!("{DARK_MEDIA} {{") {
                assert!(in_media, "the guarded block must sit inside the media query: {t}");
                block = Block::DarkMedia;
                continue;
            }
            if let Some(rest) = t.strip_prefix(".st-") {
                out.st_light.push(status_row(rest));
                continue;
            }
            if let Some(rest) = t.strip_prefix(DARK_ATTR).and_then(|r| r.strip_prefix(" .st-")) {
                assert!(!in_media, "{t}");
                out.st_dark_attr.push(status_row(rest));
                continue;
            }
            if let Some(rest) = t.strip_prefix(DARK_MEDIA).and_then(|r| r.strip_prefix(" .st-")) {
                assert!(in_media, "the guarded status rows must sit inside the media query: {t}");
                out.st_dark_media.push(status_row(rest));
                continue;
            }
            if has_color_literal(t) {
                out.strays.push(t.to_string());
            }
        }
        assert_eq!(block, Block::None, "unterminated block");
        assert!(!in_media, "unterminated media query");
        out
    }

    /// A block's declarations as `(property, value)`, comments stripped
    /// (a comment may span lines, and may say "black").
    fn declarations(block: &[String]) -> Vec<(String, String)> {
        let mut text = block.join("\n");
        while let Some(start) = text.find("/*") {
            let end = text[start..].find("*/").map(|e| start + e + 2).unwrap_or(text.len());
            text.replace_range(start..end, " ");
        }
        text.split(';')
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(|d| {
                let (name, value) = d.split_once(':').unwrap_or_else(|| panic!("not a declaration: {d}"));
                (name.trim().to_string(), value.trim().to_string())
            })
            .collect()
    }

    /// The names of a block's color tokens (custom properties whose value
    /// holds a color literal) — the set the themes must agree on.
    fn color_tokens(block: &[String]) -> std::collections::BTreeSet<String> {
        declarations(block)
            .into_iter()
            .filter(|(name, value)| name.starts_with("--") && has_color_literal(value))
            .map(|(name, _)| name)
            .collect()
    }

    /// AGS-15 criterion 4, evolved by AGS-17: color literals live only in
    /// the light `:root` block, the two dark blocks, and the two status
    /// tables (the dark one under both mechanisms). Every other rule
    /// paints through `var(--…)`.
    #[test]
    fn colors_live_only_in_the_theme_blocks_and_the_status_tables() {
        let scan = scan(&app_css());
        let light_literals = scan.light.iter().filter(|l| has_color_literal(l)).count();
        assert!(light_literals >= 10, "the light block holds the palette ({light_literals} lines)");
        for (dark, name) in [(&scan.dark_attr, "attribute"), (&scan.dark_media, "media")] {
            let n = dark.iter().filter(|l| has_color_literal(l)).count();
            assert!(n >= 10, "the dark ({name}) block holds the palette ({n} lines)");
        }
        for (rows, table) in [(&scan.st_light, "light"), (&scan.st_dark_attr, "dark attribute"), (&scan.st_dark_media, "dark media")] {
            assert_eq!(rows.len(), STATUSES.len(), "one {table} table row per status");
            for (status, body) in rows {
                assert!(has_color_literal(body), "{table} status-table row without colors: {status} {body}");
            }
        }
        assert!(
            scan.strays.is_empty(),
            "color literals outside the theme blocks / status tables:\n{}",
            scan.strays.join("\n")
        );
    }

    /// AGS-17: the dark blocks define every color token the light block
    /// defines and nothing else — a token defined in one theme only is a
    /// bug (it would paint the wrong theme's value, or nothing). The type
    /// tokens (`--mono`, `--sans`) are theme-invariant and live once, in
    /// the light block; a dark block may set `color-scheme` (form
    /// controls, scrollbars) and no other plain property.
    #[test]
    fn dark_blocks_define_exactly_the_light_blocks_color_tokens() {
        let scan = scan(&app_css());
        let light = color_tokens(&scan.light);
        assert!(light.contains("--bg") && light.contains("--ink") && light.contains("--shadow"), "{light:?}");
        for (dark, name) in [(&scan.dark_attr, "attribute"), (&scan.dark_media, "media")] {
            assert_eq!(color_tokens(dark), light, "dark ({name}) block's color tokens vs the light block's");
            for (prop, value) in declarations(dark) {
                if prop.starts_with("--") {
                    assert!(has_color_literal(&value), "dark ({name}) token without a color: {prop}: {value}");
                } else {
                    assert_eq!((prop.as_str(), value.as_str()), ("color-scheme", "dark"), "dark ({name}) block: unexpected declaration");
                }
            }
        }
    }

    /// AGS-17: the two dark mechanisms — the media query (guarded) and the
    /// stamped attribute — carry the same values, token for token and
    /// status for status, so the toggle and the system preference paint
    /// one dark, not two.
    #[test]
    fn both_dark_mechanisms_carry_identical_values() {
        let scan = scan(&app_css());
        assert_eq!(declarations(&scan.dark_attr), declarations(&scan.dark_media), "dark token blocks differ");
        assert_eq!(scan.st_dark_attr, scan.st_dark_media, "dark status tables differ");
        assert_eq!(scan.st_dark_attr.len(), STATUSES.len());
    }

    /// The guard is live: a literal planted anywhere outside the allowed
    /// blocks is reported — in a component rule, as a named color, as
    /// `rgb()`, and inside the media query but outside the guarded block
    /// — and a token added to one dark block only breaks the set equality.
    #[test]
    fn literal_guard_catches_a_planted_stray() {
        let css = app_css();
        assert!(scan(&css).strays.is_empty(), "precondition: the real stylesheet is clean");
        let plants = [
            "  #nav .row.sel { background: #cfe2f7; }",
            "  header h1 { color: black; }",
            "  td.id { color: rgb(0, 102, 204); }",
        ];
        for plant in plants {
            let planted = format!("{css}\n{plant}\n");
            let strays = scan(&planted).strays;
            assert_eq!(strays, vec![plant.trim().to_string()], "planted: {plant}");
        }
        // Inside the media query, but not in the guarded block: still a stray.
        let planted = css.replacen(MEDIA_DARK, &format!("{MEDIA_DARK}\n    body {{ background: #000; }}"), 1);
        assert_eq!(scan(&planted).strays, vec!["body { background: #000; }".to_string()]);
        // A token in one dark block only: the set equality fails.
        let planted = css.replacen(&format!("{DARK_ATTR} {{"), &format!("{DARK_ATTR} {{\n    --only-here: #123456;"), 1);
        let s = scan(&planted);
        assert!(s.strays.is_empty(), "a token inside a dark block is not a stray");
        assert_ne!(color_tokens(&s.dark_attr), color_tokens(&s.light));
        assert_ne!(declarations(&s.dark_attr), declarations(&s.dark_media));
    }

    /// AGS-18: focus is painted wherever a key can put it — the nav
    /// tree's rows, the board's rows, the palette's input — and it is
    /// painted as a *ring*, from the token layer. The selected row keeps
    /// its own idiom (a `--row-sel` tint with an inset accent bar), so
    /// "where I am" and "what is open" cannot collapse into one look.
    #[test]
    fn focus_is_a_token_ring_and_not_the_selected_row_s_tint() {
        let css = app_css();
        let lines: Vec<&str> = css.lines().map(str::trim).collect();
        let start = lines
            .iter()
            .position(|l| l.starts_with("#nav .row:focus,"))
            .expect("no focus rule for the nav and board rows");
        let block: String = lines[start..]
            .iter()
            .take_while(|l| !l.starts_with('}'))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        for reachable in ["#nav .row:focus", "tbody#board tr:focus", "#palette-input:focus"] {
            assert!(block.contains(reachable), "{reachable} has no focus ring: {block}");
        }
        assert!(block.contains("outline:") && block.contains("var(--accent)"), "{block}");
        // A ring, not a fill: the selected row is the one with a tint.
        assert!(!block.contains("background"), "focus paints a background: {block}");
        let sel = lines
            .iter()
            .find(|l| l.starts_with("#nav .row.sel {"))
            .expect("no selected-row rule");
        assert!(sel.contains("var(--row-sel)") && sel.contains("inset"), "{sel}");
    }

    #[test]
    fn color_literal_scanner_knows_ids_from_hex() {
        assert!(has_color_literal("  --bg: #fbfbf9;"));
        assert!(has_color_literal("  .st-blocked { --st-bg: #fdecec; --st-fg: #a23; }"));
        assert!(has_color_literal("  a { color: rgb(1, 2, 3); }"));
        assert!(has_color_literal("  a { background: #fff; }"));
        assert!(has_color_literal("  a { color: white; }"));
        assert!(!has_color_literal("  #error button { color: inherit; }"));
        assert!(!has_color_literal("  td.id { color: var(--accent); white-space: nowrap; }"));
        assert!(!has_color_literal("  tbody#board tr:hover td { background: var(--hover); }"));
        assert!(!has_color_literal("  #app.nonav #nav { display: none; }"));
        assert!(!has_color_literal("  .st { border-color: transparent; }"));
        assert!(!has_color_literal("  #blockers a, #unlocks a { color: var(--accent); }"));
    }
}
