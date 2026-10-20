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
//! shortcode (`data-scope`), and with no ticket (a direct message) it reads
//! only components. The SDK's `menger.input.ticket` is the home's key,
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
use std::collections::HashMap;

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
    /// its document is dropped (and before it is first made, a guess).
    static HEIGHTS: RefCell<HashMap<u64, f64>> = RefCell::new(HashMap::new());
}

/// Before a message's frame is first measured.
const GUESS_PX: f64 = 160.0;

/// How far beyond the view a frame is kept: about a screen each way.
const NEAR: &str = "100% 0px 100% 0px";

/// The height a message's frame takes while its document is not made.
pub(crate) fn kept_height(message: u64) -> f64 {
    HEIGHTS.with(|h| h.borrow().get(&message).copied()).unwrap_or(GUESS_PX)
}

/// CHAT-21: a framed message — its body (`body`, the markdown it was
/// posted as) in a [`present::Framed`] box with the body's bar plus
/// *source* (the markdown, as text) and ⤢ (the pane's whole width), the
/// frame made only while the turn is within [`NEAR`] of the view. `chat`'s
/// ticket (`ticket`) is the reach its reads are held to.
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
            let h = f64::from(el.offset_height());
            if h > 0.0 {
                HEIGHTS.with(|m| m.borrow_mut().insert(message, h));
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
    let scope = bridge::scope_of(ticket.as_deref());
    let src = body.clone();
    let made = move || {
        if near.get() {
            let actions = actions.clone();
            let scope = scope.clone().unwrap_or_default();
            view! { <present::Framed html decl ticket=None scope actions message=(chat, message) /> }.into_any()
        } else {
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

/// A message's frame took `height`: kept, so a later drop keeps the right
/// one, and said on `frame` ([`SIZED`]).
pub(crate) fn sized(frame: &web_sys::HtmlIFrameElement, message: u64, height: f64) {
    if height > 0.0 {
        HEIGHTS.with(|m| m.borrow_mut().insert(message, height));
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
}
