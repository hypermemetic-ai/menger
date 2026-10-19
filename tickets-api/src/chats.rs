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
//! CHAT-14: a chat may also be **outside tickets** — a direct message
//! between two people ([`ChatKind::Direct`]) or a group with a title
//! ([`ChatKind::Group`]). Such a chat has no ticket and no ticket's reach:
//! only its participants read it, and it lives in one organization, whose
//! members (anyone holding a grant there) may be added, and whose admins
//! may add and remove members as its participants may.
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

/// CHAT-14: what a chat is — on a ticket (CHAT-1), or outside tickets: a
/// direct message between two, or a group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatKind {
    /// Opened inside a ticket; read through the ticket's reach.
    #[default]
    Ticket,
    /// A direct message: the opener and one other (and the standing chat
    /// members, CHAT-13). Opening one again answers the one there is.
    Direct,
    /// A group outside tickets: the opener and several others, with a title.
    Group,
}

impl ChatKind {
    pub const ALL: [ChatKind; 3] = [ChatKind::Ticket, ChatKind::Direct, ChatKind::Group];

    /// The wire and stored spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChatKind::Ticket => "ticket",
            ChatKind::Direct => "direct",
            ChatKind::Group => "group",
        }
    }

    /// Whether it is outside tickets.
    pub const fn outside(self) -> bool {
        !matches!(self, ChatKind::Ticket)
    }
}

impl FromStr for ChatKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ChatKind::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown chat kind '{}' (expected ticket, direct or group)", s.trim()))
    }
}

/// CHAT-16: what an admin's add grants one who can't read the chat's
/// ticket — the ticket alone, as a viewer (ACCESS-21's single-ticket
/// scope), or this chat only (the `chatter` role on the ticket: its chats
/// they are in, nothing of the ticket but its id and title). A direct
/// message or a group takes `chat` (the `chatter` role on its organization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatGrant {
    Ticket,
    Chat,
}

impl ChatGrant {
    pub const ALL: [ChatGrant; 2] = [ChatGrant::Ticket, ChatGrant::Chat];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChatGrant::Ticket => "ticket",
            ChatGrant::Chat => "chat",
        }
    }
}

impl fmt::Display for ChatGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ChatGrant {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ChatGrant::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown grant '{}' (expected ticket or chat)", s.trim()))
    }
}

/// CHAT-18: where a participant's live work stands — `started` (it has the
/// message), `working` (a line of what it does now; a heartbeat with none),
/// `ended` (the turn is over: answered, or given up).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    Started,
    #[default]
    Working,
    Ended,
}

impl ActivityState {
    pub const ALL: [ActivityState; 3] = [ActivityState::Started, ActivityState::Working, ActivityState::Ended];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            ActivityState::Started => "started",
            ActivityState::Working => "working",
            ActivityState::Ended => "ended",
        }
    }
}

impl fmt::Display for ActivityState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ActivityState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ActivityState::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown activity state '{}' (expected started, working or ended)", s.trim()))
    }
}

/// CHAT-18: the longest activity line kept, in characters — a longer one
/// is cut, `…` at its end.
pub const ACTIVITY_LINE_MAX: usize = 500;

/// CHAT-18: how many activity items a chat keeps in memory — the newest.
pub const ACTIVITY_KEPT: usize = 200;

/// CHAT-18: one item of a participant's live work in a chat — never a
/// message, never stored: the server keeps a chat's last
/// [`ACTIVITY_KEPT`] in memory (`activity_log`), lost on a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Activity {
    /// Its place among every chat's activity since the server started:
    /// a later item has a greater `seq` (the app's join of the log and the
    /// watch).
    pub seq: u64,
    pub chat: ChatId,
    /// Who works: the caller's principal.
    pub by: String,
    /// The name it goes by, when signed in with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When, unix seconds.
    pub at: i64,
    pub state: ActivityState,
    /// What it does — a tool call, a line of its text, a heartbeat — at most
    /// [`ACTIVITY_LINE_MAX`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

/// CHAT-18: `line` as an activity keeps it — trimmed, blank none, at most
/// [`ACTIVITY_LINE_MAX`] characters (cut, `…` last).
pub fn activity_line(line: Option<&str>) -> Option<String> {
    let t = line?.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars().count() <= ACTIVITY_LINE_MAX {
        return Some(t.to_string());
    }
    let mut cut: String = t.chars().take(ACTIVITY_LINE_MAX - 1).collect();
    cut.push('…');
    Some(cut)
}

/// CHAT-14: whether `principal` is drawn as an agent — an `agent:` (AGENT-1)
/// or a `key:` principal (a key is how an agent signs in: synapse's
/// `--as builder`). The standing chat members (CHAT-13) are named as agents
/// by `Chat.agents` beside this.
pub fn is_agent_principal(principal: &str) -> bool {
    principal.starts_with("agent:") || principal.starts_with("key:")
}

/// One chat, as one caller sees it: `unread` is the caller's own count —
/// the messages after its read mark (zero for an admin who is not in it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Chat {
    /// Its id: 1, 2, … in the order chats were opened, never reused.
    pub id: ChatId,
    /// CHAT-14: a ticket's chat, a direct message or a group.
    #[serde(default)]
    pub kind: ChatKind,
    /// The ticket it was opened on, by its key as it stands now — none for a
    /// direct message or a group (CHAT-14).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// CHAT-16: its ticket's title — what one who holds this chat only
    /// sees of the ticket, beside its id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Every participant, by principal (`anonymous` on one kept before ACCESS-24, when login could be off), in
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
    /// CHAT-8: how many messages it holds — the app's spacer for what it
    /// has not fetched.
    #[serde(default)]
    pub count: u64,
    /// CHAT-8: the caller's read mark — the last message it has read
    /// (none: nothing read, or not a participant). The app opens a chat
    /// with more unread than a page around it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<MessageId>,
    /// CHAT-16: the participants whose seat came with a grant an admin's
    /// add made, still held — removing one offers to revoke it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub granted: Vec<String>,
    /// The participants waiting on `await_message` for it now (CHAT-1's
    /// "waiting for a message"): who is listening, as the pane shows it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting: Vec<String>,
    /// CHAT-13: the participants who are the server's standing chat members
    /// (`--chat-member`, in every chat) — agents, drawn as CHAT-4/7 draw an
    /// `agent:` principal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
}

impl Chat {
    /// CHAT-16: whether `principal`'s seat came with a grant an admin's add
    /// made — what removing them offers to revoke.
    pub fn granted_to(&self, principal: &str) -> bool {
        self.granted.iter().any(|g| g == principal)
    }
}

/// One message, as kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Message {
    /// Its id: 1, 2, … across every chat, in the order posted, never reused
    /// — so a later message always has a greater id.
    pub id: MessageId,
    pub chat: ChatId,
    /// Who wrote it: the caller's principal (`anonymous` on one kept before ACCESS-24, when login could be off).
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
    /// CHAT-44: when and by whom it was deleted — kept and marked, never
    /// gone: `messages` still answers it, and the app folds a run of them
    /// into one line. A deleted message is no one's unread and no
    /// `await_message` reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Deleted>,
}

/// CHAT-44: a message's deletion — when (unix seconds) and by whom (its
/// author, or an admin of the chat's ticket or organization).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Deleted {
    pub at: i64,
    pub by: String,
}

/// CHAT-44: the most messages one `delete` or `restore` takes.
pub const DELETE_MAX: usize = 1_000;

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
    /// CHAT-4: one of `candidates`' rows — a principal who could be named in
    /// a chat on the ticket, and whether it can read the ticket now (one
    /// that cannot is shown in the picker and cannot be picked).
    /// CHAT-14: `name` — the name the server knows it by (the name it last
    /// signed in with, a synapse key's name included), and `agent` — drawn
    /// as an agent (an `agent:` or `key:` principal, or a standing chat
    /// member). Without a ticket, `reads` says it may be in a chat outside
    /// tickets (it holds a grant in the caller's organization).
    Candidate {
        principal: String,
        reads: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        agent: bool,
    },
    /// `post`'s answer, and on every participant's `watch`.
    Posted { message: Message },
    /// One of `messages`' rows (oldest first), and `await_message`'s answer.
    Message { message: Message },
    /// `add`'s answer, and on `watch`: the chat with its participants now.
    /// CHAT-16: `granted` — what the add granted, in words (`viewer on
    /// KILN-7`, `chatter on KILN-7`), when it granted.
    Added {
        chat: Chat,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        granted: Option<String>,
    },
    /// CHAT-16: `add`'s answer when the one named can't read the chat's
    /// ticket (or, outside tickets, holds no grant in its organization) and
    /// the call named no `grant`: nothing written; an admin adds again
    /// with one of `choices` (`ticket`, `chat`). The app's dialog.
    NeedsGrant { chat: ChatId, principal: String, #[serde(default, skip_serializing_if = "Option::is_none")] ticket: Option<String>, choices: Vec<ChatGrant> },
    /// CHAT-14: `remove`'s answer, and on `watch` to those still in it: the
    /// chat with its participants now. The one removed hears `left`.
    /// CHAT-16: `revoked` — the grant the add made was revoked with it.
    Removed {
        chat: Chat,
        principal: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        revoked: bool,
    },
    /// `read`'s answer, and on the reader's own other `watch`es: its mark
    /// in `chat` is `upto`, and `unread` are after it.
    Read { chat: ChatId, upto: MessageId, unread: u32 },
    /// `close`'s answer, and on `watch`.
    Closed { chat: ChatId },
    /// `reopen`'s answer, and on `watch`.
    Reopened { chat: ChatId },
    /// CHAT-7: `rename`'s answer, and on `watch`: the chat with its title now.
    Renamed { chat: Chat },
    /// CHAT-44: `delete`'s answer, and on `watch`: `ids` of `chat` deleted
    /// by `by` at `at` (only those it deleted now — one already deleted is
    /// not named again); `unread` is the caller's in the chat now.
    MessagesDeleted { chat: ChatId, ids: Vec<MessageId>, by: String, at: i64, unread: u32 },
    /// CHAT-44: `restore`'s answer, and on `watch`: `ids` of `chat` put back
    /// by `by`; `unread` is the caller's in the chat now.
    MessagesRestored { chat: ChatId, ids: Vec<MessageId>, by: String, at: i64, unread: u32 },
    /// On `watch`: `principal` began (`waiting: true`) or stopped waiting on
    /// `await_message` for `chat` — the pane's "waiting for a message".
    Listening { chat: ChatId, principal: String, waiting: bool },
    /// CHAT-18: `activity`'s answer, one of `activity_log`'s rows (oldest
    /// first), and on every participant's `watch`: `by`'s live work in
    /// `chat` — started, a line, ended. Never a message.
    Activity { activity: Activity },
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
        let m = Message { id: MessageId::new(3), chat: ChatId::new(41), author: "idp:ada".into(), name: None, via: None, at: 10, body: "1240 is allowed.".into(), deleted: None };
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
    fn an_activity_line_is_trimmed_and_cut() {
        assert_eq!(activity_line(None), None);
        assert_eq!(activity_line(Some("  ")), None);
        assert_eq!(activity_line(Some(" Read a.rs ")).as_deref(), Some("Read a.rs"));
        let long = "é".repeat(ACTIVITY_LINE_MAX + 20);
        let cut = activity_line(Some(&long)).unwrap();
        assert_eq!(cut.chars().count(), ACTIVITY_LINE_MAX);
        assert!(cut.ends_with('…'));
        let a = Activity { seq: 1, chat: ChatId::new(4), by: "key:b".into(), name: None, at: 9, state: ActivityState::Started, line: None };
        assert_eq!(
            serde_json::to_value(ChatsEvent::Activity { activity: a }).unwrap(),
            json!({ "type": "activity", "activity": { "seq": 1, "chat": 4, "by": "key:b", "at": 9, "state": "started" } })
        );
        assert_eq!("ENDED".parse::<ActivityState>(), Ok(ActivityState::Ended));
    }

    #[test]
    fn a_body_is_not_blank_and_within_the_cap() {
        assert!(check_body("hi", 8).is_ok());
        assert_eq!(check_body(" \n", 8).unwrap_err(), "the message's body is empty");
        assert!(check_body("123456789", 8).unwrap_err().contains("9 bytes, past the 8"));
    }
}

/// MACRO-21: the emitted client implements the activation's trait; a client
/// failure is the in-band error, the last item of its stream.
impl plexus_client::FromClientError for ChatsEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        ChatsEvent::Error { message: error.to_string() }
    }
}
