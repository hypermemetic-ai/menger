//! CHAT-21: a chat message whose body would be framed as a ticket's
//! (`md::is_framed`) draws in its own sandboxed frame (CHAT-19 decision
//! `frame`: one per message), at the message's width, in the reader's
//! theme — and only near the view: [`MessageFrame`] makes the frame within
//! about a screen of it and drops its document past that, keeping the height
//! it had, so a long chat holds a screen's frames and never its forty.
//!
//! A message frame has a **home**, not a ticket: the chat and the message
//! the host stamps on it (`data-chat`, `data-message`), which nothing in the
//! frame can move. It carries no `data-ticket`, so no write from it can
//! answer a ticket's input; its reads are held to the chat's ticket's
//! shortcode (`data-scope`). CHAT-59: with no ticket (a direct message, a
//! group) it reads as the reader ([`bridge::READER`]) — `menger.call` and
//! `watch` as the reader's own, the server answering from the reader's
//! grants, never the author's — so `<m-dial>`, `<m-ring>` or a project's
//! own component draws with its data; it is handed the components its
//! markup names and what they use (ACCESS-26). The SDK's `menger.input.ticket` is the home's key,
//! [`key`] (`message:<id>`), so the house `m-decision` / `m-input` ask and
//! answer as they do in a body:
//!
//! - `menger.call('inputs.list', { ticket })` is answered with the message's
//!   own inputs (`chats.inputs.list`), each as an `input` whose `ticket` is
//!   the key ([`inputs`]);
//! - `menger.input.submit` / `clear` go to `chats.inputs.answer` / `clear`
//!   for the home's chat and message ([`submit`], [`clear`]) — after the
//!   same gesture check as a body's, and the server's own checks;
//! - each `input_changed` of the message (the chats watch, src/chats.rs) is
//!   relayed to its frames as a `menger:change` with the key ([`heard`]).

use std::cell::RefCell;

use leptos::prelude::*;
use menger_tickets_api::chat_inputs::ChatInput;
use menger_tickets_api::chats::ChatsEvent;
use menger_tickets_api::types::{Answer, InputHistoryEntry, InputKind, InputOption, InputState};
use menger_tickets_api::vocab::{ChatId, MessageId};
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::bridge::{self, Clear, Submit};
use crate::{dom, md, present, session};

/// What the SDK's `menger.input.ticket` names in a message's frame: the
/// message's inputs' home, which no ticket id can spell.
pub(crate) fn key(message: u64) -> String {
    format!("message:{message}")
}

/// A frame's home — `(chat, message)` — as the host stamped it; `None` for
/// any frame that is not a message's.
pub(crate) fn home_of(frame: &web_sys::Element) -> Option<(u64, u64)> {
    let chat = frame.get_attribute("data-chat")?.parse().ok()?;
    let message = frame.get_attribute("data-message")?.parse().ok()?;
    Some((chat, message))
}

/// A message's input as its frame reads it: a ticket's input's shape
/// (`types::Input`, the wire's spelling), its `ticket` the home's [`key`] —
/// what the house components compare against `menger.input.ticket`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Shown {
    pub(crate) ticket: String,
    pub(crate) id: String,
    pub(crate) kind: InputKind,
    pub(crate) question: Option<String>,
    pub(crate) options: Vec<InputOption>,
    pub(crate) recommendation: Option<String>,
    pub(crate) state: InputState,
    pub(crate) answer: Option<Answer>,
    pub(crate) history: Vec<InputHistoryEntry>,
    pub(crate) asked_unix: Option<i64>,
}

impl Shown {
    pub(crate) fn of(i: &ChatInput) -> Self {
        Shown {
            ticket: key(i.message.get()),
            id: i.id.as_str().to_string(),
            kind: i.kind,
            question: i.question.clone(),
            options: i.options.clone(),
            recommendation: i.recommendation.clone(),
            state: i.state,
            answer: i.answer.clone(),
            history: i.history.clone(),
            asked_unix: i.asked_unix,
        }
    }
}

/// One item of a frame's `inputs.list` answer, as `TicketsEvent::Input`
/// spells it.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Item {
    Input { input: Shown },
}

/// The change a frame hears for one of its message's inputs, as
/// `TicketsEvent::InputChanged` spells it.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Change {
    InputChanged { input: Shown },
}

type Client = menger_tickets_api::ChatsClient<std::rc::Rc<plexus_client::WsTransport>>;

fn client(sdk: &str) -> Result<Client, String> {
    session::chats_client().ok_or_else(|| format!("{sdk}: not connected (nothing sent)"))
}

/// The inputs of `message` in `chat` — its own, never another message's —
/// or what the server said instead, naming `sdk`.
async fn held(c: &Client, sdk: &str, chat: u64, message: u64) -> Result<Vec<ChatInput>, String> {
    let events = c.inputs().list_all(Some(ChatId::new(chat)), None, None, None, None, Some(1000)).await.map_err(|e| format!("{sdk}: {}", session::describe(&e)))?;
    let mut out = Vec::new();
    for ev in events {
        match ev {
            ChatsEvent::Input { input } if input.message.get() == message => out.push(input),
            ChatsEvent::NotFound { chat } => return Err(format!("{sdk}: chat {chat} is not one you are in")),
            ChatsEvent::Error { message } => return Err(format!("{sdk}: {message}")),
            ChatsEvent::Refused { reasons } => return Err(format!("{sdk}: {}", reasons.join("; "))),
            _ => {}
        }
    }
    Ok(out)
}

/// A frame's `inputs.list` (`inputs`) in a message's frame: the message's
/// inputs, each as an `input` of the home's key.
pub(crate) async fn inputs(chat: u64, message: u64) -> Result<Vec<Item>, String> {
    let c = client("inputs.list")?;
    Ok(held(&c, "inputs.list", chat, message).await?.iter().map(|i| Item::Input { input: Shown::of(i) }).collect())
}

/// The input `id` among `held`, still open to a write, or why not — the
/// host's own check before anything is sent (the server checks again).
pub(crate) fn open_one<'a>(sdk: &str, id: &str, held: &'a [ChatInput]) -> Result<&'a ChatInput, String> {
    let Some(input) = held.iter().find(|i| i.id.as_str() == id) else {
        let declared: Vec<String> = held.iter().map(|i| format!("'{}'", i.id.as_str())).collect();
        return Err(if declared.is_empty() {
            format!("{sdk}: this message asks nothing — it declares no input '{id}' (nothing sent)")
        } else {
            format!("{sdk}: this message declares no input '{id}' — its inputs are {} (nothing sent)", declared.join(", "))
        });
    };
    match input.state {
        InputState::Closed => Err(format!("{sdk}: '{id}' is closed: its answer is frozen until who asked reopens it (nothing sent)")),
        InputState::Removed | InputState::Withdrawn => Err(format!("{sdk}: '{id}' is no longer asked (nothing sent)")),
        InputState::Open | InputState::Answered => Ok(input),
    }
}

/// The input as a write answered it, or the refusal.
fn written(sdk: &str, events: Vec<ChatsEvent>) -> Result<ChatInput, String> {
    for ev in events {
        match ev {
            ChatsEvent::InputChanged { input } | ChatsEvent::Input { input } => return Ok(input),
            ChatsEvent::NotFound { chat } => return Err(format!("{sdk}: chat {chat} is not one you are in")),
            ChatsEvent::Error { message } => return Err(format!("{sdk}: {message}")),
            ChatsEvent::Refused { reasons } => return Err(format!("{sdk}: {}", reasons.join("; "))),
            _ => {}
        }
    }
    Err(format!("{sdk}: the server answered nothing"))
}

/// A message frame's `menger.input.submit`: the host's checks against the
/// message's inputs (declared, open, a value of its kind), then
/// `chats.inputs.answer` — the recorded answer, or why not.
pub(crate) async fn submit(chat: u64, message: u64, s: Submit) -> Result<Answer, String> {
    let sdk = bridge::SUBMIT;
    let c = client(sdk)?;
    let held = held(&c, sdk, chat, message).await?;
    let input = open_one(sdk, &s.input, &held)?;
    let value = menger_tickets_api::inputs::check(input.kind, &input.options, s.value.clone()).map_err(|why| format!("{sdk}: not an answer to '{}': {why} (nothing sent)", s.input))?;
    let named = |e: String| format!("{sdk}: {e}");
    let (id, rationale) = (crate::typed::typed(&s.input).map_err(named)?, crate::typed::opt(s.rationale).map_err(named)?);
    let events = c.inputs().answer_all(ChatId::new(chat), MessageId::new(message), id, value, rationale).await.map_err(|e| format!("{sdk}: {}", session::describe(&e)))?;
    let input = written(sdk, events)?;
    let id = input.id.as_str().to_string();
    input.answer.ok_or_else(|| format!("{sdk}: '{id}' holds no answer"))
}

/// A message frame's `menger.input.clear`: the same checks, then
/// `chats.inputs.clear` — the input as the clear left it, of the home's key.
pub(crate) async fn clear(chat: u64, message: u64, cl: Clear) -> Result<Shown, String> {
    let sdk = bridge::CLEAR;
    let c = client(sdk)?;
    let held = held(&c, sdk, chat, message).await?;
    open_one(sdk, &cl.input, &held)?;
    let id = crate::typed::typed(&cl.input).map_err(|e| format!("{sdk}: {e}"))?;
    let events = c.inputs().clear_all(ChatId::new(chat), MessageId::new(message), id).await.map_err(|e| format!("{sdk}: {}", session::describe(&e)))?;
    Ok(Shown::of(&written(sdk, events)?))
}

/// A chats event, to the frames it concerns: each changed input of a
/// message to that message's frames, as an `input_changed` of its key.
/// Called by src/chats.rs for every event the watch (or a write) brings.
pub(crate) fn heard(ev: &ChatsEvent) {
    match ev {
        ChatsEvent::InputChanged { input } => relay(input),
        ChatsEvent::Posted { inputs, .. } => inputs.iter().for_each(relay),
        _ => {}
    }
}

fn relay(input: &ChatInput) {
    crate::frame::to_message(input.message.get(), &Change::InputChanged { input: Shown::of(input) });
}

thread_local! {
    /// The height each message's frame last had, so the turn keeps it while
    /// its document is dropped and takes it while the next one settles —
    /// CHAT-61: kept beyond the tab ([`STORE_KEY`]), so a fresh load mounts
    /// at the right height too.
    static HEIGHTS: RefCell<Option<Kept>> = const { RefCell::new(None) };
}

/// Before a message's frame is first measured.
const GUESS_PX: f64 = 160.0;

/// How far beyond the view a frame is kept: about a screen each way.
const NEAR: &str = "100% 0px 100% 0px";

/// CHAT-61: where the kept heights live (localStorage), and how many.
const STORE_KEY: &str = "menger.cframe-heights";
const KEEP: usize = 400;

/// CHAT-61: the heights of the last [`KEEP`] message frames, each its
/// turn's frame box as drawn (the bar and the frame) — the most recently
/// sized last, the oldest forgotten first.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Kept(Vec<(u64, u32)>);

impl Kept {
    pub(crate) fn get(&self, message: u64) -> Option<f64> {
        self.0.iter().find(|(m, _)| *m == message).map(|(_, h)| f64::from(*h))
    }

    /// Keeps `height` for `message`, most recent; whether anything changed.
    pub(crate) fn put(&mut self, message: u64, height: f64) -> bool {
        let h = height.round().clamp(1.0, f64::from(u16::MAX)) as u32;
        if self.0.last() == Some(&(message, h)) {
            return false;
        }
        self.0.retain(|(m, _)| *m != message);
        self.0.push((message, h));
        let over = self.0.len().saturating_sub(KEEP);
        self.0.drain(..over);
        true
    }

    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_default()
    }

    /// What [`to_json`](Self::to_json) wrote — anything else is nothing kept.
    pub(crate) fn from_json(s: &str) -> Self {
        let mut k = Kept(serde_json::from_str::<Vec<(u64, u32)>>(s).unwrap_or_default());
        let over = k.0.len().saturating_sub(KEEP);
        k.0.drain(..over);
        k
    }
}

fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

fn with_kept<R>(f: impl FnOnce(&mut Kept) -> R) -> R {
    HEIGHTS.with(|h| {
        let mut h = h.borrow_mut();
        let k = h.get_or_insert_with(|| storage().and_then(|s| s.get_item(STORE_KEY).ok().flatten()).map(|s| Kept::from_json(&s)).unwrap_or_default());
        f(k)
    })
}

fn keep(message: u64, height: f64) {
    if height <= 0.0 {
        return;
    }
    if let Some(json) = with_kept(|k| k.put(message, height).then(|| k.to_json())) {
        if let Some(s) = storage() {
            let _ = s.set_item(STORE_KEY, &json);
        }
    }
}

/// The height a message's frame takes while its document is not made.
pub(crate) fn kept_height(message: u64) -> f64 {
    with_kept(|k| k.get(message)).unwrap_or(GUESS_PX)
}

/// CHAT-21: a framed message — its body (`body`, the markdown it was
/// posted as) in a [`present::Framed`] box with the body's bar plus
/// *source* (the markdown, as text) and ⤢ (the pane's whole width), the
/// frame made only while the turn is within [`NEAR`] of the view. `chat`'s
/// ticket (`ticket`) is the reach its reads are held to — none: the
/// reader's ([`scope`]).
#[component]
pub(crate) fn MessageFrame(body: String, chat: u64, message: u64, ticket: Option<String>) -> impl IntoView {
    let html = Signal::stored(md::render_body_framed(&body, &|_| None).html);
    let decl = Signal::stored(present::Decl::default());
    let near = RwSignal::new(false);
    let source = RwSignal::new(false);
    let wide = RwSignal::new(false);
    let holder = NodeRef::<leptos::html::Div>::new();
    let kept = Callback::new(move |(): ()| {
        if let Some(el) = holder.get_untracked() {
            if !waiting(&el) {
                keep(message, f64::from(el.offset_height()));
            }
        }
    });
    let watching: StoredValue<Option<Watching>, LocalStorage> = StoredValue::new_local(None);
    holder.on_load(move |el: web_sys::HtmlDivElement| {
        let w = watch_near(&el, near, kept);
        watching.set_value(w);
    });
    on_cleanup(move || {
        kept.run(());
        if let Some(Some((observer, _on))) = watching.try_update_value(Option::take) {
            observer.disconnect();
        }
    });
    let actions = ViewFn::from(move || {
        view! {
            <button type="button" class="framed-btn" data-act="frame-source" aria-pressed=move || source.get().to_string() on:click=move |_| source.update(|s| *s = !*s)>"source"</button>
            <button type="button" class="framed-btn" data-act="frame-wide" title="the pane's whole width" aria-pressed=move || wide.get().to_string() on:click=move |_| wide.update(|w| *w = !*w)>"⤢"</button>
        }
    });
    let scope = scope(ticket.as_deref());
    let src = body.clone();
    let made = move || {
        if near.get() {
            // CHAT-61: the turn holds its kept height until the frame says
            // it is settled ([`sized`]), so its steps never move the chat.
            if let Some(el) = holder.get_untracked() {
                let _ = web_sys::HtmlElement::style(&el).set_property("min-height", &format!("{}px", kept_height(message)));
            }
            let actions = actions.clone();
            let scope = scope.clone();
            view! { <present::Framed html decl ticket=None scope actions message=(chat, message) /> }.into_any()
        } else {
            if let Some(el) = holder.get_untracked() {
                let _ = web_sys::HtmlElement::style(&el).remove_property("min-height");
            }
            let h = kept_height(message);
            view! { <div class="cframe-wait" aria-hidden="true" style=format!("min-height:{h}px")></div> }.into_any()
        }
    };
    view! {
        <div class="tframe" class:src=move || source.get() class:twide=move || wide.get() node_ref=holder data-message-frame=message.to_string()>
            {made}
            {move || source.get().then(|| view! { <pre class="tsrc">{src.clone()}</pre> })}
        </div>
    }
}

/// CHAT-59: the scope a message's frame reads under — its chat's ticket's
/// shortcode, or, in a chat with no ticket, the reader's reach.
pub(crate) fn scope(ticket: Option<&str>) -> String {
    bridge::scope_of(ticket).unwrap_or_else(|| bridge::READER.to_string())
}

/// An observer and the closure it calls, held while the turn is mounted.
type Watching = (web_sys::IntersectionObserver, Closure<dyn FnMut(js_sys::Array)>);

/// Make the frame while `el` is near the view, drop it when it leaves —
/// keeping its height first (`kept`). The view is the chat's list where it
/// scrolls, else the page.
fn watch_near(el: &web_sys::HtmlDivElement, near: RwSignal<bool>, kept: Callback<()>) -> Option<Watching> {
    let root = el.closest(".pmsgs").ok().flatten().and_then(|l| dom::Scroller::list(&l)).filter(|s| !s.is_doc()).map(|s| s.element().clone());
    let on = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
        let Some(last) = entries.iter().last().and_then(|e| e.dyn_into::<web_sys::IntersectionObserverEntry>().ok()) else { return };
        let now = last.is_intersecting();
        if near.try_get_untracked() == Some(true) && !now {
            kept.run(());
        }
        near.try_set(now);
    });
    let opts = web_sys::IntersectionObserverInit::new();
    opts.set_root_margin(NEAR);
    if let Some(root) = &root {
        opts.set_root(Some(root));
    }
    let Ok(observer) = web_sys::IntersectionObserver::new_with_options(on.as_ref().unchecked_ref(), &opts) else {
        // No observer: the frame is made, as a body's is.
        near.set(true);
        return None;
    };
    observer.observe(el);
    Some((observer, on))
}

/// What a message's frame says on its element when it takes a new height
/// — bubbling, so the chat's list measures the turn again (src/chat_pane.rs)
/// and a reader at the bottom stays there.
pub(crate) const SIZED: &str = "menger-cframe-sized";

/// Whether a message's frame box still holds its kept height, its frame
/// not yet settled.
fn waiting(holder: &web_sys::HtmlElement) -> bool {
    !holder.style().get_property_value("min-height").unwrap_or_default().is_empty()
}

/// A message's frame took `height`: said on `frame` ([`SIZED`]). CHAT-61:
/// until it is `settled` its turn holds the kept height; once it is, the
/// turn takes its own — in one step — and that is kept, so a later drop,
/// or the next load, mounts at the right one.
pub(crate) fn sized(frame: &web_sys::HtmlIFrameElement, message: u64, height: f64, settled: bool) {
    let holder = frame.closest("[data-message-frame]").ok().flatten().and_then(|h| h.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(holder) = holder.filter(|_| settled && height > 0.0) {
        let _ = holder.style().remove_property("min-height");
        keep(message, f64::from(holder.offset_height()));
    }
    let init = web_sys::EventInit::new();
    init.set_bubbles(true);
    if let Ok(ev) = web_sys::Event::new_with_event_init_dict(SIZED, &init) {
        let _ = frame.dispatch_event(&ev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::types::InputValue;

    fn input(message: u64, id: &str, state: InputState) -> ChatInput {
        serde_json::from_value(serde_json::json!({
            "chat": 41, "message": message, "ticket": "KILN-40", "author": "key:b", "id": id,
            "kind": "choice", "question": "Keep the undated rows?",
            "options": [{"label": "Keep", "value": "keep", "recommended": true}, {"label": "Drop", "value": "drop", "recommended": false}],
            "recommendation": null, "state": state, "answer": null, "history": [], "asked_unix": 1
        }))
        .unwrap()
    }

    /// The home's key is what `menger.input.ticket` says — never a ticket's id.
    #[test]
    fn a_message_frame_s_inputs_are_keyed_by_the_message() {
        assert_eq!(key(812), "message:812");
        let shown = serde_json::to_value(Shown::of(&input(812, "undated", InputState::Open))).unwrap();
        assert_eq!(shown["ticket"], "message:812");
        assert_eq!(shown["id"], "undated");
        assert_eq!(shown["kind"], "choice");
        assert!(shown.get("chat").is_none() && shown.get("author").is_none(), "the frame sees an input's shape: {shown}");
        let item = serde_json::to_value(Item::Input { input: Shown::of(&input(812, "undated", InputState::Open)) }).unwrap();
        assert_eq!(item["type"], "input");
        let change = serde_json::to_value(Change::InputChanged { input: Shown::of(&input(812, "undated", InputState::Answered)) }).unwrap();
        assert_eq!(change["type"], "input_changed");
        assert_eq!(change["input"]["state"], "answered");
    }

    /// The pick: a message frames by the rule a ticket's body does — markup
    /// outside code frames; the same words in code, or none, stay inline.
    #[test]
    fn a_message_with_components_frames_and_a_plain_one_does_not() {
        assert!(md::is_framed("The kiln's week:\n\n<m-stat value=\"40\" label=\"fired\"></m-stat>"));
        assert!(md::is_framed("<m-decision id=\"undated\"><m-question>Keep them?</m-question></m-decision>"));
        assert!(!md::is_framed("Use `<m-stat>` for a number."));
        assert!(!md::is_framed("a < b, and KILN-40 is in review"));
        assert!(!md::is_framed("```html\n<m-stat value=1></m-stat>\n```"));
    }

    /// The host's own check before a write: declared in this message, open.
    #[test]
    fn a_write_answers_only_this_message_s_open_input() {
        let held = vec![input(812, "undated", InputState::Open), input(812, "dated", InputState::Closed)];
        assert!(open_one(bridge::SUBMIT, "undated", &held).is_ok());
        let e = open_one(bridge::SUBMIT, "other", &held).unwrap_err();
        assert!(e.contains("declares no input 'other'") && e.contains("'undated', 'dated'") && e.contains("nothing sent"), "{e}");
        let e = open_one(bridge::CLEAR, "dated", &held).unwrap_err();
        assert!(e.starts_with("menger.input.clear: 'dated' is closed"), "{e}");
        let e = open_one(bridge::SUBMIT, "x", &[]).unwrap_err();
        assert!(e.contains("asks nothing"), "{e}");
        // The value is checked as a ticket's is.
        let i = &held[0];
        assert!(menger_tickets_api::inputs::check(i.kind, &i.options, InputValue::Choice { option: "keep".into() }).is_ok());
        assert!(menger_tickets_api::inputs::check(i.kind, &i.options, InputValue::Choice { option: "nope".into() }).is_err());
    }

    /// CHAT-61: the kept heights — the most recent kept, the oldest past
    /// the bound forgotten, and what was stored read back.
    #[test]
    fn a_frame_s_height_is_kept_bounded_and_read_back() {
        let mut k = Kept::default();
        assert!(k.put(490, 312.4));
        assert!(!k.put(490, 312.0), "the same height again changes nothing");
        assert_eq!(k.get(490), Some(312.0));
        assert!(k.put(490, 300.0));
        assert_eq!(k.get(490), Some(300.0), "the newest height wins");
        for m in 0..KEEP as u64 {
            k.put(1000 + m, 100.0);
        }
        assert_eq!(k.get(490), None, "the oldest goes past the bound");
        assert_eq!(k.0.len(), KEEP);
        k.put(1000, 120.0);
        assert_eq!(k.0.last(), Some(&(1000, 120)), "a height sized again is the most recent");
        let back = Kept::from_json(&k.to_json());
        assert_eq!(back, k);
        assert_eq!(Kept::from_json("not json"), Kept::default(), "anything else is nothing kept");
    }

    /// CHAT-59: a chat inside a ticket reads under its shortcode; one with
    /// no ticket, as the reader.
    #[test]
    fn a_message_frame_s_scope_is_its_ticket_s_or_the_reader_s() {
        assert_eq!(scope(Some("KILN-7")), "KILN");
        assert_eq!(scope(None), bridge::READER);
        assert_eq!(bridge::scope_named(Some(&scope(None))), Some(bridge::READER.to_string()));
    }
}
