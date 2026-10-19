//! CHAT-16: the dialogs an admin's add and remove open (owner, 2026-10-17:
//! *"open a dialog when that user is being granted a scope to confirm"*).
//!
//! - **Granting** ([`GrantDialog`]): an add the server answered
//!   `needs_grant` — the one picked can't read the chat's ticket. It says
//!   who is being given what, and where, with the choice the server offers:
//!   **Ticket** (viewer on that ticket alone) or **Chat only** (this chat,
//!   and of the ticket only its id and title) — a chat outside tickets
//!   offers the chat alone — then Confirm or Cancel. Confirm adds again
//!   with the grant named. With no grant needed there is no dialog.
//! - **Revoking** ([`RevokeDialog`]): removing someone whose seat came with
//!   such a grant offers to revoke it too (checked by default).
//!
//! What it says is pure and pinned by native tests; the rest is the DOM.

use leptos::prelude::*;
use menger_tickets_api::chats::ChatGrant;
use wasm_bindgen_futures::spawn_local;

use crate::chats;

/// An add that needs a grant: the server's question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ask {
    pub(crate) chat: u64,
    pub(crate) principal: String,
    /// The chat's ticket — none outside tickets.
    pub(crate) ticket: Option<String>,
    pub(crate) choices: Vec<ChatGrant>,
}

/// The choice the dialog starts on: the narrower, chat only.
pub(crate) fn first_choice(choices: &[ChatGrant]) -> ChatGrant {
    if choices.contains(&ChatGrant::Chat) { ChatGrant::Chat } else { choices.first().copied().unwrap_or(ChatGrant::Chat) }
}

/// A choice's label and what it gives, for `who` on `ticket` (none: the
/// organization's chat).
pub(crate) fn choice_words(choice: ChatGrant, ticket: Option<&str>) -> (&'static str, String) {
    match (choice, ticket) {
        (ChatGrant::Ticket, Some(t)) => ("Ticket", format!("view {t} — its body and fields, as a viewer of that ticket alone (nothing above or below it)")),
        (ChatGrant::Ticket, None) => ("Ticket", "no ticket here".to_string()),
        (ChatGrant::Chat, Some(t)) => ("Chat only", format!("this chat and nothing else: of {t} only its id and title")),
        (ChatGrant::Chat, None) => ("Chat only", "this chat, and chats in the organization they are added to — nothing of its tickets".to_string()),
    }
}

/// The dialog's heading: who is given what, where.
pub(crate) fn heading(name: &str, ticket: Option<&str>) -> String {
    match ticket {
        Some(t) => format!("{name} can't read {t}. Adding them grants access."),
        None => format!("{name} holds no grant here. Adding them grants access."),
    }
}

/// CHAT-16: the grant's dialog — `done` with what the add granted, or why
/// not; `cancel` writes nothing.
#[component]
pub(crate) fn GrantDialog(ask: Ask, done: Callback<Result<Option<String>, String>>, cancel: Callback<()>) -> impl IntoView {
    let name = crate::chat_pane::names();
    let who = name(&ask.principal);
    let choice = RwSignal::new(first_choice(&ask.choices));
    let busy = RwSignal::new(false);
    let ticket = ask.ticket.clone();
    let options = ask
        .choices
        .iter()
        .map(|c| {
            let c = *c;
            let (label, what) = choice_words(c, ticket.as_deref());
            view! {
                <label class="gopt" class:on=move || choice.get() == c data-grant=c.as_str()>
                    <input type="radio" name="chat-grant" prop:checked=move || choice.get() == c on:change=move |_| choice.set(c) />
                    <span><b>{label}</b><small>{what}</small></span>
                </label>
            }
        })
        .collect_view();
    let (chat, principal) = (ask.chat, ask.principal.clone());
    let confirm = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let (p, g) = (principal.clone(), choice.get_untracked());
        spawn_local(async move {
            let r = chats::add_granting(chat, &p, Some(g)).await.and_then(|a| match a {
                chats::Added::Added(granted) => Ok(granted),
                chats::Added::NeedsGrant(_) => Err("the server asked again — nothing was granted".to_string()),
            });
            busy.try_set(false);
            done.run(r);
        });
    };
    let keys = move |ev: web_sys::KeyboardEvent| {
        if crate::bindings::is(&ev, crate::bindings::Context::Dialog, "dialog.close") {
            ev.prevent_default();
            ev.stop_propagation();
            cancel.run(());
        }
    };
    let principal_shown = ask.principal.clone();
    view! {
        <div class="overlay gdlg" on:click=move |_| cancel.run(())>
            <div id="chat-grant" class="dialog" role="dialog" aria-modal="true" aria-labelledby="chat-grant-h" on:click=|ev| ev.stop_propagation() on:keydown=keys>
                <h2 id="chat-grant-h">{heading(&who, ticket.as_deref())}</h2>
                <p class="gwho"><code>{principal_shown}</code></p>
                <div class="gopts" role="radiogroup" aria-label="what to grant">{options}</div>
                <div class="sacts">
                    <button data-act="cancel" on:click=move |_| cancel.run(())>"Cancel"</button>
                    <button class="primary" data-act="confirm" disabled=move || busy.get() on:click=confirm>
                        {move || if busy.get() { "granting…" } else { "Confirm" }}
                    </button>
                </div>
            </div>
        </div>
    }
}

/// CHAT-16: removing `principal` from `chat`, whose seat came with a grant
/// (`what`: its words) — revoke it too (checked), or keep it.
#[component]
pub(crate) fn RevokeDialog(chat: u64, principal: String, done: Callback<Result<(), String>>, cancel: Callback<()>) -> impl IntoView {
    let name = crate::chat_pane::names();
    let who = name(&principal);
    let revoke = RwSignal::new(true);
    let p = principal.clone();
    let confirm = move |_| {
        let (p, r) = (p.clone(), revoke.get_untracked());
        spawn_local(async move {
            done.run(chats::remove_revoking(chat, &p, r).await);
        });
    };
    let keys = move |ev: web_sys::KeyboardEvent| {
        if crate::bindings::is(&ev, crate::bindings::Context::Dialog, "dialog.close") {
            ev.prevent_default();
            ev.stop_propagation();
            cancel.run(());
        }
    };
    view! {
        <div class="overlay gdlg" on:click=move |_| cancel.run(())>
            <div id="chat-revoke" class="dialog" role="dialog" aria-modal="true" aria-labelledby="chat-revoke-h" on:click=|ev| ev.stop_propagation() on:keydown=keys>
                <h2 id="chat-revoke-h">{format!("Take {who} out of this chat")}</h2>
                <p>{format!("{who} was given access when they were added.")}</p>
                <label class="gopt on">
                    <input type="checkbox" prop:checked=move || revoke.get() on:change=move |ev| revoke.set(event_target_checked(&ev)) />
                    <span><b>"Also revoke that access"</b><small>"the grant their add made goes in the same write"</small></span>
                </label>
                <div class="sacts">
                    <button data-act="cancel" on:click=move |_| cancel.run(())>"Cancel"</button>
                    <button class="primary" data-act="confirm" on:click=confirm>"Remove"</button>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog says who is given what, where; it starts on the narrower
    /// choice; outside tickets only the chat is offered.
    #[test]
    fn the_dialog_says_who_gets_what_where() {
        assert_eq!(first_choice(&[ChatGrant::Ticket, ChatGrant::Chat]), ChatGrant::Chat);
        assert_eq!(first_choice(&[ChatGrant::Ticket]), ChatGrant::Ticket);
        assert_eq!(heading("sam", Some("KILN-7")), "sam can't read KILN-7. Adding them grants access.");
        assert!(heading("sam", None).contains("holds no grant here"));
        let (l, w) = choice_words(ChatGrant::Ticket, Some("KILN-7"));
        assert!(l == "Ticket" && w.starts_with("view KILN-7") && w.contains("alone"));
        let (l, w) = choice_words(ChatGrant::Chat, Some("KILN-7"));
        assert!(l == "Chat only" && w.contains("only its id and title"));
    }
}
