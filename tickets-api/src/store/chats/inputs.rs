//! CHAT-20: **a message's inputs** — the store half (CHAT-19's *Questions
//! in a message are inputs*; decision `home`: one store underneath).
//!
//! **Born at `post`.** A message's body is read by [`crate::inputs::read`],
//! the reader a ticket's body takes ([`declared`]): each `<m-decision id>` /
//! `<m-input id kind>` is an input whose home is the message, born `open`,
//! asked at the post. A problem the reader finds (an id twice, a kind the
//! element does not take, an option with no value) refuses the post in
//! band, naming each. The inputs are kept in the post's own transaction.
//! A message is never edited, so its inputs are never `removed`.
//!
//! **What an input goes through** is a ticket's input's ([`super::super::inputs::apply`]):
//! `answer`, `clear`, `close`, `reopen`, each one history entry with who —
//! `by` and `name`, from the caller — the source and when. A participant
//! answers; the one who answered clears (or anyone, an answer kept before
//! `by`); the message's author or an admin of the chat's ticket closes and
//! reopens. A closed chat and a deleted message (CHAT-44) take no answer
//! and no clear, and ask no one.
//!
//! **Kept** in `chat_inputs` (a row per input: chat, message, id, its
//! position in the message, kind, question, options as JSON,
//! recommendation, asked) and `chat_answers` (a row per history entry, in
//! the order recorded), read whole at open ([`load`]); a file without them
//! gains both, empty — a message posted before CHAT-20 asks nothing.
//!
//! **Under Needs you** through the cell `asked` ([`holds`]): an open input
//! in a chat on the ticket, for a reader in that chat who did not ask it.

use super::{refused, ChatChange, ChatError, Chats, Signer};
use crate::access::VerifiedUser;
use crate::address::{ShortcodeId, TicketId};
use crate::chat_inputs::ChatInput;
use crate::chats::{ChatState, Message};
use crate::store::inputs::{self as kept, Op, Refused, Who};
use crate::store::{Cache, Store};
use crate::types::{AnswerSource, Input, InputAction, InputHistoryEntry, InputKind, InputState, InputValue};
use crate::vocab::{ChatId, InputId, MessageId};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::sync::Arc;

const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS chat_inputs (
            chat           INTEGER NOT NULL,
            message        INTEGER NOT NULL,
            id             TEXT NOT NULL,
            position       INTEGER NOT NULL,
            kind           TEXT NOT NULL,
            question       TEXT,
            options        TEXT NOT NULL,
            recommendation TEXT,
            asked_unix     INTEGER NOT NULL,
            PRIMARY KEY (message, id)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS chat_answers (
            seq          INTEGER PRIMARY KEY,
            chat         INTEGER NOT NULL,
            message      INTEGER NOT NULL,
            input        TEXT NOT NULL,
            action       TEXT NOT NULL,
            value        TEXT,
            rationale    TEXT,
            source       TEXT NOT NULL,
            by_principal TEXT,
            by_name      TEXT,
            at_unix      INTEGER NOT NULL
        );
    ";

/// A file without the tables gains them, empty — once; every later open a
/// no-op. Returns whether they were made.
pub(super) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    let had: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'chat_answers'", [], |r| r.get::<_, i64>(0).map(|n| n > 0))?;
    if !had {
        conn.execute_batch(TABLES)?;
        tracing::warn!("CHAT-20: the file gained chat_inputs and chat_answers, empty; a message posted before asks nothing");
    }
    Ok(!had)
}

/// The inputs `body` declares, born `open` at `now` — or each problem the
/// reader found, in words.
pub(super) fn declared(body: &str, now: i64) -> Result<Vec<Input>, Vec<String>> {
    let (declared, problems) = crate::inputs::read(body);
    if !problems.is_empty() {
        return Err(problems.iter().map(|p| format!("the message's inputs: {p}")).collect());
    }
    Ok(declared
        .into_iter()
        .map(|d| Input {
            ticket: TicketId::new(""),
            id: InputId::stored(d.id),
            kind: d.kind,
            question: d.question,
            options: d.options,
            recommendation: d.recommendation,
            state: InputState::Open,
            answer: None,
            history: Vec::new(),
            asked_unix: Some(now),
        })
        .collect())
}

fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serialize a wire type")
}

/// `list`, the inputs of message `m`, kept inside the post's transaction.
pub(super) fn store_all(conn: &Connection, m: &Message, list: &[Input]) -> rusqlite::Result<()> {
    let mut insert = conn.prepare_cached(
        "INSERT INTO chat_inputs (chat, message, id, position, kind, question, options, recommendation, asked_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (position, i) in list.iter().enumerate() {
        insert.execute(params![
            m.chat.get() as i64,
            m.id.get() as i64,
            i.id,
            position as i64,
            i.kind.as_str(),
            i.question,
            to_json(&i.options),
            i.recommendation,
            i.asked_unix.unwrap_or(m.at),
        ])?;
    }
    Ok(())
}

/// One history entry of message `message`'s input `input`.
fn record(conn: &Connection, chat: u64, message: u64, input: &str, e: &InputHistoryEntry) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO chat_answers (chat, message, input, action, value, rationale, source, by_principal, by_name, at_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?
    .execute(params![chat as i64, message as i64, input, e.action.as_str(), e.value.as_ref().map(to_json), e.rationale, e.source.as_str(), e.by, e.name, e.at_unix])?;
    Ok(())
}

/// Every message's inputs as kept, with their histories, into `chats`.
pub(super) fn load(conn: &Connection, chats: &mut Chats) -> anyhow::Result<()> {
    let bad = |at: usize, e: String| rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into());
    let mut stmt = conn.prepare("SELECT message, id, kind, question, options, recommendation, asked_unix FROM chat_inputs ORDER BY message, position")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let input = Input {
            ticket: TicketId::new(""),
            id: InputId::stored(r.get::<_, String>(1)?),
            kind: r.get_ref(2)?.as_str()?.parse::<InputKind>().map_err(|e| bad(2, e))?,
            question: r.get(3)?,
            options: serde_json::from_str(r.get_ref(4)?.as_str()?).map_err(|e| bad(4, e.to_string()))?,
            recommendation: r.get(5)?,
            state: InputState::Open,
            answer: None,
            history: Vec::new(),
            asked_unix: r.get(6)?,
        };
        chats.inputs.entry(r.get::<_, i64>(0)? as u64).or_default().push(input);
    }
    let mut stmt = conn.prepare("SELECT message, input, action, value, rationale, source, by_principal, by_name, at_unix FROM chat_answers ORDER BY seq")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let value: Option<String> = r.get(3)?;
        let entry = InputHistoryEntry {
            action: r.get_ref(2)?.as_str()?.parse::<InputAction>().map_err(|e| bad(2, e))?,
            value: value.as_deref().map(serde_json::from_str).transpose().map_err(|e| bad(3, e.to_string()))?,
            rationale: r.get(4)?,
            source: r.get_ref(5)?.as_str()?.parse::<AnswerSource>().map_err(|e| bad(5, e))?,
            by: r.get(6)?,
            name: r.get(7)?,
            at_unix: r.get(8)?,
        };
        let (message, id): (i64, String) = (r.get(0)?, r.get(1)?);
        match chats.inputs.get_mut(&(message as u64)).and_then(|l| l.iter_mut().find(|i| i.id.as_str() == id)) {
            Some(i) => i.history.push(entry),
            None => tracing::warn!("CHAT-20: an answer of message {message}'s '{id}', which has no input row — not loaded"),
        }
    }
    for i in chats.inputs.values_mut().flatten() {
        i.answer = kept::answer_of(&i.history);
        i.state = kept::state_of(true, &i.history);
    }
    Ok(())
}

impl Chats {
    /// Message `message`'s inputs, in its body's order.
    pub(crate) fn inputs_of(&self, message: u64) -> &[Input] {
        self.inputs.get(&message).map_or(&[], Vec::as_slice)
    }

    /// `input` of message `message`, as it is now.
    fn put_input(&mut self, message: u64, input: Input) {
        if let Some(slot) = self.inputs.get_mut(&message).and_then(|l| l.iter_mut().find(|i| i.id == input.id)) {
            *slot = input;
        }
    }

    /// Whether message `m` asks someone still: kept, not deleted, in an open chat.
    fn asks(&self, m: &Message) -> bool {
        m.deleted.is_none() && self.by_id.get(&m.chat.get()).is_some_and(|c| c.state == ChatState::Open)
    }
}

/// `input` of message `m` on the wire: its home dressed from the cache —
/// the chat's ticket by its key now.
pub(super) fn wire(cache: &Cache, m: &Message, input: &Input) -> ChatInput {
    let ticket = cache.chats.by_id.get(&m.chat.get()).filter(|c| !c.kind.outside()).and_then(|c| cache.resolve(&c.ticket).ok());
    ChatInput {
        chat: m.chat,
        message: m.id,
        ticket,
        author: m.author.clone(),
        id: input.id.clone(),
        kind: input.kind,
        question: input.question.clone(),
        options: input.options.clone(),
        recommendation: input.recommendation.clone(),
        state: input.state,
        answer: input.answer.clone(),
        history: input.history.clone(),
        asked_unix: input.asked_unix,
    }
}

/// CHAT-20: a name `<message>/<input>` is a message's input, in a chat on
/// the ticket — its state, when it is one (`None` when it is no such input).
pub(in crate::store) fn state_named(cache: &Cache, ticket: &str, name: &str) -> Option<InputState> {
    let (m, i) = name.split_once('/')?;
    let message = cache.chats.messages.get(&m.parse().ok()?)?;
    let chat = cache.chats.by_id.get(&message.chat.get()).filter(|c| !c.kind.outside())?;
    (cache.resolve(&chat.ticket).ok()? == ticket).then_some(())?;
    let held = cache.chats.inputs_of(message.id.get()).iter().find(|x| x.id.as_str() == i)?;
    Some(if message.deleted.is_some() { InputState::Removed } else { held.state })
}

/// The cell `asked` for `me`: the tickets with an open input in a chat on
/// them that `me` is in and did not ask, per shortcode.
pub(in crate::store) fn holds(cache: &Cache, me: &str) -> BTreeMap<ShortcodeId, RoaringBitmap> {
    let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
    let chats = &cache.chats;
    for c in chats.by_principal.get(me).into_iter().flatten().filter_map(|id| chats.by_id.get(id)) {
        if c.kind.outside() || c.state != ChatState::Open {
            continue;
        }
        let asks = chats.ids(c.id).iter().filter_map(|i| chats.messages.get(i)).any(|m| {
            m.author != me && m.deleted.is_none() && chats.inputs_of(m.id.get()).iter().any(|i| i.state == InputState::Open)
        });
        let key = asks.then(|| cache.resolve(&c.ticket).ok()).flatten();
        if let Some(k) = key.as_deref().and_then(|k| cache.at.get(k)) {
            out.entry(k.shortcode.clone()).or_default().insert(k.seq);
        }
    }
    out
}

/// What `inputs list` asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatInputQuery {
    /// This chat; `None`: every chat the caller is in.
    pub chat: Option<u64>,
    /// Only chats on this ticket (any name for it).
    pub ticket: Option<String>,
    pub state: Option<InputState>,
    pub kind: Option<InputKind>,
}

/// What one call does to a message's input.
#[derive(Debug, Clone)]
pub enum ChatInputOp {
    Answer { value: InputValue, rationale: Option<String> },
    Clear,
    Close,
    Reopen,
}

/// What a write answered: the input as it is now, and whether it moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatInputWrite {
    Changed(ChatInput),
    Unchanged(ChatInput),
}

/// An answer `await_message` waits for: one input (`message`, `id`), or —
/// `None` — any in the caller's own messages, by someone else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnswerWait {
    pub chat: Option<u64>,
    pub input: Option<(u64, String)>,
    /// Answers at or after this moment (unix seconds).
    pub since: i64,
}

impl Store {
    /// CHAT-20: the inputs of the messages `user` reads in the chats `q`
    /// names — a chat it does not see is `not_found` — oldest message first,
    /// each message's in its body's order, after the message `after`; at
    /// most `limit`, a message's never split, and `next` the page's last
    /// message while more remain. A deleted message's are left out.
    pub fn chat_inputs(&self, user: &VerifiedUser, q: &ChatInputQuery, after: Option<u64>, limit: u32) -> Result<(Vec<ChatInput>, Option<u64>), ChatError> {
        let me = Signer::of(user).principal;
        if let Some(id) = q.chat {
            self.chat_seen(user, id)?;
        }
        let ticket = match q.ticket.as_deref() {
            Some(t) => Some(self.ticket_id(t).ok_or_else(|| refused(format!("no ticket {}", t.trim().to_ascii_uppercase())))?),
            None => None,
        };
        let candidates: Vec<crate::store::ChatRecord> = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let chats = &cache.chats;
            match q.chat {
                Some(id) => chats.by_id.get(&id).cloned().into_iter().collect(),
                None => chats.by_principal.get(&me).into_iter().flatten().filter_map(|id| chats.by_id.get(id).cloned()).collect(),
            }
        };
        let seen: Vec<u64> = candidates
            .into_iter()
            .filter_map(|c| {
                let s = self.standing(user, &c)?;
                ticket.as_ref().is_none_or(|t| Some(t) == s.key.as_ref()).then_some(c.id)
            })
            .collect();
        let cache = self.tickets.read().expect("store lock poisoned");
        let chats = &cache.chats;
        let mut messages: Vec<&Message> = seen
            .iter()
            .flat_map(|c| chats.ids(*c).iter().filter_map(|i| chats.messages.get(i)))
            .filter(|m| m.id.get() > after.unwrap_or(0) && m.deleted.is_none() && !chats.inputs_of(m.id.get()).is_empty())
            .collect();
        messages.sort_by_key(|m| m.id);
        let (mut out, mut next) = (Vec::new(), None);
        for (n, m) in messages.iter().enumerate() {
            let rows: Vec<ChatInput> = chats
                .inputs_of(m.id.get())
                .iter()
                .filter(|i| q.state.is_none_or(|s| i.state == s) && q.kind.is_none_or(|k| i.kind == k))
                .map(|i| wire(&cache, m, i))
                .collect();
            out.extend(rows);
            if out.len() >= limit as usize && n + 1 < messages.len() {
                next = Some(m.id.get());
                break;
            }
        }
        Ok((out, next))
    }

    /// CHAT-20: message `message`'s inputs on the wire — what `post` answers.
    pub fn message_inputs(&self, message: u64) -> Vec<ChatInput> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let Some(m) = cache.chats.messages.get(&message) else { return Vec::new() };
        cache.chats.inputs_of(message).iter().map(|i| wire(&cache, m, i)).collect()
    }

    /// CHAT-20: one change to message `message`'s input `input` in chat
    /// `chat`, by `user` (`who`: its source, principal and name) — through the write path every write takes: worked
    /// out under the write-serial lock from the cache, one history row
    /// committed, the cache moved, `input_changed` heard by the chat's
    /// participants (and the `asked` cell's watches). A change that changes
    /// nothing writes and says nothing ([`ChatInputWrite::Unchanged`]).
    pub async fn chat_input_write(&self, user: &VerifiedUser, who: Who, chat: u64, message: u64, input: &str, op: ChatInputOp) -> Result<ChatInputWrite, ChatError> {
        let (_, standing) = self.chat_seen(user, chat)?;
        let input = input.trim().to_string();
        let serial = self.serial().await;
        let (m, current, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let m = cache.chats.messages.get(&message).filter(|m| m.chat.get() == chat).cloned().ok_or_else(|| refused(format!("chat {chat} has no message {message}")))?;
            let current = cache.chats.inputs_of(message).iter().find(|i| i.id.as_str() == input).cloned().ok_or_else(|| {
                let known: Vec<&str> = cache.chats.inputs_of(message).iter().map(|i| i.id.as_str()).collect();
                refused(match known.is_empty() {
                    true => format!("message {message} asks nothing"),
                    false => format!("message {message} has no input '{input}' (it has {})", known.join(", ")),
                })
            })?;
            let mut reasons = Vec::new();
            let answering = matches!(op, ChatInputOp::Answer { .. } | ChatInputOp::Clear);
            if answering && !standing.participant {
                reasons.push(format!("you are not in chat {chat}: only its participants answer"));
            }
            if answering && !cache.chats.asks(&m) {
                reasons.push(match m.deleted.is_some() {
                    true => format!("message {message} was deleted: it asks no one"),
                    false => format!("chat {chat} is closed: it asks no one"),
                });
            }
            if let (ChatInputOp::Clear, Some(a)) = (&op, &current.answer) {
                if a.by.as_ref().is_some_and(|b| *b != who.by.clone().unwrap_or_default()) {
                    reasons.push(format!("only {} clears the answer they gave", a.name.clone().or_else(|| a.by.clone()).unwrap_or_default()));
                }
            }
            if matches!(op, ChatInputOp::Close | ChatInputOp::Reopen) && m.author != Signer::of(user).principal && !standing.admin {
                reasons.push(format!("only the one who asked ({}) or an admin of the chat's ticket closes and reopens it", m.name.clone().unwrap_or_else(|| m.author.clone())));
            }
            if !reasons.is_empty() {
                return Err(ChatError::Refused(reasons));
            }
            let under = standing.key.as_deref().and_then(|k| self.chats_before(&cache, k));
            (m, current, under)
        };
        let op = match op {
            ChatInputOp::Answer { value, rationale } => Op::Answer { value, rationale, who },
            ChatInputOp::Clear => Op::Clear { who },
            ChatInputOp::Close => Op::Close { who },
            ChatInputOp::Reopen => Op::Reopen { who },
        };
        let moved = kept::apply(&current, op, chrono::Utc::now().timestamp()).map_err(|r| {
            refused(match r {
                Refused::Closed => format!("'{input}' is closed: reopen it first"),
                Refused::Invalid(why) => format!("that does not answer '{input}': {why}"),
                Refused::Removed | Refused::Withdrawn => format!("'{input}' asks no one now"),
            })
        })?;
        let Some((next, entry)) = moved else {
            let cache = self.tickets.read().expect("store lock poisoned");
            return Ok(ChatInputWrite::Unchanged(wire(&cache, &m, &current)));
        };
        let said = {
            let cache = self.tickets.read().expect("store lock poisoned");
            wire(&cache, &m, &next)
        };
        // CHAT-20: closing the input a queue entry names (`<message>/<input>`)
        // takes it out, as a ticket's input's closing does.
        let closing = (next.state == InputState::Closed).then(|| {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = said.ticket.as_deref().and_then(|t| cache.get(t)).map(|r| (r.id.clone(), r.shortcode.clone()));
            row.and_then(|(t, s)| crate::store::queue::Closing::of(&cache, &t, &format!("{message}/{input}"), entry.at_unix).map(|c| (c, s)))
        });
        let closing = closing.flatten();
        let (closing_tx, closing_cache) = (closing.clone(), closing.clone());
        let (tickets, heard) = (Arc::clone(&self.tickets), said.clone());
        let id = next.id.to_string();
        self.write(
            serial,
            move |tx| {
                record(tx, chat, message, &id, &entry)?;
                if let Some((c, _)) = &closing_tx {
                    c.commit(tx)?;
                }
                Ok(())
            },
            move |cache, _| {
                cache.chats.put_input(message, next);
                if let Some((c, _)) = &closing_cache {
                    c.apply(cache);
                }
            },
            move |(), (), tx| {
                tx.chat(ChatChange::Input(heard));
                if let Some((c, s)) = closing {
                    tx.send(c.change(s));
                }
                if let Some(c) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(c);
                }
            },
        )
        .await
        .map_err(ChatError::from)?;
        Ok(ChatInputWrite::Changed(said))
    }

    /// CHAT-20 (`await_message --input`): the answer `w` waits for, when
    /// there is one — the named input's, when it holds one given at or after
    /// `since`; without one, the first answer at or after `since` by someone
    /// else to an input in `user`'s own messages, in the chats it is in.
    pub fn answer_for(&self, user: &VerifiedUser, w: &AnswerWait) -> Option<crate::chats::ChatsEvent> {
        let me = Signer::of(user).principal;
        let cache = self.tickets.read().expect("store lock poisoned");
        let chats = &cache.chats;
        let answered = |m: &Message, i: &Input, e: &InputHistoryEntry| crate::chats::ChatsEvent::Answered {
            chat: ChatId::new(m.chat.get()),
            message: MessageId::new(m.id.get()),
            input: i.id.clone(),
            value: e.value.clone().unwrap_or(InputValue::Text { text: String::new() }),
            rationale: e.rationale.clone(),
            by: e.by.clone(),
            name: e.name.clone(),
            source: e.source,
            at_unix: e.at_unix,
        };
        if let Some((message, id)) = &w.input {
            let m = chats.messages.get(message)?;
            let i = chats.inputs_of(*message).iter().find(|i| i.id.as_str() == id)?;
            let a = i.answer.as_ref().filter(|a| a.at_unix >= w.since)?;
            let e = i.history.iter().rev().find(|e| e.action == InputAction::Answered && e.at_unix == a.at_unix)?;
            return Some(answered(m, i, e));
        }
        let mine: Vec<u64> = match w.chat {
            Some(c) => vec![c],
            None => chats.by_principal.get(&me).map(|s| s.iter().copied().collect()).unwrap_or_default(),
        };
        mine.iter()
            .flat_map(|c| chats.ids(*c).iter().filter_map(|i| chats.messages.get(i)))
            .filter(|m| m.author == me && m.deleted.is_none())
            .flat_map(|m| chats.inputs_of(m.id.get()).iter().flat_map(move |i| i.history.iter().map(move |e| (m, i, e))))
            .filter(|(_, _, e)| e.action == InputAction::Answered && e.at_unix >= w.since && e.by.as_deref() != Some(me.as_str()))
            .min_by_key(|(m, _, e)| (e.at_unix, m.id))
            .map(|(m, i, e)| answered(m, i, e))
    }
}
