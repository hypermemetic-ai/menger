//! MFE-3: the live board — a read-and-status surface. Every ticket
//! renders as a row (ordered by epic, then sequence) from one initial
//! fetch plus `tickets.watch` SSE updates — no polling. Each row carries
//! a status control (the closed seven-value vocabulary; the server stays
//! the validator); in-band `error` events on the app's own calls surface
//! in a visible banner. There is deliberately NO creation affordance
//! (owner, 2026-09-05): tickets are created by agents/CLI/API — the
//! browser reads and changes status.
//!
//! Reactivity is signal-grained: each row owns an inner `RwSignal<Ticket>`,
//! so an incoming event updates the affected row's text in place — the
//! row list signal only fires on insert/remove, and untouched rows keep
//! their DOM identity across every update.

mod net;

use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;
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
struct Ticket {
    id: String,
    epic: String,
    seq: u32,
    title: String,
    status: String,
    ticket_type: String,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    unlocks: Vec<String>,
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
struct Row {
    id: String,
    epic: String,
    seq: u32,
    ticket: RwSignal<Ticket>,
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

#[component]
fn App() -> impl IntoView {
    let rows = RwSignal::new(Vec::<Row>::new());
    let health = RwSignal::new(None::<Health>);
    let conn = RwSignal::new(Conn::Connecting);
    let last_error = RwSignal::new(None::<String>);

    // The whole data plane lives in this one task; ~1s retry keeps a
    // server restart to a few seconds of "reconnecting…" (MFE-2 criterion 3).
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

    view! {
        <header>
            <h1>"menger"</h1>
            <span class="tag">"the live board (MFE-3)"</span>
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
            <table>
                <thead>
                    <tr>
                        <th>"id"</th><th>"title"</th><th>"type"</th>
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
                            view! {
                                <tr data-ticket-id=row.id.clone()>
                                    <td class="id">{row.id.clone()}</td>
                                    <td>{move || ticket.with(|t| t.title.clone())}</td>
                                    <td>{move || ticket.with(|t| t.ticket_type.clone())}</td>
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
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
