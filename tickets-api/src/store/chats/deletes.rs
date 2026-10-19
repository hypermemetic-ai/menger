//! CHAT-44: **deleting chat messages** (owner, 2026-10-19, after SYNAPSE-24
//! posted one message 97 times: *"build in the chat deletion"*). A delete is
//! soft: the message is kept, marked `deleted { at, by }`, and `messages`
//! still answers it — the app folds a run of them into one line that opens
//! them in a box. A deleted message is no one's unread
//! ([`super::Chats::unread`]) and no `await_message` reply
//! ([`Store::message_for`]); `restore` puts one back.
//!
//! **Who**: a message's author, and an admin of the chat's scope — of its
//! ticket, or, outside tickets, of its organization — whose proof is the
//! [`ChatOn`] it writes with ([`ChatOn::TicketAdmin`] / [`ChatOn::Admin`]).
//! All or none: one id the caller may not delete, or that is not the
//! chat's, refuses the whole call, naming each. One already as asked (a
//! delete of a deleted message) is not named in the answer, and a call
//! where every one is writes nothing.

use super::{refused, ChatChange, ChatError, ChatOn, Signer};
use crate::chats::{Deleted, DELETE_MAX};
use crate::store::Store;
use rusqlite::params;
use std::sync::Arc;

/// Which messages a delete or a restore names: ids, or every message of
/// the chat from one to another (both included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Ids(Vec<u64>),
    Range { from: u64, to: u64 },
}

/// What a delete or a restore did: the ids it changed, by whom, when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDeletion {
    pub chat: u64,
    pub ids: Vec<u64>,
    pub by: String,
    pub at: i64,
}

impl Store {
    /// CHAT-44: `by` deletes (`restore: false`) or restores the messages
    /// `pick` names in chat `chat`, writing with `on` — its author's own, or
    /// any for an admin (`on` an admin's proof). All or none.
    pub async fn delete_messages(&self, on: impl Into<ChatOn>, chat: u64, by: Signer, pick: Pick, restore: bool) -> Result<MessageDeletion, ChatError> {
        let on: ChatOn = on.into();
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (ids, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let c = self.held(&cache, &on, chat, &by.principal)?;
            let admin = on.administers();
            if !admin && !c.has(&by.principal) {
                return Err(refused(format!("you are not in chat {chat}: only its participants and its admins delete")));
            }
            let all = cache.chats.ids(chat);
            let named: Vec<u64> = match pick {
                Pick::Ids(ids) => {
                    let mut ids = ids;
                    ids.sort_unstable();
                    ids.dedup();
                    ids
                }
                Pick::Range { from, to } if from > to => return Err(refused(format!("the range runs backwards: {from} is after {to}"))),
                Pick::Range { from, to } => all[all.partition_point(|&i| i < from)..all.partition_point(|&i| i <= to)].to_vec(),
            };
            if named.is_empty() {
                return Err(refused("no message named: give --ids, or --from and --to around some of the chat's"));
            }
            if named.len() > DELETE_MAX {
                return Err(refused(format!("{} messages named; one call takes at most {DELETE_MAX}", named.len())));
            }
            let verb = if restore { "restores" } else { "deletes" };
            let mut reasons = Vec::new();
            let mut changing = Vec::new();
            for id in &named {
                let Some(m) = all.binary_search(id).ok().and_then(|_| cache.chats.messages.get(id)) else {
                    reasons.push(format!("no message {id} in chat {chat}"));
                    continue;
                };
                if !admin && m.author != by.principal {
                    reasons.push(format!("message {id} is {}'s: only its author or an admin {verb} it", m.name.as_deref().unwrap_or(&m.author)));
                    continue;
                }
                if m.deleted.is_some() != restore {
                    continue;
                }
                changing.push(*id);
            }
            if !reasons.is_empty() {
                return Err(ChatError::Refused(reasons));
            }
            if changing.is_empty() {
                return Ok(MessageDeletion { chat, ids: Vec::new(), by: by.principal, at: now });
            }
            let key = (!c.kind.outside()).then(|| cache.resolve(&c.ticket).ok()).flatten();
            (changing, key.and_then(|k| self.chats_before(&cache, &k)))
        };
        let mark = (!restore).then(|| Deleted { at: now, by: by.principal.clone() });
        let tickets = Arc::clone(&self.tickets);
        let (stored, kept) = ((ids.clone(), mark.clone()), (ids.clone(), mark));
        let change = ChatChange::Deleted { chat, ids: ids.clone(), by: by.principal.clone(), at: now, restored: restore };
        self.write(
            serial,
            move |tx| {
                let mut put = tx.prepare_cached("UPDATE chat_messages SET deleted_at = ?2, deleted_by = ?3 WHERE id = ?1")?;
                for id in &stored.0 {
                    put.execute(params![*id as i64, stored.1.as_ref().map(|d| d.at), stored.1.as_ref().map(|d| d.by.clone())])?;
                }
                Ok(())
            },
            move |cache, _| cache.chats.mark_deleted(chat, &kept.0, kept.1),
            move |(), (), tx| {
                tx.chat(change);
                if let Some(c) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(c);
                }
            },
        )
        .await
        .map_err(ChatError::from)?;
        Ok(MessageDeletion { chat, ids, by: by.principal, at: now })
    }
}

impl super::Chats {
    /// CHAT-44: `ids` of `chat` marked `mark` (none: put back).
    fn mark_deleted(&mut self, chat: u64, ids: &[u64], mark: Option<Deleted>) {
        let gone = self.deleted.entry(chat).or_default();
        for id in ids {
            if let Some(m) = self.messages.get_mut(id) {
                m.deleted = mark.clone();
                match mark {
                    Some(_) => gone.insert(*id),
                    None => gone.remove(id),
                };
            }
        }
    }
}
