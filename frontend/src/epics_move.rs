//! ORG-23: **Move to epic…** — the menu's move of a ticket, or of the
//! selection, into another epic: `tickets.reshortcode` to
//! `org:epic:CODE`, all or none, renumbered there in the order chosen.
//!
//! The dialog names the epic (every live one but the reader's) and the
//! shortcode there (the tickets' own letters first), and shows what the
//! move would do before it writes: `reshortcode --dry-run` answers each old
//! id and the key it becomes, drawn `now → epic:CODE-N` ([`preview_rows`]);
//! a refusal (no such shortcode there, an archived epic) is said in the
//! server's words and **move N** stays off. Each old id stays pinned to its
//! ticket, so links, chips and blockers follow it.

use leptos::prelude::*;
use menger_tickets_api::{ShortcodeMove, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::epics::{self, shown_id, shown_in};
use crate::session;

thread_local! {
    /// The tickets the dialog is open on, in order.
    static ASKED: ArcRwSignal<Option<Vec<String>>> = ArcRwSignal::new(None);
}

fn asked() -> ArcRwSignal<Option<Vec<String>>> {
    ASKED.with(Clone::clone)
}

/// Opens the dialog on `ids`.
pub(crate) fn ask(ids: Vec<String>) {
    if !ids.is_empty() {
        asked().set(Some(ids));
    }
}

/// The letters of a key — `MENGER.3-12` and `MENGER-12` are `MENGER`.
pub(crate) fn letters(id: &str) -> String {
    shown_id(id).rsplit_once('-').map(|(c, _)| c.to_string()).unwrap_or_default()
}

/// The shortcodes to offer: the tickets' own letters, each once, in order.
pub(crate) fn codes_of(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in ids.iter().map(|i| letters(i)) {
        if !c.is_empty() && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// The `to` a move into `epic`'s `code` names.
pub(crate) fn to_of(org: &str, epic: &str, code: &str) -> String {
    format!("{org}:{epic}:{}", code.trim().to_ascii_uppercase())
}

/// A dry run's moves as the preview's rows: now, and what it becomes.
pub(crate) fn preview_rows(epic: &str, moves: &[ShortcodeMove]) -> Vec<(String, String)> {
    moves.iter().map(|m| (shown_id(&m.from), shown_in(epic, &m.to))).collect()
}

/// The reshortcode's answer: its moves, or the server's words.
fn answer(evs: Vec<TicketsEvent>) -> Result<(Vec<ShortcodeMove>, Vec<menger_tickets_api::Ticket>), String> {
    for e in evs {
        match e {
            TicketsEvent::Reshortcoded { moves, tickets } => return Ok((moves, tickets)),
            TicketsEvent::Error { message } => return Err(message),
            TicketsEvent::NotFound { id } => return Err(format!("{id}: not found")),
            _ => {}
        }
    }
    Err("no answer".into())
}

/// A ticket as the move names it: its full address when the row is held —
/// a key a namesake's short form shares (`MENGER-2` in genesis and in
/// spring) is then never ambiguous — else its key.
fn named(id: &str) -> String {
    crate::cache::find(id).map(|r| r.with_untracked(|t| t.address.clone())).filter(|a| !a.is_empty()).unwrap_or_else(|| id.to_string())
}

async fn reshortcode(ids: Vec<String>, to: String, dry: bool) -> Result<(Vec<ShortcodeMove>, Vec<menger_tickets_api::Ticket>), String> {
    let client = session::client().ok_or("not connected")?;
    let ids: Vec<String> = ids.iter().map(|i| named(i)).collect();
    let (refs, to) = (crate::typed::many(ids)?, crate::typed::typed(&to)?);
    let message = Some(crate::typed::typed("moved to another epic from the app")?);
    answer(client.reshortcode_all(refs, to, None, None, None, message, Some(dry)).await.map_err(|e| session::describe(&e))?)
}

/// Mounted once by the App: the dialog while it is asked.
#[component]
pub(crate) fn MoveHost() -> impl IntoView {
    let a = asked();
    move || a.get().map(|ids| view! { <MoveDialog ids /> })
}

#[component]
fn MoveDialog(ids: Vec<String>) -> impl IntoView {
    let held = epics::held();
    let others: Vec<(String, String)> = held.with_untracked(|e| e.iter().filter(|x| !x.current).map(|x| (x.org.to_string(), x.name.to_string())).collect());
    let codes = codes_of(&ids);
    let epic = RwSignal::new(others.first().map(|o| o.1.clone()).unwrap_or_default());
    let code = RwSignal::new(codes.first().cloned().unwrap_or_default());
    let preview = RwSignal::new(None::<Result<Vec<(String, String)>, String>>);
    let busy = RwSignal::new(false);
    let n = ids.len();
    let ids = StoredValue::new(ids);
    let orgs = StoredValue::new(others.clone());
    let to = move || {
        let e = epic.get();
        let org = orgs.with_value(|o| o.iter().find(|x| x.1 == e).map(|x| x.0.clone())).unwrap_or_default();
        to_of(&org, &e, &code.get())
    };
    // The preview, asked again whenever the epic or the shortcode moves.
    Effect::new(move |_| {
        let (e, t) = (epic.get(), to());
        if e.is_empty() || code.get().trim().is_empty() {
            preview.set(None);
            return;
        }
        let list = ids.get_value();
        spawn_local(async move {
            let r = reshortcode(list, t, true).await.map(|(m, _)| preview_rows(&e, &m));
            preview.try_set(Some(r));
        });
    });
    let close = move || asked().set(None);
    let go = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let (list, t, e) = (ids.get_value(), to(), epic.get_untracked());
        spawn_local(async move {
            match reshortcode(list, t, false).await {
                Ok((moves, _)) => {
                    asked().set(None);
                    crate::selection::clear();
                    let first = moves.first().map(|m| shown_in(&e, &m.to)).unwrap_or_default();
                    crate::menu::toast(format!("moved {} to {e} — {first}{}", moves.len(), if moves.len() > 1 { " …" } else { "" }));
                }
                Err(why) => {
                    busy.try_set(false);
                    preview.try_set(Some(Err(why)));
                }
            }
        });
    };
    let ok = move || preview.with(|p| matches!(p, Some(Ok(_)))) && !busy.get();
    let none_else = others.is_empty();
    view! {
        <div class="overlay" on:click=move |_| close()>
            <div id="epic-move" class="dialog" role="dialog" aria-modal="true" aria-labelledby="em-h" data-count=n
                on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { close() }>
                <h2 id="em-h">{if n == 1 { "Move 1 ticket to another epic".to_string() } else { format!("Move {n} tickets to another epic") }}</h2>
                {if none_else {
                    view! { <p class="hint">"There is no other live epic to move into. "<a class="lnk" href="#/epics" on:click=move |_| close()>"manage epics →"</a></p> }.into_any()
                } else {
                    view! {
                        <div class="emove-pick">
                            <label>"epic "
                                <select aria-label="epic" on:change=move |ev| epic.set(event_target_value(&ev))>
                                    {others.iter().map(|(_, e)| view! { <option value=e.clone() selected=*e == epic.get_untracked()>{e.clone()}</option> }).collect_view()}
                                </select>
                            </label>
                            <label>"into "
                                <input class="mono" list="emove-codes" aria-label="shortcode" size="10" prop:value=move || code.get()
                                    on:change=move |ev| code.set(event_target_value(&ev)) />
                                <datalist id="emove-codes">{codes.iter().map(|c| view! { <option value=c.clone()></option> }).collect_view()}</datalist>
                            </label>
                        </div>
                    }.into_any()
                }}
                {move || match preview.get() {
                    None => None,
                    Some(Err(why)) => Some(view! { <p class="merr" role="alert">{why}</p> }.into_any()),
                    Some(Ok(rows)) => {
                        let pin = rows.first().map(|r| r.0.clone()).unwrap_or_default();
                        Some(view! {
                            <table class="emove-pv mono">
                                <thead><tr><th>"now"</th><th></th><th>"becomes"</th></tr></thead>
                                <tbody>{rows.into_iter().map(|(a, b)| view! { <tr><td>{a}</td><td aria-hidden="true">"→"</td><td>{b}</td></tr> }).collect_view()}</tbody>
                            </table>
                            <p class="hint">"Renumbered in this order. Each old id stays pinned: "<span class="mono">{pin}</span>" still opens its ticket, and blockers, chips and links follow it."</p>
                        }.into_any())
                    }
                }}
                <div class="sacts">
                    <button data-act="cancel" on:click=move |_| close()>"cancel"</button>
                    <button class="primary" data-act="move" disabled=move || !ok() on:click=move |_| go()>{format!("move {n}")}</button>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_are_the_tickets_own_letters_once() {
        let ids = vec!["MENGER-12".to_string(), "MENGER.3-13".into(), "KILN-4".into()];
        assert_eq!(codes_of(&ids), vec!["MENGER".to_string(), "KILN".into()]);
        assert_eq!(letters("SYNAPSE-CC-2"), "SYNAPSE-CC");
    }

    #[test]
    fn a_move_names_the_epic_and_the_preview_says_where_each_lands() {
        assert_eq!(to_of("hypermemetic", "fresh", " menger"), "hypermemetic:fresh:MENGER");
        let moves = vec![ShortcodeMove { from: "MENGER-12".into(), to: "MENGER.3-3".into() }];
        assert_eq!(preview_rows("fresh", &moves), vec![("MENGER-12".to_string(), "fresh:MENGER-3".to_string())]);
    }
}
