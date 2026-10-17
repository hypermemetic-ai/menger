//! CHAT-4: the app's copy of the reader's chats (CHAT-3's `chats`
//! activation) — what a ticket page's **chat N** field counts, what the
//! chat pane (src/chat_pane.rs) draws, and, later, the header's list (Fig 5,
//! waiting on CHAT-1's `inbox` decision: [`Held::unread_total`] and
//! [`Held::with_unread`] are its seam).
//!
//! **Asked for by what shows it**, as the comments are (src/comments.rs):
//! nothing at startup asks; a ticket page calls [`want`], and from then on,
//! on every new connection, `chats.watch` (every chat the reader is in) is
//! subscribed and then `chats.list` fetched page by page; between fetches
//! the watch keeps the copy. What the watch hears while a fetch is out is
//! replayed onto its answer ([`settle`]), so no change is lost to it.
//!
//! **Messages** are fetched per chat when the pane first shows it
//! ([`load`]), oldest first, and kept: `posted` on the watch appends to a
//! thread the app holds (once — a write's own answer and its echo are one
//! message by id).
//!
//! **Counts follow live**: `unread` is the server's, per chat, for the
//! reader; `posted` by someone else adds one, the reader's own post clears
//! it (the server moves the poster's mark), and `read` — the reader's,
//! from this app or any other open one — sets it as the server says.
//!
//! What decides — applying an event, the replay, the counts, a chat's
//! words — is pure and pinned by native tests; the rest is the socket.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::vocab::{ChatId, MessageId};
use menger_tickets_api::{Chat, ChatState, ChatsEvent, Message};
use plexus_client::ClientEvent;
use wasm_bindgen_futures::spawn_local;

use crate::session;

/// How many chats one `list` page asks for (the server's most).
const PAGE: u32 = 500;
/// How many messages one `messages` page asks for (the server's most).
const MESSAGES_PAGE: u32 = 1_000;

/// A chat's messages as the app holds them, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Thread {
    pub(crate) list: Vec<Message>,
}

impl Thread {
    /// `m` in its place by id; whether it was new.
    fn take(&mut self, m: &Message) -> bool {
        match self.list.binary_search_by(|x| x.id.get().cmp(&m.id.get())) {
            Ok(_) => false,
            Err(at) => {
                self.list.insert(at, m.clone());
                true
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn last(&self) -> Option<u64> {
        self.list.last().map(|m| m.id.get())
    }
}

/// The reader's chats, by id, and the threads the app has fetched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Held {
    chats: BTreeMap<u64, Chat>,
    threads: HashMap<u64, Thread>,
    /// The name each author went by, from the messages heard.
    names: BTreeMap<String, String>,
}

impl Held {
    /// `list` answers: one `chat` per chat; anything else contributes
    /// nothing.
    pub(crate) fn from_events(events: impl IntoIterator<Item = ChatsEvent>) -> Self {
        let mut held = Held::default();
        for ev in events {
            if let ChatsEvent::Chat { chat } = ev {
                held.chats.insert(chat.id.get(), chat);
            }
        }
        held
    }

    /// Apply a `watch` event (or a write's answer) as `me` hears it;
    /// whether the copy moved.
    pub(crate) fn apply(&mut self, ev: &ChatsEvent, me: &str) -> bool {
        match ev {
            ChatsEvent::Opened { chat, .. } | ChatsEvent::Added { chat } | ChatsEvent::Chat { chat } => {
                let id = chat.id.get();
                if self.chats.get(&id) == Some(chat) {
                    return false;
                }
                self.chats.insert(id, chat.clone());
                true
            }
            ChatsEvent::Posted { message } => self.posted(message, me),
            ChatsEvent::Read { chat, unread, .. } => self.with_chat(chat.get(), |c| {
                let moved = c.unread != *unread;
                c.unread = *unread;
                moved
            }),
            ChatsEvent::Closed { chat } => self.with_chat(chat.get(), |c| std::mem::replace(&mut c.state, ChatState::Closed) != ChatState::Closed),
            ChatsEvent::Reopened { chat } => self.with_chat(chat.get(), |c| std::mem::replace(&mut c.state, ChatState::Open) != ChatState::Open),
            ChatsEvent::Listening { chat, principal, waiting } => self.with_chat(chat.get(), |c| {
                let had = c.waiting.iter().any(|p| p == principal);
                match (*waiting, had) {
                    (true, false) => c.waiting.push(principal.clone()),
                    (false, true) => c.waiting.retain(|p| p != principal),
                    _ => return false,
                }
                true
            }),
            ChatsEvent::Left { chat } => {
                self.threads.remove(&chat.get());
                self.chats.remove(&chat.get()).is_some()
            }
            _ => false,
        }
    }

    fn with_chat(&mut self, id: u64, f: impl FnOnce(&mut Chat) -> bool) -> bool {
        self.chats.get_mut(&id).is_some_and(f)
    }

    /// A message posted: the chat's last, the reader's unread (one more for
    /// another's, none after its own — the server moves the poster's mark),
    /// and its thread, when held. Heard twice (a write's answer, then its
    /// echo), it counts once.
    fn posted(&mut self, m: &Message, me: &str) -> bool {
        if let Some(name) = &m.name {
            self.names.insert(m.author.clone(), name.clone());
        }
        let id = m.chat.get();
        // Kept though the thread is not fetched yet: the fetch merges by id.
        let fresh = self.threads.entry(id).or_default().take(m);
        let Some(c) = self.chats.get_mut(&id) else { return fresh };
        if c.last_message.is_some_and(|l| l.get() >= m.id.get()) {
            return fresh;
        }
        c.last_message = Some(m.id);
        c.last_at = m.at;
        if m.author == me {
            c.unread = 0;
        } else {
            c.unread += 1;
        }
        true
    }

    /// A chat's messages fetched: its thread, every message by id once.
    pub(crate) fn loaded(&mut self, chat: u64, messages: Vec<Message>) {
        let t = self.threads.entry(chat).or_default();
        for m in &messages {
            if let Some(name) = &m.name {
                self.names.insert(m.author.clone(), name.clone());
            }
            t.take(m);
        }
    }

    pub(crate) fn get(&self, id: u64) -> Option<&Chat> {
        self.chats.get(&id)
    }

    pub(crate) fn thread(&self, id: u64) -> Option<&Thread> {
        self.threads.get(&id)
    }

    /// The chats on `ticket` (its key, any case) in `state`, oldest first —
    /// the pane's tabs (open) and its closed list.
    pub(crate) fn on(&self, ticket: &str, state: ChatState) -> Vec<Chat> {
        self.chats.values().filter(|c| c.state == state && c.ticket.eq_ignore_ascii_case(ticket)).cloned().collect()
    }

    /// The reader's unread on `ticket` — the field's N.
    pub(crate) fn unread_on(&self, ticket: &str) -> u32 {
        self.chats.values().filter(|c| c.ticket.eq_ignore_ascii_case(ticket)).map(|c| c.unread).sum()
    }

    /// Fig 5's seam (the header's **chats N**, on CHAT-1's `inbox`
    /// decision): the reader's unread across every ticket.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn unread_total(&self) -> u32 {
        self.chats.values().map(|c| c.unread).sum()
    }

    /// Fig 5's seam: the chats with unread, newest first.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn with_unread(&self) -> Vec<&Chat> {
        let mut v: Vec<&Chat> = self.chats.values().filter(|c| c.unread > 0).collect();
        v.sort_by_key(|c| std::cmp::Reverse(c.last_at));
        v
    }

    /// The open chat on `ticket` whose participants are exactly `me` and
    /// `others` — the one `open` would answer again (`existing`), which the
    /// picker opens instead.
    pub(crate) fn existing(&self, ticket: &str, me: &str, others: &[String]) -> Option<u64> {
        let mut want: Vec<&str> = others.iter().map(String::as_str).chain([me]).collect();
        want.sort_unstable();
        want.dedup();
        self.chats
            .values()
            .find(|c| {
                let mut have: Vec<&str> = c.participants.iter().map(String::as_str).collect();
                have.sort_unstable();
                have.dedup();
                c.state == ChatState::Open && c.ticket.eq_ignore_ascii_case(ticket) && have == want
            })
            .map(|c| c.id.get())
    }

    /// Who is waiting on `await_message` in any of the reader's chats on
    /// `ticket` — the picker's pulsing dots.
    pub(crate) fn waiting_on(&self, ticket: &str) -> Vec<String> {
        let mut v: Vec<String> = self.chats.values().filter(|c| c.ticket.eq_ignore_ascii_case(ticket)).flat_map(|c| c.waiting.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    /// The name `principal` went by, when a message said.
    pub(crate) fn name_of(&self, principal: &str) -> Option<&str> {
        self.names.get(principal).map(String::as_str)
    }
}

/// The fetch's rule: `answer`, then every event heard while it was out, in
/// order — the threads already held kept.
pub(crate) fn settle(answer: Vec<ChatsEvent>, heard: &[ChatsEvent], me: &str, kept: Option<&Held>) -> Held {
    let mut held = Held::from_events(answer);
    if let Some(k) = kept {
        held.threads = k.threads.clone();
        held.names = k.names.clone();
    }
    for ev in heard {
        held.apply(ev, me);
    }
    held
}

/// A principal in few characters: its scheme and the head of its id
/// (`key:4b2e…`), or the whole of a short one (`anonymous`, `agent:builder`).
pub(crate) fn short(principal: &str) -> String {
    match principal.split_once(':') {
        Some((scheme, rest)) if rest.chars().count() > 8 => format!("{scheme}:{}…", rest.chars().take(4).collect::<String>()),
        _ => principal.to_string(),
    }
}

/// Whether `principal` is an agent (AGENT-1's `agent:<name>`): marked in
/// the participants row and on its messages.
pub(crate) fn is_agent(principal: &str) -> bool {
    principal.starts_with("agent:")
}

thread_local! {
    static HELD: ArcRwSignal<Option<Held>> = ArcRwSignal::new(None);
    static WANTED: Cell<bool> = const { Cell::new(false) };
    static FETCH: Cell<u32> = const { Cell::new(0) };
    static HEARD: RefCell<Option<Vec<ChatsEvent>>> = const { RefCell::new(None) };
    static PUMP: RefCell<Option<futures::future::AbortHandle>> = const { RefCell::new(None) };
    /// The threads asked for on this connection.
    static ASKED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// The chats, as a signal — `None` until the first fetch lands.
pub(crate) fn signal() -> ArcRwSignal<Option<Held>> {
    HELD.with(Clone::clone)
}

/// The reader's principal, as a chat names it: the signed-in person's, or
/// `anonymous` with login off.
pub(crate) fn me() -> String {
    crate::signin::account().with_untracked(|a| a.me.as_ref().map(|m| m.principal.clone())).unwrap_or_else(|| "anonymous".into())
}

/// The reader's own name, when signed in with one.
pub(crate) fn my_name() -> Option<String> {
    crate::signin::account().with_untracked(|a| a.me.as_ref().map(|m| m.name.clone())).filter(|n| !n.is_empty())
}

/// A view needs the chats: watch and fetch them now if this is the first ask.
pub(crate) fn want() {
    if !WANTED.with(|w| w.replace(true)) {
        start();
    }
}

/// A new connection: the watch subscribed on it and the chats fetched
/// again — if anything wants them.
pub(crate) fn refresh() {
    if WANTED.with(Cell::get) {
        ASKED.with(|a| a.borrow_mut().clear());
        start();
    }
}

fn start() {
    let Some(client) = session::chats_client() else { return };
    let (abort, reg) = futures::future::AbortHandle::new_pair();
    if let Some(old) = PUMP.with(|p| p.borrow_mut().replace(abort)) {
        old.abort();
    }
    let pump = async move {
        let mut stream = client.watch(None);
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(ClientEvent::Data(ChatsEvent::Lagged { missed })) => {
                    leptos::logging::log!("chats watch lagged: {missed} change(s) missed; fetching again");
                    fetch();
                }
                Ok(ClientEvent::Data(ev)) => heard(&ev),
                Ok(_) => {}
                Err(_) => break,
            }
        }
    };
    spawn_local(async move {
        let _ = futures::future::Abortable::new(pump, reg).await;
    });
    fetch();
}

/// A `watch` event (or a write's answer): the copy takes it.
fn heard(ev: &ChatsEvent) {
    HEARD.with(|h| {
        if let Some(buf) = h.borrow_mut().as_mut() {
            buf.push(ev.clone());
        }
    });
    let me = me();
    HELD.with(|s| {
        s.maybe_update(|held| held.as_mut().is_some_and(|held| held.apply(ev, &me)));
    });
}

fn fetch() {
    let Some(client) = session::chats_client() else { return };
    let n = FETCH.with(|f| {
        let n = f.get().wrapping_add(1);
        f.set(n);
        n
    });
    HEARD.with(|h| *h.borrow_mut() = Some(Vec::new()));
    spawn_local(async move {
        let mut answer = Vec::new();
        let mut after = None;
        loop {
            match client.list_all(None, None, None, after, Some(PAGE)).await {
                Ok(events) => {
                    let mut next = None;
                    for ev in events {
                        match ev {
                            ChatsEvent::Page { next: n } => next = n,
                            ChatsEvent::Error { message } => leptos::logging::log!("chats: {message}"),
                            ev => answer.push(ev),
                        }
                    }
                    match next {
                        Some(n) => after = Some(ChatId::new(n)),
                        None => break,
                    }
                }
                Err(e) => {
                    leptos::logging::log!("chats: {}", session::describe(&e));
                    return;
                }
            }
        }
        if FETCH.with(Cell::get) != n {
            return;
        }
        let heard = HEARD.with(|h| h.borrow_mut().take()).unwrap_or_default();
        let me = me();
        HELD.with(|s| {
            let kept = s.get_untracked();
            s.set(Some(settle(answer, &heard, &me, kept.as_ref())));
        });
    });
}

/// The pane shows `chat`: its messages fetched, once per connection.
pub(crate) fn load(chat: u64) {
    if ASKED.with(|a| a.borrow().contains(&chat)) {
        return;
    }
    let Some(client) = session::chats_client() else { return };
    ASKED.with(|a| a.borrow_mut().push(chat));
    spawn_local(async move {
        let mut all = Vec::new();
        let mut after = None;
        loop {
            let Ok(events) = client.messages_all(ChatId::new(chat), after, Some(MESSAGES_PAGE)).await else {
                ASKED.with(|a| a.borrow_mut().retain(|c| *c != chat));
                return;
            };
            let mut next = None;
            for ev in events {
                match ev {
                    ChatsEvent::Message { message } => all.push(message),
                    ChatsEvent::Page { next: n } => next = n,
                    _ => {}
                }
            }
            match next {
                Some(n) => after = Some(MessageId::new(n)),
                None => break,
            }
        }
        HELD.with(|s| s.update(|h| h.get_or_insert_with(Held::default).loaded(chat, all)));
    });
}

/// The one answer a write gives, applied at once — or what the server said
/// instead.
fn answer_of(what: &str, events: Result<Vec<ChatsEvent>, plexus_client::ClientError>) -> Result<ChatsEvent, String> {
    let events = events.map_err(|e| format!("{what}: {}", session::describe(&e)))?;
    for ev in events {
        match ev {
            ChatsEvent::Error { message } => return Err(message),
            ChatsEvent::Refused { reasons } => return Err(reasons.join("; ")),
            ChatsEvent::NotFound { chat } => return Err(format!("chat {chat} is not one you are in")),
            ev => {
                heard(&ev);
                return Ok(ev);
            }
        }
    }
    Err(format!("{what}: no answer"))
}

fn client() -> Result<menger_tickets_api::ChatsClient<std::rc::Rc<plexus_client::WsTransport>>, String> {
    session::chats_client().ok_or_else(|| "not connected — nothing was sent".to_string())
}

/// `chats.open { ticket, others }` — the chat's id (the one already open
/// among the same people, when there is one).
pub(crate) async fn open(ticket: &str, others: &[String]) -> Result<u64, String> {
    let c = client()?;
    let ev = answer_of("open", c.open_all(crate::typed::typed(ticket)?, Some(crate::typed::many(others)?), None).await)?;
    match ev {
        ChatsEvent::Opened { chat, .. } => Ok(chat.id.get()),
        _ => Err("open: no chat".into()),
    }
}

/// `chats.post { chat, body }`.
pub(crate) async fn post(chat: u64, body: &str) -> Result<(), String> {
    answer_of("post", client()?.post_all(ChatId::new(chat), crate::typed::typed(body)?).await).map(|_| ())
}

/// `chats.add { chat, principal }`.
pub(crate) async fn add(chat: u64, principal: &str) -> Result<(), String> {
    answer_of("add", client()?.add_all(ChatId::new(chat), crate::typed::typed(principal)?).await).map(|_| ())
}

/// `chats.read { chat, upto }`.
pub(crate) async fn read(chat: u64, upto: u64) -> Result<(), String> {
    answer_of("read", client()?.read_all(ChatId::new(chat), MessageId::new(upto)).await).map(|_| ())
}

/// `chats.close` (`open: false`) or `chats.reopen` (`open: true`).
pub(crate) async fn settle_state(chat: u64, open: bool) -> Result<(), String> {
    let c = client()?;
    let events = if open { c.reopen_all(ChatId::new(chat)).await } else { c.close_all(ChatId::new(chat)).await };
    answer_of(if open { "reopen" } else { "close" }, events).map(|_| ())
}

/// `chats.candidates { ticket }` — each principal and whether it reads the
/// ticket.
pub(crate) async fn candidates(ticket: &str) -> Result<Vec<(String, bool)>, String> {
    let events = client()?.candidates_all(crate::typed::typed(ticket)?).await.map_err(|e| session::describe(&e))?;
    let mut out = Vec::new();
    for ev in events {
        match ev {
            ChatsEvent::Candidate { principal, reads } => out.push((principal, reads)),
            ChatsEvent::Refused { reasons } => return Err(reasons.join("; ")),
            ChatsEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn chat(id: u64, ticket: &str, who: &[&str]) -> Chat {
        Chat {
            id: ChatId::new(id),
            ticket: ticket.into(),
            title: None,
            participants: who.iter().map(|w| w.to_string()).collect(),
            state: ChatState::Open,
            opened_by: who[0].into(),
            opened_at: 100,
            last_at: 100,
            last_message: None,
            unread: 0,
            waiting: Vec::new(),
        }
    }

    pub(crate) fn msg(id: u64, chat: u64, author: &str, body: &str) -> Message {
        Message { id: MessageId::new(id), chat: ChatId::new(chat), author: author.into(), name: None, via: None, at: 100 + id as i64, body: body.into() }
    }

    /// Reading row: a message by another adds one to the reader's unread,
    /// its own clears it, `read` sets it as the server says, and an answer
    /// heard twice counts once. The field and the tab follow it.
    #[test]
    fn unread_follows_posts_and_reads() {
        let mut h = Held::from_events([ChatsEvent::Chat { chat: chat(41, "KILN-7", &["ada", "builder"]) }, ChatsEvent::Chat { chat: chat(42, "KILN-7", &["ada", "reviewer"]) }]);
        assert_eq!(h.unread_on("kiln-7"), 0);
        let m = msg(1, 41, "builder", "The test.");
        assert!(h.apply(&ChatsEvent::Posted { message: m.clone() }, "ada"));
        h.apply(&ChatsEvent::Posted { message: m }, "ada");
        assert_eq!(h.get(41).unwrap().unread, 1, "heard twice, counted once");
        h.apply(&ChatsEvent::Posted { message: msg(2, 42, "reviewer", "?") }, "ada");
        assert_eq!((h.unread_on("KILN-7"), h.unread_total()), (2, 2));
        h.apply(&ChatsEvent::Read { chat: ChatId::new(41), upto: MessageId::new(1), unread: 0 }, "ada");
        assert_eq!(h.unread_on("KILN-7"), 1, "read in another open app: the count follows");
        h.apply(&ChatsEvent::Posted { message: msg(3, 42, "ada", "1240 is allowed.") }, "ada");
        assert_eq!(h.get(42).unwrap().unread, 0, "the poster's mark moves to its post");
        assert_eq!(h.get(42).unwrap().last_message.map(|m| m.get()), Some(3));
        assert!(h.with_unread().is_empty());
    }

    /// The pane's tabs: the open chats on the ticket, oldest first; closing
    /// moves one to the closed list, reopening back. A chat whose ticket
    /// left the reader's reach goes.
    #[test]
    fn tabs_and_the_closed_list() {
        let mut h = Held::from_events([
            ChatsEvent::Chat { chat: chat(42, "KILN-7", &["ada", "reviewer"]) },
            ChatsEvent::Chat { chat: chat(41, "KILN-7", &["ada", "builder"]) },
            ChatsEvent::Chat { chat: chat(9, "KILN-4", &["ada", "builder"]) },
        ]);
        let ids = |v: Vec<Chat>| v.iter().map(|c| c.id.get()).collect::<Vec<_>>();
        assert_eq!(ids(h.on("KILN-7", ChatState::Open)), [41, 42]);
        assert!(h.apply(&ChatsEvent::Closed { chat: ChatId::new(41) }, "ada"));
        assert!(!h.apply(&ChatsEvent::Closed { chat: ChatId::new(41) }, "ada"), "closed again changes nothing");
        assert_eq!((ids(h.on("KILN-7", ChatState::Open)), ids(h.on("KILN-7", ChatState::Closed))), (vec![42], vec![41]));
        h.apply(&ChatsEvent::Reopened { chat: ChatId::new(41) }, "ada");
        assert_eq!(ids(h.on("KILN-7", ChatState::Open)), [41, 42]);
        h.apply(&ChatsEvent::Left { chat: ChatId::new(42) }, "ada");
        assert_eq!(ids(h.on("KILN-7", ChatState::Open)), [41]);
    }

    /// The presence dots: `listening` adds and takes away who waits.
    #[test]
    fn listening_moves_the_waiting() {
        let mut h = Held::from_events([ChatsEvent::Chat { chat: chat(41, "KILN-7", &["ada", "builder"]) }]);
        let l = |w| ChatsEvent::Listening { chat: ChatId::new(41), principal: "builder".into(), waiting: w };
        assert!(h.apply(&l(true), "ada"));
        assert!(!h.apply(&l(true), "ada"));
        assert_eq!(h.waiting_on("KILN-7"), ["builder"]);
        assert!(h.apply(&l(false), "ada"));
        assert!(h.get(41).unwrap().waiting.is_empty());
    }

    /// The picker: the same people's open chat is the one that opens
    /// instead — whatever order they were picked in; a closed one is not.
    #[test]
    fn the_same_people_open_the_chat_they_have() {
        let mut h = Held::from_events([ChatsEvent::Chat { chat: chat(41, "KILN-7", &["ada", "builder"]) }, ChatsEvent::Chat { chat: chat(43, "KILN-7", &["ada", "builder", "rui"]) }]);
        assert_eq!(h.existing("kiln-7", "ada", &["builder".into()]), Some(41));
        assert_eq!(h.existing("KILN-7", "ada", &["rui".into(), "builder".into()]), Some(43));
        assert_eq!(h.existing("KILN-7", "ada", &["rui".into()]), None);
        assert_eq!(h.existing("KILN-4", "ada", &["builder".into()]), None);
        h.apply(&ChatsEvent::Closed { chat: ChatId::new(41) }, "ada");
        assert_eq!(h.existing("KILN-7", "ada", &["builder".into()]), None, "a closed chat is not the open one");
    }

    /// Threads: fetched messages and posts are kept once each, oldest
    /// first; a fetch's replay keeps the threads held.
    #[test]
    fn threads_hold_each_message_once_in_order() {
        let mut h = Held::from_events([ChatsEvent::Chat { chat: chat(41, "KILN-7", &["ada", "builder"]) }]);
        h.loaded(41, vec![msg(3, 41, "ada", "c"), msg(1, 41, "ada", "a")]);
        h.apply(&ChatsEvent::Posted { message: msg(2, 41, "builder", "b") }, "ada");
        h.apply(&ChatsEvent::Posted { message: msg(3, 41, "ada", "c") }, "ada");
        let bodies: Vec<&str> = h.thread(41).unwrap().list.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(bodies, ["a", "b", "c"]);
        let again = settle(vec![ChatsEvent::Chat { chat: chat(41, "KILN-7", &["ada", "builder"]) }], &[ChatsEvent::Posted { message: msg(4, 41, "builder", "d") }], "ada", Some(&h));
        assert_eq!(again.thread(41).unwrap().last(), Some(4));
        assert_eq!(again.get(41).unwrap().unread, 1);
    }

    #[test]
    fn a_principal_in_few_characters() {
        assert_eq!(short("key:4b2e0000000000aa"), "key:4b2e…");
        assert_eq!(short("anonymous"), "anonymous");
        assert_eq!(short("agent:builder"), "agent:builder");
        assert!(is_agent("agent:builder") && !is_agent("idp:ada"));
    }
}
