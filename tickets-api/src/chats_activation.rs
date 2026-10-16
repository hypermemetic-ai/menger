//! CHAT-3: the `chats` activation — **the one declaration**, as `tickets`
//! and `comments` are (MFE-11).
//!
//! | build | what the macro emits from it |
//! |---|---|
//! | `server` (default; the menger binary) | the `Activation` impl, dispatch, the schema, these bodies |
//! | `default-features = false` (the browser app, wasm32) | `ChatsClient<T>` — one typed method per `#[method]`, over [`crate::chats`]' types |
//!
//! The chats are kept by the store, behind the service-wide cache
//! ([`crate::store`]'s `chats`); this activation holds the same
//! `Arc<Store>` as `tickets`, so a chat's ticket is the store's ticket and a
//! participant's reach is the store's grants. Every method's logic lives
//! beside it, in `chats_activation/` (MENGER-29): each body here is its
//! attribute, the caller and one call.

use crate::chats::{ChatState, ChatsEvent};
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::vocab::{Body, ChatId, MessageId, Principal, TicketRef, Title};
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::{ChatQuery, Store};
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

#[cfg(feature = "server")]
mod streams;
#[cfg(feature = "server")]
use streams::*;
#[cfg(feature = "server")]
mod await_message;

/// The activation. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Chats {
    store: Arc<Store>,
    access: Access,
}

#[cfg(feature = "server")]
impl Chats {
    /// No login: every call admitted, as always.
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_access(store, Access::open())
    }

    /// Behind [`Access`].
    pub fn with_access(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
// A viewer chats, as a viewer comments: every method is a viewer's — what
// a call may do in a chat is the chat's to say (its participants), not a
// role's.
/// Chats on a ticket among participants — open, post, add, list, messages, read, close, reopen, watch, await_message.
#[plexus_macros::activation(
    namespace = "chats",
    auth = "required",
    request = MengerRequest,
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    roles(
        viewer = ["open", "post", "add", "list", "messages", "read", "close", "reopen", "watch", "await_message"],
        editor = [viewer],
    )
)]
impl Chats {
    /// Open a chat on a ticket with the principals named; yields opened { chat } — the open chat they already have there, when they do (existing: true).
    ///
    /// The caller and `others` are the participants (CHAT-1's `with`, which
    /// TypeScript does not take as a parameter's name); every one of them must be
    /// able to read the ticket, or the call is `refused { reasons }`, naming
    /// each who cannot, and nothing is made. A ticket the caller cannot read
    /// is refused as one that is none. `watch` carries `opened` to each
    /// participant.
    #[plexus_macros::method(
        group = "write",
        example(note = "ada asks builder about KILN-7", params = r#"{"ticket": "KILN-7", "others": ["key:4b2e0000000000000000000000000000000000000000000000000000000000aa"]}"#)
    )]
    pub async fn open(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The ticket the chat is about, e.g. KILN-7 (required: there is no chat without a ticket).
        ticket: TicketRef,
        /// Who else is in it: principals, each able to read the ticket.
        others: Vec<Principal>,
        /// A title for it (optional).
        title: Option<Title>,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let (store, access) = (self.store.clone(), self.access.clone());
        let others = others.into_iter().map(Principal::into_string).collect();
        futures::stream::once(async move { open(store, access, user, ticket.into_string(), others, title.map(Title::into_string)).await })
    }

    /// Post a message in a chat; yields posted { message }, signed by the caller.
    ///
    /// Only a participant posts; a closed chat is `refused`. The body is
    /// markdown, not blank, at most the ticket body cap. A chat the caller
    /// does not see is `not_found`. `watch` carries `posted` to every
    /// participant, and it answers their `await_message`.
    #[plexus_macros::method(group = "write")]
    pub async fn post(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 41.
        chat: ChatId,
        /// What it says (markdown).
        body: Body,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { post(store, user, chat.get(), body.into_string()).await })
    }

    /// Add a participant to a chat; yields added { chat }.
    ///
    /// By a participant, to an open chat; the one added must be able to read
    /// the ticket, or it is `refused`. One already in it is the chat as it is.
    #[plexus_macros::method(group = "write")]
    pub async fn add(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 41.
        chat: ChatId,
        /// Who to add: a principal.
        principal: Principal,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let (store, access) = (self.store.clone(), self.access.clone());
        futures::stream::once(async move { add(store, access, user, chat.get(), principal.into_string()).await })
    }

    /// The caller's chats, by last activity, newest first — on a ticket, with or without unread, open or closed; a chat { chat } each, then page { next }.
    ///
    /// Without `ticket`, the chats the caller is in; with it, those on that
    /// ticket the caller is in — every one there, for an admin of it. Each
    /// chat's `unread` is the caller's. `after` (a chat id) continues a page;
    /// `limit` is 50 unless given, at most 500.
    #[plexus_macros::method(group = "read", values(state = ["open", "closed"]))]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only chats on this ticket, e.g. KILN-7 (optional).
        ticket: Option<TicketRef>,
        /// Only chats with unread messages for the caller (true), or without (false) (optional).
        unread: Option<bool>,
        /// Only chats in this state: open | closed (optional).
        state: Option<ChatState>,
        /// Cursor: continue after this chat (optional).
        after: Option<ChatId>,
        /// At most this many chats (default 50, at most 500) (optional).
        limit: Option<u32>,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let q = ChatQuery { ticket: ticket.map(TicketRef::into_string), unread, state };
        list_stream(self.store.clone(), user, q, after.map(ChatId::get), limit)
    }

    /// A chat's messages, oldest first; a message { message } each, then page { next }.
    ///
    /// For a participant (or an admin of its ticket); anyone else is
    /// answered `not_found`. `after` (a message id) continues a page;
    /// `limit` is 100 unless given, at most 1,000.
    #[plexus_macros::method(group = "read")]
    pub async fn messages(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 41.
        chat: ChatId,
        /// Cursor: the messages after this one (optional).
        after: Option<MessageId>,
        /// At most this many messages (default 100, at most 1000) (optional).
        limit: Option<u32>,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        messages_stream(self.store.clone(), user, chat.get(), after.map(MessageId::get), limit)
    }

    /// Mark a chat read up to a message; yields read { chat, upto, unread }.
    ///
    /// Moves the caller's own mark forward (never back) to the latest of
    /// the chat's messages at or before `upto`; `unread` counts after it.
    /// `watch` carries `read` to the caller's other watches.
    #[plexus_macros::method(group = "write")]
    pub async fn read(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 52.
        chat: ChatId,
        /// The last message read, e.g. 7.
        upto: MessageId,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { read(store, user, chat.get(), upto.get()).await })
    }

    /// Close a chat; yields closed { chat }. A closed chat is read-only, kept on its ticket.
    ///
    /// By any participant. Closing a closed chat changes nothing.
    #[plexus_macros::method(group = "write")]
    pub async fn close(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 41.
        chat: ChatId,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { settle(store, user, chat.get(), false).await })
    }

    /// Reopen a closed chat; yields reopened { chat }.
    ///
    /// By any participant. Reopening an open chat changes nothing.
    #[plexus_macros::method(group = "write")]
    pub async fn reopen(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 41.
        chat: ChatId,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { settle(store, user, chat.get(), true).await })
    }

    /// Subscribe to the caller's chats: opened, posted, added, read, closed, reopened, listening, left — until disconnect.
    ///
    /// The chats the caller is in (with `ticket`, those on it — every one
    /// there for an admin of it), each change as the caller sees it: its own
    /// unread, its own `read`s. A chat whose ticket leaves the caller's
    /// reach is said once as `left { chat }`. ACCESS-12: the caller is
    /// verified again as its grants move; refused, the watch ends with
    /// `watch_closed { reason, message }`. `lagged { missed }` where a slow
    /// subscriber lost changes.
    #[plexus_macros::method(group = "read", streaming)]
    pub async fn watch(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only chats on this ticket, e.g. KILN-7 (optional).
        ticket: Option<TicketRef>,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        watch_stream(self.access.clone(), self.store.clone(), user, ticket.map(TicketRef::into_string))
    }

    /// Wait for the next message for the caller: waiting at once, then one message { message } by someone else — in the chat named, or any of the caller's chats (on ticket, when named) — and the end; timed_out after timeout_minutes.
    ///
    /// The first message after `since` (unix seconds, at or after it; without
    /// it, after the call) and after the message `after` by anyone but the
    /// caller — so a reconnect with the `since` that `waiting` said, or the
    /// last message it read as `after` (exact, where a second can hold two
    /// messages), loses nothing. While it waits, the chats it waits on
    /// show the caller as `listening` on `watch`. A chat the caller is not
    /// in is `not_found`.
    #[plexus_macros::method(group = "read", streaming)]
    pub async fn await_message(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 41 (optional; default any of the caller's).
        chat: Option<ChatId>,
        /// Only the caller's chats on this ticket (optional).
        ticket: Option<TicketRef>,
        /// Messages from this unix time, so a reconnect loses nothing (optional; default now).
        since: Option<i64>,
        /// Only messages after this one — the last the caller read (optional).
        after: Option<MessageId>,
        /// Give up after this many minutes with timed_out (optional; default never).
        timeout_minutes: Option<u32>,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let ask = await_message::Ask { chat: chat.map(ChatId::get), ticket: ticket.map(TicketRef::into_string), since, after: after.map(MessageId::get), timeout_minutes };
        await_message::await_message_stream(self.store.clone(), self.access.clone(), user, ask)
    }
}
