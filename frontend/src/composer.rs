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
//! - **`/`** on a one-line text opens the command menu ([`COMMANDS`];
//!   APP-83: in a chat with an agent, the agent's own first — `/clear`,
//!   `/stop`, `/status`, `/help`, sent as the message for the chat bridge
//!   to answer (CHAT-40); a command that takes nothing acts when picked;
//!   manifest's `slash.ts` shape): `/add NAME`, `/ticket CODE`, `/title
//!   TEXT`, `/close`; APP-90, in a chat on a ticket: `/assign WHO`
//!   (`tickets.assign`) and `/ask QUESTION` (`tickets.inputs.ask`, a text
//!   input on the ticket). A command runs, never goes as prose
//!   ([`act_of`]); one that cannot run is refused in a strip over the card
//!   — *not sent · reason ×* — and the text stays; one that ran says what
//!   it did in a strip of its own ([`Done`]).
//! - **The token** (APP-90, APP-83's mockup 4): a command that takes an
//!   argument, picked (or typed with its space), becomes a token in the
//!   card — `.ctok`, *`/assign` ×* — that waits for its argument; the box
//!   then holds only the argument ([`token_of`], [`line_of`]). Enter runs
//!   it; Backspace in an empty box takes it off, its word back as text
//!   ([`takes_token_off`]); × takes it off too.
//! - **Ticket codes** in the text show as chips in a strip over the card
//!   ([`codes_in`]).
//! - **Images** (CHAT-14, owner 2026-10-17: *"allow pasting images into
//!   chat and comments"*): pasted, dropped or picked, as a comment's are
//!   (COMMENT-5, src/media.rs) — checked, kept with `media.put` for the
//!   chat (seen by its participants alone), a thumbnail in the box, drawn
//!   in the turn, full size on a click. Send waits while one is on its way
//!   (CHAT-30): asked for then, it says *sends when the image is up* and
//!   goes when it is ([`send_label`]); never without it.
//! - **Send on a phone** (CHAT-27, [`tap_sends`]): a tap on Send never
//!   takes the focus from the box, so the keyboard stays up and the tap
//!   sends — it is read on its lift, not left to the click a blur loses.
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
    /// APP-83: the agent's own — sent as the message, and the chat bridge
    /// answers it itself (CHAT-40); offered only in a chat with an agent.
    pub(crate) agent: bool,
    /// APP-90: the ticket's — offered only in a chat on a ticket.
    pub(crate) ticket: bool,
    /// APP-90: the owner's (`/deploy`, owner 2026-10-19: deployments are
    /// triggered only by the owner) — offered only to an admin on `*`
    /// (`members.me`'s `admin_everywhere`), an admin of every project.
    pub(crate) admin: bool,
}

/// APP-90: where the composer is — which commands it offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Here {
    /// An agent is in the chat.
    pub(crate) agent: bool,
    /// The chat is on a ticket.
    pub(crate) ticket: bool,
    /// The reader is an admin on `*`.
    pub(crate) admin: bool,
}

impl Here {
    fn offers(self, c: &Spec) -> bool {
        (self.agent || !c.agent) && (self.ticket || !c.ticket) && (self.admin || !c.admin)
    }
}

/// The commands, in the menu's order: the agent's first (APP-83) — the
/// harmless first, since a bare `/` and Enter runs the top row — then the
/// chat's.
pub(crate) const COMMANDS: [Spec; 11] = [
    Spec { name: "status", arg: None, spaced_arg: false, hint: "what the agent is doing, and since when", agent: true, ticket: false, admin: false },
    Spec { name: "help", arg: None, spaced_arg: false, hint: "the agent's commands", agent: true, ticket: false, admin: false },
    Spec { name: "stop", arg: None, spaced_arg: false, hint: "stop the agent's running turn", agent: true, ticket: false, admin: false },
    Spec { name: "clear", arg: None, spaced_arg: false, hint: "the agent starts a fresh session — its context forgotten", agent: true, ticket: false, admin: false },
    Spec { name: "add", arg: Some("NAME"), spaced_arg: false, hint: "bring a person or an agent into this chat", agent: false, ticket: false, admin: false },
    Spec { name: "assign", arg: Some("WHO"), spaced_arg: false, hint: "assign the ticket to someone who can read it", agent: false, ticket: true, admin: false },
    Spec { name: "ask", arg: Some("QUESTION"), spaced_arg: true, hint: "file a question for the owner on the ticket, as an input", agent: false, ticket: true, admin: false },
    Spec { name: "deploy", arg: Some("ENV"), spaced_arg: false, hint: "queue a deploy of the ticket's project — test or prod", agent: false, ticket: true, admin: true },
    Spec { name: "ticket", arg: Some("CODE"), spaced_arg: false, hint: "put a ticket card in the message", agent: false, ticket: false, admin: false },
    Spec { name: "title", arg: Some("TEXT"), spaced_arg: true, hint: "rename this chat", agent: false, ticket: false, admin: false },
    Spec { name: "close", arg: None, spaced_arg: false, hint: "close it — read-only, kept on its ticket", agent: false, ticket: false, admin: false },
];

/// A command, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Add(String),
    /// APP-90: `tickets.assign` the chat's ticket to WHO.
    Assign(String),
    /// APP-90: `tickets.inputs.ask` on the chat's ticket.
    Ask(String),
    /// APP-90: `deployments.request` for the ticket's project.
    Deploy(String),
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
/// space follows it — that command alone. Only what is offered `here`: the
/// agent's commands where an agent is in the chat; APP-90: the ticket's in
/// a chat on a ticket, the owner's to an admin on `*`.
pub(crate) fn menu_for(s: &Slash, here: Here) -> Vec<&'static Spec> {
    if s.spaced {
        return spec_for(&s.word).filter(|c| !c.agent && here.offers(c)).into_iter().collect();
    }
    COMMANDS.iter().filter(|c| here.offers(c) && c.name.starts_with(s.word.as_str())).collect()
}

fn offered(here: Here) -> String {
    COMMANDS.iter().filter(|c| !c.agent && here.offers(c)).map(|c| format!("/{}", c.name)).collect::<Vec<_>>().join(", ")
}

/// The command a `/` line asks for, or why it can't be run; `None` when the
/// text is no command at all (prose).
pub(crate) fn read_slash(text: &str, here: Here) -> Option<Result<Command, String>> {
    let s = slash_of(text.trim())?;
    // APP-83: the agent's commands go as the message; the bridge answers.
    if !s.spaced && spec_for(&s.word).is_some_and(|c| c.agent) {
        return None;
    }
    let Some(spec) = spec_for(&s.word) else {
        let typed = if s.word.is_empty() { "/".to_string() } else { format!("/{}", s.word) };
        return Some(Err(format!("{typed} is not a command — the commands are {}", offered(here))));
    };
    if spec.ticket && !here.ticket {
        return Some(Err(format!("/{} works only in a chat on a ticket", spec.name)));
    }
    if spec.admin && !here.admin {
        return Some(Err(format!("/{} is the owner's: only an admin of every project queues a deploy", spec.name)));
    }
    let arg = s.rest.as_str();
    Some(match (spec.arg, arg.is_empty()) {
        (Some(what), true) => {
            let what = what.to_lowercase();
            let a = if what.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
            Err(format!("/{} needs {a} {what} — e.g. /{} {}", spec.name, spec.name, example(spec.name)))
        }
        (None, false) => Err(format!("/{} takes nothing after it", spec.name)),
        (Some(what), false) if !spec.spaced_arg && arg.contains(char::is_whitespace) => Err(format!("/{} takes one {}, not “{arg}”", spec.name, what.to_lowercase())),
        _ => match spec.name {
            "add" => Ok(Command::Add(arg.trim_start_matches('@').to_string())),
            "assign" => Ok(Command::Assign(arg.trim_start_matches('@').to_string())),
            "ask" => Ok(Command::Ask(arg.to_string())),
            "deploy" => Ok(Command::Deploy(arg.to_ascii_lowercase())),
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
        "assign" => "sam",
        "ask" => "Hold at cone 6 or 5?",
        "deploy" => "test",
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

pub(crate) fn act_of(text: &str, here: Here) -> Act {
    match read_slash(text, here) {
        None => Act::Say,
        Some(Ok(c)) => Act::Run(c),
        Some(Err(why)) => Act::Refuse(why),
    }
}

// -- the token (APP-90) ------------------------------------------------------------

/// A command typed with its space and nothing after it (`/assign `), that
/// takes an argument and is offered here: the token it becomes.
pub(crate) fn token_of(text: &str, here: Here) -> Option<&'static Spec> {
    let s = slash_of(text)?;
    if !s.spaced || !s.rest.is_empty() {
        return None;
    }
    spec_for(&s.word).filter(|c| c.arg.is_some() && !c.agent && here.offers(c))
}

/// APP-90: the project a ticket's `/deploy` queues — its shortcode's root,
/// the outermost of its address (`hypermemetic:MENGER/APP/APP-90` is
/// MENGER; a project is a shortcode, DEPLOY-11); `None` for no address.
pub(crate) fn project_of(address: &str) -> Option<String> {
    let a = menger_tickets_api::Address::parse(address).ok()?;
    Some(a.hosts.first().unwrap_or(&a.code).as_str().to_string())
}

/// The line a send runs: the token's command and the box's text as its
/// argument, or the text alone.
pub(crate) fn line_of(token: Option<&Spec>, text: &str) -> String {
    match token {
        Some(c) => format!("/{} {}", c.name, text.trim()),
        None => text.to_string(),
    }
}

/// Backspace (`chat.untoken`, from `bindings::hit` in [`Context::Composer`])
/// in an empty box with a token takes the token off.
pub(crate) fn takes_token_off(id: Option<&str>, empty: bool, token: bool) -> bool {
    id == Some("chat.untoken") && empty && token
}

/// The box's placeholder while a token waits: what it takes, and an
/// example (a phone's: the example alone).
pub(crate) fn token_placeholder(c: &Spec, touch: bool) -> String {
    let what = c.arg.unwrap_or("text").to_lowercase();
    match touch {
        true => format!("e.g. {}", example(c.name)),
        false => format!("{what} — e.g. {} · Enter runs /{} · ⌫ takes it off", example(c.name), c.name),
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
    resolve_as("add", name, cands, already, ticket, namer)
}

/// APP-90: `/assign WHO` — the one principal `name` means, as `/add`
/// resolves it, who can read the ticket.
pub(crate) fn resolve_assign(name: &str, cands: &[(String, bool)], ticket: &str, namer: impl Fn(&str) -> String) -> Result<String, String> {
    resolve_as("assign", name, cands, &[], ticket, namer)
}

fn resolve_as(verb: &str, name: &str, cands: &[(String, bool)], already: &[String], ticket: &str, namer: impl Fn(&str) -> String) -> Result<String, String> {
    let n = name.trim().trim_start_matches('@').to_lowercase();
    let hits: Vec<&(String, bool)> = cands
        .iter()
        .filter(|(p, _)| {
            let p_l = p.to_lowercase();
            namer(p).to_lowercase() == n || p_l == n || p_l.strip_prefix("agent:") == Some(n.as_str())
        })
        .collect();
    match hits.as_slice() {
        [] => Err(format!("/{verb} {name} — no one called {name} holds a grant here")),
        [(p, _)] if already.contains(p) => Err(format!("/{verb} {name} — {name} is in this chat already")),
        [(_, false)] if ticket.is_empty() => Err(format!("/{verb} {name} — {name} holds no grant here")),
        [(_, false)] => Err(format!("/{verb} {name} — {name} can't read {ticket}")),
        [(p, true)] => Ok(p.clone()),
        _ => Err(format!("/{verb} {name} — more than one goes by {name}: pick from @'s menu")),
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

/// The box's placeholder — CHAT-14: a chat outside tickets (`None`) is
/// "the chat". CHAT-29: on a phone's touch screen it is one word — the
/// rest are keyboard hints, and the box is one line.
pub(crate) fn placeholder(ticket: Option<&str>, alone: bool, touch: bool) -> String {
    if touch {
        return "message".to_string();
    }
    match (ticket, alone) {
        (Some(t), true) => format!("Write a note on {t}, or @ someone… (Enter sends · Shift+Enter newline · / commands)"),
        (Some(t), false) => format!("message {t}'s chat… (Enter sends · Shift+Enter newline · / commands · @ adds)"),
        (None, _) => "message the chat… (Enter sends · Shift+Enter newline · / commands · paste an image)".to_string(),
    }
}

/// CHAT-29: a phone's touch screen — the stylesheet's own query for the
/// one-line composer (index.html), asked once as the box mounts.
pub(crate) const TOUCH_PHONE: &str = "(max-width: 820px) and (pointer: coarse)";

fn touch_phone() -> bool {
    crate::dom::window().match_media(TOUCH_PHONE).ok().flatten().is_some_and(|m| m.matches())
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
    /// CHAT-14: none for a direct message or a group.
    pub(crate) ticket: Option<String>,
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
    // CHAT-30: a send asked for while an image was on its way.
    let queued = RwSignal::new(false);
    let refusal = RwSignal::new(None::<String>);
    // APP-90: what a command that ran did, and the token waiting for its
    // argument.
    let done = RwSignal::new(None::<String>);
    let tok = RwSignal::new(None::<&'static Spec>);
    let ticketed = ticket.is_some();
    let account = crate::signin::account();
    let here_of = {
        let account = account.clone();
        move |agent: bool| Here { agent, ticket: ticketed, admin: account.with_untracked(|a| a.admin_everywhere) }
    };
    let caret = RwSignal::new(0usize);
    let hi = RwSignal::new(0usize);
    let menu_shut = RwSignal::new(false);
    let cands = RwSignal::new(None::<Vec<(String, bool)>>);
    let area = NodeRef::<Textarea>::new();
    // CHAT-14: images, as a comment takes them (COMMENT-5), for this chat.
    let attach = crate::media::Attach::for_chat(body, area, id);
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
    // APP-93: the box takes the card's whole row once its text wraps.
    crate::grow::wrap(area, move || (body.track(), tok.track()));
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
                if let Ok(c) = chats::candidates(t.as_deref()).await {
                    cands.try_set(Some(c));
                }
            });
        }
    };
    // The menu: `/` commands, or `@` candidates.
    let rows = {
        let name = name.clone();
        let ask_cands = ask_cands.clone();
        let (here_of, account) = (here_of.clone(), account.clone());
        Memo::new(move |_| {
            if menu_shut.get() || tok.get().is_some() {
                return Vec::new();
            }
            let text = body.get();
            if let Some(s) = slash_of(&text) {
                let agent = participants.with(|p| p.iter().any(|p| chats::is_agent(p)));
                account.track();
                return menu_for(&s, here_of(agent)).into_iter().map(Row::Cmd).collect();
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
    let send = {
        let here_of = here_of.clone();
        let me = me.clone();
        let t = ticket.clone();
        let name = name.clone();
        let add_one = add_one.clone();
        move || {
            // APP-90: a token's command runs with the box's text as its argument.
            let text = line_of(tok.get_untracked(), &body.get_untracked());
            if sending.get_untracked() {
                return;
            }
            // APP-70: a drawing on the pad is put first, as an image.
            attach.flush();
            // CHAT-30: an image on its way — the send goes when it is up.
            if attach.busy_untracked() {
                queued.set(true);
                return;
            }
            queued.set(false);
            if text.trim().is_empty() {
                return;
            }
            done.set(None);
            match act_of(&text, here_of(false)) {
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
                        let r = run(cmd, id, t.as_deref(), &me, &participants.get_untracked(), &name, add_one).await;
                        sending.try_set(false);
                        match r {
                            Ok(note) => {
                                body.try_set(String::new());
                                tok.try_set(None);
                                Drafts::clear(&mut Local, id);
                                refusal.try_set(None);
                                done.try_set(note);
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
    let pick = {
        let name = name.clone();
        let add_one = add_one.clone();
        let send = send.clone();
        move |row: Row| match row {
            // APP-90: one that takes an argument becomes the token that waits
            // for it; the box holds only the argument.
            Row::Cmd(spec) if spec.arg.is_some() => {
                tok.set(Some(spec));
                set_text(String::new(), Some(0));
            }
            Row::Cmd(spec) => {
                let text = format!("/{}{}", spec.name, if spec.arg.is_some() { " " } else { "" });
                let n = text.chars().count();
                set_text(text, Some(n));
                // APP-83: a command that takes nothing acts the moment it is
                // picked (/clear, /stop, /status, /help go to the agent).
                if spec.arg.is_none() {
                    menu_shut.set(true);
                    send();
                }
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
    // CHAT-30: … and it goes once the image is up (never without it: a
    // failed one waits for *retry* or *remove*).
    Effect::new({
        let send = send.clone();
        move |_| {
            if queued.get() && !attach.busy() {
                send();
            }
        }
    });
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
            let hit = bindings::hit(&ev, Context::Composer);
            if takes_token_off(hit, empty, tok.get_untracked().is_some()) {
                ev.prevent_default();
                if let Some(c) = tok.get_untracked() {
                    tok.set(None);
                    menu_shut.set(true);
                    let word = format!("/{}", c.name);
                    let n = word.chars().count();
                    set_text(word, Some(n));
                }
                return;
            }
            match key_act(hit, empty, false) {
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
    // CHAT-27: where a touch on Send landed, until it lifts.
    let tap = StoredValue::new(None::<(f64, f64, f64)>);
    let paste = move |ev: web_sys::ClipboardEvent| attach.paste(ev);
    let drop = move |ev: web_sys::DragEvent| attach.dropped(ev);
    let dragover = move |ev: web_sys::DragEvent| attach.dragover(ev);
    let alone = Memo::new(move |_| participants.with(|p| p.len() <= 1));
    let t_ph = ticket.clone();
    let touch = touch_phone();
    let hint = move || {
        if let Some(c) = tok.get() {
            format!("Enter runs /{} · it never goes as prose · ⌫ in the empty box takes it off", c.name)
        } else if rows.with(|r| !r.is_empty()) {
            "↑↓ moves · Tab completes · Enter runs it · Esc closes · nothing here is sent as prose".to_string()
        } else if alone.get() {
            "Enter sends · Shift+Enter newline · / commands · @ brings someone in".to_string()
        } else {
            "Enter sends · Shift+Enter newline · / commands · @ adds".to_string()
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
    // `#/chats` has no dock to fold the chat into (CHAT-14).
    let on_ticket = ticket.is_some() && !crate::chat_full::in_inbox();
    view! {
        <div class="ccompose" class:full=full>
            {move || refusal.get().map(|r| view! {
                <div class="cstrip refuse" role="alert"><span>{if r.starts_with("not ") { r.clone() } else { format!("not sent · {r}") }}</span>
                    <button type="button" class="cx" aria-label="dismiss" on:click=move |_| refusal.set(None)>"×"</button></div>
            })}
            {move || done.get().map(|d| view! {
                <div class="cstrip done" role="status"><span>{d}</span>
                    <button type="button" class="cx" aria-label="dismiss" on:click=move |_| done.set(None)>"×"</button></div>
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
                <crate::draw::PadView a=attach />
                {move || tok.get().map(|c| view! {
                    <span class="ctok"><span class="tn">{format!("/{}", c.name)}</span>
                        <button type="button" class="x" aria-label="take the command off" title="take the command off"
                            on:mousedown=|ev: web_sys::MouseEvent| ev.prevent_default()
                            on:click=move |_| tok.set(None)>"×"</button></span>
                })}
                <textarea
                    node_ref=area
                    class="cbox grow"
                    rows=if touch { "1" } else { "2" }
                    aria-label=match &ticket { Some(t) => format!("message the chat on {t}"), None => "message the chat".to_string() }
                    placeholder=move || match tok.get() {
                        Some(c) => token_placeholder(c, touch),
                        None => placeholder(t_ph.as_deref(), alone.get(), touch),
                    }
                    prop:value=move || body.get()
                    on:input=move |ev| {
                        menu_shut.set(false);
                        if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok()) {
                            caret.set(el.selection_start().ok().flatten().unwrap_or(0) as usize);
                        }
                        let text = event_target_value(&ev);
                        // APP-90: `/assign ` typed by hand becomes its token too.
                        match token_of(&text, here_of(false)).filter(|_| tok.get_untracked().is_none()) {
                            Some(c) => {
                                tok.set(Some(c));
                                body.set(String::new());
                            }
                            None => body.set(text),
                        }
                    }
                    on:click=move |ev| {
                        if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok()) {
                            caret.set(el.selection_start().ok().flatten().unwrap_or(0) as usize);
                        }
                    }
                    on:keydown=keydown
                    on:paste=paste
                    on:drop=drop
                    on:dragover=dragover
                ></textarea>
                <div class="crow">
                    <span class="chint">{hint}</span>
                    <crate::media::Attachments a=attach />
                    {on_ticket.then(|| view! {
                        <button type="button" class="cfull" title=if full { "back to the dock" } else { "the chat full screen (g c)" } on:click=expand>
                            {if full { "⤡ pane" } else { "full ↗" }}
                        </button>
                    })}
                    <button type="button" class="csend" aria-label="send" title="send (Enter)" disabled=move || sending.get() || queued.get() || (!attach.busy() && !attach.drawn() && body.with(|b| b.trim().is_empty()))
                        class:queued=move || queued.get()
                        on:mousedown=|ev: web_sys::MouseEvent| ev.prevent_default()
                        on:touchstart=move |ev: web_sys::TouchEvent| tap.set_value(landed(&ev))
                        on:touchend={
                            let send = send.clone();
                            move |ev: web_sys::TouchEvent| {
                                if tapped(&ev, tap.get_value()) {
                                    ev.prevent_default();
                                    send();
                                }
                                tap.set_value(None);
                            }
                        }
                        on:click=move |_| send()>
                        {move || send_label(sending.get(), queued.get(), attach.failed())}
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

/// What Send says (CHAT-30): sending, waiting for an image to be up (or,
/// one having failed, for its *retry* or *remove*), or ready.
pub(crate) fn send_label(sending: bool, queued: bool, failed: bool) -> &'static str {
    match (sending, queued, failed) {
        (true, _, _) => "sending…",
        (false, true, false) => "sends when the image is up",
        (false, true, true) => "waits for the image",
        _ => "send",
    }
}

/// CHAT-27: a tap on Send keeps the box focused. A phone's first tap on the
/// button used to blur the box: the keyboard went down, the composer moved
/// with it (APP-56's `data-kb` came off), and the click the tap was meant
/// to make never sent. So the button never takes focus (its `mousedown` is
/// prevented), and a tap sends on its lift — prevented, so no mouse events,
/// no blur and no click follow — when it lifts on the button where it
/// landed ([`tap_sends`]). The keyboard stays up, the box empties and keeps
/// its focus for the next message.
fn landed(ev: &web_sys::TouchEvent) -> Option<(f64, f64, f64)> {
    let t = ev.touches();
    (t.length() == 1).then(|| t.get(0)).flatten().map(|p| (f64::from(p.client_x()), f64::from(p.client_y()), js_sys::Date::now()))
}

/// Whether the touch that lifts in `ev`, landed at `from`, is a tap on Send.
fn tapped(ev: &web_sys::TouchEvent, from: Option<(f64, f64, f64)>) -> bool {
    let Some(from) = from else { return false };
    let Some(p) = ev.changed_touches().get(0) else { return false };
    let Some(button) = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return false };
    let r = button.get_bounding_client_rect();
    tap_sends(from, (f64::from(p.client_x()), f64::from(p.client_y()), js_sys::Date::now()), (r.left(), r.top(), r.right(), r.bottom()))
}

/// CHAT-27: a touch that landed at `from` and lifted at `to` (x, y, ms) on
/// a button whose rect is `(left, top, right, bottom)` sends: a tap
/// ([`crate::media::is_tap`]), lifted on the button. A scroll, a hold, or a
/// finger slid off the button does not.
pub(crate) fn tap_sends(from: (f64, f64, f64), to: (f64, f64, f64), rect: (f64, f64, f64, f64)) -> bool {
    let (l, t, r, b) = rect;
    crate::media::is_tap(from.0, from.1, from.2, to.0, to.1, to.2) && (l..=r).contains(&to.0) && (t..=b).contains(&to.1)
}

/// A command, run — and what it did, said in the strip over the card
/// (APP-90), when it says more than the chat itself shows.
async fn run(cmd: Command, id: u64, ticket: Option<&str>, _me: &str, already: &[String], name: &impl Fn(&str) -> String, add_one: impl Fn(String)) -> Result<Option<String>, String> {
    let on = || ticket.ok_or_else(|| "this works only in a chat on a ticket".to_string());
    match cmd {
        Command::Close => chats::settle_state(id, false).await.map(|_| None).map_err(|e| format!("/close — {e}")),
        Command::Title(t) => chats::rename(id, Some(&t)).await.map(|_| None).map_err(|e| format!("/title — {e}")),
        Command::Ticket(code) => chats::post(id, &format!("{code}\n")).await.map(|_| None),
        Command::Add(who) => {
            let cands = chats::candidates(ticket).await?;
            let p = resolve_add(&who, &cands, already, ticket.unwrap_or(""), name)?;
            add_one(p);
            Ok(None)
        }
        // APP-90: resolved as /add resolves, then `tickets.assign`.
        Command::Assign(who) => {
            let t = on()?;
            let cands = chats::candidates(Some(t)).await?;
            let p = resolve_assign(&who, &cands, t, name)?;
            crate::assignees::assign(t.to_string(), vec![p.clone()], Vec::new()).await.map_err(|e| format!("/assign {who} — {e}"))?;
            Ok(Some(format!("assigned {t} to {}", name(&p))))
        }
        // APP-90: `deployments.request` for the ticket's project — offered
        // to an admin on `*` alone; the server's gate is the deployer's.
        Command::Deploy(env) => {
            let t = on()?;
            let address = match crate::cache::get(t).map(|r| r.with_untracked(|r| r.address.clone())).filter(|a| !a.is_empty()) {
                Some(a) => a,
                None => crate::saved::get_doc(t).await?.map(|d| d.address).unwrap_or_default(),
            };
            let project = project_of(&address).ok_or_else(|| format!("/deploy {env} — {t}'s project is not known"))?;
            let d = crate::deploys_page::queue(project.clone(), env.clone(), String::new(), format!("/deploy from the chat on {t}")).await.map_err(|e| format!("/deploy {env} — {e}"))?;
            Ok(Some(format!("deploy of {project} to {env} queued — request {} on #/deployments", d.id)))
        }
        // APP-90: `tickets.inputs.ask` — a text input on the ticket.
        Command::Ask(q) => {
            let t = on()?;
            let input = crate::inputs::ask(t, &q).await.map_err(|e| format!("/ask — {e}"))?;
            Ok(Some(format!("asked on {t} · input {input}, under Needs you")))
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
    const ON: Here = Here { agent: false, ticket: true, admin: false };
    const OFF: Here = Here { agent: false, ticket: false, admin: false };
    const OWNER: Here = Here { agent: false, ticket: true, admin: true };

    fn act_of_t(text: &str) -> Act {
        act_of(text, ON)
    }

    #[test]
    fn every_command_row() {
        let s = slash_of("/").unwrap();
        assert_eq!(menu_for(&s, Here { agent: false, ticket: false, admin: false }).len(), 4, "a bare / offers them all");
        assert_eq!(menu_for(&s, Here { agent: false, ticket: true, admin: false }).len(), 6, "APP-90: on a ticket, /assign and /ask too");
        assert_eq!(menu_for(&s, Here { agent: true, ticket: true, admin: false }).len(), 10, "with an agent in the chat, its commands too (APP-83)");
        assert_eq!(menu_for(&slash_of("/t").unwrap(), Here { agent: false, ticket: true, admin: false }).iter().map(|c| c.name).collect::<Vec<_>>(), ["ticket", "title"]);
        assert_eq!(menu_for(&slash_of("/a").unwrap(), Here { agent: false, ticket: true, admin: false }).iter().map(|c| c.name).collect::<Vec<_>>(), ["add", "assign", "ask"]);
        assert_eq!(menu_for(&slash_of("/a").unwrap(), Here { agent: false, ticket: false, admin: false }).iter().map(|c| c.name).collect::<Vec<_>>(), ["add"], "outside a ticket, /add alone");
        assert_eq!(menu_for(&slash_of("/s").unwrap(), Here { agent: true, ticket: true, admin: false }).iter().map(|c| c.name).collect::<Vec<_>>(), ["status", "stop"]);
        assert_eq!(menu_for(&slash_of("/s").unwrap(), Here { agent: false, ticket: true, admin: false }).len(), 0);
        assert_eq!(menu_for(&slash_of("/title x").unwrap(), Here { agent: false, ticket: true, admin: false }).len(), 1);
        assert_eq!(menu_for(&slash_of("/ask x").unwrap(), Here { agent: false, ticket: false, admin: false }).len(), 0, "the ticket's command outside a ticket");
        assert!(slash_of("/a\nb").is_none(), "two lines are prose");
        assert!(!menu_settled("/ad"), "the word being typed: the menu's keys");
        assert!(menu_settled("/add sam"), "its argument being typed: Enter runs it");
        assert!(slash_of("not / a command").is_none());
        assert_eq!(act_of_t("hello"), Act::Say);
        assert_eq!(act_of_t("/add reviewer"), Act::Run(Command::Add("reviewer".into())));
        assert_eq!(act_of_t("/add @rui"), Act::Run(Command::Add("rui".into())));
        assert_eq!(act_of_t("/close"), Act::Run(Command::Close));
        assert_eq!(act_of_t("/title The hold at 1240 °C"), Act::Run(Command::Title("The hold at 1240 °C".into())));
        assert_eq!(act_of_t("/ticket kiln-3"), Act::Run(Command::Ticket("KILN-3".into())));
        // APP-83: the agent's commands are the message — the bridge answers.
        for c in ["/clear", "/stop", "/status", "/help", " /clear "] {
            assert_eq!(act_of(c, ON), Act::Say, "{c}");
        }
        let refused = |t: &str| match act_of(t, ON) {
            Act::Refuse(r) => r,
            other => panic!("{t}: {other:?}"),
        };
        assert!(refused("/add").starts_with("/add needs a name"));
        assert!(refused("/add sam rui").contains("takes one name"));
        assert!(refused("/close now").contains("takes nothing"));
        assert!(refused("/ticket shelf").contains("is not one"));
        assert!(refused("/frobnicate").starts_with("/frobnicate is not a command — the commands are /add, /assign, /ask, /ticket, /title, /close"));
        // APP-90: /assign takes one name; /ask a question, spaces and all.
        assert_eq!(act_of_t("/assign @sam"), Act::Run(Command::Assign("sam".into())));
        assert_eq!(act_of_t("/ask Hold at cone 6 or 5?"), Act::Run(Command::Ask("Hold at cone 6 or 5?".into())));
        assert!(refused("/assign").starts_with("/assign needs a who"));
        assert!(refused("/assign sam rui").contains("takes one who"));
        assert!(refused("/ask").starts_with("/ask needs a question — e.g. /ask Hold at cone 6 or 5?"));
        // Outside a ticket they are not offered, and not run.
        assert_eq!(act_of("/assign sam", OFF), Act::Refuse("/assign works only in a chat on a ticket".into()));
        assert_eq!(act_of("/ask why?", OFF), Act::Refuse("/ask works only in a chat on a ticket".into()));
        match act_of("/nope", OFF) {
            Act::Refuse(r) => assert!(r.ends_with("the commands are /add, /ticket, /title, /close"), "{r}"),
            other => panic!("{other:?}"),
        }
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

    /// APP-90: `/deploy ENV` is the owner's — offered, and run, only for an
    /// admin on `*` in a chat on a ticket; it queues the ticket's root
    /// project.
    #[test]
    fn deploy_is_offered_to_the_owner_alone() {
        let names = |here| menu_for(&slash_of("/d").unwrap(), here).iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(OWNER), ["deploy"]);
        assert!(names(ON).is_empty(), "an editor is not offered it");
        assert!(names(Here { admin: true, ..OFF }).is_empty(), "outside a ticket, not even the owner");
        assert_eq!(menu_for(&slash_of("/").unwrap(), OWNER).len(), 7);
        assert_eq!(act_of("/deploy TEST", OWNER), Act::Run(Command::Deploy("test".into())));
        assert_eq!(act_of("/deploy test", ON), Act::Refuse("/deploy is the owner's: only an admin of every project queues a deploy".into()));
        assert!(matches!(act_of("/deploy test prod", OWNER), Act::Refuse(r) if r.contains("takes one env")));
        assert!(matches!(act_of("/deploy", OWNER), Act::Refuse(r) if r.starts_with("/deploy needs an env — e.g. /deploy test")));
        assert_eq!(token_of("/deploy ", OWNER).map(|c| c.name), Some("deploy"));
        assert_eq!(token_of("/deploy ", ON), None);
        assert_eq!(project_of("hypermemetic:MENGER/APP/APP-90").as_deref(), Some("MENGER"), "the root of its shortcode");
        assert_eq!(project_of("hypermemetic:MENGER-5").as_deref(), Some("MENGER"), "a root's own ticket");
        assert_eq!(project_of(""), None);
    }

    /// APP-90: `/assign WHO` resolves as `/add` does, saying `/assign`.
    #[test]
    fn assign_resolves_a_name_as_add_does() {
        let cands = vec![("agent:reviewer".to_string(), true), ("idp:sam".to_string(), false), ("idp:rui".to_string(), true)];
        let namer = |p: &str| p.rsplit(':').next().unwrap().to_string();
        assert_eq!(resolve_assign("@rui", &cands, "KILN-7", namer), Ok("idp:rui".into()));
        assert_eq!(resolve_assign("reviewer", &cands, "KILN-7", namer), Ok("agent:reviewer".into()));
        assert_eq!(resolve_assign("sam", &cands, "KILN-7", namer), Err("/assign sam — sam can't read KILN-7".into()));
        assert!(resolve_assign("zed", &cands, "KILN-7", namer).unwrap_err().starts_with("/assign zed — no one called zed"));
    }

    /// APP-90 (APP-83's mockup 4): a command that takes an argument becomes
    /// a token that waits for it — typed with its space, or picked; the
    /// send runs the token's command with the box's text; Backspace in an
    /// empty box takes it off.
    #[test]
    fn a_command_with_an_argument_becomes_a_token() {
        let name = |t: &str, on: bool| token_of(t, if on { ON } else { OFF }).map(|c| c.name);
        assert_eq!(name("/assign ", true), Some("assign"));
        assert_eq!(name("/ask ", true), Some("ask"));
        assert_eq!(name("/add ", false), Some("add"));
        assert_eq!(name("/title ", false), Some("title"));
        assert_eq!(name("/ask ", false), None, "the ticket's command outside a ticket");
        assert_eq!(name("/assign", true), None, "the word still being typed");
        assert_eq!(name("/assign sam", true), None, "its argument typed already");
        assert_eq!(name("/close ", true), None, "takes nothing: no token");
        assert_eq!(name("/clear ", true), None, "the agent's: no token");
        assert_eq!(name("/nope ", true), None);
        let assign = spec_for("assign").unwrap();
        assert_eq!(line_of(Some(assign), " sam "), "/assign sam");
        assert_eq!(line_of(None, "hello"), "hello");
        assert_eq!(act_of(&line_of(Some(spec_for("ask").unwrap()), "Hold at 6?"), ON), Act::Run(Command::Ask("Hold at 6?".into())));
        assert!(matches!(act_of(&line_of(Some(assign), ""), ON), Act::Refuse(r) if r.starts_with("/assign needs a who")), "Enter with no argument: refused, saying what it needs");
        let t = table_with(&[]);
        let backspace = pick_single(&t, &press("Backspace"), Context::Composer, false);
        assert!(takes_token_off(backspace, true, true));
        assert!(!takes_token_off(backspace, false, true), "text in the box: Backspace deletes it");
        assert!(!takes_token_off(backspace, true, false), "no token");
        assert!(!takes_token_off(pick_single(&t, &press("Delete"), Context::Composer, false), true, true));
        assert_eq!(key_act(backspace, false, false), KeyAct::Type, "otherwise Backspace is the browser's");
        assert_eq!(token_placeholder(assign, false), "who — e.g. sam · Enter runs /assign · ⌫ takes it off");
        assert_eq!(token_placeholder(spec_for("ask").unwrap(), true), "e.g. Hold at cone 6 or 5?");
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
        assert_eq!(placeholder(Some("KILN-7"), false, false), "message KILN-7's chat… (Enter sends · Shift+Enter newline · / commands · @ adds)");
        assert!(placeholder(Some("KILN-7"), true, false).starts_with("Write a note on KILN-7"));
        assert!(placeholder(None, false, false).contains("paste an image"), "CHAT-14: a chat outside tickets");
        assert_eq!(placeholder(Some("KILN-7"), false, true), "message", "CHAT-29: one word on a phone");
        assert_eq!(placeholder(None, true, true), "message");
    }

    /// CHAT-27: a tap on Send sends on its lift — one that lifts on the
    /// button, where it landed and soon; a scroll, a hold or a slide off it
    /// does not.
    #[test]
    fn chat_27_a_tap_on_send_sends_on_its_lift() {
        let button = (300.0, 600.0, 360.0, 632.0);
        assert!(tap_sends((330.0, 616.0, 0.0), (332.0, 618.0, 120.0), button), "a tap");
        assert!(!tap_sends((330.0, 616.0, 0.0), (330.0, 560.0, 120.0), button), "a scroll");
        assert!(!tap_sends((330.0, 616.0, 0.0), (330.0, 616.0, 900.0), button), "a hold");
        assert!(!tap_sends((302.0, 616.0, 0.0), (296.0, 616.0, 100.0), button), "lifted off the button");
    }

    /// CHAT-30: a send during an upload waits for it, and says so.
    #[test]
    fn chat_30_send_says_it_waits_for_the_image() {
        assert_eq!(send_label(false, false, false), "send");
        assert_eq!(send_label(true, false, false), "sending…");
        assert_eq!(send_label(false, true, false), "sends when the image is up");
        assert_eq!(send_label(false, true, true), "waits for the image", "a failed one: retry or remove");
    }
}
