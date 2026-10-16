//! CHAT-3: `await_message` as its stream — APP-24's `await_reply` shape:
//! the caller followed, the chats' channel subscribed, then `waiting`, then
//! the store asked ([`Store::message_for`]) at once and again at every
//! change heard, until the first message (`message`), the timeout
//! (`timed_out`) or the caller's access ends.
//!
//! **No gap, no duplicate.** What was there when the call came is set
//! aside by id (without `since`) before the channel is subscribed: a
//! message posted after has a greater id, and the first ask after it finds
//! it. The answer is a function of what is kept, so a change heard twice
//! answers once — and the stream ends at its first `message`.
//!
//! **Listening.** While it waits, the chats it waits on hold the caller as
//! listening ([`Store::listen`]) — `listening` on their watches, and the
//! chats' `waiting` — until the stream ends or is dropped.

use super::streams::refusal;
use crate::access::{Access, Followed, Following, VerifiedUser, Viewer};
use crate::address::TicketId;
use crate::chats::{ChatState, ChatsEvent};
use crate::store::{ChatQuery, MessageWait, Signer, Store};
use crate::vocab::ChatId;
use async_stream::stream;
use futures::Stream;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

/// What a call asks.
pub(super) struct Ask {
    pub(super) chat: Option<u64>,
    pub(super) ticket: Option<String>,
    pub(super) since: Option<i64>,
    pub(super) after: Option<u64>,
    pub(super) timeout_minutes: Option<u32>,
}

/// While held, `principal` listens on `chats`.
struct Listening {
    store: Arc<Store>,
    chats: Vec<u64>,
    principal: String,
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.store.listen(&self.chats, &self.principal, false);
    }
}

enum Woke {
    Changed,
    Closed,
    Timeout,
}

/// What is asked, checked: the chat the caller's, the ticket one it reads.
fn prepared(store: &Store, user: &VerifiedUser, ask: &Ask) -> Result<Option<String>, Box<ChatsEvent>> {
    if let Some(id) = ask.chat {
        match store.chat_seen(user, id) {
            Ok((_, s)) if s.participant => {}
            Ok(_) => return Err(Box::new(refusal(crate::store::ChatError::NotFound(id)))),
            Err(e) => return Err(Box::new(refusal(e))),
        }
    }
    match ask.ticket.as_deref() {
        None => Ok(None),
        Some(t) => match store.ticket_id(t).filter(|k| user.permit::<Viewer>(&TicketId::new(k), store).is_ok()) {
            Some(k) => Ok(Some(k)),
            None => Err(Box::new(ChatsEvent::Refused { reasons: vec![format!("no ticket {}", t.trim().to_ascii_uppercase())] })),
        },
    }
}

pub(super) fn await_message_stream(store: Arc<Store>, access: Access, user: VerifiedUser, ask: Ask) -> impl Stream<Item = ChatsEvent> + Send + 'static {
    let started = chrono::Utc::now().timestamp();
    // Set aside before the channel is subscribed: nothing after the call is.
    let after = match (ask.since, ask.after) {
        (_, Some(a)) => a,
        (Some(_), None) => 0,
        (None, None) => store.newest_message(),
    };
    let opened = Following::of(&access, &store, user);
    let mut rx = store.subscribe_chats();
    stream! {
        let mut following = match opened {
            Ok(f) => f,
            Err(message) => {
                yield ChatsEvent::Error { message };
                return;
            }
        };
        let ticket = match prepared(&store, following.user(), &ask) {
            Ok(t) => t,
            Err(ev) => {
                yield *ev;
                return;
            }
        };
        let w = MessageWait { chat: ask.chat, ticket: ticket.clone(), since: ask.since, after };
        yield ChatsEvent::Waiting { chat: ask.chat.map(ChatId::new), ticket: ticket.clone(), since: ask.since.unwrap_or(started) };
        let principal = Signer::of(following.user()).principal;
        let chats: Vec<u64> = match ask.chat {
            Some(c) => vec![c],
            None => {
                let q = ChatQuery { ticket: ticket.clone(), state: Some(ChatState::Open), unread: None };
                store.chats(following.user(), &q, None, u32::MAX).map(|(rows, _)| rows.iter().filter(|c| c.participants.contains(&principal)).map(|c| c.id.get()).collect()).unwrap_or_default()
            }
        };
        store.listen(&chats, &principal, true);
        let _listening = Listening { store: Arc::clone(&store), chats, principal };
        let deadline = ask.timeout_minutes.map(|m| tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(m) * 60));
        let sleep = tokio::time::sleep_until(deadline.unwrap_or_else(|| tokio::time::Instant::now() + std::time::Duration::from_secs(10 * 365 * 24 * 3600)));
        tokio::pin!(sleep);
        loop {
            if let Some(message) = store.message_for(following.user(), &w) {
                yield ChatsEvent::Message { message };
                return;
            }
            let woke = tokio::select! {
                biased;
                () = following.changed() => Woke::Changed,
                r = rx.recv() => if matches!(r, Err(RecvError::Closed)) { Woke::Closed } else { Woke::Changed },
                () = &mut sleep, if deadline.is_some() => Woke::Timeout,
            };
            match woke {
                Woke::Changed => {}
                Woke::Closed => return,
                Woke::Timeout => {
                    yield ChatsEvent::TimedOut;
                    return;
                }
            }
            // ACCESS-12: the caller as it stands — refused, or the chat it
            // waits on out of its reach, the wait ends saying why.
            match following.again() {
                Err(closed) => {
                    yield ChatsEvent::WatchClosed { reason: closed.reason, message: closed.message };
                    return;
                }
                Ok(Followed::Moved { .. }) if ask.chat.is_some_and(|c| store.chat_view(following.user(), c).is_none()) => {
                    let message = format!("your access changed: chat {} is out of your reach", ask.chat.unwrap_or_default());
                    yield ChatsEvent::WatchClosed { reason: crate::types::WatchEnd::Revoked, message };
                    return;
                }
                Ok(_) => {}
            }
        }
    }
}
