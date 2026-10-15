//! MENGER-12: a ticket's labels in the app — chips on its rows and on its
//! page, and the page's editor.
//!
//! - **A chip** ([`chip`]) is one label: `needs-you` drawn as a small pill,
//!   and a FILTER-22 tag (`data-tag="label:needs-you"`), so a right-click
//!   builds `label needs-you`. `needs-you` wears its own colour: it is what
//!   puts a ticket under the Needs you filter (`g n`, src/needs.rs).
//! - **On a row** ([`row_chips`]): every label the row carries, after the
//!   title — live, as the row is (`labels_changed` reaches the row cache).
//! - **On the ticket page** ([`Editor`]): the chips, each with × to take it
//!   off, and a field that adds one or more (`a, b` or `a b`) — for a person
//!   who may write (no login, or an editor's or admin's grant); a reader sees
//!   the chips alone. Each change is one `tickets.label`; what the server
//!   refuses is shown under the field, in its words.
//!
//! What decides — what a typed entry adds, or why not — is pure and pinned
//! by native tests ([`parse_add`]).

use leptos::prelude::*;
use menger_tickets_api::labels::{normalize, MAX_PER_TICKET, NEEDS_YOU};
use menger_tickets_api::{TicketSummary, TicketsEvent};
use wasm_bindgen_futures::spawn_local;

use crate::{session, signin};

/// What a typed entry adds: each label it names (split at commas and
/// spaces), normalized, each once, in the order typed — or every one that
/// is not a label, with why.
pub(crate) fn parse_add(raw: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for word in raw.split(|c: char| c == ',' || c.is_whitespace()).filter(|w| !w.is_empty()) {
        match normalize(word) {
            Ok(l) if !out.contains(&l) => out.push(l),
            Ok(_) => {}
            Err(why) => bad.push(why),
        }
    }
    match (bad.is_empty(), out.is_empty()) {
        (false, _) => Err(bad.join("; ")),
        (true, true) => Err("type a label, as `needs-you`".into()),
        (true, false) => Ok(out),
    }
}

/// Whether adding `adding` to `held` would pass the per-ticket bound — the
/// server's rule, said before asking.
pub(crate) fn fits(held: &[String], adding: &[String]) -> bool {
    held.len() + adding.iter().filter(|a| !held.contains(a)).count() <= MAX_PER_TICKET
}

/// The chip's class: `needs-you` is marked.
pub(crate) fn chip_class(label: &str) -> &'static str {
    if label == NEEDS_YOU {
        "lchip needs"
    } else {
        "lchip"
    }
}

/// One label, as a chip and a FILTER-22 tag.
pub(crate) fn chip(label: String) -> impl IntoView {
    let tag = format!("label:{label}");
    view! { <span class=chip_class(&label) data-label=label.clone() data-tag=tag title=format!("label: {label}")>{label.clone()}</span> }
}

/// A row's labels, live — nothing when it has none.
pub(crate) fn row_chips(ticket: impl Fn() -> Vec<String> + Send + Sync + 'static) -> impl IntoView {
    move || {
        let labels = ticket();
        (!labels.is_empty()).then(|| view! { <span class="lchips">{labels.into_iter().map(chip).collect_view()}</span> })
    }
}

/// Whether the person may write — no login, or a grant that writes
/// (editor or admin). The server decides each call; this only keeps a
/// reader from being offered what will be refused.
pub(crate) fn may_write() -> bool {
    signin::account().with(|a| a.may_write())
}

/// `tickets.label { id, add, remove }`, with what went wrong.
async fn label(id: String, add: Vec<String>, remove: Vec<String>) -> Result<(), String> {
    let client = session::client().ok_or_else(|| "not connected".to_string())?;
    let add = (!add.is_empty()).then_some(crate::typed::many(add)?);
    let remove = (!remove.is_empty()).then_some(crate::typed::many(remove)?);
    let events = client.label_all(Some(crate::typed::typed(&id)?), None, add, remove).await.map_err(|e| session::describe(&e))?;
    for ev in events {
        match ev {
            TicketsEvent::LabelsChanged { ticket, .. } => {
                crate::cache::put(TicketSummary::from(&ticket));
                return Ok(());
            }
            TicketsEvent::NotFound { id } => return Err(format!("{id}: not found")),
            TicketsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Err("no answer".into())
}

/// The ticket page's labels: the chips, and — for a person who may write —
/// × on each and a field to add.
#[component]
pub(crate) fn Editor(ticket: RwSignal<TicketSummary>) -> impl IntoView {
    let labels = Memo::new(move |_| ticket.with(|t| t.labels.clone()));
    let draft = RwSignal::new(String::new());
    let problem = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let writes = Memo::new(move |_| may_write());
    let send = move |add: Vec<String>, remove: Vec<String>| {
        let id = ticket.with_untracked(|t| t.id.clone());
        busy.set(true);
        problem.set(None);
        spawn_local(async move {
            let done = label(String::from(id), add, remove).await;
            busy.try_set(false);
            match done {
                Ok(()) => {
                    draft.try_set(String::new());
                }
                Err(why) => {
                    problem.try_set(Some(why));
                }
            }
        });
    };
    let add = move || {
        let held = labels.get_untracked();
        match parse_add(&draft.get_untracked()) {
            Ok(adding) if !fits(&held, &adding) => {
                problem.set(Some(format!("a ticket holds at most {MAX_PER_TICKET} labels")))
            }
            Ok(adding) => send(adding, Vec::new()),
            Err(why) => problem.set(Some(why)),
        }
    };
    view! {
        <span class="lchips" id="t-labels-chips">
            {move || {
                let w = writes.get();
                labels
                    .get()
                    .into_iter()
                    .map(|l| {
                        let gone = l.clone();
                        let x = w.then(|| {
                            view! {
                                <button
                                    class="lx"
                                    title=format!("remove the label {gone}")
                                    aria-label=format!("remove the label {gone}")
                                    disabled=move || busy.get()
                                    on:click=move |_| send(Vec::new(), vec![gone.clone()])
                                >
                                    "×"
                                </button>
                            }
                        });
                        view! { <span class="lwrap">{chip(l)}{x}</span> }
                    })
                    .collect_view()
            }}
            {move || (labels.with(Vec::is_empty) && !writes.get()).then_some("—")}
        </span>
        {move || writes.get().then(|| view! {
            <form
                class="ladd"
                on:submit=move |ev| {
                    ev.prevent_default();
                    add();
                }
            >
                <input
                    id="t-label-input"
                    type="text"
                    placeholder="add a label"
                    aria-label="add a label (letters, digits and -)"
                    maxlength="200"
                    prop:value=move || draft.get()
                    on:input=move |ev| {
                        draft.set(event_target_value(&ev));
                        problem.set(None);
                    }
                    disabled=move || busy.get()
                />
                <button type="submit" class="hbtn" disabled=move || busy.get() || draft.with(|d| d.trim().is_empty())>"add"</button>
            </form>
        })}
        {move || problem.get().map(|p| view! { <p class="lproblem" role="alert">{p}</p> })}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A typed entry adds each label it names — commas or spaces between,
    /// lower-cased, each once; anything that is not a label is refused,
    /// named; nothing typed is no label.
    #[test]
    fn a_typed_entry_adds_its_labels() {
        assert_eq!(parse_add("Needs-You"), Ok(strs(&["needs-you"])));
        assert_eq!(parse_add(" a, b  c,a "), Ok(strs(&["a", "b", "c"])));
        let err = parse_add("ok not_ok").unwrap_err();
        assert!(err.contains("`not_ok`"), "{err}");
        assert!(parse_add("  ,  ").is_err());
        assert!(parse_add(&"x".repeat(33)).is_err());
    }

    /// The bound is the server's: at most 16, a label already held not
    /// counted twice.
    #[test]
    fn adding_is_held_to_sixteen() {
        let held: Vec<String> = (0..MAX_PER_TICKET).map(|i| format!("l{i}")).collect();
        assert!(!fits(&held, &strs(&["one-more"])));
        assert!(fits(&held, &strs(&["l3"])), "already held");
        assert!(fits(&held[..15], &strs(&["one-more"])));
    }

    /// `needs-you` wears its own class; every chip is a tag.
    #[test]
    fn needs_you_is_marked() {
        assert_eq!(chip_class("needs-you"), "lchip needs");
        assert_eq!(chip_class("p1"), "lchip");
    }
}
