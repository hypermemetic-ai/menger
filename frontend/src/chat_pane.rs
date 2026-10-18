//! CHAT-4: chats inside their ticket — CHAT-1's Figs 1–4 — and, since
//! CHAT-10 (CHAT-9's answer, owner 2026-10-17: `dock`), drawn **in the
//! section dock** (src/sections.rs), not in a pane of their own.
//!
//! - **The field** ([`Field`]): **chat N** among the ticket page's fields,
//!   N the reader's unread on the ticket ([`field_count`]); it opens the
//!   dock on a chat, and closes it again.
//! - **The chat in the dock** ([`Inside`]): the dock turned into the
//!   conversation in place — its head (*← sections*, the lit section, ×),
//!   a chip per open chat on the ticket (its participants, its unread),
//!   **+ chat**, the closed list. At ≤ 820 it is the phone bar's sections
//!   sheet that turns into it, its handle and head dragged down to dismiss
//!   it (CHAT-31: every sheet's gesture, src/sheet.rs). [`Follow`] keeps the route and the shown
//!   chat in step, mounted once in the app. The participants row: a
//!   dot each, pulsing while one waits on `await_message` (under the Motion
//!   setting), agents marked ([`crate::chats::is_agent`]); *add* and *close
//!   chat*. CHAT-7: the turns (src/turns.rs — yours bubbles, theirs
//!   prose, code heads, ticket cards, manifest's scroll rules), a *new*
//!   line at the reader's read mark when the chat was opened
//!   ([`new_from`]), a message that arrives sliding in (DESIGN-39: none
//!   under Motion none), and manifest's composer (src/composer.rs). ⤢ on
//!   the head makes it full screen (src/chat_full.rs).
//! - **The picker** ([`Picker`]): `chats.candidates` — each principal with
//!   a grant in the ticket's organization, and whether it reads the ticket;
//!   one that cannot is shown and cannot be picked ([`refusal`]); the same
//!   people's open chat opens instead of a new one
//!   ([`crate::chats::Held::existing`]); nobody picked is "just me", a
//!   chat of one (CHAT-6, [`start_of`]).
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

use crate::chats::{self, short, Held};
use crate::{dom, fmt_unix, live, scale, turns};

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

/// `chat=<id>` in a route's query.
/// CHAT-8: `?at=MSG` — open the chat at that message.
pub(crate) fn at_in(hash: &str) -> Option<u64> {
    let (_, q) = hash.split_once('?')?;
    q.split('&').find_map(|p| p.strip_prefix("at=")).and_then(|v| v.parse().ok())
}

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

/// What the picker says of one who cannot be picked: it can't read the
/// ticket — or, outside tickets (CHAT-14), holds no grant there.
pub(crate) fn refusal(ticket: &str) -> String {
    match ticket {
        "the organization" => "holds no grant here, so can't be added".to_string(),
        t => format!("can't read {t}, so can't be added"),
    }
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

/// A message's time, `HH:MM` where the reader is.
fn clock(at: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(at as f64 * 1000.0));
    format!("{:02}:{:02}", d.get_hours(), d.get_minutes())
}

// -- the state ----------------------------------------------------------------

thread_local! {
    static SHOWN: ArcRwSignal<Option<Shown>> = ArcRwSignal::new(None);
}

pub(crate) fn shown() -> ArcRwSignal<Option<Shown>> {
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

/// CHAT-10: whether chat `id` is the one shown.
pub(crate) fn showing(id: u64) -> bool {
    shown().with(|s| s.as_ref().is_some_and(|s| s.view == View::Chat(id)))
}

/// Close the chat.
pub(crate) fn close() {
    shown().set(None);
}

pub(crate) fn show(view: View) {
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

/// The route and the chat shown, kept in step — mounted once in the app;
/// the chat itself is drawn in the dock ([`Inside`]).
#[component]
pub(crate) fn Follow(ticket: Signal<Option<String>>) -> impl IntoView {
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
}

/// CHAT-10: the chat as the dock draws it, while it is open on `ticket` —
/// `lit` the head's words for the section being read (`2/6 Problem`),
/// `back` what *← sections* and × do beyond closing the chat (`true` for ×).
#[component]
pub(crate) fn Inside(ticket: String, #[prop(into)] lit: Signal<String>, back: Callback<bool>) -> impl IntoView {
    let shown = shown();
    let full = crate::chat_full::signal();
    move || {
        let s = shown.get()?;
        // CHAT-7: while the chat is full screen, the dock holds nothing.
        (s.ticket.eq_ignore_ascii_case(&ticket) && !full.get()).then(|| pane_view(s, lit, back))
    }
}

pub(crate) fn names() -> impl Fn(&str) -> String + Clone + 'static {
    let me = chats::me();
    let my = chats::my_name();
    let held = chats::signal();
    // Tracked: a view that names someone draws again once a message says its name.
    move |p: &str| held.with(|h| display(p, &me, my.as_deref(), h.as_ref()))
}

fn pane_view(s: Shown, lit: Signal<String>, back: Callback<bool>) -> impl IntoView {
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
                    let waits = c.waiting.iter().any(|p| *p != me);
                    view! {
                        <button type="button" class="ptab" class:on=on role="tab" aria-selected=on.to_string() data-chat=id.to_string() on:click=move |_| show(View::Chat(id))>
                            <span class="dot" class:wait=waits></span>
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
        View::Chat(id) => view! { <ChatView ticket=Some(ticket.clone()) id /> }.into_any(),
        View::Pick => view! { <Picker ticket=Some(ticket.clone()) add_to=None on_done=Callback::new(|_| ()) /> }.into_any(),
        View::Closed => closed_view(ticket.clone()).into_any(),
    };
    let label = format!("chats on {ticket}");
    let is_pick = current == View::Pick;
    let is_closed = current == View::Closed;
    let close_x = move |_| {
        close();
        back.run(true);
    };
    view! {
        <div id="chatpane" role="region" aria-label=label data-ticket=ticket.clone()>
            <div class="dchead">
                <button type="button" class="pback" title="back to the sections" on:click=move |_| { close(); back.run(false); }>"← sections"</button>
                <span class="plit">{move || lit.get()}</span>
                {matches!(current, View::Chat(_)).then(|| view! {
                    <button type="button" class="pexpand" aria-label="the chat full screen" title="the chat full screen (g c)" on:click=move |_| crate::chat_full::set_full(true)>"⤢"</button>
                })}
                <button type="button" class="px" aria-label="close the chat" title="close the chat" on:click=close_x>"×"</button>
            </div>
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
            </div>
            {body}
        </div>
    }
}

/// One chat: who is in it (the dock's; the session's head says it), its
/// turns (src/turns.rs), the composer (src/composer.rs) — or, closed,
/// reopen. CHAT-7: the same view in the dock and full screen (`full`).
#[component]
/// CHAT-14: `ticket` none for a direct message or a group.
pub(crate) fn ChatView(ticket: Option<String>, id: u64, #[prop(optional)] full: bool) -> impl IntoView {
    let held = chats::signal();
    let chat = {
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().and_then(|h| h.get(id).cloned())))
    };
    // CHAT-8: the window of messages held (src/window.rs), not the chat.
    let window_t = {
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().and_then(|h| h.thread(id).cloned())).unwrap_or_default())
    };
    let thread = Memo::new(move |_| window_t.with(|t| t.list.clone()));
    // A page is sized to the window; the first is the newest, or the page
    // around the read mark (more unread than a page), or `?at=MSG`.
    let size = scale::page_size(dom::window().inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(900.0));
    let at_url = at_in(&dom::window().location().hash().unwrap_or_default());
    let started = StoredValue::new(false);
    Effect::new(move |_| {
        let Some(c) = chat.get() else { return };
        if started.get_value() {
            return;
        }
        started.set_value(true);
        let around = at_url.or_else(|| (c.unread > size).then_some(c.mark.map(|m| m.get())).flatten());
        chats::load(id, size, around);
        // CHAT-18: what has been done before the chat opened, mid-turn.
        chats::load_activity(id);
    });
    // The read mark as the chat opened: the *new* line (the first message
    // after the mark), what arrives after it, and where opening lands.
    let mark = StoredValue::new(None::<(Option<u64>, u64)>);
    let landing = StoredValue::new(None::<turns::Landing>);
    Effect::new(move |_| {
        if mark.get_value().is_some() {
            return;
        }
        let Some(c) = chat.get() else { return };
        let fetched = window_t.with(|t| t.fetched);
        if !fetched && c.last_message.is_some() {
            return; // not fetched yet
        }
        let (new_at, last) = window_t.with(|t| (c.mark.map(|m| m.get()).and_then(|m| t.list.iter().find(|x| x.id.get() > m)).filter(|_| c.unread > 0).map(|x| x.id.get()), t.last().unwrap_or(0)));
        mark.set_value(Some((new_at, last)));
        landing.set_value(Some(match at_url.or(new_at) {
            Some(m) => turns::Landing::At(m),
            None => turns::Landing::Newest,
        }));
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
    // CHAT-8: the virtual list — only what is within two viewports of the
    // visible is mounted, the rest spacers of measured heights; and the
    // scroll held on what is being read through every change (CHAT-7's
    // rules, src/turns.rs's Scroll, on top).
    let list_ref = NodeRef::<leptos::html::Div>::new();
    let rows_ref = NodeRef::<leptos::html::Div>::new();
    let heights = RwSignal::new(std::collections::HashMap::<u64, f64>::new());
    let view_h = RwSignal::new(0.0f64);
    let rel_top = RwSignal::new(0.0f64);
    let layout = Memo::new(move |_| heights.with(|h| scale::Layout::new(thread.with(|t| t.iter().map(|m| m.id.get()).collect()), h)));
    let range = Memo::new(move |_| layout.with(|l| l.mounted(rel_top.get(), view_h.get().max(1.0))));
    let mounted = Memo::new(move |_| {
        let (i0, i1) = range.get();
        thread.with(|t| t.get(i0..i1).map(<[Message]>::to_vec).unwrap_or_default())
    });
    let space_above = Memo::new(move |_| layout.with(|l| l.tops.get(range.get().0).copied().unwrap_or(0.0)));
    let space_below = Memo::new(move |_| layout.with(|l| l.total() - l.tops.get(range.get().1).copied().unwrap_or(l.total())));
    let scroll = StoredValue::new(turns::Scroll::default());
    let arrow = RwSignal::new(None::<String>);
    let anchor = StoredValue::new(None::<(u64, f64)>);
    let stick = StoredValue::new(true);
    let landed = StoredValue::new(false);
    let fetching = StoredValue::new(false);
    let seen_last = StoredValue::new(0u64);
    // When the view last set the scroll itself: the scroll events that
    // follow are its own, not the reader's.
    let ours = StoredValue::new(0.0f64);
    // Where the list begins inside the scroller's content.
    let list_offset = move || -> Option<(web_sys::HtmlDivElement, f64)> {
        let el = list_ref.get_untracked()?;
        let rows = rows_ref.get_untracked()?;
        let off = rows.get_bounding_client_rect().top() - el.get_bounding_client_rect().top() + f64::from(el.scroll_top());
        Some((el, off))
    };
    let read_scroll = move || {
        let Some((el, off)) = list_offset() else { return };
        view_h.set(f64::from(el.client_height()));
        rel_top.set(f64::from(el.scroll_top()) - off);
    };
    // Measure what is mounted: each row's height, once it differs.
    let measure = move || {
        let Some(rows) = rows_ref.get_untracked() else { return };
        let Ok(list) = rows.query_selector_all(".trow") else { return };
        let mut changed = Vec::new();
        for i in 0..list.length() {
            let Some(el) = list.item(i).and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) else { continue };
            let Some(mid) = el.get_attribute("data-mid").and_then(|v| v.parse::<u64>().ok()) else { continue };
            let h = el.get_bounding_client_rect().height();
            if heights.with_untracked(|m| m.get(&mid).is_none_or(|old| (old - h).abs() > 0.5)) {
                changed.push((mid, h));
            }
        }
        if !changed.is_empty() {
            heights.update(|m| m.extend(changed));
        }
    };
    // After any change of the list or a height: land once, else stick to
    // the bottom, else put the anchor back.
    let settle_scroll = move || {
        let Some((el, off)) = list_offset() else { return };
        let total = layout.with_untracked(scale::Layout::total);
        if !landed.get_value() {
            let Some(l) = landing.get_value() else { return };
            if !window_t.with_untracked(|t| t.fetched) {
                return;
            }
            landed.set_value(true);
            scroll.update_value(|s| {
                s.land(None);
            });
            match l {
                turns::Landing::Newest => stick.set_value(true),
                turns::Landing::At(m) => {
                    stick.set_value(false);
                    anchor.set_value(Some((m, -40.0)));
                }
            }
        }
        if stick.get_value() {
            ours.set_value(js_sys::Date::now());
            el.set_scroll_top(el.scroll_height());
        } else if let Some(y) = anchor.get_value().and_then(|a| layout.with_untracked(|l| l.restore(a))) {
            ours.set_value(js_sys::Date::now());
            el.set_scroll_top((off + y).round() as i32);
        }
        let _ = total;
        read_scroll();
    };
    // CHAT-8: grow the window as you near its edge, trim its far end.
    let grow = move |side: scale::Side| {
        if fetching.get_value() {
            return;
        }
        fetching.set_value(true);
        spawn_local(async move {
            chats::grow(id, side, size).await;
            fetching.try_set_value(false);
            // What is held stays bounded: the far end goes.
            let (n, (i0, i1)) = (thread.with_untracked(Vec::len), range.get_untracked());
            if let Some((far, k)) = scale::trim(n, i0, i1, size as usize) {
                chats::drop_side(id, far, k);
            }
        });
    };
    let near_edges = move || {
        let (rel, vh) = (rel_top.get_untracked(), view_h.get_untracked());
        let total = layout.with_untracked(scale::Layout::total);
        let (older, newer) = window_t.with_untracked(|t| (t.older, t.newer));
        if older && rel < vh {
            grow(scale::Side::Older);
        } else if newer && total - (rel + vh) < vh {
            grow(scale::Side::Newer);
        }
    };
    Effect::new(move |_| {
        layout.track();
        window_t.track();
        spawn_local(async move {
            dom::sleep_ms(0).await;
            settle_scroll();
            measure();
            near_edges();
        });
    });
    // Arrivals at the bottom: kept there when you were; else counted.
    Effect::new(move |_| {
        let last = thread.with(|t| t.last().map_or(0, |m| m.id.get()));
        let before = seen_last.get_value();
        seen_last.set_value(last);
        if !landed.get_value() || before == 0 || last <= before {
            return;
        }
        let n = thread.with(|t| t.iter().filter(|m| m.id.get() > before).count()) as u32;
        if !scroll.try_update_value(|s| s.arrived(n)).unwrap_or(true) {
            arrow.set(scroll.with_value(turns::Scroll::arrow));
        }
    });
    // A row's height changes after it mounts (code highlighted, a long
    // message shown whole): measured again, the scroll held.
    rows_ref.on_load(move |el| {
        let cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || measure());
        if let Ok(obs) = web_sys::MutationObserver::new(cb.as_ref().unchecked_ref()) {
            let init = web_sys::MutationObserverInit::new();
            init.set_child_list(true);
            init.set_subtree(true);
            let _ = obs.observe_with_options(&el, &init);
            on_cleanup(move || obs.disconnect());
        }
        // CHAT-14: an image drawn changes its row's height with no mutation:
        // its `load` (which does not bubble) heard on the way down.
        let _ = el.add_event_listener_with_callback_and_bool("load", cb.as_ref().unchecked_ref(), true);
        cb.forget();
    });
    let on_scroll = move |_| {
        let Some(el) = list_ref.get_untracked() else { return };
        read_scroll();
        if js_sys::Date::now() - ours.get_value() < 80.0 {
            // The view's own move: what is mounted follows, nothing else.
            near_edges();
            return;
        }
        let at = !turns::off_bottom(f64::from(el.scroll_height()), f64::from(el.scroll_top()), f64::from(el.client_height()));
        stick.set_value(at && !window_t.with_untracked(|t| t.newer));
        anchor.set_value(layout.with_untracked(|l| l.anchor(rel_top.get_untracked())));
        scroll.update_value(|s| s.moved(at));
        arrow.set(scroll.with_value(turns::Scroll::arrow));
        near_edges();
    };
    let jump = move |_| {
        if window_t.with_untracked(|t| t.newer) {
            // The window stops short of the newest: fetch the newest again.
            chats::jump_newest(id, size);
        }
        stick.set_value(true);
        if let Some(el) = list_ref.get_untracked() {
            ours.set_value(js_sys::Date::now());
            el.set_scroll_top(el.scroll_height());
        }
        scroll.update_value(|s| s.moved(true));
        arrow.set(None);
    };
    // CHAT-18: the live work — the turns still running under the last
    // turn, the folded ones after their answers (src/live.rs) — and a clock
    // for staleness, ticking while a turn is live.
    let shape = {
        let held = held.clone();
        Memo::new(move |_| {
            let items = held.with(|h| h.as_ref().map(|h| h.activity(id).to_vec()).unwrap_or_default());
            thread.with(|t| live::shape(&items, t))
        })
    };
    let now = RwSignal::new((js_sys::Date::now() / 1000.0) as i64);
    let ticking = StoredValue::new(false);
    Effect::new(move |_| {
        if shape.with(|s| s.live.is_empty()) || ticking.get_value() {
            return;
        }
        ticking.set_value(true);
        spawn_local(async move {
            loop {
                if now.try_set((js_sys::Date::now() / 1000.0) as i64).is_some() {
                    return; // the view is gone
                }
                if shape.try_with_untracked(|s| s.live.is_empty()).unwrap_or(true) {
                    ticking.try_set_value(false);
                    return;
                }
                dom::sleep_ms(5_000).await;
            }
        });
    });
    // A live block growing keeps the reader at the bottom, as a turn does.
    Effect::new(move |_| {
        shape.track();
        spawn_local(async move {
            dom::sleep_ms(0).await;
            if stick.try_get_value() == Some(true) {
                if let Some(el) = list_ref.get_untracked() {
                    ours.set_value(js_sys::Date::now());
                    el.set_scroll_top(el.scroll_height());
                }
            }
            // The live box shows its newest step.
            if let Some(el) = list_ref.get_untracked() {
                if let Ok(boxes) = el.query_selector_all(".tlive .tlsteps") {
                    for i in 0..boxes.length() {
                        if let Some(b) = boxes.item(i).and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
                            b.set_scroll_top(b.scroll_height());
                        }
                    }
                }
            }
        });
    });
    let loading_earlier = Memo::new(move |_| window_t.with(|t| t.older) && thread.with(|t| !t.is_empty()));
    let body = RwSignal::new(String::new());
    let adding = RwSignal::new(false);
    // CHAT-16: removing a seat that came with a grant asks first.
    let revoking = RwSignal::new(None::<String>);
    let name = names();
    let me = chats::me();
    let who = {
        let name = name.clone();
        move || {
            let Some(c) = chat.get() else { return view! { <div class="pwho"></div> }.into_any() };
            let open = c.state == ChatState::Open;
            let err = RwSignal::new(None::<String>);
            // CHAT-14: a group's members are taken out by its members.
            // CHAT-16: an admin takes anyone out of a ticket's chat too.
            let admin = crate::signin::account().with_untracked(|a| a.is_admin());
            let removable = open && (c.kind == menger_tickets_api::ChatKind::Group || (admin && c.kind == menger_tickets_api::ChatKind::Ticket));
            let people = c
                .participants
                .iter()
                .map(|p| {
                    let waiting = c.waiting.contains(p);
                    let tip = if waiting { format!("{p} — waiting for a message") } else { p.clone() };
                    let who = p.clone();
                    let granted = c.granted_to(p);
                    let out = move |_| {
                        let who = who.clone();
                        if granted {
                            revoking.set(Some(who));
                            return;
                        }
                        spawn_local(async move {
                            if let Err(e) = chats::remove(id, &who).await {
                                err.try_set(Some(e));
                            }
                        });
                    };
                    view! {
                        <span class="pp" title=tip data-principal=p.clone()>
                            <span class="dot" class:wait=waiting></span>
                            {name(p)}
                            {chats::is_agent(p).then(|| view! { <span class="ag">"agent"</span> })}
                            {removable.then(|| view! { <button type="button" class="prm" title="take them out of the group" aria-label=format!("remove {}", name(p)) on:click=out>"×"</button> })}
                        </span>
                    }
                })
                .collect_view();
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
    let alone = Memo::new(move |_| chat.with(|c| c.as_ref().is_some_and(|c| c.participants.len() <= 1)));
    let empty = {
        let t = ticket.clone();
        move || {
            if !thread.with(Vec::is_empty) || chat.with(|c| c.as_ref().is_some_and(|c| c.last_message.is_some())) {
                return None;
            }
            Some(if alone.get() { alone_view(t.as_deref().unwrap_or("this chat"), body).into_any() } else { view! { <p class="pempty">"No messages yet. Say what this chat is about."</p> }.into_any() })
        }
    };
    let rows = {
        let name = name.clone();
        let me = me.clone();
        move |m: Message| {
            let (new_at, seen) = mark.get_value().unwrap_or((None, u64::MAX));
            let mid = m.id.get();
            let line = (new_at == Some(mid)).then(|| view! { <div class="pnewline" role="separator">"new"</div> });
            let mine = m.author == me;
            // CHAT-18: the work that answered it, folded after it.
            let fold = move || shape.with(|s| s.fold_after(mid).cloned()).map(fold_view);
            view! { <div class="trow" data-mid=mid.to_string()>{line}{turn_view(m, mine, mid > seen, &name)}{fold}</div> }
        }
    };
    let waits = {
        let name = name.clone();
        let me = me.clone();
        move || {
            let waiting: Vec<String> = chat.with(|c| c.as_ref().map(|c| c.waiting.iter().filter(|p| **p != me).cloned().collect()).unwrap_or_default());
            waiting
                .into_iter()
                .map(|p| {
                    let line = turns::waiting_line(&name(&p), turns::claim_left(&p));
                    view! { <div class="pnote listen swait"><span class="dot wait"></span><span>{line}</span></div> }
                })
                .collect_view()
        }
    };
    let live = {
        let name = name.clone();
        move || shape.with(|s| s.live.clone()).into_iter().map(|w| live_view(w, &name, now)).collect_view()
    };
    // copy · quote · link, and a code block's copy — one listener.
    let acts = {
        let t = ticket.clone();
        move |ev: web_sys::MouseEvent| {
            let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
            let Some(btn) = el.closest("[data-act]").ok().flatten() else { return };
            let turn = btn.closest("[data-message-id]").ok().flatten();
            let mid: u64 = turn.as_ref().and_then(|t| t.get_attribute("data-message-id")).and_then(|v| v.parse().ok()).unwrap_or(0);
            let text_of = |mid: u64| thread.with_untracked(|l| l.iter().find(|m| m.id.get() == mid).map(|m| m.body.clone())).unwrap_or_default();
            match btn.get_attribute("data-act").as_deref() {
                Some("copy-code") => {
                    let code = btn.closest(".cblock").ok().flatten().and_then(|b| b.query_selector("pre").ok().flatten()).and_then(|p| p.text_content()).unwrap_or_default();
                    copy_text(&code);
                }
                Some("copy") => copy_text(&text_of(mid)),
                Some("quote") => body.update(|b| b.push_str(&turns::quoted(&text_of(mid)))),
                Some("link") => {
                    let w = dom::window();
                    let base = format!("{}{}", w.location().origin().unwrap_or_default(), w.location().pathname().unwrap_or_default());
                    let link = match &t {
                        Some(t) => turns::turn_link(t, id, mid),
                        None => format!("#/chats/{id}?at={mid}"),
                    };
                    copy_text(&format!("{base}{link}"));
                }
                _ => return,
            }
            ev.prevent_default();
        }
    };
    let foot = {
        let name = name.clone();
        let me = me.clone();
        move || {
            if !full {
                return None;
            }
            if alone.get() {
                return Some("only you can read this chat until you add someone".to_string());
            }
            let agent = thread.with(|l| l.iter().rev().find(|m| m.author != me && m.via.is_some()).map(|m| (m.author.clone(), m.via.clone().unwrap_or_default())));
            agent.map(|(a, via)| format!("{} is an agent held by {}; its messages carry via", name(&a), short(&via)))
        }
    };
    let is_open = Memo::new(move |_| chat.with(|c| c.as_ref().is_some_and(|c| c.state == ChatState::Open)));
    let found = Memo::new(move |_| chat.with(Option::is_some));
    let participants = Signal::derive(move || chat.with(|c| c.as_ref().map(|c| c.participants.clone()).unwrap_or_default()));
    let held_some = Memo::new(move |_| held.with(Option::is_some));
    let t_picker = ticket.clone();
    let t_comp = ticket.clone();
    view! {
        <div class="chatview" class:full=full>
            {move || (held_some.get() && !found.get()).then(|| view! { <p class="pempty">"This chat is not one you are in."</p> })}
            {(!full).then(|| who.clone())}
            {move || revoking.get().map(|p| view! {
                <crate::chat_grant::RevokeDialog chat=id principal=p
                    done=Callback::new(move |r: Result<(), String>| {
                        revoking.set(None);
                        if let Err(e) = r {
                            leptos::logging::log!("chats.remove: {e}");
                        }
                    })
                    cancel=Callback::new(move |_| revoking.set(None)) />
            })}
            {move || adding.get().then(|| view! {
                <Picker ticket=t_picker.clone() add_to=Some((id, participants.get_untracked())) on_done=Callback::new(move |_| adding.set(false)) />
            })}
            <div class="pmsgs" class:turns=true node_ref=list_ref aria-live="polite" on:scroll=on_scroll on:click=acts>
                <div class="tcol">
                    {move || loading_earlier.get().then(|| view! { <div class="tearlier" role="status">"loading earlier…"</div> })}
                    {empty}
                    <div class="tlist" node_ref=rows_ref data-first=move || thread.with(|t| t.first().map_or(0, |m| m.id.get())).to_string() data-held=move || thread.with(Vec::len).to_string()>
                        <div class="tspace" style=move || format!("height:{:.0}px", space_above.get())></div>
                        <For each=move || mounted.get() key=|m| m.id.get() children=rows />
                        <div class="tspace" style=move || format!("height:{:.0}px", space_below.get())></div>
                    </div>
                    {live}
                    {waits}
                </div>
            </div>
            {move || arrow.get().map(|a| view! { <button type="button" class="tjump" on:click=jump>{a}</button> })}
            {move || if is_open.get() {
                view! { <crate::composer::Composer props=crate::composer::Props { id, ticket: t_comp.clone(), body, full } /> }.into_any()
            } else if found.get() {
                view! { <div class="pshut">"closed — read-only"<button type="button" class="preopen" on:click=move |_| spawn_local(async move { let _ = chats::settle_state(id, true).await; })>"reopen"</button></div> }.into_any()
            } else {
                ().into_any()
            }}
            {move || foot().map(|f| view! { <p class="sfoot">{f}</p> })}
        </div>
    }
}

/// CHAT-18: a live turn under the last — `builder is on it`, a pulsing dot,
/// its steps in a box that shows the newest eight and scrolls to the rest;
/// its pulse (`working · 42s`), or, past a minute with no word, stale.
fn live_view(mut w: live::Work, name: &impl Fn(&str) -> String, now: RwSignal<i64>) -> impl IntoView {
    let who = name(&w.by);
    let steps = std::mem::take(&mut w.steps);
    let n = steps.len();
    let more = live::earlier(n);
    let beat = w.beat.clone();
    let quiet = Memo::new(move |_| live::stale(&w, now.get()));
    let steps = steps.into_iter().map(|l| view! { <li>{l}</li> }).collect_view();
    view! {
        <div class="tlive" class:stale=move || quiet.with(Option::is_some) role="status" aria-live="polite">
            <div class="tlhead">
                <span class="dot" class:wait=move || quiet.with(Option::is_none)></span>
                <b>{live::on_it(&who)}</b>
                <span class="tlbeat">{move || quiet.get().or_else(|| beat.clone()).unwrap_or_default()}</span>
            </div>
            {(more > 0).then(|| view! { <div class="tlmore">{format!("+{more} earlier — scroll up")}</div> })}
            {(n > 0).then(|| view! { <ol class="tlsteps">{steps}</ol> })}
        </div>
    }
}

/// CHAT-18: a finished turn's work, folded after its answer — **show work
/// (N steps)**, every step inside.
fn fold_view(w: live::Work) -> impl IntoView {
    let label = live::fold_label(w.steps.len());
    let steps = w.steps.into_iter().map(|l| view! { <li>{l}</li> }).collect_view();
    view! {
        <details class="tfold">
            <summary>{label}</summary>
            <ol class="tlsteps">{steps}</ol>
        </details>
    }
}

fn copy_text(text: &str) {
    let _ = dom::window().navigator().clipboard().write_text(text);
}

/// CHAT-6 / CHAT-7's view 5: an empty chat of one, as an empty agent chat —
/// *Just you, on KILN-7*, and four ways to begin, each into the composer.
fn alone_view(ticket: &str, body: RwSignal<String>) -> impl IntoView {
    let picks: [(&str, String, &str); 4] = [
        ("@builder", "@builder ".to_string(), "ask the agent that holds this ticket"),
        ("/add reviewer", "/add reviewer".to_string(), "a second look from another agent"),
        ("/ticket CODE", "/ticket ".to_string(), "put a ticket card in a note"),
        ("just write", String::new(), "it's kept here, read-only once closed"),
    ];
    let cards = picks
        .into_iter()
        .map(|(k, put, what)| {
            view! {
                <button type="button" class="asug" on:click=move |_| {
                    body.set(put.clone());
                    if let Some(b) = dom::window().document().and_then(|d| d.query_selector(".ccompose .cbox").ok().flatten()).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
                        let _ = b.focus();
                    }
                }>
                    <small>{k}</small><span>{what}</span>
                </button>
            }
        })
        .collect_view();
    view! {
        <div class="alone">
            <span class="amark" aria-hidden="true">"✎"</span>
            <h3>{format!("Just you, on {ticket}")}</h3>
            <p>"A chat with no one in it yet. Think out loud, keep notes beside the ticket, or bring someone in with @ or /add."</p>
            <div class="asugs">{cards}</div>
        </div>
    }
}

/// One turn (CHAT-7): yours a bubble on the right, the time under it;
/// theirs full-width prose under a mono line — a mark, the name, an agent
/// tag, `via`, the time. Each shows copy · quote · link on hover.
fn turn_view(m: Message, mine: bool, fresh: bool, name: &impl Fn(&str) -> String) -> impl IntoView {
    let who = m.name.clone().unwrap_or_else(|| name(&m.author));
    let agent = chats::is_agent(&m.author) || m.via.is_some();
    // A ticket card follows its row: drawn once the app holds the ticket,
    // its status live.
    // CHAT-8: past 200 lines, the head and *show all N lines*, the rest
    // rendered a chunk of blocks a task (src/scale.rs's split_long).
    let (head, rest, lines) = {
        let (h, r, n) = scale::split_long(&m.body);
        (h.to_string(), r.into_iter().map(str::to_string).collect::<Vec<_>>(), n)
    };
    let html = {
        let head = head.clone();
        Memo::new(move |_| {
            let card = |code: &str| crate::cache::find(code).map(|r| r.with(|t| (t.title.clone(), t.status.to_string())));
            turns::source_html(&head, &card)
        })
    };
    let chunks = RwSignal::new(Vec::<String>::new());
    let showing = RwSignal::new(false);
    let rest = StoredValue::new(rest);
    let show_all = move |_| {
        if showing.get_untracked() {
            return;
        }
        showing.set(true);
        spawn_local(async move {
            let parts = rest.try_get_value().unwrap_or_default();
            for part in parts {
                let card = |code: &str| crate::cache::get(code).map(|r| r.with_untracked(|t| (t.title.clone(), t.status.to_string())));
                let h = turns::source_html(&part, &card);
                if chunks.try_update(|c| c.push(h)).is_none() {
                    return;
                }
                // One chunk a task: the page answers between them.
                dom::sleep_ms(0).await;
            }
            showing.try_set(false);
        });
    };
    let more = move || {
        let n = rest.with_value(Vec::len);
        let done = chunks.with(Vec::len) >= n;
        (n > 0 && !done).then(|| {
            view! {
                <button type="button" class="tmore" disabled=move || showing.get() on:click=show_all>
                    {move || if showing.get() { format!("showing all {lines} lines…") } else { format!("show all {lines} lines") }}
                </button>
            }
        })
    };
    let tail = move || {
        view! {
            <For each=move || 0..chunks.with(Vec::len) key=|i| *i children=move |i| {
                let h = chunks.with_untracked(|c| c.get(i).cloned().unwrap_or_default());
                let node = NodeRef::<leptos::html::Div>::new();
                node.on_load(|el: web_sys::HtmlDivElement| {
                    crate::highlight::mount(&el);
                    crate::media::hydrate(&el);
                });
                view! { <div class="md tchunk" data-links="chat" node_ref=node inner_html=h></div> }
            } />
        }
    };
    let node = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        html.track();
        if let Some(el) = node.get() {
            crate::highlight::mount(&el);
            // CHAT-14: a message's images (COMMENT-5's), asked for as the
            // row mounts — the window mounts only what is near the view, so
            // an image far off is never fetched.
            crate::media::hydrate(&el);
        }
    });
    let acts = view! {
        <div class="tacts">
            <button type="button" data-act="copy">"copy"</button>
            <button type="button" data-act="quote">"quote"</button>
            <button type="button" data-act="link">"link"</button>
        </div>
    };
    let mid = m.id.get().to_string();
    if mine {
        view! {
            <div class="turn mine pmsg" class:fresh=fresh data-message-id=mid>
                <div class="bubble">
                    <div class="md" data-links="chat" node_ref=node inner_html=move || html.get()></div>
                    {tail}
                    {more}
                </div>
                <div class="tmeta"><span title=fmt_unix(m.at)>{format!("{who} · {}", clock(m.at))}</span>{acts}</div>
            </div>
        }
        .into_any()
    } else {
        view! {
            <div class="turn theirs pmsg" class:fresh=fresh data-message-id=mid>
                <span class="pav" class:agent=agent aria-hidden="true">{crate::conversation::initials(&who)}</span>
                <div class="pmw">
                    <div class="pmh">
                        <b>{who}</b>
                        {agent.then(|| view! { <span class="ag">"agent"</span> })}
                        {m.via.clone().map(|v| view! { <span class="via" title=v.clone()>{format!("via {}", short(&v))}</span> })}
                        <span class="t" title=fmt_unix(m.at)>{clock(m.at)}</span>
                    </div>
                    <div class="pmb">
                        <div class="md" data-links="chat" node_ref=node inner_html=move || html.get()></div>
                        {tail}
                        {more}
                    </div>
                    {acts}
                </div>
            </div>
        }
        .into_any()
    }
}


/// The picker: **+ chat** (`add_to` none) — people and agents, who can read
/// the ticket, the same people's chat opened instead; or *add* to a chat
/// (`add_to`: it and who is in it) — one picked is added. CHAT-14: with no
/// ticket, **New message** — one picked is a direct message, several a
/// group (with a title); everyone holding a grant in the organization,
/// agents by name; `on_open` goes to the chat opened.
#[component]
pub(crate) fn Picker(ticket: Option<String>, add_to: Option<(u64, Vec<String>)>, on_done: Callback<()>, #[prop(optional)] on_open: Option<Callback<u64>>) -> impl IntoView {
    let cands = RwSignal::new(None::<Result<Vec<(String, bool)>, String>>);
    {
        let t = ticket.clone();
        spawn_local(async move {
            let r = chats::candidates(t.as_deref()).await;
            cands.try_set(Some(r));
        });
    }
    let query = RwSignal::new(String::new());
    let picked = RwSignal::new(Vec::<String>::new());
    let title = RwSignal::new(String::new());
    // CHAT-16: an admin adds anyone — one who can't read it after a dialog.
    let admin = crate::signin::account().with_untracked(|a| a.is_admin());
    let asking = RwSignal::new(None::<crate::chat_grant::Ask>);
    let error = RwSignal::new(None::<String>);
    let held = chats::signal();
    let me = chats::me();
    let name = names();
    let outside = ticket.is_none();
    let (chat_id, already) = match add_to {
        Some((c, ps)) => (Some(c), ps),
        None => (None, Vec::new()),
    };
    let waiting = {
        let t = ticket.clone();
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().zip(t.as_deref()).map(|(h, t)| h.waiting_on(t)).unwrap_or_default()))
    };
    let existing = {
        let (t, me) = (ticket.clone(), me.clone());
        Memo::new(move |_| {
            let p = picked.get();
            if chat_id.is_some() {
                return None;
            }
            let t = t.as_deref()?;
            held.with(|h| h.as_ref().and_then(|h| h.existing(t, &me, &p)))
        })
    };
    let where_ = ticket.clone().unwrap_or_else(|| "the organization".to_string());
    let list = {
        let (where_, name, me) = (where_.clone(), name.clone(), me.clone());
        move || match cands.get() {
            None => view! { <p class="pempty">{if outside { "asking who is here…" } else { "asking who can read it…" }}</p> }.into_any(),
            Some(Err(e)) => view! { <p class="perr" role="alert">{e}</p> }.into_any(),
            Some(Ok(all)) => {
                let shown = rows(&all, &me, &already, &query.get(), &name);
                // CHAT-6: the picker's first row, "just me" — nobody picked is a chat of one (on a ticket).
                let alone = (chat_id.is_none() && !outside).then(|| {
                    let who = name(&me);
                    view! {
                        <ul class="pcands pjust">
                            <li class="pcand pme" class:hi=move || picked.with(Vec::is_empty) data-principal="" on:click=move |_| picked.set(Vec::new())>
                                <span class="pav" aria-hidden="true">{crate::conversation::initials(&who)}</span>
                                <div>"Just me"<small>"a chat of one — add people and agents later"</small></div>
                                <span class="dot"></span>
                            </li>
                        </ul>
                    }
                });
                if shown.is_empty() {
                    return view! { {alone}<p class="pempty">"Nobody else holds a grant here."</p> }.into_any();
                }
                let waiting = waiting.get();
                let items = shown
                    .into_iter()
                    .map(|(p, reads)| {
                        let on = picked.with(|v| v.contains(&p));
                        let grantable = !reads && admin && chat_id.is_some();
                        let what = if grantable {
                            "can't read it — adding grants access (you confirm)".to_string()
                        } else if !reads {
                            refusal(&where_)
                        } else if waiting.contains(&p) {
                            "waiting for a message".to_string()
                        } else if outside {
                            short(&p)
                        } else {
                            format!("can read {where_}")
                        };
                        let pc = p.clone();
                        let click = move |_| {
                            if !reads && !grantable {
                                return;
                            }
                            match chat_id {
                                Some(c) => {
                                    let p = pc.clone();
                                    spawn_local(async move {
                                        match chats::add_granting(c, &p, None).await {
                                            Ok(chats::Added::Added(_)) => on_done.run(()),
                                            Ok(chats::Added::NeedsGrant(ask)) => {
                                                asking.try_set(Some(ask));
                                            }
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
                            <li class="pcand" class:hi=on class:no=!reads && !grantable class:grant=grantable aria-disabled=(!reads && !grantable).to_string() data-principal=p.clone() on:click=click>
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
                view! { {alone}<ul class="pcands">{items}</ul> }.into_any()
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
            let others = picked.get_untracked();
            if let Start::Open(c) = start_of(&others, existing.get_untracked()) {
                show(View::Chat(c));
                return;
            }
            if outside && others.is_empty() {
                error.set(Some("pick someone: one for a direct message, several for a group".into()));
                return;
            }
            let t = t.clone();
            let named = title.get_untracked();
            spawn_local(async move {
                let title = (others.len() > 1).then_some(named.as_str());
                match chats::open(t.as_deref(), &others, title).await {
                    Ok(c) => match on_open {
                        Some(go) => go.run(c),
                        None => show(View::Chat(c)),
                    },
                    Err(e) => {
                        error.try_set(Some(e));
                    }
                }
            });
        }
    };
    let heading = match (chat_id, ticket.as_deref()) {
        (Some(_), _) => "Add someone to this chat".to_string(),
        (None, Some(t)) => format!("Chat about {t}"),
        (None, None) => "New message".to_string(),
    };
    let adding = chat_id.is_some();
    let hint = move || match (outside, picked.with(Vec::len)) {
        (true, 0) => "one person for a direct message, several for a group".to_string(),
        (true, 1) => "a direct message — the one you have with them, if you do".to_string(),
        (true, _) => "a group: give it a title".to_string(),
        (false, _) => start_of(&picked.get(), existing.get()).hint().to_string(),
    };
    let label = move || match (outside, picked.with(Vec::len)) {
        (true, n) if n > 1 => "start group",
        (true, _) => "message",
        (false, _) => start_of(&picked.get(), existing.get()).label(),
    };
    view! {
        <div class="ppicker" class:adding=adding class:outside=outside>
            <h4>{heading}</h4>
            <div class="pq">
                {tokens}
                <input type="text" class="pqi" aria-label="who to talk to" placeholder="who… (a name, or an agent: builder)" autocomplete="off" spellcheck="false"
                    prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev)) />
            </div>
            {move || (outside && !adding && picked.with(|p| p.len() > 1)).then(|| view! {
                <input type="text" class="ptitle" aria-label="the group's title" placeholder="the group's title" maxlength="200"
                    prop:value=move || title.get() on:input=move |ev| title.set(event_target_value(&ev)) />
            })}
            {list}
            {move || error.get().map(|e| view! { <p class="perr" role="alert">{e}</p> })}
            {(!adding).then(|| view! {
                <div class="prow">
                    <span>{hint}</span>
                    <button type="button" class="psend" on:click=start>{label}</button>
                </div>
            })}
            {adding.then(|| view! { <div class="prow"><button type="button" class="pcancel" on:click=move |_| on_done.run(())>"cancel"</button></div> })}
            {move || asking.get().map(|ask| view! {
                <crate::chat_grant::GrantDialog ask
                    done=Callback::new(move |r: Result<Option<String>, String>| {
                        asking.set(None);
                        match r {
                            Ok(_) => on_done.run(()),
                            Err(e) => error.set(Some(e)),
                        }
                    })
                    cancel=Callback::new(move |_| asking.set(None)) />
            })}
        </div>
    }
}

/// What the picker's start button does (CHAT-6): open the chat the same
/// people have, or start one — with nobody picked, a chat of one. It is
/// never disabled: nobody picked is "just me".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Start {
    /// The open chat these people (or you alone) already have.
    Open(u64),
    /// A new chat; `alone`: nobody picked, a chat of one.
    New { alone: bool },
}

pub(crate) fn start_of(picked: &[String], existing: Option<u64>) -> Start {
    match existing {
        Some(c) => Start::Open(c),
        None => Start::New { alone: picked.is_empty() },
    }
}

impl Start {
    fn label(self) -> &'static str {
        match self {
            Start::Open(_) => "open",
            Start::New { .. } => "start",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Start::Open(_) => "a chat with them is open already: it opens instead",
            Start::New { alone: true } => "just you: a chat of one, to add people and agents to later",
            Start::New { alone: false } => "people and agents who can read the ticket",
        }
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
    use crate::bindings::{pick_single, table_with, Context, Press};
    use crate::chats::tests::{chat, msg};
    use menger_tickets_api::ChatsEvent;

    /// CHAT-6: the picker's start button with nobody picked starts a chat
    /// of one (never disabled); your open chat of one opens instead; with
    /// someone picked it is the chat with them, as before.
    #[test]
    fn start_with_nobody_picked_is_a_chat_of_one() {
        assert_eq!(start_of(&[], None), Start::New { alone: true });
        assert_eq!(start_of(&[], None).label(), "start");
        assert!(start_of(&[], None).hint().starts_with("just you"));
        assert_eq!(start_of(&[], Some(7)), Start::Open(7), "your chat of one, already open");
        assert_eq!(start_of(&["builder".into()], None), Start::New { alone: false });
        assert_eq!(start_of(&["builder".into()], Some(41)).label(), "open");
        // The held chats answer the empty set with the chat of one.
        let h = crate::chats::Held::from_events([ChatsEvent::Chat { chat: chat(7, "KILN-7", &["ada"]) }, ChatsEvent::Chat { chat: chat(41, "KILN-7", &["ada", "builder"]) }]);
        assert_eq!(h.existing("KILN-7", "ada", &[]), Some(7));
        assert_eq!(h.existing("KILN-7", "rui", &[]), None);
    }

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

    /// CHAT-7: the composer's keys are its own context's (src/composer.rs
    /// holds their rows); a comment box's ⌘↵ and a field's Enter are as
    /// they were.
    #[test]
    fn the_composer_has_its_own_keys() {
        let t = table_with(&[]);
        let ctrl_enter = Press { key: "Enter".into(), ctrl: true, ..Press::default() };
        assert_eq!(pick_single(&t, &ctrl_enter, Context::Field, false), Some("comment.save"));
        assert_eq!(pick_single(&t, &Press::key("Enter"), Context::Field, false), Some("form.submit"));
        assert_eq!(pick_single(&t, &Press::key("Enter"), Context::Composer, false), Some("chat.send"));
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
}
