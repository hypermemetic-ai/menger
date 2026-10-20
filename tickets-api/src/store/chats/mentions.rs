//! CHAT-55: **a chat's tickets** (owner, 2026-10-19: *"start the work that
//! turns this chat into a ticket surface"*) — every ticket a message names,
//! and every ticket filed or linked from the chat, kept by the server, never
//! a scan of the messages in a browser.
//!
//! **Kept** in `chat_mentions`, one row per (chat, message, ticket): a
//! message's codes ([`crate::chat_tickets::mentions_in`]) written in the
//! post's own transaction ([`Store::chat_write`]); a filed or linked ticket
//! with message `0`. A file without the table gains it and is back-filled
//! from every message kept ([`migrate`]). The cache holds each chat's rows
//! in the order written; a code is kept as written and resolved at read
//! time, so a ticket moved (ACT-6) or filed after its code was written is
//! found.
//!
//! **Read** through the chat and the reader's reach ([`Store::chat_tickets`]):
//! the chat must be one the caller sees; a ticket it does not see is not
//! there; a deleted message's mentions drop out (CHAT-44), and come back
//! with it. The filter cell `in chat N` ([`holds`]) answers the same set
//! for the reader — a chat it is in.

use super::{refused, ChatChange, ChatError, ChatOn, Chats, Signer};
use crate::access::{Permit, Viewer};
use crate::address::ShortcodeId;
use crate::chat_tickets::{mentions_in, stage_of, ChatTicket, MentionHow};
use crate::chats::Message;
use crate::store::{Cache, Store};
use crate::vocab::MessageId;
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The most tickets one `link` takes.
pub const LINK_MAX: usize = 100;

const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS chat_mentions (
            chat    INTEGER NOT NULL,
            message INTEGER NOT NULL DEFAULT 0,
            ticket  TEXT NOT NULL,
            how     TEXT NOT NULL,
            at      INTEGER NOT NULL,
            by      TEXT NOT NULL,
            PRIMARY KEY (chat, message, ticket)
        );
    ";

/// One ticket in a chat, as kept: the code as written (a message's) or the
/// key (a link's), and how and when it came in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    /// The message that names it; 0 for a filed or linked ticket.
    pub message: u64,
    pub ticket: String,
    pub how: MentionHow,
    pub at: i64,
    pub by: String,
}

/// The mentions a message makes.
pub(super) fn of_message(m: &Message) -> Vec<Mention> {
    mentions_in(&m.body).into_iter().map(|ticket| Mention { message: m.id.get(), ticket, how: MentionHow::Mentioned, at: m.at, by: m.author.clone() }).collect()
}

/// A file without `chat_mentions` gains it, back-filled from every message
/// kept — once; every later open a no-op. Returns how many rows it wrote.
pub(super) fn migrate(conn: &mut Connection) -> anyhow::Result<Option<usize>> {
    let had: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'chat_mentions'", [], |r| r.get::<_, i64>(0).map(|n| n > 0))?;
    if had {
        return Ok(None);
    }
    let tx = conn.transaction()?;
    tx.execute_batch(TABLE)?;
    let mut n = 0;
    {
        let mut read = tx.prepare("SELECT id, chat, author, at, body FROM chat_messages ORDER BY id")?;
        let mut rows = read.query([])?;
        while let Some(r) = rows.next()? {
            let (id, chat, author, at): (i64, i64, String, i64) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
            for code in mentions_in(r.get_ref(4)?.as_str()?) {
                store(&tx, chat as u64, &Mention { message: id as u64, ticket: code, how: MentionHow::Mentioned, at, by: author.clone() })?;
                n += 1;
            }
        }
    }
    tx.commit()?;
    tracing::warn!("CHAT-55: the file gained chat_mentions, back-filled with {n} mentions from the messages kept");
    Ok(Some(n))
}

pub(super) fn store(conn: &Connection, chat: u64, m: &Mention) -> rusqlite::Result<()> {
    conn.prepare_cached("INSERT OR IGNORE INTO chat_mentions (chat, message, ticket, how, at, by) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?
        .execute(params![chat as i64, m.message as i64, m.ticket, m.how.as_str(), m.at, m.by])?;
    Ok(())
}

/// Every mention kept, into `chats` — what a start reads.
pub(super) fn load(conn: &Connection, chats: &mut Chats) -> anyhow::Result<()> {
    let mut stmt = conn.prepare("SELECT chat, message, ticket, how, at, by FROM chat_mentions ORDER BY rowid")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let how = MentionHow::of(r.get_ref(3)?.as_str()?).unwrap_or(MentionHow::Mentioned);
        let m = Mention { message: r.get::<_, i64>(1)? as u64, ticket: r.get(2)?, how, at: r.get(4)?, by: r.get(5)? };
        chats.put_mentions(r.get::<_, i64>(0)? as u64, vec![m]);
    }
    Ok(())
}

impl Chats {
    /// `ms` into `chat`'s, each once.
    pub(super) fn put_mentions(&mut self, chat: u64, ms: Vec<Mention>) {
        let kept = self.mentions.entry(chat).or_default();
        for m in ms {
            if !kept.iter().any(|k| k.message == m.message && k.ticket == m.ticket) {
                kept.push(m);
            }
        }
    }

    /// `chat`'s tickets as `cache` resolves them now: each key once, with
    /// its first mention, in the order they came in — a deleted message's
    /// left out.
    fn tickets_of(&self, cache: &Cache, chat: u64) -> Vec<(String, Mention)> {
        let mut out: Vec<(String, Mention)> = Vec::new();
        for m in self.mentions.get(&chat).into_iter().flatten() {
            if m.message > 0 && self.messages.get(&m.message).is_none_or(|x| x.deleted.is_some()) {
                continue;
            }
            let Ok(key) = cache.resolve(&m.ticket) else { continue };
            if !cache.at.contains_key(&key) || out.iter().any(|(k, _)| *k == key) {
                continue;
            }
            out.push((key, m.clone()));
        }
        out
    }
}

/// CHAT-55's cell, `in chat N`, for `me`: the tickets chat `chat` names, per
/// shortcode — none when `me` is not in it.
pub(in crate::store) fn holds(cache: &Cache, me: &str, chat: u64) -> BTreeMap<ShortcodeId, RoaringBitmap> {
    let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
    if !cache.chats.by_id.get(&chat).is_some_and(|c| c.has(me)) {
        return out;
    }
    for (key, _) in cache.chats.tickets_of(cache, chat) {
        if let Some(k) = cache.at.get(&key) {
            out.entry(k.shortcode.clone()).or_default().insert(k.seq);
        }
    }
    out
}

/// DEPLOY-21, for one ticket: whether it is staged somewhere (ready for,
/// deploying or failed) and whether it runs anywhere.
fn where_runs(cache: &Cache, key: &str) -> (bool, bool) {
    let d = &cache.deploys;
    let running = d.envs.values().any(|k| k.running.contains_key(key));
    let none = super::super::deployments::Kept::default();
    let staged = d.env_keys().into_iter().any(|(p, n)| {
        let k = d.envs.get(&(p.clone(), n.clone())).unwrap_or(&none);
        k.deploying.contains(key) || k.failed.contains(key) || d.ready_for(&p, &n, k).contains(&key)
    });
    (staged, running)
}

impl Store {
    /// CHAT-55: the tickets chat `chat` names, filed or linked, as `user`
    /// reads them — each with its stage, its first mention and its open
    /// inputs, the newest first. A chat it does not see is not found.
    pub fn chat_tickets(&self, user: &crate::access::VerifiedUser, chat: u64) -> Result<Vec<ChatTicket>, ChatError> {
        self.chat_seen(user, chat)?;
        let seen = super::super::Seen::of(&user.sight()).map_err(ChatError::from)?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let mut out: Vec<ChatTicket> = Vec::new();
        for (key, m) in cache.chats.tickets_of(&cache, chat) {
            let Some(mut row) = cache.get(&key).filter(|_| sees.id(&key)).cloned() else { continue };
            sees.row(&mut row);
            let asking = cache.inputs.of(&key).iter().filter(|i| i.state == crate::types::InputState::Open).count() as u32;
            let (staged, running) = where_runs(&cache, &key);
            let stage = stage_of(row.kind, row.status, asking > 0, staged, running);
            out.push(ChatTicket { summary: row, stage, how: m.how, message: (m.message > 0).then(|| MessageId::new(m.message)), at: m.at, asking });
        }
        // Newest first by arrival — a message's own in the order written.
        let mut arrival = 0usize;
        let mut last = None;
        let mut out: Vec<(usize, ChatTicket)> = out.into_iter().map(|t| {
            let m = t.message.map(|m| m.get());
            arrival += usize::from(m.is_none() || m != last);
            last = m;
            (arrival, t)
        }).collect();
        out.sort_by_key(|(a, _)| std::cmp::Reverse(*a));
        let out = out.into_iter().map(|(_, t)| t).collect();
        Ok(out)
    }

    /// CHAT-55: `by` brings the tickets `on` permits into chat `chat` —
    /// filed from it (`tickets create --chat`) or linked (`chats link`).
    /// Writes with `chat_on`, `by`'s standing in the chat (a participant's:
    /// it must be in it). Answers the keys new to the chat; one already there
    /// is not written again.
    pub async fn link_chat(&self, chat_on: impl Into<ChatOn>, chat: u64, by: Signer, on: Vec<Permit<Viewer>>, how: MentionHow) -> Result<Vec<String>, ChatError> {
        let chat_on: ChatOn = chat_on.into();
        if on.len() > LINK_MAX {
            return Err(refused(format!("{} tickets named; one call links at most {LINK_MAX}", on.len())));
        }
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (new, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let c = self.held(&cache, &chat_on, chat, &by.principal)?;
            if !c.has(&by.principal) {
                return Err(refused(format!("you are not in chat {chat}: only its participants link tickets to it")));
            }
            let there: Vec<String> = cache.chats.tickets_of(&cache, chat).into_iter().map(|(k, _)| k).collect();
            let mut new: Vec<String> = Vec::new();
            for p in &on {
                let key = p.id().as_str().to_string();
                if !cache.at.contains_key(&key) {
                    return Err(ChatError::NotFound(chat));
                }
                if !there.contains(&key) && !new.contains(&key) {
                    new.push(key);
                }
            }
            if new.is_empty() {
                return Ok(Vec::new());
            }
            let keys: Vec<(&ShortcodeId, u32)> = new.iter().filter_map(|k| cache.at.get(k)).map(|k| (&k.shortcode, k.seq)).collect();
            (new.clone(), super::super::lineage::chats_before_with(&self.unders, &cache, &keys))
        };
        let rows: Vec<Mention> = new.iter().map(|k| Mention { message: 0, ticket: k.clone(), how, at: now, by: by.principal.clone() }).collect();
        let tickets = Arc::clone(&self.tickets);
        let (stored, kept) = (rows.clone(), rows);
        let change = ChatChange::Linked { chat, tickets: new.clone(), how };
        self.write(
            serial,
            move |tx| Ok(stored.iter().try_for_each(|m| store(tx, chat, m))?),
            move |cache, _| cache.chats.put_mentions(chat, kept),
            move |(), (), tx| {
                tx.chat(change);
                if let Some(c) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(c);
                }
            },
        )
        .await
        .map_err(ChatError::from)?;
        Ok(new)
    }
}
