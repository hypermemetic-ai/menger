//! CHAT-20: `chats inputs` — a message's inputs (CHAT-19 decision `home`),
//! a child of `chats` on the frame: `list`, `answer`, `clear`, `close`,
//! `reopen`, reached as `chats.inputs.<method>`. Its grants are its own
//! (`message_inputs.<method>`, beside `tickets inputs`' `inputs.<method>`),
//! listed in its `roles(...)`: a viewer's and a chatter's, as every chat
//! method is — what a call may do is the chat's to say (its participants).
//!
//! An answer's `source` and `by` are the server's, from the verified caller
//! ([`crate::activation::who_of`]), as `tickets inputs`' are (ACCESS-14).

#[cfg(feature = "server")]
use crate::chat_inputs::{INPUTS_PAGE_DEFAULT, INPUTS_PAGE_MAX};
use crate::chats::ChatsEvent;
use crate::types::{InputKind, InputState, InputValue};
use crate::vocab::{ChatId, InputId, Message, MessageId, TicketRef};
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::{ChatInputOp, ChatInputQuery, ChatInputWrite, Store};
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The inputs child. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct MessageInputsActivation {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

/// One write's answer as its event: the input as it is now (`input` when
/// nothing moved), or the refusal.
#[cfg(feature = "server")]
fn written(answer: Result<ChatInputWrite, crate::store::ChatError>) -> ChatsEvent {
    match answer {
        Ok(ChatInputWrite::Changed(input)) => ChatsEvent::InputChanged { input },
        Ok(ChatInputWrite::Unchanged(input)) => ChatsEvent::Input { input },
        Err(e) => super::streams::refusal(e),
    }
}

/// `answer`, `clear`, `close`, `reopen`: one write by the caller.
#[cfg(feature = "server")]
fn write(store: Arc<Store>, user: VerifiedUser, chat: ChatId, message: MessageId, input: InputId, op: ChatInputOp) -> impl Stream<Item = ChatsEvent> + Send + 'static {
    futures::stream::once(async move {
        let who = crate::activation::who_of(&user);
        written(store.chat_input_write(&user, who, chat.get(), message.get(), input.as_str(), op).await)
    })
}

// Not `#[cfg]`-gated: the client-only build needs `MessageInputsClient` too.
/// A message's inputs: asked in a chat, answered by its participants, closed by who asked (CHAT-20).
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["list", "answer", "clear", "close", "reopen"],
        editor = [viewer],
        chatter = ["list", "answer", "clear", "close", "reopen"],
    )
)]
impl MessageInputsActivation {
    /// The inputs of the messages in the caller's chats (an <m-decision id> / <m-input id kind> in a message's body), oldest message first; an input { input } each, then page { next }.
    ///
    /// Each with its home (chat, message, the chat's ticket, who asked),
    /// kind, question, options, recommendation, state, answer (`by`, `name`,
    /// `source`, `at_unix`) and whole history. `chat` one chat (one the
    /// caller does not see is `not_found`), else every chat the caller is
    /// in; `ticket` only chats on it. `after` (a message id) continues a
    /// page; `limit` 100 unless given, at most 1,000 — a message's inputs
    /// are never split. A deleted message's are not listed.
    #[plexus_macros::method(
        group = "read",
        values(state = ["open", "answered", "closed"], kind = ["choice", "multi", "text"]),
        example(note = "every question waiting in the caller's chats", params = r#"{"state": "open"}"#)
    )]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only this chat's, e.g. 12 (optional).
        chat: Option<ChatId>,
        /// Only chats on this ticket, e.g. KILN-40 (optional).
        ticket: Option<TicketRef>,
        /// Only inputs in this state: open | answered | closed (optional).
        state: Option<InputState>,
        /// Only inputs of this kind: choice | multi | text (optional).
        kind: Option<InputKind>,
        /// Cursor: the inputs of messages after this one (optional).
        after: Option<MessageId>,
        /// At most this many inputs (default 100, at most 1000) (optional).
        limit: Option<u32>,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        let q = ChatInputQuery { chat: chat.map(ChatId::get), ticket: ticket.map(TicketRef::into_string), state, kind };
        let limit = limit.unwrap_or(INPUTS_PAGE_DEFAULT).clamp(1, INPUTS_PAGE_MAX);
        let events = match self.store.chat_inputs(&user, &q, after.map(MessageId::get), limit) {
            Ok((rows, next)) => rows.into_iter().map(|input| ChatsEvent::Input { input }).chain([ChatsEvent::Page { next }]).collect(),
            Err(e) => vec![super::streams::refusal(e)],
        };
        futures::stream::iter(events)
    }

    /// Answer a message's input — a value checked against its kind and options, with a rationale; who, the source and the time said by the server; yields input_changed { input }.
    ///
    /// By a participant of the chat, while the chat is open and the message
    /// kept. The value is typed as `tickets inputs answer`'s:
    /// `{"type": "choice", "option": …}`, `{"type": "multi", "options": […]}`,
    /// `{"type": "text", "text": …}`. Every answer is a history entry. A
    /// closed input, a value that does not answer it, or one the caller may
    /// not give is `refused { reasons }`, and nothing is written. `watch`
    /// carries the `input_changed` to every participant; it answers an
    /// `await_message` waiting on it.
    #[plexus_macros::method(
        group = "write",
        example(note = "pick an option", params = r#"{"chat": 12, "message": 812, "input": "undated", "value": {"type": "choice", "option": "keep"}}"#)
    )]
    pub async fn answer(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 12.
        chat: ChatId,
        /// The message that asks, e.g. 812.
        message: MessageId,
        /// The input's id in that message's body, e.g. undated.
        input: InputId,
        /// Typed by the input's kind: {"type": "choice", "option": "keep"} | {"type": "multi", "options": ["a", "b"]} | {"type": "text", "text": "a reply"}; through synapse, --value.type choice --value.option keep.
        value: InputValue,
        /// Why (optional).
        rationale: Option<Message>,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        write(self.store.clone(), user, chat, message, input, ChatInputOp::Answer { value, rationale: rationale.and_then(Message::given) })
    }

    /// Clear a message's input's answer (open again; kept in its history); yields input_changed { input }, or input { input } when it held none.
    ///
    /// By the one who gave the answer (any participant, for an answer kept
    /// before who answered was), while the chat is open. A closed input is
    /// `refused`.
    #[plexus_macros::method(group = "write")]
    pub async fn clear(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 12.
        chat: ChatId,
        /// The message that asks, e.g. 812.
        message: MessageId,
        /// The input's id in that message's body.
        input: InputId,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        write(self.store.clone(), user, chat, message, input, ChatInputOp::Clear)
    }

    /// Close a message's input: freeze its answer (DECIDED, for a decision); yields input_changed { input }, or input { input } when it was closed.
    ///
    /// By the message's author — who asked — or an admin of the chat's
    /// ticket. An answer or a clear is refused until it is reopened.
    #[plexus_macros::method(group = "write")]
    pub async fn close(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 12.
        chat: ChatId,
        /// The message that asks, e.g. 812.
        message: MessageId,
        /// The input's id in that message's body.
        input: InputId,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        write(self.store.clone(), user, chat, message, input, ChatInputOp::Close)
    }

    /// Reopen a closed message's input; yields input_changed { input }, or input { input } when it was not closed.
    ///
    /// By the message's author or an admin of the chat's ticket: `answered`
    /// again when it holds an answer, else `open`.
    #[plexus_macros::method(group = "write")]
    pub async fn reopen(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The chat, e.g. 12.
        chat: ChatId,
        /// The message that asks, e.g. 812.
        message: MessageId,
        /// The input's id in that message's body.
        input: InputId,
    ) -> impl Stream<Item = ChatsEvent> + Send + 'static {
        write(self.store.clone(), user, chat, message, input, ChatInputOp::Reopen)
    }
}
