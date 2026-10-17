//! CHAT-14: `#/chats` — every chat the reader is in, on tickets and outside
//! them (owner, 2026-10-17: *"I think we should also support direct
//! messages and groups outside of ticketing"*), as CHAT-7's full-screen
//! session with no ticket beside it.
//!
//! - **The rail**: **＋ New message** (`c`), then every chat — those with
//!   unread first, then by last activity; the closed last, dimmed ([`rows`]).
//!   Each says what it is — its ticket's chip, *direct* or *group* — who
//!   (or its title), its last line and the reader's unread.
//! - **New message** (`#/chats/new`): the picker with no ticket — one person
//!   is a direct message (the one the two have, when they have one), several
//!   a group with a title; agents by name (`builder`). It opens at
//!   `#/chats/ID`.
//! - **A chat** (`#/chats/ID`): CHAT-7's column ([`crate::chat_pane::ChatView`]),
//!   the head saying who is in it (agents marked; a group's members taken
//!   out with ×, src/chat_pane.rs), **＋ add** (a group, or a ticket's chat),
//!   **ticket ⟩** for a ticket's chat (its page, the chat in its dock), and
//!   **⋯** (rename, close, leave a group, copy link).
//! - **Esc** (outside the composer) leaves for the board; at ≤ 760 the rail
//!   is the page until a chat is chosen, then a drawer behind ☰.
//!
//! What decides is pure and pinned by native tests; the rest is the DOM.

use leptos::prelude::*;
use menger_tickets_api::{Chat, ChatKind, ChatState};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::chat_pane;
use crate::chats::{self, Held};
use crate::dom;

/// What `#/chats…` names: the list, a new message, or one chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    List,
    New,
    Chat(u64),
}

/// The route's rest after `chats/` — `new`, a chat's id, or nothing.
pub(crate) fn pick_of(rest: &str) -> Pick {
    match rest.trim_matches('/') {
        "" => Pick::List,
        "new" => Pick::New,
        n => n.parse().map_or(Pick::List, Pick::Chat),
    }
}

/// One row of the rail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) id: u64,
    /// Its ticket's key, or `direct` / `group`.
    pub(crate) mark: String,
    pub(crate) ticket: bool,
    pub(crate) label: String,
    pub(crate) last: String,
    pub(crate) unread: u32,
    pub(crate) closed: bool,
}

/// The rail: every chat, unread first, then the newest; the closed last.
pub(crate) fn rows(held: &Held, me: &str, name: &impl Fn(&str) -> String) -> Vec<Row> {
    held.inbox()
        .iter()
        .map(|c| Row {
            id: c.id.get(),
            mark: c.ticket.clone().unwrap_or_else(|| chats::kind_word(c).to_string()),
            ticket: c.ticket.is_some(),
            label: c.title.clone().unwrap_or_else(|| chat_pane::tab_label(c, me, name)),
            last: last_line(held, c, me, name),
            unread: c.unread,
            closed: c.state == ChatState::Closed,
        })
        .collect()
}

fn last_line(held: &Held, c: &Chat, me: &str, name: &impl Fn(&str) -> String) -> String {
    if c.state == ChatState::Closed {
        return "closed, read-only".to_string();
    }
    if let Some(m) = held.thread(c.id.get()).and_then(|t| t.list.last()) {
        let line = m.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        let line = if line.starts_with("![") { "an image" } else { line };
        let who = if m.author == me { "you".to_string() } else { name(&m.author) };
        return format!("{who}: {line}");
    }
    match (c.last_message.is_some(), c.kind) {
        (false, ChatKind::Direct) => "no messages yet — say hello".to_string(),
        (false, _) => "no messages yet".to_string(),
        (true, _) => format!("{} message{}", c.count, if c.count == 1 { "" } else { "s" }),
    }
}

/// Whether the reader may leave `c` — a group it is in.
pub(crate) fn may_leave(c: &Chat, me: &str) -> bool {
    c.kind == ChatKind::Group && c.state == ChatState::Open && c.participants.iter().any(|p| p == me)
}

/// Whether `c` takes **＋ add**: an open group or ticket's chat (a direct
/// message stays between two).
pub(crate) fn takes_add(c: &Chat) -> bool {
    c.state == ChatState::Open && c.kind != ChatKind::Direct
}

fn go(hash: &str) {
    let _ = dom::window().location().set_hash(hash);
}

/// The page, at `pick`.
#[component]
pub(crate) fn ChatsPage(#[prop(into)] pick: Signal<Pick>) -> impl IntoView {
    chats::want();
    crate::chat_full::set_inbox(true);
    on_cleanup(|| crate::chat_full::set_inbox(false));
    let held = chats::signal();
    let me = chats::me();
    let name = chat_pane::names();
    let drawer = RwSignal::new(false);
    let adding = RwSignal::new(false);
    let more = RwSignal::new(false);
    let renaming = RwSignal::new(None::<String>);
    let problem = RwSignal::new(None::<String>);
    // Names and agents, from the server, once: every row says them.
    spawn_local(async {
        let _ = chats::candidates(None).await;
    });
    Effect::new(move |_| {
        pick.track();
        adding.set(false);
        more.set(false);
        renaming.set(None);
        problem.set(None);
        drawer.set(false);
    });
    let chat_id = Memo::new(move |_| match pick.get() {
        Pick::Chat(id) => Some(id),
        _ => None,
    });
    let chat = {
        let held = held.clone();
        Memo::new(move |_| chat_id.get().and_then(|id| held.with(|h| h.as_ref().and_then(|h| h.get(id).cloned()))))
    };
    let root = NodeRef::<leptos::html::Div>::new();
    root.on_load(|el| {
        spawn_local(async move {
            dom::sleep_ms(0).await;
            let in_box = dom::window().document().and_then(|d| d.active_element()).is_some_and(|a| a.tag_name() == "TEXTAREA");
            if !in_box {
                let _ = el.focus();
            }
        });
    });
    let keys = move |ev: web_sys::KeyboardEvent| {
        let typing = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).is_some_and(|t| matches!(t.tag_name().as_str(), "INPUT" | "TEXTAREA"));
        if typing {
            return;
        }
        match bindings::hit(&ev, Context::Session) {
            Some("session.new") => {
                ev.prevent_default();
                go("#/chats/new");
            }
            Some("session.leave") => {
                ev.prevent_default();
                ev.stop_propagation();
                if more.get_untracked() || adding.get_untracked() || drawer.get_untracked() {
                    more.set(false);
                    adding.set(false);
                    drawer.set(false);
                } else {
                    go("#/");
                }
            }
            _ => {}
        }
    };
    let rail_view = {
        let (me, name, held) = (me.clone(), name.clone(), held.clone());
        move || {
            let list = held.with(|h| h.as_ref().map(|h| rows(h, &me, &name)));
            let Some(list) = list else { return view! { <p class="pempty">"loading chats…"</p> }.into_any() };
            if list.is_empty() {
                return view! { <p class="pempty">"No chats yet. Start one with ＋ New message."</p> }.into_any();
            }
            list.into_iter()
                .map(|r| {
                    let id = r.id;
                    let on = move || chat_id.get() == Some(id);
                    let un = r.unread > 0;
                    view! {
                        <a class="srow" class:on=on class:closed=r.closed class:un=un data-chat=id.to_string() href=format!("#/chats/{id}")>
                            <span class="sl">
                                {if r.ticket { view! { <span class="tchip">{r.mark.clone()}</span> }.into_any() } else { view! { <span class="kchip">{r.mark.clone()}</span> }.into_any() }}
                                <span class="slabel">{r.label.clone()}</span>
                            </span>
                            {(r.unread > 0).then(|| view! { <span class="count">{r.unread}</span> })}
                            <small>{r.last.clone()}</small>
                        </a>
                    }
                })
                .collect_view()
                .into_any()
        }
    };
    let title = {
        let (me, name) = (me.clone(), name.clone());
        move || match pick.get() {
            Pick::New => "New message".to_string(),
            Pick::List => "Chats".to_string(),
            Pick::Chat(_) => chat.with(|c| c.as_ref().map(|c| c.title.clone().unwrap_or_else(|| chat_pane::tab_label(c, &me, &name)))).unwrap_or_else(|| "a chat".into()),
        }
    };
    let people = {
        let (me, name) = (me.clone(), name.clone());
        move || {
            let Some(c) = chat.get() else { return ().into_any() };
            let dots = c
                .participants
                .iter()
                .map(|p| {
                    let waiting = c.waiting.contains(p);
                    let mine = *p == me;
                    view! {
                        <span class="sp" data-principal=p.clone() title=if waiting { format!("{} — waiting for your next message", name(p)) } else { format!("{} · {}", name(p), p) }>
                            <span class="dot" class:on=true class:wait=waiting></span>
                            <span class="spn">{if mine { format!("{} (you)", name(p)) } else { name(p) }}</span>
                            {chats::is_agent(p).then(|| view! { <span class="ag">"agent"</span> })}
                        </span>
                    }
                })
                .collect_view();
            view! { {dots} }.into_any()
        }
    };
    let copy_link = move |_| {
        more.set(false);
        let Some(id) = chat_id.get_untracked() else { return };
        let w = dom::window();
        let url = format!("{}{}#/chats/{id}", w.location().origin().unwrap_or_default(), w.location().pathname().unwrap_or_default());
        let _ = w.navigator().clipboard().write_text(&url);
    };
    let settle = move |open: bool| {
        more.set(false);
        let Some(id) = chat_id.get_untracked() else { return };
        spawn_local(async move {
            if let Err(e) = chats::settle_state(id, open).await {
                problem.try_set(Some(e));
            }
        });
    };
    let leave = {
        let me = me.clone();
        move |_| {
            more.set(false);
            let Some(id) = chat_id.get_untracked() else { return };
            let me = me.clone();
            spawn_local(async move {
                match chats::remove(id, &me).await {
                    Ok(()) => go("#/chats"),
                    Err(e) => {
                        problem.try_set(Some(e));
                    }
                }
            });
        }
    };
    let save_title = move || {
        let (Some(id), Some(t)) = (chat_id.get_untracked(), renaming.get_untracked()) else { return };
        renaming.set(None);
        spawn_local(async move {
            if let Err(e) = chats::rename(id, Some(&t)).await {
                problem.try_set(Some(e));
            }
        });
    };
    let main = move || match pick.get() {
        Pick::Chat(id) => {
            let ticket = chat.with_untracked(|c| c.as_ref().and_then(|c| c.ticket.clone()));
            view! { <chat_pane::ChatView ticket id full=true /> }.into_any()
        }
        Pick::New => view! {
            <div class="spick">
                <chat_pane::Picker ticket=None add_to=None on_done=Callback::new(|_| ()) on_open=Callback::new(|id: u64| go(&format!("#/chats/{id}"))) />
            </div>
        }
        .into_any(),
        Pick::List => view! {
            <div class="sintro">
                <h3>"Your chats"</h3>
                <p>"Direct messages, groups and the chats on tickets you are in — those with unread first. Agents are here by name: message builder as you would a person."</p>
                <a class="sbtn primary" href="#/chats/new">"＋ New message"</a>
            </div>
        }
        .into_any(),
    };
    // The chat chosen is drawn once its record is held (a reload, a link) —
    // and drawn again only when that changes, never on its unread moving.
    let shown = {
        let held = held.clone();
        Memo::new(move |_| {
            let p = pick.get();
            (p, matches!(p, Pick::Chat(_)) && chat.with(Option::is_some), held.with(Option::is_some))
        })
    };
    let head_buttons = move || {
        let c = chat.get()?;
        let me = chats::me();
        let ticket = c.ticket.clone();
        let (open, leave_ok, add_ok) = (c.state == ChatState::Open, may_leave(&c, &me), takes_add(&c));
        let (copy_link, leave) = (copy_link, leave.clone());
        Some(view! {
            {add_ok.then(|| view! { <button type="button" class="sbtn" on:click=move |_| adding.update(|a| *a = !*a)>"＋ add"</button> })}
            {ticket.map(|t| view! { <a class="sbtn" href=format!("#/ticket/{t}?chat={}", c.id.get()) title="its ticket, the chat in its dock">{format!("{t} ⟩")}</a> })}
            <button type="button" class="sbtn smore" aria-label="more" aria-expanded=move || more.get().to_string() on:click=move |_| more.update(|m| *m = !*m)>"⋯"</button>
            {move || { let (leave, copy_link) = (leave.clone(), copy_link); more.get().then(|| view! {
                <div class="smenu-pop" role="menu">
                    {(c.kind != ChatKind::Direct).then(|| view! { <button type="button" role="menuitem" on:click=move |_| { more.set(false); renaming.set(Some(chat.with_untracked(|c| c.as_ref().and_then(|c| c.title.clone())).unwrap_or_default())); }>"rename"</button> })}
                    {if open {
                        view! { <button type="button" role="menuitem" on:click=move |_| settle(false)>"close chat"</button> }.into_any()
                    } else {
                        view! { <button type="button" role="menuitem" on:click=move |_| settle(true)>"reopen"</button> }.into_any()
                    }}
                    {leave_ok.then(|| view! { <button type="button" role="menuitem" class="sleave" on:click=leave.clone()>"leave the group"</button> })}
                    <button type="button" role="menuitem" on:click=copy_link>"copy link"</button>
                </div>
            }) }}
        })
    };
    view! {
        <div id="chatfull" class="inbox" role="dialog" aria-modal="true" aria-label="chats" tabindex="-1"
            node_ref=root on:keydown=keys class:drawer=move || drawer.get() class:list=move || pick.get() == Pick::List>
            <aside class="srail" aria-label="chats">
                <div class="srhead">
                    <a class="brand" href="#/" title="back to the board (Esc)">"menger"</a>
                    <a class="kbd" href="#/" title="back to the board (Esc)">"esc"</a>
                </div>
                <a class="snew" class:on=move || pick.get() == Pick::New href="#/chats/new">"＋ New message"<span class="kbd">"c"</span></a>
                <h4 class="sh">"chats"</h4>
                <div class="srows">{rail_view}</div>
            </aside>
            <section class="smain">
                <header class="shead">
                    <button type="button" class="smenu" aria-label="the chats" title="the chats" on:click=move |_| drawer.update(|d| *d = !*d)>"☰"</button>
                    <a class="sback" href="#/chats" title="every chat">"← chats"</a>
                    {move || match renaming.get() {
                        Some(v) => view! {
                            <input class="stitle-edit" type="text" aria-label="the chat's title" prop:value=v
                                on:input=move |ev| renaming.set(Some(event_target_value(&ev)))
                                on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                                    Some("form.submit") => { ev.prevent_default(); save_title(); }
                                    Some("form.cancel") => { ev.prevent_default(); ev.stop_propagation(); renaming.set(None); }
                                    _ => {}
                                } />
                        }.into_any(),
                        None => view! { <h2 class="stitle">{title.clone()}</h2> }.into_any(),
                    }}
                    {move || chat.with(|c| c.as_ref().map(|c| chats::kind_word(c))).map(|k| view! { <span class="kchip">{k}</span> })}
                    <div class="speople">{people}</div>
                    {head_buttons}
                </header>
                {move || problem.get().map(|p| view! { <p class="perr" role="alert">{p}</p> })}
                {move || (adding.get()).then(|| chat.get_untracked()).flatten().map(|c| {
                    let ps = c.participants.clone();
                    view! { <div class="sadd"><chat_pane::Picker ticket=c.ticket.clone() add_to=Some((c.id.get(), ps)) on_done=Callback::new(move |_| adding.set(false)) /></div> }
                })}
                {move || {
                    match shown.get() {
                        (Pick::Chat(_), false, true) => view! { <p class="pempty">"This chat is not one you are in."</p> }.into_any(),
                        (Pick::Chat(_), false, false) => view! { <p class="pempty">"loading…"</p> }.into_any(),
                        _ => main().into_any(),
                    }
                }}
            </section>
            <button type="button" class="sscrim" aria-label="close the chats" on:click=move |_| drawer.set(false)></button>
        </div>
    }
}

/// The header's **chats N**: the reader's unread across every chat.
#[component]
pub(crate) fn HeaderLink(#[prop(into)] on: Signal<bool>) -> impl IntoView {
    let held = chats::signal();
    let n = Memo::new(move |_| held.with(|h| h.as_ref().map(Held::unread_total)));
    view! {
        <a class="hlink" id="chats-link" href="#/chats" aria-current=move || on.get().then_some("page")>
            "chats"
            {move || n.get().filter(|n| *n > 0).map(|n| view! { <span class="hbadge" title=format!("{n} unread")>{n}</span> })}
        </a>
    }
}

/// A person's **Chat** (the members page, a person's card): the direct
/// message with them — the one there is, or a new one — opened.
pub(crate) fn message(principal: String, problem: impl Fn(String) + 'static) {
    spawn_local(async move {
        match chats::open(None, &[principal], None).await {
            Ok(id) => go(&format!("#/chats/{id}")),
            Err(e) => problem(e),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chats::tests::{chat, msg};
    use menger_tickets_api::ChatsEvent;

    #[test]
    fn the_route_names_the_list_a_new_message_or_a_chat() {
        assert_eq!(pick_of(""), Pick::List);
        assert_eq!(pick_of("/"), Pick::List);
        assert_eq!(pick_of("new"), Pick::New);
        assert_eq!(pick_of("41"), Pick::Chat(41));
        assert_eq!(pick_of("41/"), Pick::Chat(41));
        assert_eq!(pick_of("nonsense"), Pick::List);
    }

    /// The rail: unread first, then the newest, the closed last; each says
    /// what it is — its ticket, or direct / group — and who.
    #[test]
    fn the_rail_holds_every_chat_and_says_what_each_is() {
        let mut dm = chat(5, "", &["ada", "key:54a9"]);
        dm.last_at = 500;
        let mut on_ticket = chat(6, "KILN-7", &["ada", "rui"]);
        on_ticket.last_at = 900;
        let mut g = chat(7, "", &["ada", "rui", "sam"]);
        g.kind = ChatKind::Group;
        g.title = Some("the kiln crew".into());
        g.unread = 3;
        let mut h = Held::from_events([dm, on_ticket, g].map(|chat| ChatsEvent::Chat { chat }));
        h.learn([("key:54a9".to_string(), "builder".to_string())]);
        h.loaded(5, vec![msg(1, 5, "key:54a9", "On it.\nmore")]);
        let name = |p: &str| h.name_of(p).map_or_else(|| p.to_string(), str::to_string);
        let got = rows(&h, "ada", &name);
        assert_eq!(got.iter().map(|r| (r.id, r.mark.as_str(), r.label.as_str())).collect::<Vec<_>>(), [(7, "group", "the kiln crew"), (6, "KILN-7", "rui"), (5, "direct", "builder")]);
        assert!(got[1].ticket && !got[2].ticket);
        assert_eq!(got[2].last, "builder: On it.");
        assert_eq!(got[0].unread, 3);
    }

    #[test]
    fn a_group_is_left_and_added_to_and_a_direct_message_is_not() {
        let mut g = chat(7, "", &["ada", "rui", "sam"]);
        g.kind = ChatKind::Group;
        let dm = chat(5, "", &["ada", "rui"]);
        let t = chat(6, "KILN-7", &["ada"]);
        assert!(may_leave(&g, "ada") && !may_leave(&g, "zed") && !may_leave(&dm, "ada") && !may_leave(&t, "ada"));
        assert!(takes_add(&g) && takes_add(&t) && !takes_add(&dm));
        g.state = ChatState::Closed;
        assert!(!takes_add(&g) && !may_leave(&g, "ada"));
    }
}
