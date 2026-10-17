//! CHAT-5: the ticket menu's **Chat** (owner, 2026-10-16: "chat should be
//! open able from the right click in the nav"). One ticket's menu — a nav
//! row's, a board row's, a wave card's, wherever the one menu opens — holds
//! it beside Open; what it does is src/menu/model.rs's ([`model::chat_entry`]),
//! from the reader's own chats on the ticket (CHAT-4's copy, src/chats.rs:
//! participants only):
//!
//! - none there: **Chat** opens the ticket with its pane on the picker;
//! - some: **Chat ▸** with the reader's unread there (the ticket page's
//!   field's N), its submenu each open chat (who, unread) and **+ new chat**;
//! - not held yet: **Chat** opens the pane as the field does (it chooses);
//! - the reader cannot chat: disabled, the reason under it.
//!
//! Choosing one goes to `#/ticket/ID` (`?chat=N` for a chat) with the pane
//! already open there (src/chat_pane.rs's [`chat_pane::open_view`]) — the
//! same pane, at 390 the sheet. `c` in the open menu chooses it
//! (`menu.chat`), drawn beside it as the table spells it. The submenu is a
//! `.sub`, so it opens with the menu's motion (APP-34), under the Motion
//! setting.

use super::*;
use crate::chat_pane::{self, View};
use crate::chats;
use menger_tickets_api::ChatState;
use model::{ChatDoes, ChatFacts, ChatItem, ChatLine};

/// What the reader's chats on `ticket` say, for the menu — tracked: the
/// account, the chats (asked for here, as the field asks), each count.
pub(super) fn facts(ticket: &str) -> ChatFacts {
    let (known, may_view) = signin::account().with(|a| (a.known(), a.may_view()));
    if let Some(why) = model::chat_refusal(known, may_view, session::chats_client().is_some()) {
        return ChatFacts::Refused(why);
    }
    chats::want();
    let Some(open) = chats::signal().with(|h| h.as_ref().map(|h| h.on(ticket, ChatState::Open))) else { return ChatFacts::Unknown };
    let (me, name) = (chats::me(), chat_pane::names());
    ChatFacts::Held(open.iter().map(|c| ChatLine { id: c.id.get(), label: chat_pane::tab_label(c, &me, &name), unread: c.unread }).collect())
}

/// Go to `ticket` with its pane open at `view` — the menu closed first.
fn go(ticket: &str, view: View, target: RwSignal<Option<Target>>) {
    close_menu(target);
    let chat = match view {
        View::Chat(c) => Some(c),
        _ => None,
    };
    chat_pane::open_view(ticket, view);
    let _ = dom::window().location().set_hash(&chat_pane::with_chat(&format!("#/ticket/{ticket}"), chat));
}

/// The item's count — the reader's unread there — and its key.
fn tail(unread: u32) -> impl IntoView {
    let count = (unread > 0).then(|| view! { <span class="count" title="your unread here">{unread}</span> });
    let key = bindings::shown("menu.chat").map(|k| view! { <kbd class="mkey" aria-hidden="true">{k}</kbd> });
    view! { {count}{key} }
}

/// The Chat item (and, with chats, its submenu under it).
pub(super) fn item(c: ChatItem, open_sub: RwSignal<Option<Sub>>, target: RwSignal<Option<Target>>) -> AnyView {
    let ChatItem { ticket, unread, does } = c;
    match does {
        ChatDoes::Refused(why) => {
            let (said, tip) = (why.clone(), why.clone());
            view! {
                <button class="item chat off" role="menuitem" aria-disabled="true" data-act="chat" data-chat-does="refused" title=tip
                    on:click=move |_| toast(said.clone())>
                    <span class="ilabel">"Chat"</span>
                    {tail(0)}
                    <small class="why">{why}</small>
                </button>
            }
            .into_any()
        }
        ChatDoes::Pane | ChatDoes::Pick => {
            let (view_at, name) = if does == ChatDoes::Pick { (View::Pick, "pick") } else { (View::Auto, "pane") };
            view! {
                <button class="item chat" role="menuitem" data-act="chat" data-chat-does=name title=format!("chat about {ticket}")
                    on:click=move |_| go(&ticket, view_at.clone(), target)>
                    <span class="ilabel">"Chat"</span>
                    {tail(unread)}
                </button>
            }
            .into_any()
        }
        ChatDoes::Choose(lines) => {
            let open = move || open_sub.get() == Some(Sub::Chat);
            view! {
                <button class="item chat" role="menuitem" aria-haspopup="menu" data-sub=Sub::Chat.name() data-act="chat" data-chat-does="choose"
                    aria-expanded=move || open().to_string()
                    on:click=move |_| open_sub.update(|o| *o = if *o == Some(Sub::Chat) { None } else { Some(Sub::Chat) })>
                    <span class="ilabel">"Chat "<span class="arrow" aria-hidden="true">{move || if open() { "▾" } else { "▸" }}</span></span>
                    {tail(unread)}
                </button>
                {move || open().then(|| view! { <div class="sub" role="group" data-sub=Sub::Chat.name()>{lines_view(&ticket, &lines, target)}</div> })}
            }
            .into_any()
        }
    }
}

/// Chat ▸'s items: each chat (who, unread), then + new chat.
fn lines_view(ticket: &str, lines: &[ChatLine], target: RwSignal<Option<Target>>) -> impl IntoView {
    let rows = lines
        .iter()
        .map(|l| {
            let (t, id) = (ticket.to_string(), l.id);
            view! {
                <button class="item chat" role="menuitem" data-chat=id.to_string() on:click=move |_| go(&t, View::Chat(id), target)>
                    <span class="ilabel">{l.label.clone()}</span>
                    {(l.unread > 0).then(|| view! { <span class="count">{l.unread}</span> })}
                </button>
            }
        })
        .collect_view();
    let t = ticket.to_string();
    view! {
        {rows}
        <button class="item" role="menuitem" data-act="new-chat" on:click=move |_| go(&t, View::Pick, target)>
            <span class="mark" aria-hidden="true">"+"</span>
            "new chat"
        </button>
    }
}

/// `c` in the open menu: Chat chosen — its submenu opened at its first chat,
/// or the item itself (a disabled one says why).
pub(super) fn key(root: NodeRef<leptos::html::Div>, open_sub: RwSignal<Option<Sub>>) {
    let Some(el) = root.get_untracked() else { return };
    let Some(item) = el.query_selector("[data-act='chat']").ok().flatten().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) else { return };
    if item.has_attribute("aria-haspopup") {
        view::open_at_first(root, open_sub, Sub::Chat);
    } else {
        focus(&item);
        item.click();
    }
}
