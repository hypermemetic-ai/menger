//! CHAT-3: what the `chats` methods do, beside the activation (its `impl`
//! keeps only the methods — MENGER-29, ACCESS-13's frame). Each takes the
//! [`VerifiedUser`] the dispatch resolved; nothing here admits by hand. A
//! chat's write takes a viewer's permit on its ticket (ACCESS-11), minted
//! through the caller's standing ([`Store::standing`]); the store checks,
//! under its write lock, that the permit is for that chat's ticket and the
//! signer is in it.

use crate::access::{Access, Following, Followed, VerifiedUser, Viewer};
use crate::address::TicketId;
use crate::chats::{Chat, ChatsEvent, CHATS_PAGE_DEFAULT, CHATS_PAGE_MAX, MESSAGES_PAGE_DEFAULT, MESSAGES_PAGE_MAX};
use crate::store::{ChatChange, ChatError, ChatQuery, Signer, Store};
use crate::vocab::{ChatId, MessageId};
use async_stream::stream;
use futures::Stream;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

/// A store refusal as its wire event.
pub(super) fn refusal(e: ChatError) -> ChatsEvent {
    match e {
        ChatError::NotFound(chat) => ChatsEvent::NotFound { chat: ChatId::new(chat) },
        ChatError::Refused(reasons) => ChatsEvent::Refused { reasons },
        ChatError::Store(message) => ChatsEvent::Error { message },
    }
}

/// The chat `id` as `user` sees it now, as `ev` carries it.
fn viewed(store: &Store, user: &VerifiedUser, id: u64, ev: impl FnOnce(Chat) -> ChatsEvent) -> ChatsEvent {
    store.chat_view(user, id).map_or(ChatsEvent::NotFound { chat: ChatId::new(id) }, ev)
}

/// `chats.open`: the caller must read the ticket (one it cannot is one
/// that is none); each of `others` must too ([`Access::reads_ticket`]).
pub(super) async fn open(store: Arc<Store>, access: Access, user: VerifiedUser, ticket: String, with: Vec<String>, title: Option<String>) -> ChatsEvent {
    let permit = store.ticket_id(&ticket).and_then(|k| user.permit::<Viewer>(&TicketId::new(k), &store).ok());
    let Some(on) = permit else {
        return ChatsEvent::Refused { reasons: vec![format!("no ticket {}", ticket.trim().to_ascii_uppercase())] };
    };
    let key = on.id().as_str().to_string();
    let reads = |p: &str| access.reads_ticket(p, &key, &store);
    match store.open_chat(on, Signer::of(&user), with, title, reads).await {
        Ok((c, existing)) => viewed(&store, &user, c.id, |chat| ChatsEvent::Opened { chat, existing }),
        Err(e) => refusal(e),
    }
}

/// `chats.post`.
pub(super) async fn post(store: Arc<Store>, user: VerifiedUser, chat: u64, body: String) -> ChatsEvent {
    let s = match store.chat_seen(&user, chat) {
        Ok((_, s)) => s,
        Err(e) => return refusal(e),
    };
    match store.post_chat(s.permit, chat, Signer::of(&user), body).await {
        Ok(message) => ChatsEvent::Posted { message },
        Err(e) => refusal(e),
    }
}

/// `chats.add`: the one added must read the ticket.
pub(super) async fn add(store: Arc<Store>, access: Access, user: VerifiedUser, chat: u64, principal: String) -> ChatsEvent {
    let s = match store.chat_seen(&user, chat) {
        Ok((_, s)) => s,
        Err(e) => return refusal(e),
    };
    let key = s.key.clone();
    let reads = |p: &str| access.reads_ticket(p, &key, &store);
    match store.add_to_chat(s.permit, chat, Signer::of(&user), principal, reads).await {
        Ok(c) => viewed(&store, &user, c.id, |chat| ChatsEvent::Added { chat }),
        Err(e) => refusal(e),
    }
}

/// `chats.read`.
pub(super) async fn read(store: Arc<Store>, user: VerifiedUser, chat: u64, upto: u64) -> ChatsEvent {
    let s = match store.chat_seen(&user, chat) {
        Ok((_, s)) => s,
        Err(e) => return refusal(e),
    };
    match store.read_chat(s.permit, chat, Signer::of(&user), upto).await {
        Ok((upto, unread)) => ChatsEvent::Read { chat: ChatId::new(chat), upto: MessageId::new(upto), unread },
        Err(e) => refusal(e),
    }
}

/// `chats.close` (`open: false`) and `chats.reopen` (`open: true`).
pub(super) async fn settle(store: Arc<Store>, user: VerifiedUser, chat: u64, open: bool) -> ChatsEvent {
    let s = match store.chat_seen(&user, chat) {
        Ok((_, s)) => s,
        Err(e) => return refusal(e),
    };
    let chat_id = ChatId::new(chat);
    match store.set_chat_state(s.permit, chat, Signer::of(&user), open).await {
        Ok(_) if open => ChatsEvent::Reopened { chat: chat_id },
        Ok(_) => ChatsEvent::Closed { chat: chat_id },
        Err(e) => refusal(e),
    }
}

/// `chats.list`.
pub(super) fn list_stream(store: Arc<Store>, user: VerifiedUser, q: ChatQuery, after: Option<u64>, limit: Option<u32>) -> impl Stream<Item = ChatsEvent> + Send + 'static {
    let limit = crate::limits::page(limit, CHATS_PAGE_DEFAULT, CHATS_PAGE_MAX);
    let answered = store.chats(&user, &q, after, limit);
    stream! {
        match answered {
            Ok((rows, next)) => {
                for chat in rows {
                    yield ChatsEvent::Chat { chat };
                }
                yield ChatsEvent::Page { next };
            }
            Err(e) => yield refusal(e),
        }
    }
}

/// `chats.messages`.
pub(super) fn messages_stream(store: Arc<Store>, user: VerifiedUser, chat: u64, after: Option<u64>, limit: Option<u32>) -> impl Stream<Item = ChatsEvent> + Send + 'static {
    let limit = crate::limits::page(limit, MESSAGES_PAGE_DEFAULT, MESSAGES_PAGE_MAX);
    let answered = store.chat_messages(&user, chat, after, limit);
    stream! {
        match answered {
            Ok((rows, next)) => {
                for message in rows {
                    yield ChatsEvent::Message { message };
                }
                yield ChatsEvent::Page { next };
            }
            Err(e) => yield refusal(e),
        }
    }
}

/// A chat's change as `user` hears it on a watch — of its own chats (with
/// `ticket`, every chat it sees on that ticket), its own `read`s — or
/// nothing.
fn relay(store: &Store, user: &VerifiedUser, ticket: Option<&str>, change: &ChatChange) -> Option<ChatsEvent> {
    let view = store.chat_view(user, change.chat())?;
    let me = Signer::of(user).principal;
    match ticket {
        Some(t) if view.ticket != t => return None,
        None if !view.participants.contains(&me) => return None,
        _ => {}
    }
    let chat = view.id;
    Some(match change {
        ChatChange::Opened(_) => ChatsEvent::Opened { chat: view, existing: false },
        ChatChange::Posted(m) => ChatsEvent::Posted { message: m.clone() },
        ChatChange::Added(_) => ChatsEvent::Added { chat: view },
        ChatChange::Read { principal, upto, .. } if *principal == me => ChatsEvent::Read { chat, upto: MessageId::new(*upto), unread: view.unread },
        ChatChange::Read { .. } => return None,
        ChatChange::Closed(_) => ChatsEvent::Closed { chat },
        ChatChange::Reopened(_) => ChatsEvent::Reopened { chat },
        ChatChange::Listening { principal, waiting, .. } => ChatsEvent::Listening { chat, principal: principal.clone(), waiting: *waiting },
    })
}

/// `chats.watch`: ACCESS-12 — the caller followed; every change relayed as
/// it sees it then; a chat it saw and no longer sees, said once as `left`.
pub(super) fn watch_stream(access: Access, store: Arc<Store>, user: VerifiedUser, ticket: Option<String>) -> impl Stream<Item = ChatsEvent> + Send + 'static {
    let mut rx = store.subscribe_chats();
    let opened = Following::of(&access, &store, user);
    stream! {
        let mut following = match opened {
            Ok(f) => f,
            Err(message) => {
                yield ChatsEvent::Error { message };
                return;
            }
        };
        // The ticket by its key; one the caller cannot name relays nothing.
        let ticket = ticket.map(|t| store.ticket_id(&t).unwrap_or(t));
        let q = ChatQuery { ticket: ticket.clone(), ..ChatQuery::default() };
        let mut seen: BTreeSet<u64> = store.chats(following.user(), &q, None, u32::MAX).map(|(rows, _)| rows.iter().map(|c| c.id.get()).collect()).unwrap_or_default();
        loop {
            let next = tokio::select! {
                biased;
                () = following.changed() => None,
                r = rx.recv() => Some(r),
            };
            match following.again() {
                Err(closed) => {
                    yield ChatsEvent::WatchClosed { reason: closed.reason, message: closed.message };
                    return;
                }
                Ok(Followed::Moved { .. }) => {
                    let gone: Vec<u64> = seen.iter().copied().filter(|id| store.chat_view(following.user(), *id).is_none()).collect();
                    for chat in gone {
                        seen.remove(&chat);
                        yield ChatsEvent::Left { chat: ChatId::new(chat) };
                    }
                }
                Ok(Followed::Same) => {}
            }
            match next {
                None => {}
                Some(Ok(change)) => {
                    if let Some(ev) = relay(&store, following.user(), ticket.as_deref(), &change) {
                        seen.insert(change.chat());
                        yield ev;
                    }
                }
                Some(Err(RecvError::Lagged(missed))) => yield ChatsEvent::Lagged { missed },
                Some(Err(RecvError::Closed)) => break,
            }
        }
    }
}
