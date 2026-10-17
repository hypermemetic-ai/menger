//! CHAT-7: the chat's composer — manifest's (`SessionPage.tsx`,
//! `composerKeys.ts`, `slash.ts`, `vox.css`'s `.vox-input`), in menger's
//! tokens, for the dock's chat and the full-screen session alike.
//!
//! - **The card**: one rounded card, the textarea then a row of buttons;
//!   at `:focus-within` its border turns accent with a ring. The textarea
//!   grows from two lines to 40% of the view, then scrolls (and drags
//!   taller); its size is Settings → Chat's composer font.
//! - **Keys** ([`key_act`], bindings `chat.*` in [`Context::Composer`]):
//!   Enter sends, Shift+Enter is a newline, ⌘/Ctrl+Enter sends too, Esc
//!   leaves the box (AGENT-3's *stop the session* is [`stops_the_session`],
//!   a stub until an agent can be mid-turn), ↑ in an empty box recalls your
//!   last message to edit and send as new ([`recall`]); IME composition
//!   never sends (`bindings::hit` ignores a composing key).
//! - **`@`** at a word's start ([`mention_at`]) opens the participant menu
//!   over the card: ↑↓ move, Tab or Enter picks — the one picked is added
//!   to the chat and named in the text — Esc closes it
//!   ([`Context::ComposerMenu`]).
//! - **`/`** on a one-line text opens the command menu ([`COMMANDS`],
//!   manifest's `slash.ts` shape): `/add NAME`, `/ticket CODE`, `/title
//!   TEXT`, `/close`. A command runs, never goes as prose ([`act_of`]); one
//!   that cannot run is refused in a strip over the card — *not sent ·
//!   reason ×* — and the text stays.
//! - **Ticket codes** in the text show as chips in a strip over the card
//!   ([`codes_in`]); a pasted or dropped image is refused there too
//!   ([`IMAGES_REFUSED`]) rather than lost.
//! - **Drafts** ([`Drafts`]): each chat's unsent text, per browser, under
//!   `menger.chat.draft.<id>` — restored on return, cleared on send, put
//!   back when a send is refused.
//!
//! What decides is pure and pinned by native tests (manifest's
//! `composerKeys` tests are the model); the rest is the DOM.

use std::collections::HashMap;

use leptos::html::Textarea;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::chats;

// -- the commands ---------------------------------------------------------------

/// One `/` command: its word, what it takes, and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    pub(crate) name: &'static str,
    /// What it takes, as the menu spells it (`NAME`), or nothing.
    pub(crate) arg: Option<&'static str>,
    /// Whether the argument may hold spaces (`/title`'s text).
    pub(crate) spaced_arg: bool,
    pub(crate) hint: &'static str,
}

/// The commands, in the menu's order.
pub(crate) const COMMANDS: [Spec; 4] = [
    Spec { name: "add", arg: Some("NAME"), spaced_arg: false, hint: "bring a person or an agent into this chat" },
    Spec { name: "ticket", arg: Some("CODE"), spaced_arg: false, hint: "put a ticket card in the message" },
    Spec { name: "title", arg: Some("TEXT"), spaced_arg: true, hint: "rename this chat" },
    Spec { name: "close", arg: None, spaced_arg: false, hint: "close it — read-only, kept on its ticket" },
];

/// A command, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Add(String),
    Ticket(String),
    Title(String),
    Close,
}

/// What a `/` line has typed so far: its word, whether a space followed it,
/// and what follows the space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Slash {
    pub(crate) word: String,
    pub(crate) spaced: bool,
    pub(crate) rest: String,
}

/// A one-line text that begins with `/`, read as a command being typed.
pub(crate) fn slash_of(text: &str) -> Option<Slash> {
    let line = text.strip_prefix('/')?;
    if line.contains(['\n', '\r']) {
        return None;
    }
    let word_end = line.find(|c: char| !(c.is_ascii_lowercase() || c == '-')).unwrap_or(line.len());
    let (word, after) = line.split_at(word_end);
    if after.is_empty() {
        return Some(Slash { word: word.to_string(), spaced: false, rest: String::new() });
    }
    // Anything but a space after the word (`/a1`, `//`) is no command.
    let rest = after.strip_prefix([' ', '\t'])?;
    if word.is_empty() {
        return None;
    }
    Some(Slash { word: word.to_string(), spaced: true, rest: rest.trim().to_string() })
}

fn spec_for(word: &str) -> Option<&'static Spec> {
    COMMANDS.iter().find(|c| c.name == word)
}

/// The menu for what is typed: every command the word begins, or — once a
/// space follows it — that command alone.
pub(crate) fn menu_for(s: &Slash) -> Vec<&'static Spec> {
    if s.spaced {
        return spec_for(&s.word).into_iter().collect();
    }
    COMMANDS.iter().filter(|c| c.name.starts_with(s.word.as_str())).collect()
}

fn offered() -> String {
    COMMANDS.iter().map(|c| format!("/{}", c.name)).collect::<Vec<_>>().join(", ")
}

/// The command a `/` line asks for, or why it can't be run; `None` when the
/// text is no command at all (prose).
pub(crate) fn read_slash(text: &str) -> Option<Result<Command, String>> {
    let s = slash_of(text.trim())?;
    let Some(spec) = spec_for(&s.word) else {
        let typed = if s.word.is_empty() { "/".to_string() } else { format!("/{}", s.word) };
        return Some(Err(format!("{typed} is not a command — the commands are {}", offered())));
    };
    let arg = s.rest.as_str();
    Some(match (spec.arg, arg.is_empty()) {
        (Some(what), true) => Err(format!("/{} needs a {} — e.g. /{} {}", spec.name, what.to_lowercase(), spec.name, example(spec.name))),
        (None, false) => Err(format!("/{} takes nothing after it", spec.name)),
        (Some(what), false) if !spec.spaced_arg && arg.contains(char::is_whitespace) => Err(format!("/{} takes one {}, not “{arg}”", spec.name, what.to_lowercase())),
        _ => match spec.name {
            "add" => Ok(Command::Add(arg.trim_start_matches('@').to_string())),
            "ticket" if is_code(&arg.to_ascii_uppercase()) => Ok(Command::Ticket(arg.to_ascii_uppercase())),
            "ticket" => Err(format!("/ticket takes a ticket's code — {arg} is not one (e.g. KILN-3)")),
            "title" => Ok(Command::Title(arg.to_string())),
            _ => Ok(Command::Close),
        },
    })
}

fn example(name: &str) -> &'static str {
    match name {
        "add" => "reviewer",
        "ticket" => "KILN-3",
        _ => "The hold at 1240 °C",
    }
}

/// What a send does with the text: say it, run a command, or refuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    Say,
    Run(Command),
    Refuse(String),
}

pub(crate) fn act_of(text: &str) -> Act {
    match read_slash(text) {
        None => Act::Say,
        Some(Ok(c)) => Act::Run(c),
        Some(Err(why)) => Act::Refuse(why),
    }
}

// -- codes, mentions, names --------------------------------------------------------

/// A ticket's code: capitals (and digits after the first), a dash, a
/// number from 1 — `KILN-7`, `A2B-10`.
pub(crate) fn is_code(s: &str) -> bool {
    let Some((head, n)) = s.split_once('-') else { return false };
    let mut hc = head.chars();
    hc.next().is_some_and(|c| c.is_ascii_uppercase())
        && hc.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && !n.is_empty()
        && !n.starts_with('0')
        && n.chars().all(|c| c.is_ascii_digit())
}

/// Every ticket code in `text`, once each, in the order first written — a
/// word on its own (`KILN-7`, `KILN-7's`, `(KILN-7)`), never part of a
/// longer word.
pub(crate) fn codes_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        let w = raw.trim_matches('-');
        if is_code(w) && !out.iter().any(|o| o == w) {
            out.push(w.to_string());
        }
    }
    out
}

/// An `@` being typed at a word's start, the caret after it: where the `@`
/// is (a char index) and what follows it so far.
pub(crate) fn mention_at(text: &str, caret: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let mut i = caret;
    while i > 0 {
        let c = chars[i - 1];
        if c == '@' {
            let start = i - 1;
            let at_word_start = start == 0 || chars[start - 1].is_whitespace() || chars[start - 1] == '(';
            return at_word_start.then(|| (start, chars[i..caret].iter().collect()));
        }
        if c.is_whitespace() {
            return None;
        }
        i -= 1;
    }
    None
}

/// The text with the `@query` from `start` to `caret` replaced by `@name `,
/// and where the caret goes.
pub(crate) fn complete_mention(text: &str, start: usize, caret: usize, name: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let head: String = chars[..start].iter().collect();
    let tail: String = chars[caret..].iter().collect();
    let put = format!("@{name} ");
    let at = start + put.chars().count();
    (format!("{head}{put}{tail}"), at)
}

/// The candidates an `@query` matches: principals that are not in the chat
/// yet, by their name or principal, those that can read the ticket first.
pub(crate) fn mention_rows(cands: &[(String, bool)], already: &[String], query: &str, name: impl Fn(&str) -> String) -> Vec<(String, bool)> {
    let q = query.to_lowercase();
    let mut v: Vec<(String, bool)> = cands
        .iter()
        .filter(|(p, _)| !already.contains(p))
        .filter(|(p, _)| q.is_empty() || name(p).to_lowercase().contains(&q) || p.to_lowercase().contains(&q))
        .cloned()
        .collect();
    v.sort_by_key(|(p, reads)| (!reads, name(p).to_lowercase()));
    v
}

/// `/add NAME`: the one principal `name` means among the candidates — by
/// its name, its principal, or an agent's bare name — or why not.
pub(crate) fn resolve_add(name: &str, cands: &[(String, bool)], already: &[String], ticket: &str, namer: impl Fn(&str) -> String) -> Result<String, String> {
    let n = name.trim().trim_start_matches('@').to_lowercase();
    let hits: Vec<&(String, bool)> = cands
        .iter()
        .filter(|(p, _)| {
            let p_l = p.to_lowercase();
            namer(p).to_lowercase() == n || p_l == n || p_l.strip_prefix("agent:") == Some(n.as_str())
        })
        .collect();
    match hits.as_slice() {
        [] => Err(format!("/add {name} — no one called {name} holds a grant here")),
        [(p, _)] if already.contains(p) => Err(format!("/add {name} — {name} is in this chat already")),
        [(_, false)] => Err(format!("/add {name} — {name} can't read {ticket}")),
        [(p, true)] => Ok(p.clone()),
        _ => Err(format!("/add {name} — more than one goes by {name}: pick from @'s menu")),
    }
}

// -- keys --------------------------------------------------------------------------

/// What a key in the composer does (the menu closed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyAct {
    /// Send (or run the command).
    Send,
    /// Let the browser have it: a newline, a character.
    Type,
    /// Leave the box.
    Blur,
    /// AGENT-3: stop the agent mid-turn.
    Stop,
    /// Bring back your last message.
    Recall,
}

/// The composer's answer to a binding `id` (from `bindings::hit` in
/// [`Context::Composer`]), the box `empty` or not.
pub(crate) fn key_act(id: Option<&str>, empty: bool, agent_mid_turn: bool) -> KeyAct {
    match id {
        Some("chat.send") => KeyAct::Send,
        Some("chat.leave") if stops_the_session(agent_mid_turn) => KeyAct::Stop,
        Some("chat.leave") => KeyAct::Blur,
        Some("chat.recall") if empty => KeyAct::Recall,
        _ => KeyAct::Type,
    }
}

/// manifest's `stopsTheSession`: Esc stops an agent mid-turn. AGENT-3 —
/// claims, and an agent's turn — is not built, so nothing is ever mid-turn
/// here and Esc always leaves the box. When it is, the session passes the
/// agent's state in.
pub(crate) fn stops_the_session(agent_mid_turn: bool) -> bool {
    agent_mid_turn
}

/// What a key does while the `@` or `/` menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuAct {
    Move(i32),
    Pick,
    Close,
    Pass,
}

pub(crate) fn menu_act(id: Option<&str>) -> MenuAct {
    match id {
        Some("chat.menu-up") => MenuAct::Move(-1),
        Some("chat.menu-down") => MenuAct::Move(1),
        Some("chat.menu-pick") => MenuAct::Pick,
        Some("chat.menu-close") => MenuAct::Close,
        _ => MenuAct::Pass,
    }
}

/// Whether the `/` menu only hints: the command's word is settled and its
/// argument is being typed, so the keys are the composer's (Enter runs it).
pub(crate) fn menu_settled(text: &str) -> bool {
    slash_of(text).is_some_and(|s| s.spaced)
}

/// The highlighted row after a move of `by` in a menu of `n` rows (wraps).
pub(crate) fn moved(at: usize, by: i32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    (at as i64 + i64::from(by)).rem_euclid(n as i64) as usize
}

/// ↑ in an empty box: the body of your last message in the chat.
pub(crate) fn recall(thread: &[menger_tickets_api::Message], me: &str) -> Option<String> {
    thread.iter().rev().find(|m| m.author == me).map(|m| m.body.clone())
}

// -- the rest ----------------------------------------------------------------------

/// What the strip says of a pasted or dropped image.
pub(crate) const IMAGES_REFUSED: &str = "images aren't kept yet — paste or drop text";

/// The box's placeholder.
pub(crate) fn placeholder(ticket: &str, alone: bool) -> String {
    if alone {
        format!("Write a note on {ticket}, or @ someone… (Enter sends · Shift+Enter newline · / commands)")
    } else {
        format!("message {ticket}'s chat… (Enter sends · Shift+Enter newline · / commands · @ adds)")
    }
}

/// Where a chat's draft is kept, per browser.
pub(crate) fn draft_key(chat: u64) -> String {
    format!("menger.chat.draft.{chat}")
}

/// A place drafts are kept: the browser's localStorage, or a map in tests.
pub(crate) trait Kv {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&mut self, key: &str, value: &str);
    fn remove(&mut self, key: &str);
}

impl Kv for HashMap<String, String> {
    fn get(&self, key: &str) -> Option<String> {
        HashMap::get(self, key).cloned()
    }
    fn set(&mut self, key: &str, value: &str) {
        self.insert(key.to_string(), value.to_string());
    }
    fn remove(&mut self, key: &str) {
        HashMap::remove(self, key);
    }
}

/// The browser's localStorage — every call caught: a browser that refuses
/// storage keeps no drafts and breaks nothing.
pub(crate) struct Local;

impl Kv for Local {
    fn get(&self, key: &str) -> Option<String> {
        web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
    }
    fn set(&mut self, key: &str, value: &str) {
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item(key, value);
        }
    }
    fn remove(&mut self, key: &str) {
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.remove_item(key);
        }
    }
}

/// Each chat's unsent text.
pub(crate) struct Drafts;

impl Drafts {
    /// Keep `text` as `chat`'s draft; a blank one is none.
    pub(crate) fn keep(kv: &mut impl Kv, chat: u64, text: &str) {
        if text.trim().is_empty() {
            kv.remove(&draft_key(chat));
        } else {
            kv.set(&draft_key(chat), text);
        }
    }
    pub(crate) fn restore(kv: &impl Kv, chat: u64) -> String {
        kv.get(&draft_key(chat)).unwrap_or_default()
    }
    pub(crate) fn clear(kv: &mut impl Kv, chat: u64) {
        kv.remove(&draft_key(chat));
    }
}

// -- the composer ----------------------------------------------------------------

/// What the composer is handed: the chat, its ticket, the text box's
/// signal (a turn's *quote* and a suggestion write into it), and whether
/// it is the session's (`full`).
#[derive(Clone)]
pub(crate) struct Props {
    pub(crate) id: u64,
    pub(crate) ticket: String,
    pub(crate) body: RwSignal<String>,
    pub(crate) full: bool,
}

/// One row of the menu over the card.
#[derive(Clone, Debug, PartialEq)]
enum Row {
    Cmd(&'static Spec),
    Who(String, bool),
}

/// The composer: the card, its strip, its menu.
#[component]
pub(crate) fn Composer(props: Props) -> impl IntoView {
    let Props { id, ticket, body, full } = props;
    let held = chats::signal();
    let me = chats::me();
    let name = crate::chat_pane::names();
    let sending = RwSignal::new(false);
    let refusal = RwSignal::new(None::<String>);
    let caret = RwSignal::new(0usize);
    let hi = RwSignal::new(0usize);
    let menu_shut = RwSignal::new(false);
    let cands = RwSignal::new(None::<Vec<(String, bool)>>);
    let area = NodeRef::<Textarea>::new();
    // Settings → Chat: the composer's font.
    crate::settings::apply_chat_px(crate::settings::load().chat_px);
    // The draft: restored once, kept on every change.
    body.set(Drafts::restore(&Local, id));
    Effect::new(move |first: Option<()>| {
        let text = body.get();
        if first.is_some() {
            Drafts::keep(&mut Local, id, &text);
        }
    });
    // Focus: opening a chat, switching to it, going full.
    area.on_load(move |el| {
        spawn_local(async move {
            crate::dom::sleep_ms(0).await;
            let _ = el.focus();
        });
    });
    crate::grow::follow(area, move || body.track());
    let chat = {
        let held = held.clone();
        Memo::new(move |_| held.with(|h| h.as_ref().and_then(|h| h.get(id).cloned())))
    };
    let participants = Memo::new(move |_| chat.with(|c| c.as_ref().map(|c| c.participants.clone()).unwrap_or_default()));
    let ask_cands = {
        let t = ticket.clone();
        move || {
            if cands.get_untracked().is_some() {
                return;
            }
            cands.set(Some(Vec::new()));
            let t = t.clone();
            spawn_local(async move {
                if let Ok(c) = chats::candidates(&t).await {
                    cands.try_set(Some(c));
                }
            });
        }
    };
    // The menu: `/` commands, or `@` candidates.
    let rows = {
        let name = name.clone();
        let ask_cands = ask_cands.clone();
        Memo::new(move |_| {
            if menu_shut.get() {
                return Vec::new();
            }
            let text = body.get();
            if let Some(s) = slash_of(&text) {
                return menu_for(&s).into_iter().map(Row::Cmd).collect();
            }
            let Some((_, q)) = mention_at(&text, caret.get()) else { return Vec::new() };
            ask_cands();
            let all = cands.get().unwrap_or_default();
            mention_rows(&all, &participants.get(), &q, &name).into_iter().take(8).map(|(p, r)| Row::Who(p, r)).collect()
        })
    };
    Effect::new(move |_| {
        rows.track();
        hi.set(0);
    });
    let codes = Memo::new(move |_| body.with(|b| codes_in(b)));
    let set_text = move |text: String, at: Option<usize>| {
        body.set(text);
        if let (Some(el), Some(at)) = (area.get_untracked(), at) {
            let _ = el.set_selection_range(at as u32, at as u32);
            caret.set(at);
        }
    };
    let add_one = {
        let t = ticket.clone();
        move |p: String| {
            let t = t.clone();
            spawn_local(async move {
                if let Err(e) = chats::add(id, &p).await {
                    refusal.try_set(Some(format!("not added · {e}")));
                }
                let _ = t;
            });
        }
    };
    let pick = {
        let name = name.clone();
        let add_one = add_one.clone();
        move |row: Row| match row {
            Row::Cmd(spec) => {
                let text = format!("/{}{}", spec.name, if spec.arg.is_some() { " " } else { "" });
                let n = text.chars().count();
                set_text(text, Some(n));
            }
            Row::Who(p, reads) => {
                let text = body.get_untracked();
                let Some((start, _)) = mention_at(&text, caret.get_untracked()) else { return };
                if !reads {
                    refusal.set(Some(format!("not added · {} can't read the ticket", name(&p))));
                    return;
                }
                let (next, at) = complete_mention(&text, start, caret.get_untracked(), &name(&p));
                set_text(next, Some(at));
                add_one(p);
            }
        }
    };
    let send = {
        let me = me.clone();
        let t = ticket.clone();
        let name = name.clone();
        let add_one = add_one.clone();
        move || {
            let text = body.get_untracked();
            if text.trim().is_empty() || sending.get_untracked() {
                return;
            }
            match act_of(&text) {
                Act::Refuse(why) => refusal.set(Some(why)),
                Act::Say => {
                    sending.set(true);
                    body.set(String::new());
                    Drafts::clear(&mut Local, id);
                    spawn_local(async move {
                        let r = chats::post(id, &text).await;
                        sending.try_set(false);
                        if let Err(e) = r {
                            // A refused send puts the text back.
                            body.try_set(text.clone());
                            refusal.try_set(Some(e));
                        } else {
                            refusal.try_set(None);
                        }
                    });
                }
                Act::Run(cmd) => {
                    let (t, me, name, add_one) = (t.clone(), me.clone(), name.clone(), add_one.clone());
                    sending.set(true);
                    spawn_local(async move {
                        let r = run(cmd, id, &t, &me, &participants.get_untracked(), &name, add_one).await;
                        sending.try_set(false);
                        match r {
                            Ok(()) => {
                                body.try_set(String::new());
                                Drafts::clear(&mut Local, id);
                                refusal.try_set(None);
                            }
                            Err(e) => {
                                refusal.try_set(Some(e));
                            }
                        }
                    });
                }
            }
        }
    };
    let keydown = {
        let send = send.clone();
        let pick = pick.clone();
        let me = me.clone();
        let held = held.clone();
        move |ev: web_sys::KeyboardEvent| {
            let n = rows.with(Vec::len);
            // A command whose word is settled (`/add sam`) is being given its
            // argument: Enter runs it, Esc leaves — the menu only hints.
            if n > 0 && !menu_settled(&body.get_untracked()) {
                match menu_act(bindings::hit(&ev, Context::ComposerMenu)) {
                    MenuAct::Move(by) => {
                        ev.prevent_default();
                        hi.update(|h| *h = moved(*h, by, n));
                        return;
                    }
                    MenuAct::Pick => {
                        ev.prevent_default();
                        if let Some(r) = rows.with_untracked(|r| r.get(hi.get_untracked()).cloned()) {
                            pick(r);
                        }
                        return;
                    }
                    MenuAct::Close => {
                        ev.prevent_default();
                        ev.stop_propagation();
                        menu_shut.set(true);
                        return;
                    }
                    MenuAct::Pass => {}
                }
            }
            let empty = body.with_untracked(|b| b.is_empty());
            match key_act(bindings::hit(&ev, Context::Composer), empty, false) {
                KeyAct::Send => {
                    ev.prevent_default();
                    send();
                }
                KeyAct::Blur | KeyAct::Stop => {
                    ev.prevent_default();
                    ev.stop_propagation();
                    leave_box(&ev, full);
                }
                KeyAct::Recall => {
                    let thread = held.with_untracked(|h| h.as_ref().and_then(|h| h.thread(id).map(|t| t.list.clone()))).unwrap_or_default();
                    if let Some(last) = recall(&thread, &me) {
                        ev.prevent_default();
                        let n = last.chars().count();
                        set_text(last, Some(n));
                    }
                }
                KeyAct::Type => {}
            }
        }
    };
    let paste = move |ev: web_sys::ClipboardEvent| {
        let has_image = ev.clipboard_data().is_some_and(|d| has_files(&d));
        if has_image {
            ev.prevent_default();
            refusal.set(Some(format!("not pasted · {IMAGES_REFUSED}")));
        }
    };
    let drop = move |ev: web_sys::DragEvent| {
        if ev.data_transfer().is_some_and(|d| has_files(&d)) {
            ev.prevent_default();
            refusal.set(Some(format!("not dropped · {IMAGES_REFUSED}")));
        }
    };
    let alone = Memo::new(move |_| participants.with(|p| p.len() <= 1));
    let t_ph = ticket.clone();
    let hint = move || {
        if rows.with(|r| !r.is_empty()) {
            "↑↓ moves · Tab completes · Enter runs it · Esc closes · nothing here is sent as prose"
        } else if alone.get() {
            "Enter sends · Shift+Enter newline · / commands · @ brings someone in"
        } else {
            "Enter sends · Shift+Enter newline · / commands · @ adds"
        }
    };
    let menu = {
        let name = name.clone();
        let pick = pick.clone();
        move || {
            let list = rows.get();
            if list.is_empty() {
                return None;
            }
            let at = hi.get();
            let items = list
                .into_iter()
                .enumerate()
                .map(|(i, r)| {
                    let (label, what, no) = match &r {
                        Row::Cmd(s) => (format!("/{}{}", s.name, s.arg.map(|a| format!(" {a}")).unwrap_or_default()), s.hint.to_string(), false),
                        Row::Who(p, reads) => (format!("@{}", name(p)), if *reads { if chats::is_agent(p) { "agent · can read the ticket".to_string() } else { "can read the ticket".to_string() } } else { "can't read the ticket".to_string() }, !reads),
                    };
                    let pick = pick.clone();
                    view! {
                        <button type="button" class="cmrow" class:on=i == at class:no=no role="option" aria-selected=(i == at).to_string()
                            on:mousedown=move |ev: web_sys::MouseEvent| { ev.prevent_default(); pick(r.clone()); }>
                            <b>{label}</b><span>{what}</span>
                        </button>
                    }
                })
                .collect_view();
            Some(view! { <div class="cmenu" role="listbox" aria-label="commands and people">{items}</div> })
        }
    };
    let expand = move |_| crate::chat_full::set_full(!full);
    view! {
        <div class="ccompose" class:full=full>
            {move || refusal.get().map(|r| view! {
                <div class="cstrip refuse" role="alert"><span>{if r.starts_with("not ") { r.clone() } else { format!("not sent · {r}") }}</span>
                    <button type="button" class="cx" aria-label="dismiss" on:click=move |_| refusal.set(None)>"×"</button></div>
            })}
            {move || {
                let c = codes.get();
                (!c.is_empty()).then(|| view! {
                    <div class="cstrip codes"><span class="muted">"in this message"</span>
                        {c.into_iter().map(|code| view! { <a class="tchip" href=format!("#/ticket/{code}")>{code.clone()}</a> }).collect_view()}
                    </div>
                })
            }}
            {menu}
            <div class="ccard">
                <textarea
                    node_ref=area
                    class="cbox grow"
                    rows="2"
                    aria-label=format!("message the chat on {ticket}")
                    placeholder=move || placeholder(&t_ph, alone.get())
                    prop:value=move || body.get()
                    on:input=move |ev| {
                        menu_shut.set(false);
                        if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok()) {
                            caret.set(el.selection_start().ok().flatten().unwrap_or(0) as usize);
                        }
                        body.set(event_target_value(&ev));
                    }
                    on:click=move |ev| {
                        if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok()) {
                            caret.set(el.selection_start().ok().flatten().unwrap_or(0) as usize);
                        }
                    }
                    on:keydown=keydown
                    on:paste=paste
                    on:drop=drop
                ></textarea>
                <div class="crow">
                    <span class="chint">{hint}</span>
                    <button type="button" class="cfull" title=if full { "back to the dock" } else { "the chat full screen (g c)" } on:click=expand>
                        {if full { "⤡ pane" } else { "full ↗" }}
                    </button>
                    <button type="button" class="csend" aria-label="send" title="send (Enter)" disabled=move || sending.get() || body.with(|b| b.trim().is_empty()) on:click=move |_| send()>
                        {move || if sending.get() { "sending…" } else { "send" }}
                    </button>
                </div>
            </div>
        </div>
    }
}

/// Esc in the box: out of it — to the session (whose Esc then leaves full
/// screen), or plainly blurred in the dock.
fn leave_box(ev: &web_sys::KeyboardEvent, full: bool) {
    let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) else { return };
    let session = full.then(|| el.closest("#chatfull").ok().flatten()).flatten().and_then(|s| s.dyn_into::<web_sys::HtmlElement>().ok());
    match session {
        Some(s) => {
            let _ = s.focus();
        }
        None => {
            let _ = el.blur();
        }
    }
}

fn has_files(d: &web_sys::DataTransfer) -> bool {
    d.types().iter().any(|t| t.as_string().is_some_and(|t| t == "Files"))
}

/// A command, run.
async fn run(cmd: Command, id: u64, ticket: &str, _me: &str, already: &[String], name: &impl Fn(&str) -> String, add_one: impl Fn(String)) -> Result<(), String> {
    match cmd {
        Command::Close => chats::settle_state(id, false).await.map_err(|e| format!("/close — {e}")),
        Command::Title(t) => chats::rename(id, Some(&t)).await.map_err(|e| format!("/title — {e}")),
        Command::Ticket(code) => chats::post(id, &format!("{code}\n")).await,
        Command::Add(who) => {
            let cands = chats::candidates(ticket).await?;
            let p = resolve_add(&who, &cands, already, ticket, name)?;
            add_one(p);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{pick_single, table_with, Press};

    fn press(key: &str) -> Press {
        Press { key: key.into(), ..Press::default() }
    }

    /// The keys, as the bindings table reads them in the composer —
    /// manifest's `composerKeys` tests are the model.
    #[test]
    fn every_key_does_what_the_table_says() {
        let t = table_with(&[]);
        let hit = |p: Press, ctx| pick_single(&t, &p, ctx, false);
        let enter = press("Enter");
        let shift_enter = Press { shift: true, ..press("Enter") };
        let ctrl_enter = Press { ctrl: true, ..press("Enter") };
        let mac_cmd_enter = Press { meta: true, ..press("Enter") };
        assert_eq!(key_act(hit(enter.clone(), Context::Composer), false, false), KeyAct::Send, "Enter sends");
        assert_eq!(key_act(hit(shift_enter, Context::Composer), false, false), KeyAct::Type, "Shift+Enter is a newline");
        assert_eq!(key_act(hit(ctrl_enter, Context::Composer), false, false), KeyAct::Send, "Ctrl+Enter sends too");
        assert_eq!(key_act(pick_single(&t, &mac_cmd_enter, Context::Composer, true), false, false), KeyAct::Send, "⌘↵ sends on a Mac");
        assert_eq!(key_act(hit(press("Escape"), Context::Composer), false, false), KeyAct::Blur, "Esc leaves the box");
        assert_eq!(key_act(hit(press("Escape"), Context::Composer), false, true), KeyAct::Stop, "AGENT-3: Esc stops an agent mid-turn");
        assert_eq!(key_act(hit(press("ArrowUp"), Context::Composer), true, false), KeyAct::Recall, "↑ in an empty box recalls");
        assert_eq!(key_act(hit(press("ArrowUp"), Context::Composer), false, false), KeyAct::Type, "↑ in text moves the caret");
        assert_eq!(key_act(hit(press("a"), Context::Composer), false, false), KeyAct::Type);
        // The menu open.
        assert_eq!(menu_act(hit(press("ArrowDown"), Context::ComposerMenu)), MenuAct::Move(1));
        assert_eq!(menu_act(hit(press("ArrowUp"), Context::ComposerMenu)), MenuAct::Move(-1));
        assert_eq!(menu_act(hit(press("Tab"), Context::ComposerMenu)), MenuAct::Pick);
        assert_eq!(menu_act(hit(enter, Context::ComposerMenu)), MenuAct::Pick);
        assert_eq!(menu_act(hit(press("Escape"), Context::ComposerMenu)), MenuAct::Close);
        assert_eq!(menu_act(hit(press("x"), Context::ComposerMenu)), MenuAct::Pass);
        assert_eq!((moved(0, -1, 4), moved(3, 1, 4), moved(1, 1, 0)), (3, 0, 0), "the menu wraps");
    }

    /// AGENT-3 is not built: nothing is mid-turn, so Esc never stops.
    #[test]
    fn esc_stops_only_an_agent_mid_turn() {
        assert!(!stops_the_session(false));
        assert!(stops_the_session(true));
    }

    /// The command rows: `/` opens the menu, a word narrows it, a space
    /// settles it; each command runs, or is refused with why.
    #[test]
    fn every_command_row() {
        let s = slash_of("/").unwrap();
        assert_eq!(menu_for(&s).len(), 4, "a bare / offers them all");
        assert_eq!(menu_for(&slash_of("/t").unwrap()).iter().map(|c| c.name).collect::<Vec<_>>(), ["ticket", "title"]);
        assert_eq!(menu_for(&slash_of("/title x").unwrap()).len(), 1);
        assert!(slash_of("/a\nb").is_none(), "two lines are prose");
        assert!(!menu_settled("/ad"), "the word being typed: the menu's keys");
        assert!(menu_settled("/add sam"), "its argument being typed: Enter runs it");
        assert!(slash_of("not / a command").is_none());
        assert_eq!(act_of("hello"), Act::Say);
        assert_eq!(act_of("/add reviewer"), Act::Run(Command::Add("reviewer".into())));
        assert_eq!(act_of("/add @rui"), Act::Run(Command::Add("rui".into())));
        assert_eq!(act_of("/close"), Act::Run(Command::Close));
        assert_eq!(act_of("/title The hold at 1240 °C"), Act::Run(Command::Title("The hold at 1240 °C".into())));
        assert_eq!(act_of("/ticket kiln-3"), Act::Run(Command::Ticket("KILN-3".into())));
        let refused = |t: &str| match act_of(t) {
            Act::Refuse(r) => r,
            other => panic!("{t}: {other:?}"),
        };
        assert!(refused("/add").starts_with("/add needs a name"));
        assert!(refused("/add sam rui").contains("takes one name"));
        assert!(refused("/close now").contains("takes nothing"));
        assert!(refused("/ticket shelf").contains("is not one"));
        assert!(refused("/frobnicate").starts_with("/frobnicate is not a command — the commands are /add, /ticket, /title, /close"));
        assert!(refused("/").starts_with("/ is not a command"));
    }

    /// `/add NAME` finds one principal, or says why not.
    #[test]
    fn add_resolves_a_name() {
        let cands = vec![("agent:reviewer".to_string(), true), ("idp:sam".to_string(), false), ("idp:rui".to_string(), true)];
        let namer = |p: &str| p.rsplit(':').next().unwrap().to_string();
        assert_eq!(resolve_add("reviewer", &cands, &[], "KILN-7", namer), Ok("agent:reviewer".into()));
        assert_eq!(resolve_add("Rui", &cands, &[], "KILN-7", namer), Ok("idp:rui".into()));
        assert_eq!(resolve_add("sam", &cands, &[], "KILN-7", namer), Err("/add sam — sam can't read KILN-7".into()));
        assert_eq!(resolve_add("rui", &cands, &["idp:rui".into()], "KILN-7", namer), Err("/add rui — rui is in this chat already".into()));
        assert!(resolve_add("zed", &cands, &[], "KILN-7", namer).unwrap_err().contains("no one called zed"));
    }

    /// `@` at a word's start opens the menu; inside a word it is text.
    #[test]
    fn at_opens_the_menu_at_a_word_s_start() {
        assert_eq!(mention_at("@", 1), Some((0, String::new())));
        assert_eq!(mention_at("ask @bui", 8), Some((4, "bui".into())));
        assert_eq!(mention_at("ask @bui now", 12), None, "the caret past a space");
        assert_eq!(mention_at("ada@kiln", 8), None, "inside a word");
        assert_eq!(complete_mention("ask @bui now", 4, 8, "builder"), ("ask @builder  now".into(), 13));
        let cands = vec![("agent:builder".to_string(), true), ("idp:sam".to_string(), false), ("idp:bea".to_string(), true)];
        let namer = |p: &str| p.rsplit(':').next().unwrap().to_string();
        assert_eq!(mention_rows(&cands, &["agent:builder".into()], "", namer).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["idp:bea", "idp:sam"], "who can read first; those in it out");
        assert_eq!(mention_rows(&cands, &[], "bu", namer).len(), 1);
    }

    /// Ticket codes in the text: each once, as words, never inside one.
    #[test]
    fn codes_show_as_chips() {
        assert_eq!(codes_in("KILN-7's hold, see (KILN-3) and KILN-7 again"), ["KILN-7", "KILN-3"]);
        assert!(codes_in("MY-KILN-7x kiln-7 KILN-07 KILN-").is_empty());
        assert!(is_code("A2B-10") && !is_code("2AB-1") && !is_code("AB-0"));
    }

    /// Drafts: kept per chat, restored on return, cleared on send; a blank
    /// one is none.
    #[test]
    fn drafts_keep_restore_and_clear() {
        let mut kv: HashMap<String, String> = HashMap::new();
        Drafts::keep(&mut kv, 41, "half a thought");
        Drafts::keep(&mut kv, 52, "another");
        assert_eq!(kv.get("menger.chat.draft.41").map(String::as_str), Some("half a thought"));
        assert_eq!(Drafts::restore(&kv, 41), "half a thought");
        assert_eq!(Drafts::restore(&kv, 52), "another", "per chat");
        Drafts::clear(&mut kv, 41);
        assert_eq!(Drafts::restore(&kv, 41), "");
        Drafts::keep(&mut kv, 52, "  ");
        assert!(!kv.contains_key("menger.chat.draft.52"), "blank is no draft");
    }

    /// ↑ recalls your own last message, not theirs.
    #[test]
    fn up_recalls_your_last_message() {
        use crate::chats::tests::msg;
        let t = [msg(1, 41, "ada", "first"), msg(2, 41, "ada", "second"), msg(3, 41, "builder", "theirs")];
        assert_eq!(recall(&t, "ada").as_deref(), Some("second"));
        assert_eq!(recall(&t, "rui"), None);
    }

    #[test]
    fn the_placeholder_says_the_keys() {
        assert_eq!(placeholder("KILN-7", false), "message KILN-7's chat… (Enter sends · Shift+Enter newline · / commands · @ adds)");
        assert!(placeholder("KILN-7", true).starts_with("Write a note on KILN-7"));
    }
}
