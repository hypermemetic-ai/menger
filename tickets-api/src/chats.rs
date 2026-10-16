//! CHAT-3: the chats vocabulary — the one declaration.
//!
//! A **chat** is a conversation among named participants, opened inside one
//! ticket (CHAT-1, owner 2026-10-16: *"chats should be opened inside of
//! tickets"*, and *"it's its own record"* — not a kind of comment). It is
//! kept by the server beside the tickets (`store::chats`) and reached over
//! the `chats` activation ([`crate::chats_activation`]); the browser app
//! reads *these* types, as it reads the tickets' (MFE-9), so a renamed field
//! fails its build.
//!
//! Only a chat's participants (and an admin of its ticket) read it; to
//! anyone else it is not there (ACCESS-9). Every participant can read the
//! ticket, and one who loses that reach loses the chat. A message is signed
//! by its caller; `via` is kept for AGENT-3 (an agent's message names the
//! process that held the agent) and is never set by a call.
//!
//! Wasm-safe, with no feature gate.

use crate::vocab::{ChatId, MessageId};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The most participants a chat holds, the caller included.
pub const PARTICIPANTS_MAX: usize = 32;

/// `messages`' page: 100 unless `limit` says, at most 1,000.
pub const MESSAGES_PAGE_DEFAULT: u32 = 100;
pub const MESSAGES_PAGE_MAX: u32 = 1_000;

/// `list`'s page: 50 unless `limit` says, at most 500.
pub const CHATS_PAGE_DEFAULT: u32 = 50;
pub const CHATS_PAGE_MAX: u32 = 500;

/// Where a chat stands: `open` until a participant closes it; a closed chat
/// is read-only (no post, no add) and is reopened by any participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatState {
    Open,
    Closed,
}

impl ChatState {
    pub const ALL: [ChatState; 2] = [ChatState::Open, ChatState::Closed];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChatState::Open => "open",
            ChatState::Closed => "closed",
        }
    }
}

impl fmt::Display for ChatState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ChatState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ChatState::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown chat state '{}' (expected open or closed)", s.trim()))
    }
}

/// One chat, as one caller sees it: `unread` is the caller's own count —
/// the messages after its read mark (zero for an admin who is not in it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Chat {
    /// Its id: 1, 2, … in the order chats were opened, never reused.
    pub id: ChatId,
    /// The ticket it was opened on, by its key as it stands now.
    pub ticket: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Every participant, by principal (`anonymous` with login off), in
    /// the order they joined — the opener first.
    pub participants: Vec<String>,
    pub state: ChatState,
    /// Who opened it, and when (unix seconds).
    pub opened_by: String,
    pub opened_at: i64,
    /// The time of its last message — its opening, until one is posted.
    pub last_at: i64,
    /// Its last message's id, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<MessageId>,
    /// The caller's unread messages in it.
    pub unread: u32,
    /// The participants waiting on `await_message` for it now (CHAT-1's
    /// "waiting for a message"): who is listening, as the pane shows it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting: Vec<String>,
}

/// One message, as kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Message {
    /// Its id: 1, 2, … across every chat, in the order posted, never reused
    /// — so a later message always has a greater id.
    pub id: MessageId,
    pub chat: ChatId,
    /// Who wrote it: the caller's principal (`anonymous` with login off).
    pub author: String,
    /// The name the author went by when it wrote it, when signed in with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// AGENT-3: the process that held the agent who wrote it (its claimant).
    /// Kept, never set by a call; `None` until agents claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// When, unix seconds.
    pub at: i64,
    /// What it says: markdown, rendered by the app's `md.rs`, never framed.
    pub body: String,
}

/// What the `chats` activation's methods yield. In-band errors are
/// `Error { message }`, as every menger method's; a chat the caller does
/// not see is `not_found`, whether it is there or not (ACCESS-9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatsEvent {
    /// `open`'s answer, and on `watch` a chat opened with the caller in it.
    /// `existing`: the same participants already had this chat open on the
    /// ticket, and nothing was made.
    Opened {
        chat: Chat,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        existing: bool,
    },
    /// One of `list`'s rows.
    Chat { chat: Chat },
    /// `post`'s answer, and on every participant's `watch`.
    Posted { message: Message },
    /// One of `messages`' rows (oldest first), and `await_message`'s answer.
    Message { message: Message },
    /// `add`'s answer, and on `watch`: the chat with its participants now.
    Added { chat: Chat },
    /// `read`'s answer, and on the reader's own other `watch`es: its mark
    /// in `chat` is `upto`, and `unread` are after it.
    Read { chat: ChatId, upto: MessageId, unread: u32 },
    /// `close`'s answer, and on `watch`.
    Closed { chat: ChatId },
    /// `reopen`'s answer, and on `watch`.
    Reopened { chat: ChatId },
    /// On `watch`: `principal` began (`waiting: true`) or stopped waiting on
    /// `await_message` for `chat` — the pane's "waiting for a message".
    Listening { chat: ChatId, principal: String, waiting: bool },
    /// AGENT-3: a participating agent's claim began or ended — a line in the
    /// chat. Declared for AGENT-3's claims; nothing emits it yet.
    ClaimLine { chat: ChatId, agent: String, holder: String, began: bool, at: i64 },
    /// `await_message`'s first event: waiting for a message in `chat` (or
    /// any of the caller's chats, on `ticket` when one is named), from
    /// `since` (unix seconds) — what to pass again after a dropped
    /// connection.
    Waiting {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chat: Option<ChatId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ticket: Option<String>,
        since: i64,
    },
    /// `await_message`: nothing came within `timeout_minutes`. Last.
    TimedOut,
    /// Where `list` or `messages` stands, once, after its rows: `next` is
    /// the cursor to pass as the next call's `after`, `null` when nothing
    /// more matches.
    Page { next: Option<u64> },
    /// On `watch`: the caller can no longer read `chat` (its reach on the
    /// ticket went); it is not found for it from now.
    Left { chat: ChatId },
    /// The chat is not one the caller sees — not there, or not its.
    NotFound { chat: ChatId },
    /// Refused in band, every reason named; nothing was written.
    Refused { reasons: Vec<String> },
    /// `watch`: this subscriber fell behind and `missed` changes were
    /// dropped — `list` again to catch up.
    Lagged { missed: u64 },
    /// ACCESS-12 (`watch`, `await_message`): the stream ended — its caller,
    /// verified again when the grants moved or its sign-in ended, was
    /// refused. Last.
    WatchClosed { reason: crate::types::WatchEnd, message: String },
    Error { message: String },
}

/// A body as a message keeps it — not blank, at most `max` bytes (the
/// store's ticket body cap) — or why not.
pub fn check_body(body: &str, max: usize) -> Result<(), String> {
    if body.trim().is_empty() {
        return Err("the message's body is empty".into());
    }
    if body.len() > max {
        return Err(format!("the message's body is {} bytes, past the {max} a message keeps", body.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_wire_shapes_are_the_spec_s() {
        let m = Message { id: MessageId::new(3), chat: ChatId::new(41), author: "idp:ada".into(), name: None, via: None, at: 10, body: "1240 is allowed.".into() };
        assert_eq!(
            serde_json::to_value(ChatsEvent::Posted { message: m.clone() }).unwrap(),
            json!({ "type": "posted", "message": { "id": 3, "chat": 41, "author": "idp:ada", "at": 10, "body": "1240 is allowed." } })
        );
        assert_eq!(serde_json::to_value(ChatsEvent::TimedOut).unwrap(), json!({ "type": "timed_out" }));
        assert_eq!(serde_json::to_value(ChatsEvent::Closed { chat: ChatId::new(41) }).unwrap(), json!({ "type": "closed", "chat": 41 }));
        assert_eq!(serde_json::to_value(ChatsEvent::Refused { reasons: vec!["chat 41 is closed".into()] }).unwrap()["reasons"][0], "chat 41 is closed");
        let back: ChatsEvent = serde_json::from_value(json!({ "type": "message", "message": serde_json::to_value(&m).unwrap() })).unwrap();
        assert_eq!(back, ChatsEvent::Message { message: m });
        assert_eq!("Closed".parse::<ChatState>(), Ok(ChatState::Closed));
        assert!("resolved".parse::<ChatState>().unwrap_err().contains("open or closed"));
    }

    #[test]
    fn a_body_is_not_blank_and_within_the_cap() {
        assert!(check_body("hi", 8).is_ok());
        assert_eq!(check_body(" \n", 8).unwrap_err(), "the message's body is empty");
        assert!(check_body("123456789", 8).unwrap_err().contains("9 bytes, past the 8"));
    }
}
