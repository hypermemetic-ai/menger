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
//!
//! **Outside tickets** (CHAT-14): a direct message or a group has no ticket
//! (`ticket` stored `''`) and lives in one organization (`org`). Its reach
//! is its participants alone — no ticket's, and no admin's read; an admin
//! of its organization adds and removes members as a participant does
//! ([`ChatOn::Admin`]). Who may be in one: whoever holds a grant in its
//! organization. A direct message is the opener and one other (and the
//! standing chat members); opening it again answers the one there is. The
//! two columns, `kind` and `org`, are gained by a file that lacks them
//! ([`migrate`]); a row without them is a ticket's chat, as it was.
//!
//! **Names** (CHAT-14): the name each principal last signed in with — a
//! synapse key's name too (`--as builder`) — kept in `principal_names`
//! ([`Store::note_name`]), and, before that, the name its messages carry.

use super::lineage::{self, UnderBefore};
use super::{Cache, Store, StoreError};
use crate::access::{Admin, Permit, VerifiedUser, Viewer};
use crate::address::{ShortcodeId, TicketId};
use crate::chats::{check_body, Chat, ChatKind, ChatState, Message, PARTICIPANTS_MAX};
use crate::vocab::{ChatId, MessageId};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

// CHAT-14: chats outside tickets — opening, removing, names.
mod outside;

/// CHAT-8: which way a page of messages runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paging {
    /// The messages after this id (0: from the first).
    After(u64),
    /// The messages before this id, the nearest last.
    Before(u64),
    /// The newest messages.
    Newest,
}

/// The tables.
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS chats (
            id        INTEGER PRIMARY KEY,
            ticket    TEXT NOT NULL,
            title     TEXT,
            state     TEXT NOT NULL,
            opened_by TEXT NOT NULL,
            opened_at INTEGER NOT NULL,
            kind      TEXT NOT NULL DEFAULT 'ticket',
            org       TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS chat_participants (
            chat      INTEGER NOT NULL,
            principal TEXT NOT NULL,
            added_by  TEXT NOT NULL,
            added_at  INTEGER NOT NULL,
            read_upto INTEGER NOT NULL DEFAULT 0,
            granted   TEXT,
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

/// CHAT-14: the names principals signed in with, and the chats' two columns
/// a file made before it lacks.
const NAMES: &str = "
        CREATE TABLE IF NOT EXISTS principal_names (
            principal TEXT PRIMARY KEY,
            name      TEXT NOT NULL,
            at        INTEGER NOT NULL
        );
    ";

/// A file without the chats' tables gains them, empty, here — on every
/// open a no-op once they are there. Returns whether they were made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    let had: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'chat_messages'", [], |r| {
        r.get::<_, i64>(0).map(|n| n > 0)
    })?;
    if !had {
        conn.execute_batch(TABLES)?;
        tracing::warn!("CHAT-3: the file gained the chats' tables, empty, durably; every later open reads them");
    }
    // CHAT-14: the kind and organization columns, and the names.
    let cols: Vec<String> = conn.prepare("SELECT name FROM pragma_table_info('chats')")?.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    if !cols.iter().any(|c| c == "kind") {
        conn.execute_batch("ALTER TABLE chats ADD COLUMN kind TEXT NOT NULL DEFAULT 'ticket'; ALTER TABLE chats ADD COLUMN org TEXT NOT NULL DEFAULT '';")?;
        tracing::warn!("CHAT-14: the chats gained their kind and organization (every chat kept is a ticket's)");
    }
    conn.execute_batch(NAMES)?;
    // CHAT-16: the grant an admin's add made, beside a seat.
    let cols: Vec<String> = conn.prepare("SELECT name FROM pragma_table_info('chat_participants')")?.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    if !cols.iter().any(|c| c == "granted") {
        conn.execute_batch("ALTER TABLE chat_participants ADD COLUMN granted TEXT;")?;
        tracing::warn!("CHAT-16: chat_participants gained `granted` (no seat kept made a grant)");
    }
    Ok(!had)
}

/// One participant's place in a chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub principal: String,
    pub added_by: String,
    pub added_at: i64,
    /// The last message it has read (0: none).
    pub read_upto: u64,
    /// CHAT-16: the scope of the grant an admin's add made with this seat
    /// (its stored text, `KILN-7` or `hypermemetic:`) — what removing it
    /// offers to revoke.
    pub granted: Option<String>,
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
    /// CHAT-14: a ticket's chat, a direct message or a group.
    pub kind: ChatKind,
    /// CHAT-14: the organization a chat outside tickets lives in (`''` for a
    /// ticket's: its ticket's).
    pub org: String,
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
    /// CHAT-14: `principal` was taken out of the chat (as it is now).
    Removed { chat: ChatRecord, principal: String },
}

impl ChatChange {
    /// The chat it is a change of.
    pub fn chat(&self) -> u64 {
        match self {
            ChatChange::Opened(c) | ChatChange::Added(c) | ChatChange::Closed(c) | ChatChange::Reopened(c) | ChatChange::Renamed(c) => c.id,
            ChatChange::Posted(m) => m.chat.get(),
            ChatChange::Read { chat, .. } | ChatChange::Listening { chat, .. } => *chat,
            ChatChange::Removed { chat, .. } => chat.id,
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

/// Who acts in a chat: the caller's principal and the name it goes by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signer {
    pub principal: String,
    pub name: Option<String>,
}

impl Signer {
    /// The caller, as it signs.
    pub fn of(user: &VerifiedUser) -> Self {
        Self {
            principal: user.principal_text(),
            name: user.name().map(str::to_string),
        }
    }

    /// A signer by principal alone — a test's.
    #[cfg(test)]
    pub fn named(principal: &str) -> Self {
        Self { principal: principal.into(), name: None }
    }
}

/// How a caller stands to one chat it sees.
pub struct Standing {
    /// The chat's ticket, by its key now — none outside tickets (CHAT-14).
    pub key: Option<String>,
    /// What a write in the chat takes: a viewer's permit on its ticket, or,
    /// outside tickets, the caller's seat in it.
    pub on: ChatOn,
    pub participant: bool,
    pub admin: bool,
}

/// ACCESS-11: what a chat write takes — checked by the store under its
/// write lock against the chat written.
#[derive(Debug)]
pub enum ChatOn {
    /// A viewer's permit on the chat's ticket.
    Ticket(Permit<Viewer>),
    /// CHAT-14: a seat in a chat outside tickets — minted only by
    /// [`Store::standing`] for a participant; the signer must still be in it.
    Seat(Seat),
    /// CHAT-14: an admin of the organization of a chat outside tickets — it
    /// adds and removes members, and reads nothing.
    Admin(crate::access::AdminOf),
    /// CHAT-16: an admin of a ticket's chat's ticket — it adds anyone and
    /// removes, without being in it.
    TicketAdmin(Permit<crate::access::Admin>),
}

/// CHAT-14: the proof of a seat — made here alone.
#[derive(Debug)]
pub struct Seat(());

impl From<Permit<Viewer>> for ChatOn {
    fn from(p: Permit<Viewer>) -> Self {
        ChatOn::Ticket(p)
    }
}

impl ChatOn {
    /// The ticket's key it is for, when it is a permit.
    fn key(&self) -> Option<&str> {
        match self {
            ChatOn::Ticket(p) => Some(p.id().as_str()),
            ChatOn::TicketAdmin(p) => Some(p.id().as_str()),
            _ => None,
        }
    }

    /// CHAT-16: whether it adds and removes without a seat.
    fn administers(&self) -> bool {
        matches!(self, ChatOn::Admin(_) | ChatOn::TicketAdmin(_))
    }
}

/// What `list` asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatQuery {
    /// Only chats on this ticket (any name for it) — and, for an admin of
    /// it, every chat on it, not only its own.
    pub ticket: Option<String>,
    /// CHAT-14: only chats of this kind.
    pub kind: Option<ChatKind>,
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
    /// CHAT-13: the standing members (`--chat-member`) — in every chat
    /// opened, and added to every open chat at startup
    /// ([`Store::seat_chat_members`]). Configuration, never stored.
    members: Vec<String>,
    /// CHAT-14: the name each principal is known by — signed in with
    /// ([`Store::note_name`]), else the last its messages carried.
    pub(super) names: HashMap<String, String>,
    /// The principals whose name came from a sign-in: a message's name
    /// never overrides it.
    signed: BTreeSet<String>,
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
        // CHAT-14: one removed is no longer indexed as in it.
        if let Some(old) = self.by_id.get(&c.id) {
            for gone in old.parts.iter().filter(|p| !c.has(&p.principal)) {
                if let Some(ids) = self.by_principal.get_mut(&gone.principal) {
                    ids.remove(&c.id);
                }
            }
        }
        for p in &c.parts {
            self.by_principal.entry(p.principal.clone()).or_default().insert(c.id);
        }
        self.by_id.insert(c.id, c);
    }

    fn put_message(&mut self, m: Message) {
        if let Some(n) = m.name.as_ref().filter(|n| !n.trim().is_empty()) {
            if !self.signed.contains(&m.author) {
                self.names.insert(m.author.clone(), n.clone());
            }
        }
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

    /// `c` as `me` sees it, its ticket named by `key` (none outside tickets).
    fn view(&self, c: &ChatRecord, key: Option<String>, me: &str) -> Chat {
        let last = self.ids(c.id).last().and_then(|i| self.messages.get(i));
        Chat {
            id: ChatId::new(c.id),
            kind: c.kind,
            ticket: key,
            title: c.title.clone(),
            participants: c.parts.iter().map(|p| p.principal.clone()).collect(),
            state: c.state,
            opened_by: c.opened_by.clone(),
            opened_at: c.opened_at,
            last_at: last.map_or(c.opened_at, |m| m.at),
            last_message: last.map(|m| m.id),
            unread: self.unread(c, me),
            count: self.ids(c.id).len() as u64,
            mark: c.part(me).map(|p| p.read_upto).filter(|m| *m > 0).map(MessageId::new),
            waiting: self.listening.get(&c.id).map(|w| w.keys().cloned().collect()).unwrap_or_default(),
            ticket_title: None,
            granted: Vec::new(),
            agents: c.parts.iter().filter(|p| self.members.contains(&p.principal)).map(|p| p.principal.clone()).collect(),
        }
    }
}

/// `c` as `me` sees it, dressed from the rest of the cache (CHAT-16): its
/// ticket's title, and who holds a grant its add made.
pub(super) fn dressed(cache: &Cache, c: &ChatRecord, key: Option<String>, me: &str) -> Chat {
    let mut v = cache.chats.view(c, key, me);
    v.ticket_title = v.ticket.as_deref().and_then(|k| cache.get(k)).map(|r| r.title.clone());
    v.granted = c
        .parts
        .iter()
        .filter(|p| p.granted.as_ref().is_some_and(|s| cache.members.of(&p.principal).iter().any(|g| g.scope.text() == *s)))
        .map(|p| p.principal.clone())
        .collect();
    v
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
        if c.kind.outside() {
            // CHAT-14: its participants alone.
            return c.has(&Signer::of(user).principal).then_some(Standing { key: None, on: ChatOn::Seat(Seat(())), participant: true, admin: false });
        }
        let key = self.ticket_id(&c.ticket)?;
        let id = TicketId::new(&key);
        let participant = c.has(&Signer::of(user).principal);
        let Ok(permit) = user.permit::<Viewer>(&id, self) else {
            // CHAT-16: one who holds the chat only (`chatter` on the ticket)
            // reads the chats it is in there, and nothing of the ticket.
            let seated = participant && user.permit::<crate::access::Chatter>(&id, self).is_ok();
            return seated.then_some(Standing { key: Some(key), on: ChatOn::Seat(Seat(())), participant: true, admin: false });
        };
        let admin = user.permit::<Admin>(&id, self).is_ok();
        (participant || admin).then_some(Standing { key: Some(key), on: ChatOn::Ticket(permit), participant, admin })
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
        Some(dressed(&self.tickets.read().expect("store lock poisoned"), &c, s.key, &Signer::of(user).principal))
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
        let seen: Vec<(ChatRecord, Option<String>)> = candidates
            .into_iter()
            .filter(|c| q.state.is_none_or(|s| c.state == s) && q.kind.is_none_or(|k| c.kind == k))
            .filter_map(|c| {
                let s = self.standing(user, &c)?;
                let here = ticket.as_ref().is_none_or(|t| Some(t) == s.key.as_ref());
                (here && (s.participant || ticket.is_some())).then_some((c, s.key))
            })
            .collect();
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut rows: Vec<Chat> = seen.into_iter().map(|(c, key)| dressed(&cache, &c, key, &me)).filter(|v| q.unread.is_none_or(|u| u == (v.unread > 0))).collect();
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
        self.chat_page(user, id, Paging::After(after.unwrap_or(0)), limit)
    }

    /// CHAT-8: a page of chat `id`'s messages, oldest first within it —
    /// after a message (`next` the page's last, while newer remain), or
    /// back from one, or from the newest end (`next` the page's first,
    /// while older remain: what `before` takes to go on back).
    pub fn chat_page(&self, user: &VerifiedUser, id: u64, paging: Paging, limit: u32) -> Result<(Vec<Message>, Option<u64>), ChatError> {
        self.chat_seen(user, id)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let ids = cache.chats.ids(id);
        let n = limit as usize;
        let (from, to, more_back) = match paging {
            Paging::After(after) => {
                let from = ids.partition_point(|&i| i <= after);
                let to = (from + n).min(ids.len());
                let page: Vec<Message> = ids[from..to].iter().filter_map(|i| cache.chats.messages.get(i).cloned()).collect();
                let next = (to < ids.len()).then(|| page.last().map(|m| m.id.get())).flatten();
                return Ok((page, next));
            }
            Paging::Before(before) => {
                let to = ids.partition_point(|&i| i < before);
                (to.saturating_sub(n), to, true)
            }
            Paging::Newest => (ids.len().saturating_sub(n), ids.len(), true),
        };
        let page: Vec<Message> = ids[from..to].iter().filter_map(|i| cache.chats.messages.get(i).cloned()).collect();
        let next = (more_back && from > 0).then(|| page.first().map(|m| m.id.get())).flatten();
        Ok((page, next))
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
                (s.participant && w.ticket.as_ref().is_none_or(|t| Some(t) == s.key.as_ref())).then_some(c.id)
            })
            .collect();
        let cache = self.tickets.read().expect("store lock poisoned");
        let chats = &cache.chats;
        mine.iter()
            .flat_map(|chat| {
                let ids = chats.ids(*chat);
                ids[ids.partition_point(|&i| i <= w.after)..].iter().filter_map(|i| chats.messages.get(i))
            })
            .filter(|m| w.since.is_none_or(|s| m.at >= s) && m.author != signer.principal)
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
        let mut reasons: Vec<String> = set[1..].iter().filter(|p| !reads(p)).map(|p| format!("{p} can't read {key}")).collect();
        // CHAT-13: and the standing members, whoever opens it — one that
        // can't read the ticket is added anyway, and said in the log.
        for m in self.chat_members() {
            if !set.contains(&m) {
                if !reads(&m) {
                    tracing::warn!("CHAT-13: standing chat member {m} can't read {key}; added to the chat anyway");
                }
                set.push(m);
            }
        }
        if set.len() > PARTICIPANTS_MAX {
            reasons.insert(0, format!("a chat holds at most {PARTICIPANTS_MAX} participants; this one would hold {}", set.len()));
        }
        if !reasons.is_empty() {
            return Err(ChatError::Refused(reasons));
        }
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let wanted: BTreeSet<&str> = set.iter().map(String::as_str).collect();
            let same = cache.chats.by_id.values().find(|c| c.state == ChatState::Open && !c.kind.outside() && c.set() == wanted && cache.resolve(&c.ticket).ok().as_deref() == Some(key.as_str()));
            if let Some(c) = same {
                return Ok((c.clone(), true));
            }
            let parts = set.iter().map(|p| Part { principal: p.clone(), added_by: by.principal.clone(), added_at: now, read_upto: 0, granted: None }).collect();
            let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            let record = ChatRecord { id: cache.chats.next_chat(), ticket: key.clone(), title, state: ChatState::Open, opened_by: by.principal.clone(), opened_at: now, parts, kind: ChatKind::Ticket, org: String::new() };
            (record, self.chats_before(&cache, &key))
        };
        self.chat_write(serial, record.clone(), None, under, ChatChange::Opened(record.clone())).await.map(|()| (record, false))
    }

    /// CHAT-13: the standing chat members (main's `--chat-member`) — set
    /// once, at startup, before anything is served.
    pub fn set_chat_members(&self, members: Vec<String>) {
        let mut members: Vec<String> = members.into_iter().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect();
        let mut seen = BTreeSet::new();
        members.retain(|m| seen.insert(m.clone()));
        self.tickets.write().expect("store lock poisoned").chats.members = members;
    }

    /// CHAT-13: the standing chat members.
    pub fn chat_members(&self) -> Vec<String> {
        self.tickets.read().expect("store lock poisoned").chats.members.clone()
    }

    /// CHAT-13: every open chat gains the standing members it lacks — each
    /// an `added` participant signed `system`, one write per chat. Run at
    /// startup; a chat that holds them all is not written, so a second run
    /// writes nothing. A member that `reads` (principal, ticket) says can't
    /// read the chat's ticket is added anyway, and said in the log. Answers
    /// the chats written.
    pub async fn seat_chat_members(&self, _sys: &super::System, reads: impl Fn(&str, &str) -> bool) -> Result<Vec<u64>, ChatError> {
        let lacking: Vec<u64> = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let members = &cache.chats.members;
            cache.chats.by_id.values().filter(|c| c.state == ChatState::Open && members.iter().any(|m| !c.has(m))).map(|c| c.id).collect()
        };
        let mut seated = Vec::new();
        for chat in lacking {
            let serial = self.serial().await;
            let now = chrono::Utc::now().timestamp();
            let (record, under) = {
                let cache = self.tickets.read().expect("store lock poisoned");
                let Some(mut c) = cache.chats.by_id.get(&chat).cloned() else { continue };
                let key = cache.resolve(&c.ticket).ok().unwrap_or_else(|| c.ticket.clone());
                let mut added = false;
                for m in &cache.chats.members {
                    if c.state != ChatState::Open || c.has(m) || c.parts.len() >= PARTICIPANTS_MAX {
                        continue;
                    }
                    // CHAT-14: a chat outside tickets has no ticket to read.
                    if !c.kind.outside() && !reads(m, &key) {
                        tracing::warn!("CHAT-13: standing chat member {m} can't read {key}; added to chat {chat} anyway");
                    }
                    c.parts.push(Part { principal: m.clone(), added_by: "system".into(), added_at: now, read_upto: 0, granted: None });
                    added = true;
                }
                if !added {
                    continue;
                }
                (c, self.chats_before(&cache, &key))
            };
            self.chat_write(serial, record.clone(), None, under, ChatChange::Added(record)).await?;
            seated.push(chat);
        }
        Ok(seated)
    }

    /// CHAT-3: `by` posts `body` in chat `chat`, on the ticket `on` names.
    pub async fn post_chat(&self, on: impl Into<ChatOn>, chat: u64, by: Signer, body: String) -> Result<Message, ChatError> {
        let on: ChatOn = on.into();
        check_body(&body, self.body_max()).map_err(refused)?;
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (record, message, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat, &by.principal)?;
            if !c.has(&by.principal) {
                return Err(refused(format!("you are not in chat {chat}: only its participants post")));
            }
            if c.state == ChatState::Closed {
                return Err(refused(format!("chat {chat} is closed")));
            }
            let message = Message { id: MessageId::new(cache.chats.next_message()), chat: ChatId::new(chat), author: by.principal.clone(), name: by.name.clone(), via: None, at: now, body };
            if let Some(p) = c.parts.iter_mut().find(|p| p.principal == by.principal) {
                p.read_upto = message.id.get();
            }
            (c, message, on.key().and_then(|k| self.chats_before(&cache, k)))
        };
        self.chat_write(serial, record, Some(message.clone()), under, ChatChange::Posted(message.clone())).await.map(|()| message)
    }

    /// CHAT-3: `by` adds `principal` to chat `chat` — one `reads` says can
    /// read the ticket. One already in it is the chat as it is.
    pub async fn add_to_chat(&self, on: impl Into<ChatOn>, chat: u64, by: Signer, principal: String, reads: impl Fn(&str) -> bool) -> Result<ChatRecord, ChatError> {
        self.add_inner(on.into(), chat, by, principal, reads, None).await
    }

    /// CHAT-3, CHAT-16: the add, and — `grant`, an admin's (its authority
    /// and the grant) — the grant it makes for one who can't read the
    /// chat's ticket, in the same write.
    pub(super) async fn add_inner(&self, on: ChatOn, chat: u64, by: Signer, principal: String, reads: impl Fn(&str) -> bool, grant: Option<(crate::access::AdminOf, crate::members::Grant)>) -> Result<ChatRecord, ChatError> {
        let principal = principal.trim().to_string();
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat, &by.principal)?;
            if !c.has(&by.principal) && !on.administers() {
                return Err(refused(format!("you are not in chat {chat}: only its participants add")));
            }
            if c.has(&principal) {
                return Ok(c);
            }
            if let Some((a, g)) = &grant {
                if a.scope() != &g.scope || g.principal.to_string() != principal {
                    return Err(refused(format!("the grant is not {principal}'s on this chat's scope (nothing written)")));
                }
            }
            let mut reasons = Vec::new();
            if c.kind == ChatKind::Direct {
                reasons.push(format!("chat {chat} is a direct message: it stays between two — start a group to bring in {principal}"));
            }
            if c.state == ChatState::Closed {
                reasons.push(format!("chat {chat} is closed"));
            }
            if grant.is_none() && !reads(&principal) {
                reasons.push(match on.key() {
                    Some(k) => format!("{principal} can't read {k}"),
                    None => format!("{principal} holds no grant in {}", c.org),
                });
            }
            if c.parts.len() >= PARTICIPANTS_MAX {
                reasons.push(format!("a chat holds at most {PARTICIPANTS_MAX} participants"));
            }
            if !reasons.is_empty() {
                return Err(ChatError::Refused(reasons));
            }
            let granted = grant.as_ref().map(|(_, g)| g.scope.text());
            c.parts.push(Part { principal, added_by: by.principal.clone(), added_at: now, read_upto: 0, granted });
            (c, on.key().and_then(|k| self.chats_before(&cache, k)))
        };
        let grant = grant.map(|(_, g)| members_change::Grant::Keep(g));
        self.chat_write_granting(serial, record.clone(), under, ChatChange::Added(record.clone()), grant).await.map(|()| record)
    }

    /// CHAT-3: `by`'s mark in chat `chat` moved forward to `upto` (the
    /// latest of its messages at or before it); answers the mark and the
    /// unread after it. A mark never moves back.
    pub async fn read_chat(&self, on: impl Into<ChatOn>, chat: u64, by: Signer, upto: u64) -> Result<(u64, u32), ChatError> {
        let on: ChatOn = on.into();
        let serial = self.serial().await;
        let (mut record, mark, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let c = self.held(&cache, &on, chat, &by.principal)?;
            let Some(part) = c.part(&by.principal) else {
                return Err(refused(format!("you are not in chat {chat}: only a participant has a read mark")));
            };
            let mark = cache.chats.last_upto(chat, upto).max(part.read_upto);
            if mark == part.read_upto {
                return Ok((mark, cache.chats.unread(&c, &by.principal)));
            }
            (c.clone(), mark, on.key().and_then(|k| self.chats_before(&cache, k)))
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
    pub async fn set_chat_state(&self, on: impl Into<ChatOn>, chat: u64, by: Signer, open: bool) -> Result<ChatRecord, ChatError> {
        let on: ChatOn = on.into();
        let serial = self.serial().await;
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat, &by.principal)?;
            if !c.has(&by.principal) {
                return Err(refused(format!("you are not in chat {chat}: only its participants close and reopen it")));
            }
            let state = if open { ChatState::Open } else { ChatState::Closed };
            if c.state == state {
                return Ok(c);
            }
            c.state = state;
            (c, on.key().and_then(|k| self.chats_before(&cache, k)))
        };
        let change = if open { ChatChange::Reopened(record.clone()) } else { ChatChange::Closed(record.clone()) };
        self.chat_write(serial, record.clone(), None, under, change).await.map(|()| record)
    }

    /// CHAT-7: `by` sets chat `chat`'s title (`None`, or blank, clears it) —
    /// the composer's `/title` and the session's *rename*. As it already is:
    /// the chat, and nothing written.
    pub async fn rename_chat(&self, on: impl Into<ChatOn>, chat: u64, by: Signer, title: Option<String>) -> Result<ChatRecord, ChatError> {
        let on: ChatOn = on.into();
        let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        let serial = self.serial().await;
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat, &by.principal)?;
            if !c.has(&by.principal) {
                return Err(refused(format!("you are not in chat {chat}: only its participants rename it")));
            }
            if c.title == title {
                return Ok(c);
            }
            c.title = title;
            (c, on.key().and_then(|k| self.chats_before(&cache, k)))
        };
        self.chat_write(serial, record.clone(), None, under, ChatChange::Renamed(record.clone())).await.map(|()| record)
    }

    /// ACCESS-11: chat `chat` as kept, when `on` is for it — checked under
    /// the write-serial lock: a permit on its ticket; CHAT-14: outside
    /// tickets, a seat (and `by` in it), or an admin of its organization.
    fn held(&self, cache: &Cache, on: &ChatOn, chat: u64, by: &str) -> Result<ChatRecord, ChatError> {
        let c = cache.chats.by_id.get(&chat).ok_or(ChatError::NotFound(chat))?;
        let fits = match on {
            ChatOn::Ticket(p) => !c.kind.outside() && cache.resolve(&c.ticket).ok().as_deref() == Some(p.id().as_str()),
            ChatOn::TicketAdmin(p) => !c.kind.outside() && cache.resolve(&c.ticket).ok().as_deref() == Some(p.id().as_str()),
            // CHAT-16: a seat in a ticket's chat too — one who holds the chat only.
            ChatOn::Seat(_) => c.has(by),
            ChatOn::Admin(a) => c.kind.outside() && admin_covers(a.scope(), &c.org),
        };
        match fits {
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

/// CHAT-14: whether an admin's scope covers the organization `org`.
fn admin_covers(scope: &crate::members::Scope, org: &str) -> bool {
    match scope {
        crate::members::Scope::Everywhere => true,
        crate::members::Scope::Org(o) => o.as_str() == org,
        _ => false,
    }
}

/// CHAT-16: the grant a chat write makes or takes back with it.
pub(super) mod members_change {
    pub enum Grant {
        Keep(crate::members::Grant),
        Revoke(crate::vocab::Grantee, crate::members::Scope),
    }
}

impl Store {
    /// CHAT-16: [`Store::chat_write`]'s, with a grant kept or revoked in the
    /// same transaction — the add that grants, the remove that revokes.
    pub(super) async fn chat_write_granting(&self, serial: tokio::sync::OwnedMutexGuard<()>, record: ChatRecord, under: Option<UnderBefore>, change: ChatChange, grant: Option<members_change::Grant>) -> Result<(), ChatError> {
        let Some(grant) = grant else {
            return self.chat_write(serial, record, None, under, change).await;
        };
        let tickets = Arc::clone(&self.tickets);
        let (stored, kept) = (record.clone(), record);
        let (g_db, g_cache) = match &grant {
            members_change::Grant::Keep(g) => (members_change::Grant::Keep(g.clone()), grant),
            members_change::Grant::Revoke(p, s) => (members_change::Grant::Revoke(p.clone(), s.clone()), grant),
        };
        self.write(
            serial,
            move |tx| {
                store_chat(tx, &stored)?;
                match &g_db {
                    members_change::Grant::Keep(g) => super::members::store(tx, g)?,
                    members_change::Grant::Revoke(p, s) => {
                        super::members::delete(tx, p, s)?;
                    }
                }
                Ok(())
            },
            move |cache, _| {
                cache.chats.put_chat(kept);
                match g_cache {
                    members_change::Grant::Keep(g) => cache.members.put(g),
                    members_change::Grant::Revoke(p, s) => {
                        cache.members.remove(&p, &s);
                    }
                }
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

/// Writes `c` whole — the chat, each participant and its mark. CHAT-14: a
/// participant no longer in it is taken out.
fn store_chat(conn: &Connection, c: &ChatRecord) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO chats (id, ticket, title, state, opened_by, opened_at, kind, org) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET state = excluded.state, title = excluded.title",
    )?
    .execute(params![c.id as i64, c.ticket, c.title, c.state.as_str(), c.opened_by, c.opened_at, c.kind.as_str(), c.org])?;
    let kept: Vec<String> = conn.prepare_cached("SELECT principal FROM chat_participants WHERE chat = ?1")?.query_map([c.id as i64], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for gone in kept.iter().filter(|p| !c.has(p)) {
        conn.prepare_cached("DELETE FROM chat_participants WHERE chat = ?1 AND principal = ?2")?.execute(params![c.id as i64, gone])?;
    }
    let mut part = conn.prepare_cached(
        "INSERT INTO chat_participants (chat, principal, added_by, added_at, read_upto, granted) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (chat, principal) DO UPDATE SET read_upto = excluded.read_upto, granted = excluded.granted",
    )?;
    for p in &c.parts {
        part.execute(params![c.id as i64, p.principal, p.added_by, p.added_at, p.read_upto as i64, p.granted])?;
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
    let mut stmt = conn.prepare("SELECT id, ticket, title, state, opened_by, opened_at, kind, org FROM chats ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id = r.get::<_, i64>(0)? as u64;
        let state = r.get_ref(3)?.as_str()?.parse::<ChatState>().map_err(|e| bad(3, e))?;
        let kind = r.get_ref(6)?.as_str()?.parse::<ChatKind>().map_err(|e| bad(6, e))?;
        records.insert(id, ChatRecord { id, ticket: r.get(1)?, title: r.get(2)?, state, opened_by: r.get(4)?, opened_at: r.get(5)?, parts: Vec::new(), kind, org: r.get(7)? });
    }
    let mut stmt = conn.prepare("SELECT chat, principal, added_by, added_at, read_upto, granted FROM chat_participants ORDER BY chat, added_at, rowid")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        if let Some(c) = records.get_mut(&(r.get::<_, i64>(0)? as u64)) {
            c.parts.push(Part { principal: r.get(1)?, added_by: r.get(2)?, added_at: r.get(3)?, read_upto: r.get::<_, i64>(4)? as u64, granted: r.get(5)? });
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
    // CHAT-14: the names principals signed in with, over their messages'.
    let mut stmt = conn.prepare("SELECT principal, name FROM principal_names")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let p: String = r.get(0)?;
        chats.names.insert(p.clone(), r.get(1)?);
        chats.signed.insert(p);
    }
    Ok(chats)
}

/// CHAT-8: one synthetic chat for the perf probe — so many messages of
/// mixed length (1–40 lines, some code), or one message so many lines long.
/// Spelled `1000` or `L5000` (`--seed-synthetic-chat 10,1000,L5000`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatSeed {
    Messages(usize),
    Lines(usize),
}

impl std::str::FromStr for ChatSeed {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (lines, n) = match s.strip_prefix(['L', 'l']) {
            Some(n) => (true, n),
            None => (false, s),
        };
        let n: usize = n.replace('_', "").parse().map_err(|_| format!("{s}: a count of messages (1000) or of one message's lines (L5000)"))?;
        if n == 0 {
            return Err(format!("{s}: at least one"));
        }
        Ok(if lines { ChatSeed::Lines(n) } else { ChatSeed::Messages(n) })
    }
}

/// The `i`th synthetic message: 1–40 lines, every ninth a code block.
pub fn synthetic_message(i: usize) -> String {
    let lines = 1 + (i.wrapping_mul(7919).wrapping_add(13)) % 40;
    if i % 9 == 4 {
        let code: Vec<String> = (0..lines).map(|k| format!("    reading[{k}] = Reading {{ at: {}, celsius: {} }};", 1_000 + k, 1_000 + (i + k) % 300)).collect();
        return format!("The schedule at step {i}:\n\n```rust kiln/src/schedule.rs\nfn step_{i}() {{\n{}\n}}\n```", code.join("\n"));
    }
    (0..lines).map(|k| format!("Line {} of message {i}: the kiln read {} °C, cone {} bending on shelf {}.", k + 1, 1_000 + (i + k) % 300, 5 + (i + k) % 3, ["A", "B", "C"][k % 3])).collect::<Vec<_>>().join("\n")
}

/// One synthetic message `n` lines long: paragraphs, a fence every 50 lines.
pub fn synthetic_lines(n: usize) -> String {
    let mut out = Vec::with_capacity(n + n / 10);
    let mut k = 0;
    while k < n {
        if k % 50 == 40 && k + 6 <= n {
            out.push("```text".to_string());
            for j in 0..4 {
                out.push(format!("{:>6}  {:>4} °C", k + j, 1_000 + (k + j) % 300));
            }
            out.push("```".to_string());
            k += 6;
            continue;
        }
        out.push(format!("Line {}: the firing log reads {} °C at minute {}.", k + 1, 1_000 + k % 300, k));
        k += 1;
        if k % 10 == 0 {
            out.push(String::new());
        }
    }
    out.join("\n")
}

impl Store {
    /// CHAT-8: seed `seeds` as chats before serving (`--seed-synthetic-chat`)
    /// — a scope CHATS-1, a ticket per chat under it, each chat between
    /// `reader` (who has read it all) and `agent:builder`, its messages
    /// written in bulk transactions and put in the cache. Answers each
    /// chat's ticket, id and messages.
    pub async fn seed_synthetic_chats(&self, sys: &super::System, seeds: &[ChatSeed], reader: &str) -> Result<Vec<(String, u64, usize)>, StoreError> {
        let new = |title: String, kind, parent: Option<String>| super::NewTicket { epic: "CHATS".into(), title, kind, severity: None, blocked_by: Vec::new(), parent, path: None, body: String::new() };
        let root = self.create(sys, new("Synthetic chats (CHAT-8's probe)".into(), crate::types::Kind::Scope, None)).await?;
        let mut out = Vec::new();
        for seed in seeds {
            let title = match seed {
                ChatSeed::Messages(n) => format!("a chat of {n} messages"),
                ChatSeed::Lines(n) => format!("one message of {n} lines"),
            };
            let t = self.create(sys, new(title.clone(), crate::types::Kind::Build, Some(root.id.to_string()))).await?;
            let bodies: Vec<String> = match *seed {
                ChatSeed::Messages(n) => (0..n).map(synthetic_message).collect(),
                ChatSeed::Lines(n) => vec![synthetic_lines(n)],
            };
            let _serial = self.write_serial.lock().await;
            let now = chrono::Utc::now().timestamp();
            let (chat, first) = {
                let cache = self.tickets.read().expect("store lock poisoned");
                (cache.chats.next_chat(), cache.chats.next_message())
            };
            let count = bodies.len();
            let messages: Vec<Message> = bodies
                .into_iter()
                .enumerate()
                .map(|(i, body)| {
                    let author = if i % 2 == 0 { reader.to_string() } else { "agent:builder".to_string() };
                    Message { id: MessageId::new(first + i as u64), chat: ChatId::new(chat), author, name: None, via: None, at: now - (count - i) as i64, body }
                })
                .collect();
            let last = first + count as u64 - 1;
            let parts = [reader, "agent:builder"].map(|p| Part { principal: p.to_string(), added_by: reader.to_string(), added_at: now, read_upto: if p == reader { last } else { 0 }, granted: None }).into_iter().collect();
            let record = ChatRecord { id: chat, ticket: t.id.to_string(), title: Some(title), state: ChatState::Open, opened_by: reader.to_string(), opened_at: now - count as i64 - 1, parts, kind: ChatKind::Ticket, org: String::new() };
            {
                let mut conn = self.db.lock().expect("db lock poisoned");
                for (i, chunk) in messages.chunks(50_000).enumerate() {
                    let tx = conn.transaction().map_err(|e| StoreError::Db(e.to_string()))?;
                    if i == 0 {
                        store_chat(&tx, &record).map_err(|e| StoreError::Db(e.to_string()))?;
                    }
                    for m in chunk {
                        store_message(&tx, m).map_err(|e| StoreError::Db(e.to_string()))?;
                    }
                    tx.commit().map_err(|e| StoreError::Db(e.to_string()))?;
                }
            }
            {
                let mut cache = self.tickets.write().expect("store lock poisoned");
                for m in messages {
                    cache.chats.put_message(m);
                }
                cache.chats.put_chat(record);
            }
            out.push((t.id.to_string(), chat, count));
        }
        Ok(out)
    }
}
