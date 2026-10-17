//! CHAT-7: a chat full screen, as a traditional agent chat — the dock's
//! conversation (CHAT-10) expanded, not a second home for chats (the
//! coordinator's reading, 2026-10-17: ⤢ on the dock's chat head expands it).
//!
//! - **Opening**: ⤢ on the dock's chat head, *full ↗* in its composer,
//!   `g c` with a chat open, or `#/ticket/X?chat=ID&full` — which a reload
//!   keeps ([`full_in`], [`with_full`]). **Esc** (outside the composer) or
//!   **← X** goes back to the ticket with the chat in its dock.
//! - **The rail** (264 px; at ≤ 760 a drawer behind ☰): **＋ New chat on X**
//!   (`c`), this ticket's chats — title, last line, unread, the closed ones
//!   dimmed and kept — then **Other chats with unread** across tickets, each
//!   with its ticket's chip ([`rail`]). A row opens that chat here.
//! - **The head**: ← the ticket's code, the chat's title, who is in it with
//!   presence dots (agents tagged; *just you* alone), **＋ add**, **ticket ⟩**
//!   and **⋯** (rename, close, copy link).
//! - **The column**: [`crate::chat_pane::ChatView`] at 780 px, centred —
//!   the same turns, composer and scrolling as the dock.
//! - **The ticket beside it**: **ticket ⟩** (or dragging the divider) brings
//!   the ticket back as a right column, 440 px unless dragged; whether it is
//!   open and how wide is kept per browser ([`Split`], manifest's
//!   `sections.ts`).
//! - At 390 it is the whole phone: no rail, the head down to dots, the
//!   composer above the keyboard.
//!
//! What decides is pure and pinned by native tests; the rest is the DOM.

use leptos::prelude::*;
use menger_tickets_api::{Chat, ChatState};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::chat_pane::{self, Shown, View};
use crate::chats::{self, Held};
use crate::dom;

// -- the route ----------------------------------------------------------------

/// Whether the route's query asks for the chat full screen (`full`, or
/// `full=1`).
pub(crate) fn full_in(hash: &str) -> bool {
    hash.split_once('?').is_some_and(|(_, q)| q.split('&').any(|p| p == "full" || p == "full=1"))
}

/// The hash with `full` in its query (`on`) or out of it, the rest kept.
pub(crate) fn with_full(hash: &str, on: bool) -> String {
    let (path, query) = hash.split_once('?').unwrap_or((hash, ""));
    let mut params: Vec<&str> = query.split('&').filter(|p| !p.is_empty() && *p != "full" && *p != "full=1").collect();
    if on {
        params.push("full");
    }
    if params.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", params.join("&"))
    }
}

// -- the rail -----------------------------------------------------------------

/// One row of the rail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RailRow {
    pub(crate) id: u64,
    pub(crate) ticket: String,
    pub(crate) label: String,
    pub(crate) last: String,
    pub(crate) unread: u32,
    pub(crate) closed: bool,
}

/// The rail: this ticket's chats (open ones by last activity, then the
/// closed, kept), and the reader's other chats with unread, across tickets.
pub(crate) fn rail(held: &Held, ticket: &str, me: &str, name: &impl Fn(&str) -> String) -> (Vec<RailRow>, Vec<RailRow>) {
    let row = |c: &Chat| RailRow {
        id: c.id.get(),
        ticket: c.ticket.clone(),
        label: c.title.clone().unwrap_or_else(|| chat_pane::tab_label(c, me, name)),
        last: last_line(held, c, me, name),
        unread: c.unread,
        closed: c.state == ChatState::Closed,
    };
    let by_last = |v: &mut Vec<Chat>| v.sort_by(|a, b| b.last_at.cmp(&a.last_at).then(b.id.get().cmp(&a.id.get())));
    let mut open = held.on(ticket, ChatState::Open);
    let mut closed = held.on(ticket, ChatState::Closed);
    by_last(&mut open);
    by_last(&mut closed);
    let here = open.iter().chain(closed.iter()).map(row).collect();
    let mut others: Vec<Chat> = held.with_unread().into_iter().filter(|c| !c.ticket.eq_ignore_ascii_case(ticket)).cloned().collect();
    by_last(&mut others);
    (here, others.iter().map(row).collect())
}

fn last_line(held: &Held, c: &Chat, me: &str, name: &impl Fn(&str) -> String) -> String {
    if c.state == ChatState::Closed {
        return "kept, read-only".to_string();
    }
    if let Some(m) = held.thread(c.id.get()).and_then(|t| t.list.last()) {
        let line = m.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        let who = if m.author == me { "you".to_string() } else { name(&m.author) };
        return format!("{who}: {line}");
    }
    if c.participants.len() <= 1 {
        return "just you".to_string();
    }
    if c.last_message.is_none() {
        return "no messages yet".to_string();
    }
    String::new()
}

// -- the split ----------------------------------------------------------------

/// Where the ticket beside the chat is kept, per browser.
pub(crate) const SPLIT_KEY: &str = "menger.chat.split";
/// The ticket column's width unless dragged.
pub(crate) const ASIDE_PX: f64 = 440.0;
const ASIDE_MIN: f64 = 320.0;
/// What the chat keeps when the ticket is widest.
const CHAT_MIN: f64 = 520.0;

/// The ticket beside the chat: open or not, and its width.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Split {
    pub(crate) open: bool,
    pub(crate) width: f64,
}

impl Default for Split {
    fn default() -> Self {
        Split { open: false, width: ASIDE_PX }
    }
}

impl Split {
    /// What `raw` (the stored string) says; anything unreadable is the default.
    pub(crate) fn decode(raw: Option<&str>) -> Self {
        raw.and_then(|r| serde_json::from_str::<Split>(r).ok()).filter(|s| s.width.is_finite()).unwrap_or_default()
    }
    pub(crate) fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    /// The width dragged to, kept between [`ASIDE_MIN`] and what leaves the
    /// chat its room in a view `vw` wide (less the rail, `rail`).
    pub(crate) fn clamp(width: f64, vw: f64, rail: f64) -> f64 {
        let max = (vw - rail - CHAT_MIN).max(ASIDE_MIN);
        width.clamp(ASIDE_MIN, max)
    }
}

fn load_split() -> Split {
    use crate::composer::Kv;
    Split::decode(crate::composer::Local.get(SPLIT_KEY).as_deref())
}

fn keep_split(s: Split) {
    use crate::composer::Kv;
    crate::composer::Local.set(SPLIT_KEY, &s.encode());
}

// -- the state ----------------------------------------------------------------

thread_local! {
    static FULL: ArcRwSignal<bool> = ArcRwSignal::new(false);
}

/// Whether the chat is full screen.
pub(crate) fn signal() -> ArcRwSignal<bool> {
    FULL.with(Clone::clone)
}

/// The keys stand down while the session is up (src/keys.rs).
pub(crate) fn is_up() -> bool {
    signal().get_untracked()
}

/// Full screen on or off — the URL follows (no history entry).
pub(crate) fn set_full(on: bool) {
    let s = signal();
    if s.get_untracked() != on {
        s.set(on);
    }
    keep_url(on);
}

fn keep_url(on: bool) {
    let w = dom::window();
    let hash = w.location().hash().unwrap_or_default();
    if !hash.starts_with("#/ticket/") || full_in(&hash) == on {
        return;
    }
    if let Ok(h) = w.history() {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&with_full(&hash, on)));
    }
}

/// `g c`: the chat open on this ticket, full screen — whether one was open.
pub(crate) fn expand() -> bool {
    let open = chat_pane::shown().with_untracked(|s| matches!(s.as_ref().map(|s| &s.view), Some(View::Chat(_))));
    if open {
        set_full(true);
    }
    open
}

// -- the views ----------------------------------------------------------------

/// The session, mounted once in the app: drawn while the chat is full
/// screen on the route's ticket.
#[component]
pub(crate) fn Session(ticket: Signal<Option<String>>) -> impl IntoView {
    let full = signal();
    let shown = chat_pane::shown();
    // The route: `?chat=ID&full` opens it full; without `full`, not.
    let from_url = {
        let full = full.clone();
        move || {
            let hash = dom::window().location().hash().unwrap_or_default();
            let on = full_in(&hash) && chat_pane::chat_in(&hash).is_some() && hash.starts_with("#/ticket/");
            if full.get_untracked() != on {
                full.set(on);
            }
        }
    };
    {
        let from_url = from_url.clone();
        Effect::new(move |_| {
            ticket.track();
            from_url();
        });
    }
    let _listen = window_event_listener(leptos::ev::hashchange, move |_| from_url());
    // No chat shown: no session.
    {
        let (full, shown) = (full.clone(), shown.clone());
        Effect::new(move |_| {
            if full.get() && shown.with(Option::is_none) {
                set_full(false);
            }
        });
    }
    move || {
        if !full.get() {
            return None;
        }
        let s = shown.get()?;
        Some(view! { <SessionView s /> })
    }
}

#[component]
fn SessionView(s: Shown) -> impl IntoView {
    let held = chats::signal();
    let me = chats::me();
    let name = chat_pane::names();
    let ticket = s.ticket.clone();
    let current = s.view.clone();
    let drawer = RwSignal::new(false);
    let split = RwSignal::new(load_split());
    let adding = RwSignal::new(false);
    let more = RwSignal::new(false);
    let renaming = RwSignal::new(None::<String>);
    let problem = RwSignal::new(None::<String>);
    let root = NodeRef::<leptos::html::Div>::new();
    root.on_load(|el| {
        // The composer takes focus when it mounts; else the session has it.
        spawn_local(async move {
            dom::sleep_ms(0).await;
            let in_box = dom::window().document().and_then(|d| d.active_element()).is_some_and(|a| a.tag_name() == "TEXTAREA");
            if !in_box {
                let _ = el.focus();
            }
        });
    });
    let chat_id = match current {
        View::Chat(id) => Some(id),
        _ => None,
    };
    let chat = {
        let held = held.clone();
        Memo::new(move |_| chat_id.and_then(|id| held.with(|h| h.as_ref().and_then(|h| h.get(id).cloned()))))
    };
    let keys = move |ev: web_sys::KeyboardEvent| {
        let typing = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).is_some_and(|t| matches!(t.tag_name().as_str(), "INPUT" | "TEXTAREA"));
        if typing {
            return;
        }
        match bindings::hit(&ev, Context::Session) {
            Some("session.new") => {
                ev.prevent_default();
                chat_pane::show(View::Pick);
            }
            Some("session.leave") => {
                ev.prevent_default();
                ev.stop_propagation();
                if more.get_untracked() || adding.get_untracked() || drawer.get_untracked() {
                    more.set(false);
                    adding.set(false);
                    drawer.set(false);
                } else {
                    set_full(false);
                }
            }
            _ => {}
        }
    };
    let rail_view = {
        let (t, me, name, held) = (ticket.clone(), me.clone(), name.clone(), held.clone());
        move || {
            let (here, others) = held.with(|h| h.as_ref().map(|h| rail(h, &t, &me, &name)).unwrap_or_default());
            let row = move |r: RailRow, other: bool| {
                let on = chat_id == Some(r.id);
                let (id, tk) = (r.id, r.ticket.clone());
                let open = move |_| {
                    drawer.set(false);
                    if other {
                        let _ = dom::window().location().set_hash(&format!("/ticket/{tk}?chat={id}&full"));
                    } else {
                        chat_pane::show(View::Chat(id));
                    }
                };
                view! {
                    <button type="button" class="srow" class:on=on class:closed=r.closed data-chat=r.id.to_string() on:click=open>
                        <span class="sl">
                            {other.then(|| view! { <span class="tchip">{r.ticket.clone()}</span> })}
                            {r.label.clone()}
                        </span>
                        {(r.unread > 0).then(|| view! { <span class="count">{r.unread}</span> })}
                        <small>{r.last.clone()}</small>
                    </button>
                }
            };
            let others_view = (!others.is_empty()).then(|| {
                let rows = others.into_iter().map(|r| row(r, true)).collect_view();
                view! { <h4 class="sh">"Other chats with unread"</h4>{rows} }
            });
            view! {
                {here.into_iter().map(|r| row(r, false)).collect_view()}
                {others_view}
            }
        }
    };
    let t_head = ticket.clone();
    let title = {
        let (me, name) = (me.clone(), name.clone());
        move || chat.with(|c| c.as_ref().map(|c| c.title.clone().unwrap_or_else(|| chat_pane::tab_label(c, &me, &name)))).unwrap_or_else(|| if current_is_pick(chat_id) { "a new chat".into() } else { "chats".into() })
    };
    let people = {
        let (me, name) = (me.clone(), name.clone());
        move || {
            let Some(c) = chat.get() else { return ().into_any() };
            let alone = c.participants.len() <= 1;
            let dots = c
                .participants
                .iter()
                .map(|p| {
                    let waiting = c.waiting.contains(p);
                    view! {
                        <span class="sp" title=if waiting { format!("{} — waiting for your next message", name(p)) } else { name(p) }>
                            <span class="dot" class:on=true class:wait=waiting></span>
                            <span class="spn">{if *p == me { name(p) } else { name(p) }}</span>
                            {chats::is_agent(p).then(|| view! { <span class="ag">"agent"</span> })}
                        </span>
                    }
                })
                .collect_view();
            view! { {dots}{alone.then(|| view! { <span class="ag alone">"just you"</span> })} }.into_any()
        }
    };
    let copy_link = {
        let t = ticket.clone();
        move |_| {
            more.set(false);
            let Some(id) = chat_id else { return };
            let w = dom::window();
            let url = format!("{}{}#/ticket/{t}?chat={id}&full", w.location().origin().unwrap_or_default(), w.location().pathname().unwrap_or_default());
            let _ = w.navigator().clipboard().write_text(&url);
        }
    };
    let close_chat = move |_| {
        more.set(false);
        let Some(id) = chat_id else { return };
        spawn_local(async move {
            if let Err(e) = chats::settle_state(id, false).await {
                problem.try_set(Some(e));
            }
        });
    };
    let save_title = move || {
        let (Some(id), Some(t)) = (chat_id, renaming.get_untracked()) else { return };
        renaming.set(None);
        spawn_local(async move {
            if let Err(e) = chats::rename(id, Some(&t)).await {
                problem.try_set(Some(e));
            }
        });
    };
    let main = {
        let t = ticket.clone();
        match current.clone() {
            View::Chat(id) => view! { <chat_pane::ChatView ticket=t id full=true /> }.into_any(),
            View::Pick => view! { <div class="spick"><chat_pane::Picker ticket=t add_to=None on_done=Callback::new(|_| ()) /></div> }.into_any(),
            View::Closed | View::Auto => view! { <p class="pempty">"Pick a chat in the rail, or start one."</p> }.into_any(),
        }
    };
    let t_add = ticket.clone();
    let aside = {
        let t = ticket.clone();
        move || split.get().open.then(|| view! { <TicketBeside ticket=t.clone() split /> })
    };
    let ticket_btn = move |_| {
        split.update(|s| s.open = !s.open);
        keep_split(split.get_untracked());
    };
    let back = move |_| set_full(false);
    let t_new = ticket.clone();
    view! {
        <div id="chatfull" role="dialog" aria-modal="true" aria-label=format!("the chat on {ticket}, full screen") tabindex="-1"
            node_ref=root on:keydown=keys class:drawer=move || drawer.get() class:aside=move || split.get().open
            style=move || format!("--aside-w:{}px", split.get().width.round())>
            <aside class="srail" aria-label="chats">
                <div class="srhead">
                    <span class="brand">"menger"</span>
                    <button type="button" class="kbd" title="back to the ticket (Esc)" on:click=back>"esc"</button>
                </div>
                <button type="button" class="snew" class:on=current_is_pick(chat_id) on:click=move |_| { drawer.set(false); chat_pane::show(View::Pick); }>
                    {format!("＋ New chat on {t_new}")}<span class="kbd">"c"</span>
                </button>
                <h4 class="sh">"on "<span class="tchip">{ticket.clone()}</span></h4>
                <div class="srows">{rail_view}</div>
            </aside>
            <section class="smain">
                <header class="shead">
                    <button type="button" class="smenu" aria-label="the chats" title="the chats" on:click=move |_| drawer.update(|d| *d = !*d)>"☰"</button>
                    <button type="button" class="sback" title="back to the ticket, the chat in its dock (Esc)" on:click=back>{format!("← {t_head}")}</button>
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
                    <div class="speople">{people}</div>
                    {chat_id.map(|_| view! { <button type="button" class="sbtn" on:click=move |_| adding.update(|a| *a = !*a)>"＋ add"</button> })}
                    <button type="button" class="sbtn" aria-pressed=move || split.get().open.to_string() on:click=ticket_btn>
                        {move || if split.get().open { "‹ hide ticket" } else { "ticket ⟩" }}
                    </button>
                    {chat_id.map(|_| view! { <button type="button" class="sbtn smore" aria-label="more" aria-expanded=move || more.get().to_string() on:click=move |_| more.update(|m| *m = !*m)>"⋯"</button> })}
                    {move || { let (close_chat, copy_link) = (close_chat.clone(), copy_link.clone()); more.get().then(|| view! {
                        <div class="smenu-pop" role="menu">
                            <button type="button" role="menuitem" on:click=move |_| { more.set(false); renaming.set(Some(title_now(chat))); }>"rename"</button>
                            <button type="button" role="menuitem" on:click=close_chat>"close chat"</button>
                            <button type="button" role="menuitem" on:click=copy_link>"copy link"</button>
                        </div>
                    }) }}
                </header>
                {move || problem.get().map(|p| view! { <p class="perr" role="alert">{p}</p> })}
                {move || (adding.get() && chat_id.is_some()).then(|| {
                    let id = chat_id.unwrap_or_default();
                    let ps = chat.with(|c| c.as_ref().map(|c| c.participants.clone()).unwrap_or_default());
                    view! { <div class="sadd"><chat_pane::Picker ticket=t_add.clone() add_to=Some((id, ps)) on_done=Callback::new(move |_| adding.set(false)) /></div> }
                })}
                {main}
            </section>
            {aside}
            <button type="button" class="sscrim" aria-label="close the chats" on:click=move |_| drawer.set(false)></button>
        </div>
    }
}

fn current_is_pick(chat_id: Option<u64>) -> bool {
    chat_id.is_none() && chat_pane::shown().with_untracked(|s| matches!(s.as_ref().map(|s| &s.view), Some(View::Pick)))
}

fn title_now(chat: Memo<Option<Chat>>) -> String {
    chat.with_untracked(|c| c.as_ref().and_then(|c| c.title.clone())).unwrap_or_default()
}

/// The ticket beside the chat: its fields, then its body, at the width the
/// divider was dragged to.
#[component]
fn TicketBeside(ticket: String, split: RwSignal<Split>) -> impl IntoView {
    let body = RwSignal::new(None::<String>);
    let not_found = RwSignal::new(false);
    let blockers = RwSignal::new(false);
    let last_error = RwSignal::new(None::<String>);
    let holder = crate::cache::Holder::new();
    crate::session::load_ticket(ticket.clone(), holder, body, not_found, blockers, last_error);
    let t = ticket.clone();
    let row = Memo::new(move |_| crate::cache::find(&t).map(|r| r.get()));
    let drag = move |ev: web_sys::PointerEvent| {
        ev.prevent_default();
        if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
    };
    let moving = move |ev: web_sys::PointerEvent| {
        let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if !el.has_pointer_capture(ev.pointer_id()) {
            return;
        }
        let w = dom::window();
        let vw = w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1440.0);
        let rail = if vw > 760.0 { 264.0 } else { 0.0 };
        let width = Split::clamp(vw - f64::from(ev.client_x()), vw, rail);
        split.update(|s| s.width = width);
    };
    let dropped = move |_| keep_split(split.get_untracked());
    let fields = move || {
        row.get().map(|t| {
            let status = t.status.to_string();
            view! {
                <h2 class="sttitle">{t.title.clone()}</h2>
                <dl class="stfields">
                    <div><dt>"status"</dt><dd><span class=crate::st_class(&status)>{status.clone()}</span></dd></div>
                    <div><dt>"kind"</dt><dd>{t.kind.to_string()}</dd></div>
                    {t.parent.clone().map(|p| view! { <div><dt>"parent"</dt><dd><a class="tchip" href=format!("#/ticket/{p}")>{p.to_string()}</a></dd></div> })}
                    {(!t.commits.is_empty()).then(|| view! { <div><dt>"commits"</dt><dd class="mono">{t.commits.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")}</dd></div> })}
                </dl>
            }
        })
    };
    let text = Signal::derive(move || body.get().unwrap_or_default());
    let t_body = ticket.clone();
    view! {
        <div class="sdiv" role="separator" aria-orientation="vertical" aria-label="drag to resize the ticket" title="drag to resize"
            on:pointerdown=drag on:pointermove=moving on:pointerup=dropped></div>
        <aside class="sticket" aria-label=format!("{ticket}, beside the chat")>
            <div class="sthead">
                <a class="tchip" href=format!("#/ticket/{ticket}")>{ticket.clone()}</a>
                <span class="muted">"the ticket, beside the chat"</span>
                <button type="button" class="px" aria-label="hide the ticket" on:click=move |_| { split.update(|s| s.open = false); keep_split(split.get_untracked()); }>"×"</button>
            </div>
            {fields}
            {move || not_found.get().then(|| view! { <p class="pempty">"Not found."</p> })}
            {move || body.with(Option::is_some).then(|| view! { <crate::notes::BodyView body=text refs=None ticket=Some(t_body.clone()) /> })}
        </aside>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chats::tests::{chat, msg};
    use menger_tickets_api::ChatsEvent;

    /// `?chat=ID&full` survives a reload; leaving full takes `full` out and
    /// keeps the rest.
    #[test]
    fn the_url_says_full() {
        assert!(full_in("#/ticket/KILN-7?chat=41&full"));
        assert!(full_in("#/ticket/KILN-7?full=1&chat=41"));
        assert!(!full_in("#/ticket/KILN-7?chat=41"));
        assert!(!full_in("#/ticket/KILN-7?view=full"), "the body's full screen is another thing");
        assert_eq!(with_full("#/ticket/KILN-7?chat=41", true), "#/ticket/KILN-7?chat=41&full");
        assert_eq!(with_full("#/ticket/KILN-7?chat=41&full", false), "#/ticket/KILN-7?chat=41");
        assert_eq!(with_full("#/ticket/KILN-7?full", false), "#/ticket/KILN-7");
    }

    /// The rail: this ticket's open chats by last activity, then the closed
    /// (dimmed, kept); then the other tickets' chats with unread.
    #[test]
    fn the_rail_lists_this_ticket_then_other_unread() {
        let mut a = chat(41, "KILN-7", &["ada", "builder"]);
        a.last_at = 300;
        let mut b = chat(42, "KILN-7", &["ada", "reviewer"]);
        b.last_at = 400;
        b.unread = 2;
        let mut notes = chat(43, "KILN-7", &["ada"]);
        notes.last_at = 100;
        notes.title = Some("my notes".into());
        let mut old = chat(44, "KILN-7", &["ada", "builder", "rui"]);
        old.state = ChatState::Closed;
        let mut shelf = chat(45, "KILN-3", &["ada", "builder"]);
        shelf.unread = 5;
        let quiet = chat(46, "KILN-9", &["ada", "sam"]);
        let mut h = Held::from_events([a, b, notes, old, shelf, quiet].map(|chat| ChatsEvent::Chat { chat }));
        h.loaded(41, vec![msg(9, 41, "builder", "Fixed in 9a0d3e1, in review.\nmore")]);
        let name = |p: &str| p.to_string();
        let (here, others) = rail(&h, "kiln-7", "ada", &name);
        assert_eq!(here.iter().map(|r| r.id).collect::<Vec<_>>(), [42, 41, 43, 44]);
        assert_eq!(here[1].last, "builder: Fixed in 9a0d3e1, in review.");
        assert_eq!((here[2].label.as_str(), here[2].last.as_str()), ("my notes", "just you"));
        assert!(here[3].closed && here[3].last == "kept, read-only");
        assert_eq!(others.iter().map(|r| (r.id, r.ticket.as_str(), r.unread)).collect::<Vec<_>>(), [(45, "KILN-3", 5)], "only the others with unread");
    }

    /// The ticket beside the chat: 440 unless dragged, kept; a drag keeps
    /// the chat its room; a stored record that does not read is the default.
    #[test]
    fn the_split_is_kept_and_clamped() {
        assert_eq!(Split::default(), Split { open: false, width: 440.0 });
        let s = Split { open: true, width: 512.0 };
        assert_eq!(Split::decode(Some(&s.encode())), s);
        assert_eq!(Split::decode(Some("{nonsense")), Split::default());
        assert_eq!(Split::decode(None), Split::default());
        assert_eq!(Split::clamp(100.0, 1440.0, 264.0), 320.0);
        assert_eq!(Split::clamp(900.0, 1440.0, 264.0), 656.0, "the chat keeps 520");
        assert_eq!(Split::clamp(500.0, 1440.0, 264.0), 500.0);
    }
}
