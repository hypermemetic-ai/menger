//! ORG-13: the epics of the reader's organization, and the nav head's
//! switcher between them.
//!
//! - **What is held**: `epics.list` on every connection ([`refresh`]) —
//!   the organization's epics, each saying whether it is the reader's
//!   current one.
//! - **The switcher** ([`EpicSwitcher`]) shows in the nav's head only when
//!   the organization holds more than one epic. The current one is the
//!   server's: choosing another calls `epics.switch` — the reader's own
//!   setting, kept by the server, so every device and every agent of that
//!   principal reads the same epic — and the page reloads: every filtered
//!   read (the board, the nav, the counts, the palette) and the nav's
//!   shortcodes answer from the epic the server holds the reader in.
//!
//! What decides — which epic is current, whether there is a choice — is
//! pure here, pinned by native tests.

use leptos::prelude::*;
use menger_tickets_api::{Epic, EpicsEvent};

thread_local! {
    /// The organization's epics — empty until the first answer.
    static HELD: ArcRwSignal<Vec<Epic>> = ArcRwSignal::new(Vec::new());
}

/// The epics held, as a signal.
pub(crate) fn held() -> ArcRwSignal<Vec<Epic>> {
    HELD.with(Clone::clone)
}

/// Asks `epics.list` on the session's connection (every connect).
pub(crate) fn refresh() {
    let Some(client) = crate::session::epics_client() else { return };
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(events) = client.list_all(None).await {
            held().set(epics_of(events));
        }
    });
}

/// The epics a `list` answered.
fn epics_of(events: Vec<EpicsEvent>) -> Vec<Epic> {
    events
        .into_iter()
        .filter_map(|e| match e {
            EpicsEvent::Epic { epic } => Some(epic),
            _ => None,
        })
        .collect()
}

/// The reader's current epic among `epics`, when there is a choice — more
/// than one; `None`: nothing to show.
pub(crate) fn current_of(epics: &[Epic]) -> Option<String> {
    if epics.len() < 2 {
        return None;
    }
    epics.iter().find(|e| e.current).or_else(|| epics.last()).map(|e| e.name.to_string())
}

/// ORG-13: a ticket's key as a row shows it — a namesake's (`MENGER.3-1`,
/// a shortcode born again in a new epic, or another host's) as its letters
/// and its number, `MENGER-1`: the address says which. Any other key as it is.
pub(crate) fn shown_id(id: &str) -> String {
    match id.rsplit_once('-') {
        Some((code, n)) => match code.split_once('.') {
            Some((letters, k)) if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) => format!("{letters}-{n}"),
            _ => id.to_string(),
        },
        None => id.to_string(),
    }
}

/// ORG-13: the nav head's epic switcher — only when the organization holds
/// more than one: "epic", then each by name, the current one chosen.
#[component]
pub(crate) fn EpicSwitcher() -> impl IntoView {
    let epics = held();
    move || {
        let now = epics.with(|e| current_of(e))?;
        let names: Vec<(String, u32)> = epics.with(|e| e.iter().map(|x| (x.name.to_string(), x.tickets)).collect());
        Some(view! {
            <label class="nepic" title="the epic the board, the nav and every filtered read answer from — yours, kept by the server">
                <span class="nepic-k">"epic"</span>
                <select
                    class="epic-switch"
                    aria-label="epic"
                    on:change=move |ev| {
                        let v = event_target_value(&ev);
                        switch(v);
                    }
                >
                    {names
                        .into_iter()
                        .map(|(n, tickets)| {
                            let selected = n == now;
                            let label = format!("{n} · {tickets}");
                            view! { <option value=n.clone() selected=selected>{label}</option> }
                        })
                        .collect_view()}
                </select>
            </label>
        })
    }
}

/// `epics.switch` to `name`, then the page read again in it.
fn switch(name: String) {
    let Some(client) = crate::session::epics_client() else { return };
    let Ok(name) = name.parse() else { return };
    wasm_bindgen_futures::spawn_local(async move {
        if client.switch_all(name, None).await.is_ok() {
            if let Some(w) = web_sys::window() {
                let _ = w.location().reload();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{EpicName, OrgId};

    fn epic(name: &str, current: bool) -> Epic {
        Epic {
            org: OrgId::hypermemetic(),
            name: EpicName::new(name).unwrap(),
            first: name == "genesis",
            current,
            shortcodes: 0,
            tickets: 0,
            created_unix: 0,
        }
    }

    #[test]
    fn the_switcher_shows_only_with_a_choice_and_names_the_current() {
        assert_eq!(current_of(&[epic("genesis", true)]), None, "one epic: nothing to choose");
        assert_eq!(current_of(&[epic("genesis", false), epic("fresh", true)]).as_deref(), Some("fresh"));
        assert_eq!(current_of(&[epic("genesis", true), epic("fresh", false)]).as_deref(), Some("genesis"));
        assert_eq!(current_of(&[]), None);
    }

    #[test]
    fn a_namesakes_key_shows_as_its_letters_and_number() {
        assert_eq!(shown_id("MENGER.3-1"), "MENGER-1");
        assert_eq!(shown_id("SYNAPSE-CC.4-12"), "SYNAPSE-CC-12");
        assert_eq!(shown_id("MENGER-7"), "MENGER-7");
        assert_eq!(shown_id("SYNAPSE-CC-2"), "SYNAPSE-CC-2");
    }

    #[test]
    fn a_list_is_its_epics() {
        let evs = vec![EpicsEvent::Epic { epic: epic("genesis", false) }, EpicsEvent::Error { message: "x".into() }];
        assert_eq!(epics_of(evs).len(), 1);
    }
}
