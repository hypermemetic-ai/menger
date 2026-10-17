//! CHAT-14: chats **outside tickets** — a direct message between two people,
//! or a group with a title (owner, 2026-10-17: *"I think we should also
//! support direct messages and groups outside of ticketing"*) — and the
//! names principals are known by, so an agent's key is found by its name in
//! the picker.
//!
//! A chat outside tickets lives in one organization: who may be in it is
//! whoever holds a grant there (`member`, the caller's to say from the
//! grants: [`crate::access::Access::member_of`]). Its reach is its
//! participants alone ([`Store::standing`]); an admin of its organization
//! adds and removes members ([`ChatOn::Admin`]) and reads nothing.
//!
//! The standing chat members (CHAT-13's `--chat-member`) are in every chat
//! opened, these too. A direct message is known by its two people: the
//! standing members beside them do not make it another.

use super::{refused, ChatChange, ChatError, ChatOn, ChatRecord, Part, Signer};
use crate::access::VerifiedUser;
use crate::chats::{ChatKind, ChatState, PARTICIPANTS_MAX};
use crate::store::Store;
use rusqlite::params;
use std::collections::BTreeSet;

/// The most characters a known name keeps.
const NAME_MAX: usize = 120;

impl Store {
    /// CHAT-14: `user` opens a chat outside tickets in `org` with `with` —
    /// one other: a direct message (the one the two already have, open,
    /// answered `true` and nothing written); several: a group, titled
    /// `title`. Each named, and the caller, must be a `member` of `org`, or
    /// it is refused, naming each who is not; nobody named is refused (a
    /// chat of one belongs on a ticket, CHAT-6). The standing chat members
    /// join it, as every chat.
    pub async fn open_outside(&self, user: &VerifiedUser, org: String, with: Vec<String>, title: Option<String>, member: impl Fn(&str) -> bool) -> Result<(ChatRecord, bool), ChatError> {
        let by = Signer::of(user);
        let mut set: Vec<String> = vec![by.principal.clone()];
        for p in with.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            if !set.iter().any(|s| s == p) {
                set.push(p.to_string());
            }
        }
        if set.len() < 2 {
            return Err(refused("a chat outside a ticket names someone else: one for a direct message, several for a group (a chat of one is opened on a ticket)"));
        }
        let kind = if set.len() == 2 { ChatKind::Direct } else { ChatKind::Group };
        let mut reasons: Vec<String> = set.iter().filter(|p| !member(p)).map(|p| format!("{p} holds no grant in {org}")).collect();
        let people: BTreeSet<String> = set.iter().cloned().collect();
        for m in self.chat_members() {
            if !set.contains(&m) {
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
        let record = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if kind == ChatKind::Direct {
                let standing = self.chat_members();
                let same = cache.chats.by_id.values().find(|c| {
                    c.kind == ChatKind::Direct && c.state == ChatState::Open && c.org == org && c.parts.iter().map(|p| p.principal.clone()).filter(|p| !standing.contains(p) || people.contains(p)).collect::<BTreeSet<_>>() == people
                });
                if let Some(c) = same {
                    return Ok((c.clone(), true));
                }
            }
            let parts = set.iter().map(|p| Part { principal: p.clone(), added_by: by.principal.clone(), added_at: now, read_upto: 0 }).collect();
            let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            ChatRecord { id: cache.chats.next_chat(), ticket: String::new(), title, state: ChatState::Open, opened_by: by.principal.clone(), opened_at: now, parts, kind, org }
        };
        self.chat_write(serial, record.clone(), None, None, ChatChange::Opened(record.clone())).await.map(|()| (record, false))
    }

    /// CHAT-14: `by` takes `principal` out of chat `chat` — a group's or a
    /// ticket's (a direct message stays between its two). By a participant
    /// (any one, itself too: it leaves) or, outside tickets, an admin of the
    /// chat's organization. One not in it is the chat as it is.
    pub async fn remove_from_chat(&self, on: impl Into<ChatOn>, chat: u64, by: Signer, principal: String) -> Result<ChatRecord, ChatError> {
        let on: ChatOn = on.into();
        let principal = principal.trim().to_string();
        let serial = self.serial().await;
        let (record, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut c = self.held(&cache, &on, chat, &by.principal)?;
            if !c.has(&by.principal) && !matches!(on, ChatOn::Admin(_)) {
                return Err(refused(format!("you are not in chat {chat}: only its participants remove")));
            }
            if !c.has(&principal) {
                return Ok(c);
            }
            if c.kind == ChatKind::Direct {
                return Err(refused(format!("chat {chat} is a direct message: it stays between its two — close it instead")));
            }
            if c.state == ChatState::Closed {
                return Err(refused(format!("chat {chat} is closed")));
            }
            c.parts.retain(|p| p.principal != principal);
            (c, on.key().and_then(|k| self.chats_before(&cache, k)))
        };
        let change = ChatChange::Removed { chat: record.clone(), principal };
        self.chat_write(serial, record.clone(), None, under, change).await.map(|()| record)
    }

    /// CHAT-14: the name `principal` is known by — signed in with, else the
    /// last its messages carried.
    pub fn name_of(&self, principal: &str) -> Option<String> {
        self.tickets.read().expect("store lock poisoned").chats.names.get(principal).cloned()
    }

    /// CHAT-14: whether `principal` is known, from a sign-in, by `name`.
    pub fn knows_name(&self, principal: &str, name: &str) -> bool {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.chats.signed.contains(principal) && cache.chats.names.get(principal).map(String::as_str) == Some(name.trim())
    }

    /// CHAT-14: `principal` signed in as `name` — kept, when it differs from
    /// the name known, so the picker finds it by that name after a restart.
    /// A name is the caller's own word (a synapse key's name, a provider's
    /// display name): shown, never trusted for access.
    pub async fn note_name(&self, _sys: &super::super::System, principal: &str, name: &str) -> Result<bool, ChatError> {
        let name: String = name.trim().chars().take(NAME_MAX).collect();
        if name.is_empty() || principal.trim().is_empty() {
            return Ok(false);
        }
        if self.knows_name(principal, &name) {
            return Ok(false);
        }
        let serial = self.serial().await;
        if self.knows_name(principal, &name) {
            return Ok(false);
        }
        let at = chrono::Utc::now().timestamp();
        let (p, n) = (principal.to_string(), name.clone());
        let (p2, n2) = (p.clone(), n.clone());
        self.write(
            serial,
            move |tx| {
                tx.prepare_cached("INSERT INTO principal_names (principal, name, at) VALUES (?1, ?2, ?3) ON CONFLICT (principal) DO UPDATE SET name = excluded.name, at = excluded.at")?
                    .execute(params![p, n, at])?;
                Ok(())
            },
            move |cache, _| {
                cache.chats.names.insert(p2.clone(), n2);
                cache.chats.signed.insert(p2);
            },
            |(), (), _| (),
        )
        .await
        .map_err(ChatError::from)?;
        Ok(true)
    }
}

impl Store {
    /// CHAT-14: chat `id` as kept, whoever asks — for the check that an
    /// admin of its organization may add or remove (never answered whole).
    pub fn chat_record(&self, id: u64) -> Option<ChatRecord> {
        self.tickets.read().expect("store lock poisoned").chats.by_id.get(&id).cloned()
    }

    /// CHAT-14: `c` as no one in particular sees it — what an admin who
    /// added or removed someone without reading the chat is answered: no
    /// unread, no mark.
    pub fn chat_bare(&self, c: &ChatRecord) -> crate::chats::Chat {
        let cache = self.tickets.read().expect("store lock poisoned");
        let key = (!c.kind.outside()).then(|| cache.resolve(&c.ticket).ok()).flatten();
        cache.chats.view(c, key, "")
    }
}
