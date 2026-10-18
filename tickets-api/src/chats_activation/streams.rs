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
use crate::address::OrgId;
use crate::store::{ChatChange, ChatError, ChatOn, ChatQuery, Paging, Signer, Store};
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
/// CHAT-14: without a ticket, a direct message or a group in the caller's
/// organization ([`home_org`]), each of `others` a member of it.
pub(super) async fn open(store: Arc<Store>, access: Access, user: VerifiedUser, ticket: Option<String>, with: Vec<String>, title: Option<String>) -> ChatsEvent {
    let Some(ticket) = ticket else {
        let Some(org) = home_org(&store, &user) else {
            return ChatsEvent::Refused { reasons: vec!["you hold a grant in no organization: there is no one to message".into()] };
        };
        let member = |p: &str| access.member_of(p, &org, &store);
        return match store.open_outside(&user, org.to_string(), with, title, member).await {
            Ok((c, existing)) => viewed(&store, &user, c.id, |chat| ChatsEvent::Opened { chat, existing }),
            Err(e) => refusal(e),
        };
    };
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

/// CHAT-14: the organization a chat outside tickets opens in — the
/// caller's only one, or (holding roles in several, or everywhere) the
/// first the store keeps among them.
pub(super) fn home_org(store: &Store, user: &VerifiedUser) -> Option<OrgId> {
    let all: Vec<OrgId> = store.orgs().into_iter().map(|o| o.name).collect();
    match user.orgs() {
        Some(mine) => all.into_iter().find(|o| mine.contains(o)).or_else(|| mine.into_iter().next()),
        None => all.into_iter().next(),
    }
}

/// `chats.candidates` (CHAT-4's picker): who could be named in a chat on
/// `ticket` — every principal holding a grant in its organization (on `*`,
/// on the org or a shortcode in it, on a ticket or a subtree there; a
/// team's grant by each of its members; the operator's `--admin`s), and
/// every participant of the caller's chats there — but the caller, each
/// with whether it reads the ticket now ([`Access::reads_ticket`]). By
/// principal. A ticket the caller cannot read is refused as one that is
/// none, as `open` refuses it. CHAT-14: without a ticket, the members of
/// the caller's organizations and the participants of its chats outside
/// tickets, `reads` whether each may be in one; each named as the server
/// knows it, agents marked.
pub(super) fn candidates(store: &Store, access: &Access, user: &VerifiedUser, ticket: Option<&str>) -> Vec<ChatsEvent> {
    let me = Signer::of(user).principal;
    let standing = store.chat_members();
    let row = |principal: String, reads: bool| {
        let agent = crate::chats::is_agent_principal(&principal) || standing.contains(&principal);
        ChatsEvent::Candidate { name: store.name_of(&principal), agent, principal, reads }
    };
    let Some(ticket) = ticket else {
        let orgs: Vec<OrgId> = match user.orgs() {
            Some(o) => o,
            None => store.orgs().into_iter().map(|o| o.name).collect(),
        };
        let mut who: BTreeSet<String> = orgs.iter().flat_map(|o| access.members_in(Some(o), store)).collect();
        // CHAT-16: an admin adds anyone — those signed in who hold nothing yet too.
        if orgs.iter().any(|o| user.admin_of(&crate::members::Scope::Org(o.clone()), store).is_ok()) {
            who.extend(access.waiting().into_iter().map(|w| w.principal));
        }
        let q = ChatQuery::default();
        if let Ok((rows, _)) = store.chats(user, &q, None, u32::MAX) {
            who.extend(rows.into_iter().filter(|c| c.kind.outside()).flat_map(|c| c.participants));
        }
        return who.into_iter().filter(|p| *p != me).map(|p| {
            let reads = orgs.iter().any(|o| access.member_of(&p, o, store));
            row(p, reads)
        }).collect();
    };
    let on = store.ticket_id(ticket).filter(|k| user.permit::<Viewer>(&TicketId::new(k.clone()), store).is_ok());
    let Some(key) = on else {
        return vec![ChatsEvent::Refused { reasons: vec![format!("no ticket {}", ticket.trim().to_ascii_uppercase())] }];
    };
    let mut who = access.members_in(store.org_of(&key).as_ref(), store);
    // CHAT-16: an admin of the ticket adds anyone — those waiting for a role too.
    if user.permit::<crate::access::Admin>(&TicketId::new(key.clone()), store).is_ok() {
        who.extend(access.waiting().into_iter().map(|w| w.principal));
    }
    let q = ChatQuery { ticket: Some(key.clone()), ..ChatQuery::default() };
    if let Ok((rows, _)) = store.chats(user, &q, None, u32::MAX) {
        who.extend(rows.into_iter().flat_map(|c| c.participants));
    }
    who.into_iter()
        .filter(|p| *p != me)
        .map(|principal| {
            let reads = access.reads_ticket(&principal, &key, store);
            row(principal, reads)
        })
        .collect()
}

/// CHAT-14: what an add or a remove in `chat` takes — the caller's standing
/// in it, or, outside tickets, admin of its organization (which reads
/// nothing); and who may be in it: one who reads its ticket, or, outside
/// tickets, a member of its organization.
/// CHAT-16: an admin of a ticket's chat's ticket who is not in it adds and
/// removes as an admin ([`ChatOn::TicketAdmin`]).
pub(super) fn membership(store: &Store, user: &VerifiedUser, chat: u64) -> Result<(ChatOn, Fits), ChatsEvent> {
    match store.chat_seen(user, chat) {
        Ok((c, s)) => {
            let fits = s.key.clone().map_or_else(|| Fits::Org(OrgId::new(&c.org).ok()), Fits::Ticket);
            let on = match (&s.key, s.admin && !s.participant) {
                (Some(k), true) => user.permit::<crate::access::Admin>(&TicketId::new(k.clone()), store).map(ChatOn::TicketAdmin).unwrap_or(s.on),
                _ => s.on,
            };
            Ok((on, fits))
        }
        Err(e) => {
            let hidden = || ChatsEvent::NotFound { chat: ChatId::new(chat) };
            let c = store.chat_record(chat).filter(|c| c.kind.outside()).ok_or_else(|| refusal(e))?;
            let org = OrgId::new(&c.org).map_err(|_| hidden())?;
            let admin = user.admin_of(&crate::members::Scope::Org(org.clone()), store).map_err(|_| hidden())?;
            Ok((ChatOn::Admin(admin), Fits::Org(Some(org))))
        }
    }
}

/// CHAT-14: who may be in a chat — one who reads its ticket, or a member
/// of its organization.
pub(super) enum Fits {
    Ticket(String),
    Org(Option<OrgId>),
}

impl Fits {
    pub(super) fn admits(&self, access: &Access, store: &Store, p: &str) -> bool {
        match self {
            Fits::Ticket(k) => access.reads_ticket(p, k, store),
            Fits::Org(Some(o)) => access.member_of(p, o, store),
            Fits::Org(None) => false,
        }
    }
}

/// `chats.post`.
pub(super) async fn post(store: Arc<Store>, user: VerifiedUser, chat: u64, body: String) -> ChatsEvent {
    let s = match store.chat_seen(&user, chat) {
        Ok((_, s)) => s,
        Err(e) => return refusal(e),
    };
    match store.post_chat(s.on, chat, Signer::of(&user), body).await {
        Ok(message) => ChatsEvent::Posted { message },
        Err(e) => refusal(e),
    }
}

/// The chat as `user` sees it, or — an admin who adds or removes in a chat
/// it does not read, or one who just left — as kept, named by no one's
/// reach: its participants, its kind, no unread.
pub(super) fn viewed_or_record(store: &Store, user: &VerifiedUser, c: &crate::store::ChatRecord, ev: impl FnOnce(Chat) -> ChatsEvent) -> ChatsEvent {
    match store.chat_view(user, c.id) {
        Some(chat) => ev(chat),
        None => ev(store.chat_bare(c)),
    }
}

/// `chats.read`.
pub(super) async fn read(store: Arc<Store>, user: VerifiedUser, chat: u64, upto: u64) -> ChatsEvent {
    let s = match store.chat_seen(&user, chat) {
        Ok((_, s)) => s,
        Err(e) => return refusal(e),
    };
    match store.read_chat(s.on, chat, Signer::of(&user), upto).await {
        Ok((upto, unread)) => ChatsEvent::Read { chat: ChatId::new(chat), upto: MessageId::new(upto), unread },
        Err(e) => refusal(e),
    }
}

/// `chats.rename` (CHAT-7).
pub(super) async fn rename(store: Arc<Store>, user: VerifiedUser, chat: u64, title: Option<String>) -> ChatsEvent {
    let s = match store.chat_seen(&user, chat) {
        Ok((_, s)) => s,
        Err(e) => return refusal(e),
    };
    match store.rename_chat(s.on, chat, Signer::of(&user), title).await {
        Ok(_) => store.chat_view(&user, chat).map_or(ChatsEvent::NotFound { chat: ChatId::new(chat) }, |chat| ChatsEvent::Renamed { chat }),
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
    match store.set_chat_state(s.on, chat, Signer::of(&user), open).await {
        Ok(_) if open => ChatsEvent::Reopened { chat: chat_id },
        Ok(_) => ChatsEvent::Closed { chat: chat_id },
        Err(e) => refusal(e),
    }
}

/// `chats.activity` (CHAT-18).
pub(super) fn activity(store: &Store, user: &VerifiedUser, chat: u64, state: crate::chats::ActivityState, line: Option<&str>) -> ChatsEvent {
    match store.chat_activity(user, chat, state, line) {
        Ok(activity) => ChatsEvent::Activity { activity },
        Err(e) => refusal(e),
    }
}

/// `chats.activity_log` (CHAT-18): the kept items, oldest first, then the end.
pub(super) fn activity_log(store: &Store, user: &VerifiedUser, chat: u64) -> Vec<ChatsEvent> {
    match store.chat_activity_log(user, chat) {
        Ok(items) => items.into_iter().map(|activity| ChatsEvent::Activity { activity }).collect(),
        Err(e) => vec![refusal(e)],
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
/// CHAT-8: which way `messages` pages — `before` over `newest` over `after`.
pub(super) fn paging(after: Option<MessageId>, before: Option<MessageId>, newest: Option<bool>) -> Paging {
    match (before, newest, after) {
        (Some(b), _, _) => Paging::Before(b.get()),
        (None, Some(true), _) => Paging::Newest,
        (None, _, a) => Paging::After(a.map_or(0, MessageId::get)),
    }
}

pub(super) fn messages_stream(store: Arc<Store>, user: VerifiedUser, chat: u64, paging: Paging, limit: Option<u32>) -> impl Stream<Item = ChatsEvent> + Send + 'static {
    let limit = crate::limits::page(limit, MESSAGES_PAGE_DEFAULT, MESSAGES_PAGE_MAX);
    let answered = store.chat_page(&user, chat, paging, limit);
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
    let me = Signer::of(user).principal;
    // CHAT-14: the one taken out hears it as `left`.
    if let ChatChange::Removed { principal, chat } = change {
        if *principal == me && ticket.is_none_or(|t| t == chat.ticket) {
            return Some(ChatsEvent::Left { chat: ChatId::new(chat.id) });
        }
    }
    let view = store.chat_view(user, change.chat())?;
    // CHAT-18: live work is for the participants alone (an admin of the
    // ticket watching it hears none).
    if matches!(change, ChatChange::Activity(_)) && !view.participants.contains(&me) {
        return None;
    }
    match ticket {
        Some(t) if view.ticket.as_deref() != Some(t) => return None,
        None if !view.participants.contains(&me) => return None,
        _ => {}
    }
    let chat = view.id;
    Some(match change {
        ChatChange::Opened(_) => ChatsEvent::Opened { chat: view, existing: false },
        ChatChange::Posted(m) => ChatsEvent::Posted { message: m.clone() },
        ChatChange::Added(_) => ChatsEvent::Added { chat: view, granted: None },
        ChatChange::Read { principal, upto, .. } if *principal == me => ChatsEvent::Read { chat, upto: MessageId::new(*upto), unread: view.unread },
        ChatChange::Read { .. } => return None,
        ChatChange::Closed(_) => ChatsEvent::Closed { chat },
        ChatChange::Reopened(_) => ChatsEvent::Reopened { chat },
        ChatChange::Renamed(_) => ChatsEvent::Renamed { chat: view },
        ChatChange::Listening { principal, waiting, .. } => ChatsEvent::Listening { chat, principal: principal.clone(), waiting: *waiting },
        ChatChange::Removed { principal, .. } => ChatsEvent::Removed { chat: view, principal: principal.clone(), revoked: false },
        ChatChange::Activity(a) => ChatsEvent::Activity { activity: a.clone() },
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
