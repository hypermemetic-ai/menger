//! CHAT-20: **a message's inputs** — the wire half (wasm-safe, no feature
//! gate): an `<m-decision id>` or `<m-input id kind>` in a chat message's
//! body is an input whose home is that message (CHAT-19's *Questions in a
//! message are inputs*; decision `home`: `chats inputs …`, one store
//! underneath). It is read from the body at `post` by the same
//! [`crate::inputs::read`] a ticket's body is, born `open`, and moves as a
//! ticket's input moves — answered, cleared, closed, reopened — by the
//! chat's participants, each change kept in its history with who
//! (`by`, `name`), the source and when. A message is never edited, so its
//! inputs are never `removed`.

use crate::types::{Answer, InputHistoryEntry, InputKind, InputOption, InputState};
use crate::vocab::{ChatId, InputId, MessageId};
use serde::{Deserialize, Serialize};

/// `chats inputs list`'s page: 100 unless `limit` says, at most 1,000.
pub const INPUTS_PAGE_DEFAULT: u32 = 100;
pub const INPUTS_PAGE_MAX: u32 = 1_000;

/// One input of a message, as the store keeps it: its home — the chat, the
/// message, the chat's ticket by its key now (none outside tickets) and
/// who asked (the message's author) — then what a ticket's input carries:
/// its id (unique in the message), kind, question, options, recommendation,
/// state, current answer (`null` with none) and whole history, oldest
/// first, and when it was asked (the post).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatInput {
    pub chat: ChatId,
    pub message: MessageId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    pub author: String,
    pub id: InputId,
    pub kind: InputKind,
    pub question: Option<String>,
    pub options: Vec<InputOption>,
    pub recommendation: Option<String>,
    pub state: InputState,
    pub answer: Option<Answer>,
    pub history: Vec<InputHistoryEntry>,
    #[serde(default)]
    pub asked_unix: Option<i64>,
}

impl ChatInput {
    /// `<message>/<input>` — what `await_message --input` and `tickets
    /// queue --message` name it by.
    pub fn name(&self) -> String {
        format!("{}/{}", self.message.get(), self.id.as_str())
    }
}
