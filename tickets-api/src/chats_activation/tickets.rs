//! CHAT-55: `chats.tickets` and `chats.link`, beside the activation — a
//! chat's tickets read through the chat and the caller's reach
//! ([`Store::chat_tickets`]), and tickets brought into a chat with the
//! caller's seat in it and a viewer's permit on each
//! ([`Store::link_chat`]). `tickets.create --chat` links the ticket it
//! files through [`filed`].

use super::streams::refusal;
use crate::access::{VerifiedUser, Viewer};
use crate::address::TicketId;
use crate::chat_tickets::MentionHow;
use crate::chats::ChatsEvent;
use crate::store::{ChatError, Signer, Store};
use crate::vocab::ChatId;
use std::sync::Arc;

/// `chats.tickets`: a `ticket` row each, newest first — or `not_found`.
pub(super) fn tickets(store: &Store, user: &VerifiedUser, chat: u64) -> Vec<ChatsEvent> {
    match store.chat_tickets(user, chat) {
        Ok(rows) => rows.into_iter().map(|ticket| ChatsEvent::Ticket { ticket }).collect(),
        Err(e) => vec![refusal(e)],
    }
}

/// The permits `names` take, each a ticket the caller reads — or the
/// refusal naming each one it does not (as one that is none).
fn permits(store: &Store, user: &VerifiedUser, names: &[String]) -> Result<Vec<crate::access::Permit<Viewer>>, Vec<String>> {
    let mut out = Vec::new();
    let mut missing = Vec::new();
    for n in names {
        match store.ticket_id(n).and_then(|k| user.permit::<Viewer>(&TicketId::new(k), store).ok()) {
            Some(p) => out.push(p),
            None => missing.push(format!("no ticket {n}")),
        }
    }
    match missing.is_empty() {
        true => Ok(out),
        false => Err(missing),
    }
}

/// `chats.link` (and a filing's link): `names` into `chat` as `how` — all or
/// none; answers `linked` with those new to it.
pub(super) async fn link(store: Arc<Store>, user: &VerifiedUser, chat: u64, names: Vec<String>, how: MentionHow) -> ChatsEvent {
    if names.is_empty() {
        return ChatsEvent::Refused { reasons: vec!["no ticket named: give --tickets".into()] };
    }
    let s = match store.chat_seen(user, chat) {
        Ok((_, s)) => s,
        Err(e) => return refusal(e),
    };
    let on = match permits(&store, user, &names) {
        Ok(p) => p,
        Err(reasons) => return ChatsEvent::Refused { reasons },
    };
    match store.link_chat(s.on, chat, Signer::of(user), on, how).await {
        Ok(tickets) => ChatsEvent::Linked { chat: ChatId::new(chat), tickets, how },
        Err(e) => refusal(e),
    }
}

/// `tickets.create --chat N`, before the birth: the caller must be in chat
/// `chat` — a refusal in words, and nothing filed. Answers the chat's number.
pub fn may_file_in(store: &Store, user: &VerifiedUser, chat: Option<ChatId>) -> Result<Option<u64>, String> {
    let Some(chat) = chat.map(|c| c.get()) else { return Ok(None) };
    match store.chat_seen(user, chat) {
        Ok((_, s)) if s.participant => Ok(Some(chat)),
        Ok(_) => Err(format!("you are not in chat {chat}: only its participants file tickets from it")),
        Err(ChatError::NotFound(_)) | Err(_) => Err(format!("no chat {chat}")),
    }
}

/// …and after it: the ticket filed, linked to the chat (`filed`). A link
/// that fails leaves the ticket filed and says why in the log.
pub async fn filed(store: Arc<Store>, user: &VerifiedUser, chat: Option<u64>, key: String) {
    let Some(chat) = chat else { return };
    match link(store, user, chat, vec![key.clone()], MentionHow::Filed).await {
        ChatsEvent::Linked { .. } => {}
        other => tracing::warn!("CHAT-55: {key} was filed but not linked to chat {chat}: {other:?}"),
    }
}
