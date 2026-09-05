//! MFE-2: the Leptos CSR scaffold. Proves the data plane end to end —
//! renders health + the ticket list from unary REST calls, then keeps the
//! list live from the `tickets.watch` SSE stream (no polling; nothing is
//! requested after page load except the stream itself and the one-time
//! resync a *re*connect performs). The view layer owns no data routing:
//! everything goes through the same `/rest/tickets/{method}` surface any
//! curl user sees (MFE-1 house rule).

mod net;

use leptos::prelude::*;
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen_futures::spawn_local;

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

fn upsert(tickets: RwSignal<Vec<Ticket>>, t: Ticket) {
    tickets.update(|v| {
        match v.iter_mut().find(|x| x.id == t.id) {
            Some(slot) => *slot = t,
            None => v.push(t),
        }
        v.sort_by(|a, b| (&a.epic, a.seq).cmp(&(&b.epic, b.seq)));
    });
}

fn parse_ticket(ev: &serde_json::Value) -> Option<Ticket> {
    serde_json::from_value(ev.get("ticket")?.clone()).ok()
}

/// One connected session: subscribe first (so nothing created between
/// list and watch is missed — events are idempotent upserts by id), then
/// snapshot via unary calls, then pump the stream until it dies. Err ends
/// the session; the caller loop reconnects.
async fn session(
    tickets: RwSignal<Vec<Ticket>>,
    health: RwSignal<Option<Health>>,
    conn: RwSignal<Conn>,
) -> Result<(), String> {
    let mut watch = net::open_watch(&json!({})).await?;

    let health_events = net::call("health", &json!({})).await?;
    if let Some(h) = health_events
        .first()
        .and_then(|e| serde_json::from_value::<Health>(e.clone()).ok())
    {
        health.set(Some(h));
    }

    let list_events = net::call("list", &json!({})).await?;
    let mut snapshot: Vec<Ticket> = list_events.iter().filter_map(parse_ticket).collect();
    snapshot.sort_by(|a, b| (&a.epic, a.seq).cmp(&(&b.epic, b.seq)));
    tickets.set(snapshot);
    conn.set(Conn::Live);

    loop {
        let ev = watch.next_event().await?;
        match ev.get("type").and_then(|t| t.as_str()) {
            // Every mutation event carries the full updated ticket.
            Some("created") | Some("status_changed") | Some("unlocks_changed") => {
                if let Some(t) = parse_ticket(&ev) {
                    let created = ev["type"] == "created";
                    upsert(tickets, t);
                    if created {
                        health.update(|h| {
                            if let Some(h) = h {
                                h.ticket_count += 1;
                            }
                        });
                    }
                }
            }
            _ => {} // in-band errors / future event kinds: ignore, stay live
        }
    }
}

#[component]
fn App() -> impl IntoView {
    let tickets = RwSignal::new(Vec::<Ticket>::new());
    let health = RwSignal::new(None::<Health>);
    let conn = RwSignal::new(Conn::Connecting);

    // The whole data plane lives in this one task; ~1s retry keeps a
    // server restart to a few seconds of "reconnecting…" (MFE-2 criterion 3).
    spawn_local(async move {
        loop {
            let err = session(tickets, health, conn)
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
            <span class="tag">"Leptos scaffold (MFE-2)"</span>
            <span id="conn" class=move || conn_view().0>{move || conn_view().1}</span>
        </header>
        <main>
            <div id="health">{health_view}</div>
            <table>
                <thead>
                    <tr>
                        <th>"id"</th><th>"title"</th><th>"type"</th>
                        <th>"status"</th><th>"severity"</th><th>"blocked by"</th>
                    </tr>
                </thead>
                <tbody id="board">
                    <For
                        each=move || tickets.get()
                        // status/updated in the key: a change replaces the row.
                        key=|t| (t.id.clone(), t.status.clone(), t.updated_unix)
                        children=move |t: Ticket| {
                            view! {
                                <tr data-ticket-id=t.id.clone()>
                                    <td class="id">{t.id.clone()}</td>
                                    <td>{t.title.clone()}</td>
                                    <td>{t.ticket_type.clone()}</td>
                                    <td class="status">{t.status.clone()}</td>
                                    <td>{t.severity.clone().unwrap_or_default()}</td>
                                    <td>{t.blocked_by.join(" ")}</td>
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
