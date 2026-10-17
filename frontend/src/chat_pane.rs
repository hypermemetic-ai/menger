//! CHAT-4: chats inside their ticket — CHAT-1's Figs 1–4 and its 390 sheet.
//!
//! - **The field** ([`Field`]): **chat N** among the ticket page's fields,
//!   N the reader's unread on the ticket ([`field_count`]); it opens the
//!   pane, and closes it again.
//! - **The pane** ([`Pane`]): a column of the app's grid beside the content
//!   at a desktop width; under 760px a sheet over the ticket, its handle
//!   dragged down to dismiss it ([`dismisses`]), fixed while the page
//!   scrolls under it. A tab per open chat on the ticket (its participants,
//!   its unread), **+ chat**, the closed list, ×. The participants row: a
//!   dot each, pulsing while one waits on `await_message` (under the Motion
//!   setting), agents marked ([`crate::chats::is_agent`]); *add* and *close
//!   chat*. The messages through `md.rs` (never framed), an agent's `via`
//!   shown, a *new* line at the reader's read mark when the chat was opened
//!   ([`new_from`]), a message that arrives sliding in (DESIGN-39: none
//!   under Motion none). The composer: markdown, `@` (at a word's start)
//!   adds a participant ([`wants_add`]), ⌘↵ / Ctrl+Enter sends
//!   (`comment.save`'s key, src/bindings.rs).
//! - **The picker** ([`Picker`]): `chats.candidates` — each principal with
//!   a grant in the ticket's organization, and whether it reads the ticket;
//!   one that cannot is shown and cannot be picked ([`refusal`]); the same
//!   people's open chat opens instead of a new one
//!   ([`crate::chats::Held::existing`]).
//! - **Reading**: what the pane shows is read — `chats.read` up to its last
//!   message, whenever it has unread — and the counts follow in every open
//!   app (the server says `read` to the reader's other watches).
//! - **Closing**: *close chat* moves it to the closed list, read-only, with
//!   *reopen*.
//! - **The URL**: `#/ticket/KILN-7?chat=41` opens the pane on chat 41
//!   ([`chat_in`]); the pane keeps the query to the chat it shows
//!   ([`with_chat`]), in place.
//!
//! Out (CHAT-1): the header's list (Fig 5) waits on the owner's `inbox`
//! decision — src/chats.rs holds its seam; agents' claim lines and the
//! free-agent note (Figs 3, 4) are AGENT-3's.
//!
//! What decides is pure and pinned by native tests; the rest is the DOM.

use leptos::prelude::*;
use menger_tickets_api::{Chat, ChatState, Message};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::chats::{self, short, Held};
use crate::{dom, fmt_unix, md};

/// What the pane shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum View {
    /// Not chosen yet: the first open chat with unread, else the first open
    /// one, else the picker ([`default_view`]).
    Auto,
    Chat(u64),
    /// **+ chat**.
    Pick,
    /// The closed chats.
    Closed,
}

/// The pane, open on a ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) ticket: String,
    pub(crate) view: View,
}

/// The sheet's handle dragged down this far (px) dismisses it.
pub(crate) const DISMISS_PX: f64 = 80.0;

/// `chat=<id>` in a route's query.
pub(crate) fn chat_in(hash: &str) -> Option<u64> {
    let (_, q) = hash.split_once('?')?;
    q.split('&').find_map(|p| p.strip_prefix("chat=")).and_then(|v| v.parse().ok())
}

/// The hash with `chat=<id>` in its query (`Some`) or out of it, the rest
/// of the query kept in order.
pub(crate) fn with_chat(hash: &str, chat: Option<u64>) -> String {
    let (path, query) = hash.split_once('?').unwrap_or((hash, ""));
    let mut params: Vec<String> = query.split('&').filter(|p| !p.is_empty() && !p.starts_with("chat=")).map(str::to_string).collect();
    if let Some(c) = chat {
        params.push(format!("chat={c}"));
    }
    if params.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", params.join("&"))
    }
}

/// [`View::Auto`] resolved: the first open chat with unread, else the
/// first open chat, else the picker.
pub(crate) fn default_view(open: &[Chat]) -> View {
    open.iter().find(|c| c.unread > 0).or(open.first()).map_or(View::Pick, |c| View::Chat(c.id.get()))
}

/// The field's N: the reader's unread on the ticket — none drawn at 0.
pub(crate) fn field_count(n: u32) -> Option<String> {
    (n > 0).then(|| n.to_string())
}

/// How `principal` reads in the pane: the reader by its own name, another
/// by the name a message said, else in few characters.
pub(crate) fn display(principal: &str, me: &str, my_name: Option<&str>, held: Option<&Held>) -> String {
    if principal == me {
        if let Some(n) = my_name {
            return n.to_string();
        }
    }
    held.and_then(|h| h.name_of(principal)).map_or_else(|| short(principal), str::to_string)
}

/// A chat's tab: its title, else who the reader talks to in it
/// (`builder · rui`) — the reader too when it is alone there.
pub(crate) fn tab_label(c: &Chat, me: &str, name: impl Fn(&str) -> String) -> String {
    if let Some(t) = &c.title {
        return t.clone();
    }
    let others: Vec<&String> = c.participants.iter().filter(|p| *p != me).collect();
    let who: Vec<&String> = if others.is_empty() { c.participants.iter().collect() } else { others };
    who.into_iter().map(|p| name(p)).collect::<Vec<_>>().join(" · ")
}

/// Where the *new* line goes: before the first of the `unread` last
/// messages — none when nothing is unread.
pub(crate) fn new_from(thread: &[Message], unread: u32) -> Option<u64> {
    let n = unread as usize;
    (n > 0).then(|| thread.len().checked_sub(n).and_then(|at| thread.get(at)).map(|m| m.id.get())).flatten()
}

/// Whether what was typed asks to add a participant: an `@` just typed at
/// the start of a word.
pub(crate) fn wants_add(text: &str) -> bool {
    match text.strip_suffix('@') {
        Some(before) => before.chars().last().is_none_or(char::is_whitespace),
        None => false,
    }
}

/// The composer's placeholder.
pub(crate) fn placeholder(label: &str) -> String {
    format!("Message {label}…")
}

/// What the picker says of one who cannot be picked.
pub(crate) fn refusal(ticket: &str) -> String {
    format!("can't read {ticket}, so can't be added")
}

/// The picker's rows: every candidate but the reader (and, adding, those
/// already in the chat) whose principal or name holds `query` — those who
/// read the ticket first, then by name.
pub(crate) fn rows(cands: &[(String, bool)], me: &str, already: &[String], query: &str, name: impl Fn(&str) -> String) -> Vec<(String, bool)> {
    let q = query.trim().to_lowercase();
    let mut v: Vec<(String, bool)> = cands
        .iter()
        .filter(|(p, _)| p != me && !already.contains(p))
        .filter(|(p, _)| q.is_empty() || p.to_lowercase().contains(&q) || name(p).to_lowercase().contains(&q))
        .cloned()
        .collect();
    v.sort_by_key(|(p, reads)| (!reads, name(p)));
    v
}

/// Whether the sheet, dragged down `dy` px, is dismissed.
pub(crate) fn dismisses(dy: f64) -> bool {
    dy >= DISMISS_PX
}

/// A message's time, `HH:MM` where the reader is.
fn clock(at: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(at as f64 * 1000.0));
    format!("{:02}:{:02}", d.get_hours(), d.get_minutes())
}

// -- the state ----------------------------------------------------------------

thread_local! {
    static SHOWN: ArcRwSignal<Option<Shown>> = ArcRwSignal::new(None);
}

fn shown() -> ArcRwSignal<Option<Shown>> {
    SHOWN.with(Clone::clone)
}

/// Open the pane on `ticket` (at `chat`, or as [`View::Auto`] chooses).
pub(crate) fn open_on(ticket: &str, chat: Option<u64>) {
    chats::want();
    let view = chat.map_or(View::Auto, View::Chat);
    shown().set(Some(Shown { ticket: ticket.to_string(), view }));
}

/// CHAT-5: open the pane on `ticket` at `view` — the ticket menu's Chat
/// opening a chat, the picker, or the pane as it chooses.
pub(crate) fn open_view(ticket: &str, view: View) {
    chats::want();
    shown().set(Some(Shown { ticket: ticket.to_string(), view }));
}

/// Close the pane.
pub(crate) fn close() {
    shown().set(None);
}

fn show(view: View) {
    shown().update(|s| {
        if let Some(s) = s.as_mut() {
            s.view = view;
        }
    });
}

/// Whether the pane is open on `ticket` (the route's).
pub(crate) fn open_for(ticket: Option<&str>) -> bool {
    shown().with(|s| s.as_ref().zip(ticket).is_some_and(|(s, t)| s.ticket.eq_ignore_ascii_case(t)))
}

/// The route's query rewritten to the chat shown — no history entry, no
/// hashchange.
fn keep_url(chat: Option<u64>) {
    let w = dom::window();
    let hash = w.location().hash().unwrap_or_default();
    if !hash.starts_with("#/ticket/") || chat_in(&hash) == chat {
        return;
    }
    if let Ok(h) = w.history() {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&with_chat(&hash, chat)));
    }
}

// -- the views ----------------------------------------------------------------

/// The ticket page's **chat N** field.
#[component]
pub(crate) fn Field(ticket: String) -> impl IntoView {
    chats::want();
    let held = chats::signal();
    let id = ticket.clone();
    let n = Memo::new(move |_| held.with(|h| h.as_ref().map_or(0, |h| h.unread_on(&id))));
    let t = ticket.clone();
    let on = Memo::new(move |_| open_for(Some(&t)));
    let tip = format!("the chats on {ticket} — your unread there");
    view! {
        <button
            type="button"
            class="chatbtn"
            id="t-chat"
            title=tip
            aria-pressed=move || on.get().to_string()
            on:click=move |_| if on.get_untracked() { close() } else { open_on(&ticket, None) }
        >
            "chat"
            {move || field_count(n.get()).map(|c| view! { <span class="count">{c}</span> })}
        </button>
    }
}

/// The pane: mounted once in the app's grid; drawn while it is open on the
/// route's ticket.
#[component]
pub(crate) fn Pane(ticket: Signal<Option<String>>) -> impl IntoView {
    let shown = shown();
    // The route: `?chat=` opens it; another page closes it.
    {
        let shown = shown.clone();
        Effect::new(move |_| {
            let t = ticket.get();
            let hash = dom::window().location().hash().unwrap_or_default();
            match (t, chat_in(&hash)) {
                (Some(t), Some(c)) => {
                    if shown.with_untracked(|s| s.as_ref().map(|s| (s.ticket.to_ascii_uppercase(), s.view.clone()))) != Some((t.to_ascii_uppercase(), View::Chat(c))) {
                        open_on(&t, Some(c));
                    }
                }
                (Some(t), None) if shown.with_untracked(|s| s.as_ref().is_some_and(|s| !s.ticket.eq_ignore_ascii_case(&t))) => close(),
                (None, _) => close(),
                _ => {}
            }
        });
    }
    // The URL follows the chat shown.
    {
        let shown = shown.clone();
        Effect::new(move |_| {
            let chat = shown.with(|s| match s.as_ref().map(|s| &s.view) {
                Some(View::Chat(c)) => Some(*c),
                _ => None,
            });
            keep_url(chat);
        });
    }
    // Auto resolved once the chats are held.
    {
        let shown = shown.clone();
        let held = chats::signal();
        Effect::new(move |_| {
            let Some(s) = shown.get() else { return };
            if s.view != View::Auto {
                return;
            }
            let Some(open) = held.with(|h| h.as_ref().map(|h| h.on(&s.ticket, ChatState::Open))) else { return };
            show(default_view(&open));
        });
    }
    move || {
        let s = shown.get()?;
        ticket.get().is_some_and(|t| t.eq_ignore_ascii_case(&s.ticket)).then(|| pane_view(s))
    }
}

pub(crate) fn names() -> impl Fn(&str) -> String + Clone + 'static {
    let me = chats::me();
    let my = chats::my_name();
    let held = chats::signal();
    // Tracked: a view that names someone draws again once a message says its name.
    move |p: &str| held.with(|h| display(p, &me, my.as_deref(), h.as_ref()))
}

fn pane_view(s: Shown) -> impl IntoView {
    let held = chats::signal();
    let ticket = s.ticket.clone();
    let open = {
        let t = ticket.clone();
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().map(|h| h.on(&t, ChatState::Open)).unwrap_or_default()))
    };
    let closed_n = {
        let t = ticket.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().map_or(0, |h| h.on(&t, ChatState::Closed).len())))
    };
    let current = s.view.clone();
    let name = names();
    let me = chats::me();
    let tabs = {
        let current = current.clone();
        move || {
            let name = name.clone();
            open.get()
                .into_iter()
                .map(|c| {
                    let id = c.id.get();
                    let on = current == View::Chat(id);
                    let label = tab_label(&c, &me, &name);
                    view! {
                        <button type="button" class="ptab" class:on=on role="tab" aria-selected=on.to_string() data-chat=id.to_string() on:click=move |_| show(View::Chat(id))>
                            <span class="pl">{label}</span>
                            {(c.unread > 0).then(|| view! { <span class="count">{c.unread}</span> })}
                        </button>
                    }
                })
                .collect_view()
        }
    };
    let body = match current.clone() {
        View::Auto => view! { <p class="pempty">"loading chats…"</p> }.into_any(),
        View::Chat(id) => view! { <ChatView ticket=ticket.clone() id /> }.into_any(),
        View::Pick => view! { <Picker ticket=ticket.clone() add_to=None on_done=Callback::new(|_| ()) /> }.into_any(),
        View::Closed => closed_view(ticket.clone()).into_any(),
    };
    let label = format!("chats on {ticket}");
    let is_pick = current == View::Pick;
    let is_closed = current == View::Closed;
    view! {
        <aside id="chatpane" aria-label=label data-ticket=ticket.clone()>
            <Grab />
            <div class="ptabs" role="tablist">
                {tabs}
                <button type="button" class="pnew" class:on=is_pick on:click=move |_| show(View::Pick)>"+ chat"</button>
                {move || {
                    let n = closed_n.get();
                    (n > 0).then(|| view! {
                        <button type="button" class="pclosed" class:on=is_closed title="the closed chats, read-only" on:click=move |_| show(View::Closed)>
                            {format!("closed {n}")}
                        </button>
                    })
                }}
                <button type="button" class="px" aria-label="close the chats" title="close the chats" on:click=move |_| close()>"×"</button>
            </div>
            {body}
        </aside>
    }
}

/// The sheet's handle (under 760px): dragged down past [`DISMISS_PX`], the
/// sheet goes; less, it springs back.
#[component]
fn Grab() -> impl IntoView {
    let start = StoredValue::new(None::<f64>);
    let pane = || dom::window().document().and_then(|d| d.get_element_by_id("chatpane")).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    let place = move |dy: f64| {
        if let Some(p) = pane() {
            let _ = p.style().set_property("translate", &format!("0 {}px", dy.max(0.0)));
        }
    };
    let end = move |ev: web_sys::PointerEvent| {
        let Some(y0) = start.get_value() else { return };
        start.set_value(None);
        place(0.0);
        if dismisses(f64::from(ev.client_y()) - y0) {
            close();
        }
    };
    view! {
        <div
            class="pgrab"
            aria-hidden="true"
            on:pointerdown=move |ev: web_sys::PointerEvent| {
                // No text selection or native drag to cancel the pointer.
                ev.prevent_default();
                if let Some(t) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                    let _ = t.set_pointer_capture(ev.pointer_id());
                }
                start.set_value(Some(f64::from(ev.client_y())));
            }
            on:pointermove=move |ev: web_sys::PointerEvent| {
                if let Some(y0) = start.get_value() {
                    place(f64::from(ev.client_y()) - y0);
                }
            }
            on:pointerup=end
            on:pointercancel=move |_| {
                start.set_value(None);
                place(0.0);
            }
        ></div>
    }
}

/// One chat: who is in it, its messages, the composer (or, closed, reopen).
#[component]
fn ChatView(ticket: String, id: u64) -> impl IntoView {
    chats::load(id);
    let held = chats::signal();
    let chat = {
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().and_then(|h| h.get(id).cloned())))
    };
    let thread = {
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().and_then(|h| h.thread(id).map(|t| t.list.clone()))).unwrap_or_default())
    };
    // The read mark as the chat opened: the *new* line, and what arrives after.
    let mark = StoredValue::new(None::<(Option<u64>, u64)>);
    Effect::new(move |_| {
        if mark.get_value().is_some() {
            return;
        }
        let (Some(c), list) = (chat.get(), thread.get()) else { return };
        if c.last_message.is_some() && list.is_empty() {
            return; // not fetched yet
        }
        mark.set_value(Some((new_from(&list, c.unread), list.last().map_or(0, |m| m.id.get()))));
    });
    // Reading: what is shown is read, up to its last message.
    let sent = StoredValue::new(0u64);
    Effect::new(move |_| {
        let Some(c) = chat.get() else { return };
        let last = thread.with(|t| t.last().map(|m| m.id.get()));
        let Some(last) = last.filter(|l| c.unread > 0 && *l > sent.get_value()) else { return };
        if dom::window().document().is_some_and(|d| d.hidden()) {
            return;
        }
        sent.set_value(last);
        spawn_local(async move {
            if let Err(e) = chats::read(id, last).await {
                leptos::logging::log!("chats.read: {e}");
            }
        });
    });
    // The newest message in view.
    let list_ref = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        thread.track();
        if let Some(el) = list_ref.get() {
            el.set_scroll_top(el.scroll_height());
        }
    });
    let adding = RwSignal::new(false);
    let name = names();
    let me = chats::me();
    let who = {
        let name = name.clone();
        let me = me.clone();
        move || {
            let Some(c) = chat.get() else { return view! { <div class="pwho"></div> }.into_any() };
            let open = c.state == ChatState::Open;
            let people = c
                .participants
                .iter()
                .map(|p| {
                    let waiting = c.waiting.contains(p);
                    let tip = if waiting { format!("{p} — waiting for a message") } else { p.clone() };
                    view! {
                        <span class="pp" title=tip data-principal=p.clone()>
                            <span class="dot" class:wait=waiting></span>
                            {name(p)}
                            {chats::is_agent(p).then(|| view! { <span class="ag">"agent"</span> })}
                        </span>
                    }
                })
                .collect_view();
            let err = RwSignal::new(None::<String>);
            let flip = move |_| {
                spawn_local(async move {
                    match chats::settle_state(id, !open).await {
                        Ok(()) if open => show(View::Closed),
                        Ok(()) => {}
                        Err(e) => {
                            err.try_set(Some(e));
                        }
                    }
                });
            };
            let _ = &me;
            view! {
                <div class="pwho">
                    {people}
                    <span class="pmore">
                        {open.then(|| view! { <button type="button" class="padd" on:click=move |_| adding.update(|a| *a = !*a)>"add"</button> })}
                        <button type="button" class="pflip" data-act=if open { "close" } else { "reopen" } on:click=flip>
                            {if open { "close chat" } else { "reopen" }}
                        </button>
                    </span>
                    {move || err.get().map(|e| view! { <span class="perr" role="alert">{e}</span> })}
                </div>
            }
            .into_any()
        }
    };
    let messages = {
        let name = name.clone();
        let me = me.clone();
        move || {
            let list = thread.get();
            let (new_at, seen) = mark.get_value().unwrap_or((None, u64::MAX));
            let waiting: Vec<String> = chat.with(|c| c.as_ref().map(|c| c.waiting.iter().filter(|p| **p != me).cloned().collect()).unwrap_or_default());
            let empty = list.is_empty();
            let rows = list
                .into_iter()
                .map(|m| {
                    let id = m.id.get();
                    let line = (new_at == Some(id)).then(|| view! { <div class="pnewline" role="separator">"new"</div> });
                    view! { {line}{message_view(m, id > seen, &name)} }
                })
                .collect_view();
            let notes = waiting
                .into_iter()
                .map(|p| {
                    let who = name(&p);
                    view! { <div class="pnote listen"><span class="dot wait"></span><span>{format!("{who} is waiting for a message")}</span></div> }
                })
                .collect_view();
            view! {
                {empty.then(|| view! { <p class="pempty">"No messages yet. Say what this chat is about."</p> })}
                {rows}
                {notes}
            }
        }
    };
    let label = {
        let name = name.clone();
        let me_l = me.clone();
        Memo::new(move |_| chat.with(|c| c.as_ref().map(|c| tab_label(c, &me_l, &name)).unwrap_or_default()))
    };
    let is_open = Memo::new(move |_| chat.with(|c| c.as_ref().is_some_and(|c| c.state == ChatState::Open)));
    let found = Memo::new(move |_| chat.with(Option::is_some));
    let participants = Signal::derive(move || chat.with(|c| c.as_ref().map(|c| c.participants.clone()).unwrap_or_default()));
    let held_some = Memo::new(move |_| held.with(Option::is_some));
    view! {
        {move || (held_some.get() && !found.get()).then(|| view! { <p class="pempty">"This chat is not one you are in."</p> })}
        {who}
        {move || adding.get().then(|| view! {
            <Picker ticket=ticket.clone() add_to=Some((id, participants.get_untracked())) on_done=Callback::new(move |_| adding.set(false)) />
        })}
        <div class="pmsgs" node_ref=list_ref aria-live="polite">{messages}</div>
        {move || if is_open.get() {
            view! { <Composer id label=label.into() adding /> }.into_any()
        } else if found.get() {
            view! { <div class="pshut">"closed — read-only"<button type="button" class="preopen" on:click=move |_| spawn_local(async move { let _ = chats::settle_state(id, true).await; })>"reopen"</button></div> }.into_any()
        } else {
            ().into_any()
        }}
    }
}

/// One message: an avatar, who (an agent marked, its `via`), when, and the
/// words through `md.rs`.
fn message_view(m: Message, fresh: bool, name: &impl Fn(&str) -> String) -> impl IntoView {
    let who = m.name.clone().unwrap_or_else(|| name(&m.author));
    let agent = chats::is_agent(&m.author) || m.via.is_some();
    let html = md::render_body(&m.body).html;
    let node = NodeRef::<leptos::html::Div>::new();
    node.on_load(|el: web_sys::HtmlDivElement| crate::highlight::mount(&el));
    view! {
        <div class="pmsg" class:fresh=fresh data-message-id=m.id.get().to_string()>
            <span class="pav" class:agent=agent aria-hidden="true">{crate::conversation::initials(&who)}</span>
            <div class="pmw">
                <div class="pmh">
                    <b>{who}</b>
                    {agent.then(|| view! { <span class="ag">"agent"</span> })}
                    <span class="t" title=fmt_unix(m.at)>{clock(m.at)}</span>
                    {m.via.clone().map(|v| view! { <span class="via" title=v.clone()>{format!("via {}", short(&v))}</span> })}
                </div>
                <div class="pmb md" data-links="chat" node_ref=node inner_html=html></div>
            </div>
        </div>
    }
}

/// The composer: markdown; `@` at a word's start adds a participant;
/// ⌘↵ / Ctrl+Enter sends.
#[component]
fn Composer(id: u64, label: Signal<String>, adding: RwSignal<bool>) -> impl IntoView {
    let body = RwSignal::new(String::new());
    let sending = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let send = move || {
        let text = body.get_untracked();
        if text.trim().is_empty() || sending.get_untracked() {
            return;
        }
        sending.set(true);
        spawn_local(async move {
            let r = chats::post(id, &text).await;
            sending.try_set(false);
            match r {
                Ok(()) => {
                    body.try_set(String::new());
                    error.try_set(None);
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    };
    let area = NodeRef::<leptos::html::Textarea>::new();
    crate::grow::follow(area, move || body.track());
    view! {
        <div class="pcompose">
            <textarea
                node_ref=area
                class="pbox grow"
                rows="2"
                aria-label=move || format!("message {}", label.get())
                placeholder=move || placeholder(&label.get())
                prop:value=move || body.get()
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    if wants_add(&v) {
                        adding.set(true);
                    }
                    body.set(v);
                }
                on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                    Some("comment.save") => {
                        ev.prevent_default();
                        send();
                    }
                    Some("form.cancel") => {
                        ev.prevent_default();
                        if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                            let _ = el.blur();
                        }
                    }
                    _ => {}
                }
            ></textarea>
            <div class="prow">
                <span>{if bindings::is_mac() { "markdown · @ to add · ⌘↵ sends" } else { "markdown · @ to add · Ctrl+Enter sends" }}</span>
                {move || error.get().map(|e| view! { <span class="perr" role="alert">{e}</span> })}
                <button type="button" class="psend" disabled=move || sending.get() || body.with(|b| b.trim().is_empty()) on:click=move |_| send()>
                    {move || if sending.get() { "sending…" } else { "send" }}
                </button>
            </div>
        </div>
    }
}

/// The picker: **+ chat** (`add_to` none) — people and agents, who can read
/// the ticket, the same people's chat opened instead; or *add* to a chat
/// (`add_to`: it and who is in it) — one picked is added.
#[component]
fn Picker(ticket: String, add_to: Option<(u64, Vec<String>)>, on_done: Callback<()>) -> impl IntoView {
    let cands = RwSignal::new(None::<Result<Vec<(String, bool)>, String>>);
    {
        let t = ticket.clone();
        spawn_local(async move {
            let r = chats::candidates(&t).await;
            cands.try_set(Some(r));
        });
    }
    let query = RwSignal::new(String::new());
    let picked = RwSignal::new(Vec::<String>::new());
    let error = RwSignal::new(None::<String>);
    let held = chats::signal();
    let me = chats::me();
    let name = names();
    let (chat_id, already) = match add_to {
        Some((c, ps)) => (Some(c), ps),
        None => (None, Vec::new()),
    };
    let waiting = {
        let t = ticket.clone();
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().map(|h| h.waiting_on(&t)).unwrap_or_default()))
    };
    let existing = {
        let (t, me) = (ticket.clone(), me.clone());
        Memo::new(move |_| {
            let p = picked.get();
            if p.is_empty() || chat_id.is_some() {
                return None;
            }
            held.with(|h| h.as_ref().and_then(|h| h.existing(&t, &me, &p)))
        })
    };
    let list = {
        let (ticket, name, me) = (ticket.clone(), name.clone(), me.clone());
        move || match cands.get() {
            None => view! { <p class="pempty">"asking who can read it…"</p> }.into_any(),
            Some(Err(e)) => view! { <p class="perr" role="alert">{e}</p> }.into_any(),
            Some(Ok(all)) => {
                let shown = rows(&all, &me, &already, &query.get(), &name);
                if shown.is_empty() {
                    return view! { <p class="pempty">"Nobody else holds a grant here."</p> }.into_any();
                }
                let waiting = waiting.get();
                let items = shown
                    .into_iter()
                    .map(|(p, reads)| {
                        let on = picked.with(|v| v.contains(&p));
                        let what = if !reads {
                            refusal(&ticket)
                        } else if waiting.contains(&p) {
                            "waiting for a message".to_string()
                        } else {
                            format!("can read {ticket}")
                        };
                        let pc = p.clone();
                        let click = move |_| {
                            if !reads {
                                return;
                            }
                            match chat_id {
                                Some(c) => {
                                    let p = pc.clone();
                                    spawn_local(async move {
                                        match chats::add(c, &p).await {
                                            Ok(()) => on_done.run(()),
                                            Err(e) => {
                                                error.try_set(Some(e));
                                            }
                                        }
                                    });
                                }
                                None => picked.update(|v| {
                                    if let Some(at) = v.iter().position(|x| *x == pc) {
                                        v.remove(at);
                                    } else {
                                        v.push(pc.clone());
                                    }
                                }),
                            }
                        };
                        let who = name(&p);
                        view! {
                            <li class="pcand" class:hi=on class:no=!reads aria-disabled=(!reads).to_string() data-principal=p.clone() on:click=click>
                                <span class="pav" class:agent=chats::is_agent(&p) aria-hidden="true">{crate::conversation::initials(&who)}</span>
                                <div>
                                    {who}
                                    {chats::is_agent(&p).then(|| view! { <span class="ag">"agent"</span> })}
                                    <small class:refuse=!reads>{what}</small>
                                </div>
                                <span class="dot" class:wait=waiting.contains(&p)></span>
                            </li>
                        }
                    })
                    .collect_view();
                view! { <ul class="pcands">{items}</ul> }.into_any()
            }
        }
    };
    let tokens = {
        let name = name.clone();
        move || {
            picked
                .get()
                .into_iter()
                .map(|p| {
                    let label = format!("{} ×", name(&p));
                    let agent = chats::is_agent(&p);
                    view! { <button type="button" class="ptok" class:agent=agent on:click=move |_| picked.update(|v| v.retain(|x| *x != p))>{label}</button> }
                })
                .collect_view()
        }
    };
    let start = {
        let t = ticket.clone();
        move |_| {
            if let Some(c) = existing.get_untracked() {
                show(View::Chat(c));
                return;
            }
            let others = picked.get_untracked();
            if others.is_empty() {
                return;
            }
            let t = t.clone();
            spawn_local(async move {
                match chats::open(&t, &others).await {
                    Ok(c) => show(View::Chat(c)),
                    Err(e) => {
                        error.try_set(Some(e));
                    }
                }
            });
        }
    };
    let heading = match chat_id {
        Some(_) => "Add someone to this chat".to_string(),
        None => format!("Chat about {ticket}"),
    };
    let adding = chat_id.is_some();
    view! {
        <div class="ppicker" class:adding=adding>
            <h4>{heading}</h4>
            <div class="pq">
                {tokens}
                <input type="text" class="pqi" aria-label="who to talk to" placeholder="who…" autocomplete="off" spellcheck="false"
                    prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev)) />
            </div>
            {list}
            {move || error.get().map(|e| view! { <p class="perr" role="alert">{e}</p> })}
            {(!adding).then(|| view! {
                <div class="prow">
                    <span>{move || if existing.get().is_some() { "a chat with them is open already: it opens instead" } else { "people and agents who can read the ticket" }}</span>
                    <button type="button" class="psend" disabled=move || picked.with(Vec::is_empty) on:click=start>
                        {move || if existing.get().is_some() { "open" } else { "start" }}
                    </button>
                </div>
            })}
            {adding.then(|| view! { <div class="prow"><button type="button" class="pcancel" on:click=move |_| on_done.run(())>"cancel"</button></div> })}
        </div>
    }
}

/// The closed list: each closed chat, opened read-only, with reopen.
fn closed_view(ticket: String) -> impl IntoView {
    let held = chats::signal();
    let name = names();
    let me = chats::me();
    move || {
        let closed = held.with(|h| h.as_ref().map(|h| h.on(&ticket, ChatState::Closed)).unwrap_or_default());
        if closed.is_empty() {
            return view! { <p class="pempty">"No closed chats."</p> }.into_any();
        }
        let rows = closed
            .into_iter()
            .map(|c| {
                let id = c.id.get();
                let label = tab_label(&c, &me, &name);
                view! {
                    <li class="pclosedrow" data-chat=id.to_string()>
                        <button type="button" class="popen" on:click=move |_| show(View::Chat(id))>{label}</button>
                        <span class="t" title=fmt_unix(c.last_at)>{crate::fmt_relative(crate::now_unix(), c.last_at)}</span>
                        <button type="button" class="preopen" on:click=move |_| spawn_local(async move {
                            if chats::settle_state(id, true).await.is_ok() {
                                show(View::Chat(id));
                            }
                        })>"reopen"</button>
                    </li>
                }
            })
            .collect_view();
        view! { <ul class="pclosedlist">{rows}</ul> }.into_any()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{pick_single, table_with, Press};
    use crate::chats::tests::{chat, msg};
    use menger_tickets_api::ChatsEvent;

    /// URL row: `?chat=ID` opens the pane on that chat, and the pane keeps
    /// the query to the chat it shows, the rest of the query as it was.
    #[test]
    fn the_url_names_the_chat() {
        assert_eq!(chat_in("#/ticket/KILN-7?chat=41"), Some(41));
        assert_eq!(chat_in("#/ticket/KILN-7?view=full&chat=41"), Some(41));
        assert_eq!(chat_in("#/ticket/KILN-7?chat=x"), None);
        assert_eq!(chat_in("#/ticket/KILN-7"), None);
        assert_eq!(with_chat("#/ticket/KILN-7", Some(41)), "#/ticket/KILN-7?chat=41");
        assert_eq!(with_chat("#/ticket/KILN-7?chat=41&g=x", Some(42)), "#/ticket/KILN-7?g=x&chat=42");
        assert_eq!(with_chat("#/ticket/KILN-7?chat=41", None), "#/ticket/KILN-7");
    }

    /// Field row: **chat N** — N the reader's unread on the ticket, none
    /// drawn at 0.
    #[test]
    fn the_field_counts_the_unread_here() {
        let mut h = Held::from_events([ChatsEvent::Chat { chat: chat(41, "KILN-7", &["ada", "builder"]) }]);
        assert_eq!(field_count(h.unread_on("KILN-7")), None);
        h.apply(&ChatsEvent::Posted { message: msg(1, 41, "builder", "x") }, "ada");
        h.apply(&ChatsEvent::Posted { message: msg(2, 41, "builder", "y") }, "ada");
        assert_eq!(field_count(h.unread_on("KILN-7")).as_deref(), Some("2"));
    }

    /// Pane row: a tab per chat, labelled by its participants (the reader by
    /// its name, an agent by the name its messages gave, else few
    /// characters); the pane opens on the first chat with unread.
    #[test]
    fn tabs_say_who_and_the_pane_opens_on_unread() {
        let mut h = Held::from_events([ChatsEvent::Chat { chat: chat(41, "KILN-7", &["idp:ada", "agent:builder"]) }, ChatsEvent::Chat { chat: chat(42, "KILN-7", &["idp:ada", "key:4b2e0000aa"]) }]);
        let name = |p: &str| display(p, "idp:ada", Some("ada"), Some(&h));
        assert_eq!(tab_label(h.get(41).unwrap(), "idp:ada", name), "agent:builder");
        assert_eq!(tab_label(h.get(42).unwrap(), "idp:ada", name), "key:4b2e…");
        assert_eq!(tab_label(&chat(7, "KILN-7", &["idp:ada"]), "idp:ada", name), "ada", "alone in it: the reader");
        let open = h.on("KILN-7", ChatState::Open);
        assert_eq!(default_view(&open), View::Chat(41));
        h.apply(&ChatsEvent::Posted { message: msg(1, 42, "key:4b2e0000aa", "x") }, "idp:ada");
        assert_eq!(default_view(&h.on("KILN-7", ChatState::Open)), View::Chat(42), "the one with unread");
        assert_eq!(default_view(&[]), View::Pick, "none yet: the picker");
        let mut titled = chat(9, "KILN-7", &["idp:ada"]);
        titled.title = Some("the firing".into());
        assert_eq!(tab_label(&titled, "idp:ada", |p: &str| p.to_string()), "the firing");
    }

    /// Pane row: the *new* line sits before the first unread message as the
    /// chat opened; none when nothing is unread.
    #[test]
    fn the_new_line_is_at_the_read_mark() {
        let t = vec![msg(1, 41, "ada", "a"), msg(2, 41, "builder", "b"), msg(5, 41, "builder", "c")];
        assert_eq!(new_from(&t, 0), None);
        assert_eq!(new_from(&t, 1), Some(5));
        assert_eq!(new_from(&t, 2), Some(2));
        assert_eq!(new_from(&t, 9), None, "more unread than held: not fetched yet");
    }

    /// Composer row: `@` at a word's start adds a participant (not inside a
    /// word, an address); ⌘↵ / Ctrl+Enter sends, a bare Enter is a new line.
    #[test]
    fn the_composer_adds_with_at_and_sends_with_mod_enter() {
        assert!(wants_add("@") && wants_add("ask @") && wants_add("hi\n@"));
        assert!(!wants_add("ada@") && !wants_add("ada@x") && !wants_add(""));
        assert_eq!(placeholder("ada · builder"), "Message ada · builder…");
        let t = table_with(&[]);
        let ctrl_enter = Press { key: "Enter".into(), ctrl: true, ..Press::default() };
        assert_eq!(pick_single(&t, &ctrl_enter, Context::Field, false), Some("comment.save"));
        assert_eq!(pick_single(&t, &Press::key("Enter"), Context::Field, false), Some("form.submit"));
    }

    /// Picker row: everyone but the reader (and, adding, who is in it);
    /// those who read the ticket first; one who can't is listed, said so.
    #[test]
    fn the_picker_lists_who_can_read_first() {
        let cands = vec![("idp:sam".to_string(), false), ("agent:reviewer".to_string(), true), ("idp:ada".to_string(), true), ("idp:rui".to_string(), true)];
        let name = |p: &str| p.split(':').nth(1).unwrap_or(p).to_string();
        let got = rows(&cands, "idp:ada", &[], "", name);
        assert_eq!(got, [("agent:reviewer".to_string(), true), ("idp:rui".into(), true), ("idp:sam".into(), false)]);
        assert_eq!(rows(&cands, "idp:ada", &["idp:rui".into()], "", name).len(), 2, "adding: not who is in it");
        assert_eq!(rows(&cands, "idp:ada", &[], "rev", name), [("agent:reviewer".to_string(), true)]);
        assert_eq!(refusal("KILN-7"), "can't read KILN-7, so can't be added");
    }

    /// 390 row: the sheet's handle dragged down far enough dismisses it.
    #[test]
    fn the_sheet_drags_away() {
        assert!(!dismisses(10.0) && !dismisses(79.0));
        assert!(dismisses(80.0) && dismisses(300.0));
    }
}
