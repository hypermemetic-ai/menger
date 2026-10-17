//! CHAT-3: the **chats store** — conversations among named participants,
//! each opened inside one ticket (CHAT-1), kept beside the tickets as
//! records of their own (owner, 2026-10-16: *"it's its own record"*).
//!
//! **Behind the cache, as everything stored is.** Every chat, participant
//! and message is read into the cache at open ([`load`]) and every read
//! answers from it. A write — `open`, `post`, `add`, `read`, `close`,
//! `reopen` — takes the write path every write takes ([`Store::write`]):
//! worked out under the write-serial lock from the cache, committed in one
//! transaction, put into the cache, then broadcast on the chats' own channel
//! ([`ChatChange`]) — and, when an open `watch { filter }` asks `chatting`
//! or `unread`, the tickets it moves ([`lineage::chats_before_with`]). A
//! change that changes nothing writes and says nothing.
//!
//! **Who sees a chat** ([`Store::standing`]): a participant who can read
//! its ticket, or an admin of the ticket; to anyone else it is not there
//! (ACCESS-9's `Hidden` — the same answer as a chat that is none). A
//! participant who loses its reach on the ticket loses the chat with it;
//! one who regains it has it back, read mark and all.
//!
//! **Read marks**: each participant's mark is the last message it has read;
//! its unread are the messages after it. Posting moves the poster's own
//! mark to its message, so a chat's unread are always someone else's.
//!
//! **Listening** (CHAT-1's "waiting for a message"): who is waiting on
//! `await_message` for which chat — held in the cache only, never stored:
//! it is the present, and a restart ends every wait.
//!
//! **Stored** in three tables — `chats`, `chat_participants` (each one's
//! read mark) and `chat_messages` — gained, empty, on open ([`migrate`]).
//! An older build never looks at them.

use super::lineage::{self, UnderBefore};
use super::{Cache, Store, StoreError};
use crate::access::{Admin, Permit, VerifiedUser, Viewer};
use crate::address::{ShortcodeId, TicketId};
use crate::chats::{check_body, Chat, ChatState, Message, PARTICIPANTS_MAX};
use crate::vocab::{ChatId, MessageId};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// The tables.
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS chats (
            id        INTEGER PRIMARY KEY,
            ticket    TEXT NOT NULL,
            title     TEXT,
            state     TEXT NOT NULL,
            opened_by TEXT NOT NULL,
            opened_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chat_participants (
            chat      INTEGER NOT NULL,
            principal TEXT NOT NULL,
            added_by  TEXT NOT NULL,
            added_at  INTEGER NOT NULL,
            read_upto INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (chat, principal)
        );
        CREATE TABLE IF NOT EXISTS chat_messages (
            id     INTEGER PRIMARY KEY,
            chat   INTEGER NOT NULL,
            author TEXT NOT NULL,
            name   TEXT,
            via    TEXT,
            at     INTEGER NOT NULL,
            body   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS chat_messages_by_chat ON chat_messages (chat, id);
    ";

/// A file without the chats' tables gains them, empty, here — on every
/// open a no-op once they are there. Returns whether they were made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    let had: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'chat_messages'", [], |r| {
        r.get::<_, i64>(0).map(|n| n > 0)
    })?;
    if had {
        return Ok(false);
    }
    conn.execute_batch(TABLES)?;
    tracing::warn!("CHAT-3: the file gained the chats' tables, empty, durably; every later open reads them");
    Ok(true)
}

/// One participant's place in a chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub principal: String,
    pub added_by: String,
    pub added_at: i64,
    /// The last message it has read (0: none).
    pub read_upto: u64,
}

/// One chat as kept: its participants in the order they joined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRecord {
    pub id: u64,
    /// The ticket it was opened on, by its key then (a pin follows a move).
    pub ticket: String,
    pub title: Option<String>,
    pub state: ChatState,
    pub opened_by: String,
    pub opened_at: i64,
    pub parts: Vec<Part>,
}

impl ChatRecord {
    fn part(&self, principal: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.principal == principal)
    }

    /// Whether `principal` is in it.
    pub fn has(&self, principal: &str) -> bool {
        self.part(principal).is_some()
    }

    fn set(&self) -> BTreeSet<&str> {
        self.parts.iter().map(|p| p.principal.as_str()).collect()
    }
}

/// A change to the chats, as `watch` and `await_message` hear it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatChange {
    Opened(ChatRecord),
    Posted(Message),
    Added(ChatRecord),
    /// `principal`'s mark in `chat` moved to `upto`.
    Read { chat: u64, principal: String, upto: u64 },
    Closed(ChatRecord),
    Reopened(ChatRecord),
    /// CHAT-7: the chat's title set (or cleared).
    Renamed(ChatRecord),
    /// `principal` began or stopped waiting on `await_message` for `chat`.
    Listening { chat: u64, principal: String, waiting: bool },
}

impl ChatChange {
    /// The chat it is a change of.
    pub fn chat(&self) -> u64 {
        match self {
            ChatChange::Opened(c) | ChatChange::Added(c) | ChatChange::Closed(c) | ChatChange::Reopened(c) | ChatChange::Renamed(c) => c.id,
            ChatChange::Posted(m) => m.chat.get(),
            ChatChange::Read { chat, .. } | ChatChange::Listening { chat, .. } => *chat,
        }
    }
}

/// Why a chat's read or write answered nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatError {
    /// Not a chat the caller sees — there or not (ACCESS-9).
    NotFound(u64),
    /// Refused, every reason named; nothing written.
    Refused(Vec<String>),
    Store(String),
}

impl From<StoreError> for ChatError {
    fn from(e: StoreError) -> Self {
        ChatError::Store(e.to_string())
    }
}

/// Who acts in a chat: the caller's principal (`anonymous` with login
/// off), the name it goes by, and whether it is anyone (login off: every
/// caller is, and may do what a participant does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signer {
    pub principal: String,
    pub name: Option<String>,
    pub anyone: bool,
}

impl Signer {
    /// The caller, as it signs.
    pub fn of(user: &VerifiedUser) -> Self {
        Self {
            principal: user.principal_text().unwrap_or_else(|| crate::types::ANONYMOUS_ACTOR.to_string()),
            name: user.name().map(str::to_string),
            anyone: user.principal().is_none(),
        }
    }

    /// A signer by principal alone — a test's.
    #[cfg(test)]
    pub fn named(principal: &str) -> Self {
        Self { principal: principal.into(), name: None, anyone: false }
    }
}

/// How a caller stands to one chat it sees.
pub struct Standing {
    /// The chat's ticket, by its key now.
    pub key: String,
    /// A viewer's permit on it — what a write in the chat takes.
    pub permit: Permit<Viewer>,
    pub participant: bool,
    pub admin: bool,
}

/// What `list` asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatQuery {
    /// Only chats on this ticket (any name for it) — and, for an admin of
    /// it, every chat on it, not only its own.
    pub ticket: Option<String>,
    /// Only chats with (true) or without (false) unread for the caller.
    pub unread: Option<bool>,
    pub state: Option<ChatState>,
}

/// What `await_message` asks of [`Store::message_for`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageWait {
    /// This chat; `None`: any of the caller's.
    pub chat: Option<u64>,
    /// Only chats on this ticket (its key).
    pub ticket: Option<String>,
    /// Messages at or after this moment (unix seconds).
    pub since: Option<i64>,
    /// Messages strictly after this id — what was there when the call came,
    /// set aside when it gave no `since`.
    pub after: u64,
}

/// The store's chats, as the cache holds them.
#[derive(Debug, Default, Clone)]
pub(crate) struct Chats {
    pub(super) by_id: BTreeMap<u64, ChatRecord>,
    messages: BTreeMap<u64, Message>,
    /// Each chat's message ids, ascending.
    by_chat: HashMap<u64, Vec<u64>>,
    /// The chats each principal is in.
    by_principal: HashMap<String, BTreeSet<u64>>,
    /// Who waits on each chat now, and how many waits each holds.
    listening: HashMap<u64, BTreeMap<String, u32>>,
}

impl Chats {
    fn next_chat(&self) -> u64 {
        self.by_id.keys().next_back().map_or(1, |n| n + 1)
    }

    fn next_message(&self) -> u64 {
        self.messages.keys().next_back().map_or(1, |n| n + 1)
    }

    /// The newest message's id (0: none).
    pub(crate) fn newest_message(&self) -> u64 {
        self.messages.keys().next_back().copied().unwrap_or(0)
    }

    fn put_chat(&mut self, c: ChatRecord) {
        for p in &c.parts {
            self.by_principal.entry(p.principal.clone()).or_default().insert(c.id);
        }
        self.by_id.insert(c.id, c);
    }

    fn put_message(&mut self, m: Message) {
        self.by_chat.entry(m.chat.get()).or_default().push(m.id.get());
        self.messages.insert(m.id.get(), m);
    }

    fn ids(&self, chat: u64) -> &[u64] {
        self.by_chat.get(&chat).map_or(&[], Vec::as_slice)
    }

    /// `principal`'s unread in `c`: the messages after its mark.
    fn unread(&self, c: &ChatRecord, principal: &str) -> u32 {
        let Some(p) = c.part(principal) else { return 0 };
        let ids = self.ids(c.id);
        (ids.len() - ids.partition_point(|&i| i <= p.read_upto)) as u32
    }

    /// The latest message of `chat` at or before `upto`.
    fn last_upto(&self, chat: u64, upto: u64) -> u64 {
        let ids = self.ids(chat);
        ids[..ids.partition_point(|&i| i <= upto)].last().copied().unwrap_or(0)
    }

    /// `c` as `me` sees it, its ticket named by `key`.
    fn view(&self, c: &ChatRecord, key: String, me: &str) -> Chat {
        let last = self.ids(c.id).last().and_then(|i| self.messages.get(i));
        Chat {
            id: ChatId::new(c.id),
            ticket: key,
            title: c.title.clone(),
            participants: c.parts.iter().map(|p| p.principal.clone()).collect(),
            state: c.state,
            opened_by: c.opened_by.clone(),
            opened_at: c.opened_at,
            last_at: last.map_or(c.opened_at, |m| m.at),
            last_message: last.map(|m| m.id),
            unread: self.unread(c, me),
            waiting: self.listening.get(&c.id).map(|w| w.keys().cloned().collect()).unwrap_or_default(),
        }
    }
}

/// CHAT-3's cells, for `me`: the tickets where it is in an open chat
/// (`chatting`), or has unread messages in one (`unread`), per shortcode.
pub(super) fn holds(cache: &Cache, me: &str, unread: bool) -> BTreeMap<ShortcodeId, RoaringBitmap> {
    let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
    let chats = &cache.chats;
    for c in chats.by_principal.get(me).into_iter().flatten().filter_map(|id| chats.by_id.get(id)) {
        let holds = match unread {
            true => chats.unread(c, me) > 0,
            false => c.state == ChatState::Open,
        };
        let key = holds.then(|| cache.resolve(&c.ticket).ok()).flatten();
        if let Some(k) = key.as_deref().and_then(|k| cache.at.get(k)) {
            out.entry(k.shortcode.clone()).or_default().insert(k.seq);
        }
    }
    out
}

/// A refusal of one reason.
fn refused(why: impl Into<String>) -> ChatError {
    ChatError::Refused(vec![why.into()])
}

impl Store {
    /// CHAT-3: how `user` stands to `c` — `None` when it does not see it:
    /// its ticket out of the store or out of reach, or it neither in the
    /// chat nor an admin of the ticket.
    pub fn standing(&self, user: &VerifiedUser, c: &ChatRecord) -> Option<Standing> {
        let key = self.ticket_id(&c.ticket)?;
        let id = TicketId::new(&key);
        let permit = user.permit::<Viewer>(&id, self).ok()?;
        let participant = c.has(&Signer::of(user).principal);
        let admin = user.permit::<Admin>(&id, self).is_ok();
        (participant || admin).then_some(Standing { key, permit, participant, admin })
    }

    /// CHAT-3: the chat `id` as kept, when `user` sees it.
    pub fn chat_seen(&self, user: &VerifiedUser, id: u64) -> Result<(ChatRecord, Standing), ChatError> {
        let c = self.tickets.read().expect("store lock poisoned").chats.by_id.get(&id).cloned().ok_or(ChatError::NotFound(id))?;
        let s = self.standing(user, &c).ok_or(ChatError::NotFound(id))?;
        Ok((c, s))
    }

    /// CHAT-3: the chat `id` as `user` sees it, its unread its own — `None`
    /// when it does not.
    pub fn chat_view(&self, user: &VerifiedUser, id: u64) -> Option<Chat> {
        let (c, s) = self.chat_seen(user, id).ok()?;
        Some(self.tickets.read().expect("store lock poisoned").chats.view(&c, s.key, &Signer::of(user).principal))
    }

    /// CHAT-3: one page of the chats `q` admits that `user` sees — its own
    /// (with `ticket`, an admin of it every one there) — by last activity,
    /// newest first; strictly after the chat `after`; at most `limit`.
    pub fn chats(&self, user: &VerifiedUser, q: &ChatQuery, after: Option<u64>, limit: u32) -> Result<(Vec<Chat>, Option<u64>), ChatError> {
        let me = Signer::of(user).principal;
        let ticket = match q.ticket.as_deref() {
            Some(t) => match self.ticket_id(t) {
                Some(k) => Some(k),
                None => return Ok((Vec::new(), None)),
            },
            None => None,
        };
        let candidates: Vec<ChatRecord> = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let chats = &cache.chats;
            match ticket {
                Some(_) => chats.by_id.values().cloned().collect(),
                None => chats.by_principal.get(&me).into_iter().flatten().filter_map(|id| chats.by_id.get(id).cloned()).collect(),
            }
        };
        let seen: Vec<(ChatRecord, String)> = candidates
            .into_iter()
            .filter(|c| q.state.is_none_or(|s| c.state == s))
            .filter_map(|c| {
                let s = self.standing(user, &c)?;
                let here = ticket.as_ref().is_none_or(|t| *t == s.key);
                (here && (s.participant || ticket.is_some())).then_some((c, s.key))
            })
            .collect();
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut rows: Vec<Chat> = seen.into_iter().map(|(c, key)| cache.chats.view(&c, key, &me)).filter(|v| q.unread.is_none_or(|u| u == (v.unread > 0))).collect();
        rows.sort_by_key(|r| std::cmp::Reverse((r.last_at, r.id)));
        if let Some(a) = after {
            let at = rows.iter().position(|r| r.id.get() == a).ok_or_else(|| refused(format!("no chat {a} to page after")))?;
            rows.drain(..=at);
        }
        let more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next = more.then(|| rows.last().map(|r| r.id.get())).flatten();
        Ok((rows, next))
    }

    /// CHAT-3: one page of the messages of chat `id`, oldest first, strictly
    /// after the message `after`, at most `limit` — when `user` sees it.
    pub fn chat_messages(&self, user: &VerifiedUser, id: u64, after: Option<u64>, limit: u32) -> Result<(Vec<Message>, Option<u64>), ChatError> {
        self.chat_seen(user, id)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let ids = cache.chats.ids(id);
        let from = ids.partition_point(|&i| i <= after.unwrap_or(0));
        let page: Vec<Message> = ids[from..].iter().take(limit as usize).filter_map(|i| cache.chats.messages.get(i).cloned()).collect();
        let more = ids.len() - from > limit as usize;
        Ok((page.clone(), more.then(|| page.last().map(|m| m.id.get())).flatten()))
    }

    /// CHAT-3 (`await_message`): the first message `w` asks for, by someone
    /// other than `user`, in a chat `user` is in and sees — `None` while
    /// there is none.
    pub fn message_for(&self, user: &VerifiedUser, w: &MessageWait) -> Option<Message> {
        let signer = Signer::of(user);
        let candidates: Vec<ChatRecord> = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let chats = &cache.chats;
            match w.chat {
                Some(id) => chats.by_id.get(&id).cloned().into_iter().collect(),
                None => chats.by_principal.get(&signer.principal).into_iter().flatten().filter_map(|id| chats.by_id.get(id).cloned()).collect(),
            }
        };
        let mine: Vec<u64> = candidates
            .into_iter()
            .filter_map(|c| {
                let s = self.standing(user, &c)?;
                (s.participant && w.ticket.as_ref().is_none_or(|t| *t == s.key)).then_some(c.id)
            })
            .collect();
        let cache = self.tickets.read().expect("store lock poisoned");
        let chats = &cache.chats;
        mine.iter()
            .flat_map(|chat| {
                let ids = chats.ids(*chat);
                ids[ids.partition_point(|&i| i <= w.after)..].iter().filter_map(|i| chats.messages.get(i))
            })
            .filter(|m| w.since.is_none_or(|s| m.at >= s) && (signer.anyone || m.author != signer.principal))
            .min_by_key(|m| m.id)
            .cloned()
    }

    /// CHAT-3: the newest message's id — what a wait without `since` sets
    /// aside.
    pub fn newest_message(&self) -> u64 {
        self.tickets.read().expect("store lock poisoned").chats.newest_message()
    }

    /// CHAT-3: `watch` and `await_message`'s channel.
    pub fn subscribe_chats(&self) -> tokio::sync::broadcast::Receiver<ChatChange> {
        self.tx.chats.subscribe()
    }

    /// CHAT-3: `principal` waits on `chats` (`on`), or stops (`!on`) —
    /// held in the cache only; `listening` said when a chat's waiting moves.
    pub fn listen(&self, chats: &[u64], principal: &str, on: bool) {
        let mut moved = Vec::new();
        {
            let mut cache = self.tickets.write().expect("store lock poisoned");
            for chat in chats {
                let w = cache.chats.listening.entry(*chat).or_default();
                let n = w.entry(principal.to_string()).or_insert(0);
                let was = *n;
                *n = if on { was + 1 } else { was.saturating_sub(1) };
                if (was == 0) != (*n == 0) {
                    moved.push(*chat);
                }
                if *n == 0 {
                    w.remove(principal);
                }
            }
        }
        for chat in moved {
            self.tx.chat(ChatChange::Listening { chat, principal: principal.to_string(), waiting: on });
        }
    }

    /// The open `chatting` / `unread` watches, before a write on `key`.
    fn chats_before(&self, cache: &Cache, key: &str) -> Option<UnderBefore> {
        let k = cache.at.get(key)?;
        lineage::chats_before_with(&self.unders, cache, &[(&k.shortcode, k.seq)])
    }

    /// CHAT-3: opens a chat on the ticket `on` names among `by` and `with` (the call's `others`)
    /// — each of `with` one `reads` says can read the ticket; CHAT-6: `with`
    /// may name no one, and the chat is `by`'s alone — or answers
    /// the open chat those participants already have there (`true`), and
    /// writes nothing.
    pub async fn open_chat(&self, on: Permit<Viewer>, by: Signer, with: Vec<String>, title: Option<String>, reads: impl Fn(&str) -> bool) -> Result<(ChatRecord, bool), ChatError> {
        let key = on.id().as_str().to_string();
        let mut set: Vec<String> = vec![by.principal.clone()];
        for p in with.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            if !set.iter().any(|s| s == p) {
                set.push(p.to_string());
            }
        }
        let mut reasons = Vec::new();
        if set.len() > PARTICIPANTS_MAX {
            reasons.push(format!("a chat holds at most {PARTICIPANTS_MAX} participants; this one would hold {}", set.len()));
        }
        reasons.extend(set[1..].iter().filter(|p| !reads(p)).map(|p| format!("{p} can't read {key}")));
        if !reasons.is_empty() {
            return Err(ChatError::Refused(reasons));
        }
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let wanted: BTreeSet<&str> = set.iter().map(String::as_str).collect();
            let same = cache.chats.by_id.values().find(|c| c.state == ChatState::Open && c.set() == wanted && cache.resolve(&c.ticket).ok().as_deref() == Some(key.as_str()));
            if let Some(c) = same {
                return Ok((c.clone(), true));
            }
            let parts = set.iter().map(|p| Part { principal: p.clone(), added_by: by.principal.clone(), added_at: now, read_upto: 0 }).collect();
            let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            let record = ChatRecord { id: cache.chats.next_chat(), ticket: key.clone(), title, state: ChatState::Open, opened_by: by.principal.clone(), opened_at: now, parts };
            (record, self.chats_before(&cache, &key))
        };
        self.chat_write(serial, record.clone(), None, under, ChatChange::Opened(record.clone())).await.map(|()| (record, false))
    }

    /// CHAT-3: `by` posts `body` in chat `chat`, on the ticket `on` names.
    pub async fn post_chat(&self, on: Permit<Viewer>, chat: u64, by: Signer, body: String) -> Result<Message, ChatError> {
        check_body(&body, self.body_max()).map_err(refused)?;
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (record, message, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat)?;
            if !c.has(&by.principal) && !by.anyone {
                return Err(refused(format!("you are not in chat {chat}: only its participants post")));
            }
            if c.state == ChatState::Closed {
                return Err(refused(format!("chat {chat} is closed")));
            }
            let message = Message { id: MessageId::new(cache.chats.next_message()), chat: ChatId::new(chat), author: by.principal.clone(), name: by.name.clone(), via: None, at: now, body };
            if let Some(p) = c.parts.iter_mut().find(|p| p.principal == by.principal) {
                p.read_upto = message.id.get();
            }
            (c, message, self.chats_before(&cache, on.id().as_str()))
        };
        self.chat_write(serial, record, Some(message.clone()), under, ChatChange::Posted(message.clone())).await.map(|()| message)
    }

    /// CHAT-3: `by` adds `principal` to chat `chat` — one `reads` says can
    /// read the ticket. One already in it is the chat as it is.
    pub async fn add_to_chat(&self, on: Permit<Viewer>, chat: u64, by: Signer, principal: String, reads: impl Fn(&str) -> bool) -> Result<ChatRecord, ChatError> {
        let principal = principal.trim().to_string();
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat)?;
            if !c.has(&by.principal) && !by.anyone {
                return Err(refused(format!("you are not in chat {chat}: only its participants add")));
            }
            if c.has(&principal) {
                return Ok(c);
            }
            let mut reasons = Vec::new();
            if c.state == ChatState::Closed {
                reasons.push(format!("chat {chat} is closed"));
            }
            if !reads(&principal) {
                reasons.push(format!("{principal} can't read {}", on.id()));
            }
            if c.parts.len() >= PARTICIPANTS_MAX {
                reasons.push(format!("a chat holds at most {PARTICIPANTS_MAX} participants"));
            }
            if !reasons.is_empty() {
                return Err(ChatError::Refused(reasons));
            }
            c.parts.push(Part { principal, added_by: by.principal.clone(), added_at: now, read_upto: 0 });
            (c, self.chats_before(&cache, on.id().as_str()))
        };
        self.chat_write(serial, record.clone(), None, under, ChatChange::Added(record.clone())).await.map(|()| record)
    }

    /// CHAT-3: `by`'s mark in chat `chat` moved forward to `upto` (the
    /// latest of its messages at or before it); answers the mark and the
    /// unread after it. A mark never moves back.
    pub async fn read_chat(&self, on: Permit<Viewer>, chat: u64, by: Signer, upto: u64) -> Result<(u64, u32), ChatError> {
        let serial = self.serial().await;
        let (mut record, mark, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let c = self.held(&cache, &on, chat)?;
            let Some(part) = c.part(&by.principal) else {
                return Err(refused(format!("you are not in chat {chat}: only a participant has a read mark")));
            };
            let mark = cache.chats.last_upto(chat, upto).max(part.read_upto);
            if mark == part.read_upto {
                return Ok((mark, cache.chats.unread(&c, &by.principal)));
            }
            (c.clone(), mark, self.chats_before(&cache, on.id().as_str()))
        };
        if let Some(p) = record.parts.iter_mut().find(|p| p.principal == by.principal) {
            p.read_upto = mark;
        }
        let change = ChatChange::Read { chat, principal: by.principal.clone(), upto: mark };
        self.chat_write(serial, record.clone(), None, under, change).await?;
        let cache = self.tickets.read().expect("store lock poisoned");
        Ok((mark, cache.chats.unread(&record, &by.principal)))
    }

    /// CHAT-3: `by` closes (`open: false`) or reopens chat `chat`. As it
    /// already is: the chat, and nothing written.
    pub async fn set_chat_state(&self, on: Permit<Viewer>, chat: u64, by: Signer, open: bool) -> Result<ChatRecord, ChatError> {
        let serial = self.serial().await;
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat)?;
            if !c.has(&by.principal) && !by.anyone {
                return Err(refused(format!("you are not in chat {chat}: only its participants close and reopen it")));
            }
            let state = if open { ChatState::Open } else { ChatState::Closed };
            if c.state == state {
                return Ok(c);
            }
            c.state = state;
            (c, self.chats_before(&cache, on.id().as_str()))
        };
        let change = if open { ChatChange::Reopened(record.clone()) } else { ChatChange::Closed(record.clone()) };
        self.chat_write(serial, record.clone(), None, under, change).await.map(|()| record)
    }

    /// CHAT-7: `by` sets chat `chat`'s title (`None`, or blank, clears it) —
    /// the composer's `/title` and the session's *rename*. As it already is:
    /// the chat, and nothing written.
    pub async fn rename_chat(&self, on: Permit<Viewer>, chat: u64, by: Signer, title: Option<String>) -> Result<ChatRecord, ChatError> {
        let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        let serial = self.serial().await;
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat)?;
            if !c.has(&by.principal) && !by.anyone {
                return Err(refused(format!("you are not in chat {chat}: only its participants rename it")));
            }
            if c.title == title {
                return Ok(c);
            }
            c.title = title;
            (c, self.chats_before(&cache, on.id().as_str()))
        };
        self.chat_write(serial, record.clone(), None, under, ChatChange::Renamed(record.clone())).await.map(|()| record)
    }

    /// ACCESS-11: chat `chat` as kept, when it is on the ticket `on` names
    /// — checked under the write-serial lock, so the permit is for this chat.
    fn held(&self, cache: &Cache, on: &Permit<Viewer>, chat: u64) -> Result<ChatRecord, ChatError> {
        let c = cache.chats.by_id.get(&chat).ok_or(ChatError::NotFound(chat))?;
        match cache.resolve(&c.ticket).ok().as_deref() == Some(on.id().as_str()) {
            true => Ok(c.clone()),
            false => Err(ChatError::NotFound(chat)),
        }
    }

    /// One chat write, through the write path every write takes: the chat
    /// whole (its participants and marks) and a message, when one is posted.
    async fn chat_write(&self, serial: tokio::sync::OwnedMutexGuard<()>, record: ChatRecord, message: Option<Message>, under: Option<UnderBefore>, change: ChatChange) -> Result<(), ChatError> {
        let tickets = Arc::clone(&self.tickets);
        let (stored, kept) = ((record.clone(), message.clone()), (record, message));
        self.write(
            serial,
            move |tx| {
                store_chat(tx, &stored.0)?;
                if let Some(m) = &stored.1 {
                    store_message(tx, m)?;
                }
                Ok(())
            },
            move |cache, _| {
                if let Some(m) = kept.1 {
                    cache.chats.put_message(m);
                }
                cache.chats.put_chat(kept.0);
            },
            move |(), (), tx| {
                tx.chat(change);
                if let Some(c) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(c);
                }
            },
        )
        .await
        .map_err(ChatError::from)
    }
}

/// Writes `c` whole — the chat, each participant and its mark.
fn store_chat(conn: &Connection, c: &ChatRecord) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO chats (id, ticket, title, state, opened_by, opened_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET state = excluded.state, title = excluded.title",
    )?
    .execute(params![c.id as i64, c.ticket, c.title, c.state.as_str(), c.opened_by, c.opened_at])?;
    let mut part = conn.prepare_cached(
        "INSERT INTO chat_participants (chat, principal, added_by, added_at, read_upto) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (chat, principal) DO UPDATE SET read_upto = excluded.read_upto",
    )?;
    for p in &c.parts {
        part.execute(params![c.id as i64, p.principal, p.added_by, p.added_at, p.read_upto as i64])?;
    }
    Ok(())
}

fn store_message(conn: &Connection, m: &Message) -> rusqlite::Result<()> {
    conn.prepare_cached("INSERT INTO chat_messages (id, chat, author, name, via, at, body) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?
        .execute(params![m.id.get() as i64, m.chat.get() as i64, m.author, m.name, m.via, m.at, m.body])?;
    Ok(())
}

/// Every chat, participant and message as stored — what a start reads.
pub(super) fn load(conn: &Connection) -> anyhow::Result<Chats> {
    let bad = |at: usize, e: String| rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into());
    let mut records: BTreeMap<u64, ChatRecord> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT id, ticket, title, state, opened_by, opened_at FROM chats ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id = r.get::<_, i64>(0)? as u64;
        let state = r.get_ref(3)?.as_str()?.parse::<ChatState>().map_err(|e| bad(3, e))?;
        records.insert(id, ChatRecord { id, ticket: r.get(1)?, title: r.get(2)?, state, opened_by: r.get(4)?, opened_at: r.get(5)?, parts: Vec::new() });
    }
    let mut stmt = conn.prepare("SELECT chat, principal, added_by, added_at, read_upto FROM chat_participants ORDER BY chat, added_at, rowid")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        if let Some(c) = records.get_mut(&(r.get::<_, i64>(0)? as u64)) {
            c.parts.push(Part { principal: r.get(1)?, added_by: r.get(2)?, added_at: r.get(3)?, read_upto: r.get::<_, i64>(4)? as u64 });
        }
    }
    let mut chats = Chats::default();
    for c in records.into_values() {
        chats.put_chat(c);
    }
    let mut stmt = conn.prepare("SELECT id, chat, author, name, via, at, body FROM chat_messages ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        chats.put_message(Message {
            id: MessageId::new(r.get::<_, i64>(0)? as u64),
            chat: ChatId::new(r.get::<_, i64>(1)? as u64),
            author: r.get(2)?,
            name: r.get(3)?,
            via: r.get(4)?,
            at: r.get(5)?,
            body: r.get(6)?,
        });
    }
    Ok(chats)
}
