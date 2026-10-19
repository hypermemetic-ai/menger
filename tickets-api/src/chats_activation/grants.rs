//! CHAT-14 and CHAT-16: `chats.add` and `chats.remove` — beside the
//! activation (MENGER-29). An admin of a chat's scope adds anyone, in it or
//! not; one who can't read the chat's ticket (outside tickets: holds no
//! grant in its organization) is added only with a grant the call names —
//! `ticket` (viewer on the ticket alone, ACCESS-21's single-ticket scope)
//! or `chat` (`chatter` on it: its chats they are in, nothing of the
//! ticket; outside tickets, `chatter` on the organization) — the add and
//! the grant one write. Without one, `needs_grant { choices }` (the app's
//! dialog), so nothing grants by accident. Removing one whose seat made a
//! grant revokes it too when the call says `revoke`.

use super::streams::{membership, refusal, viewed_or_record};
use crate::access::{Access, VerifiedUser};
use crate::address::{OrgId, TicketId};
use crate::chats::{ChatGrant, ChatsEvent};
use crate::members::{Grant, Role, Scope};
use crate::store::{ChatRecord, Signer, Store};
use crate::vocab::{ChatId, GrantedBy, Grantee};
use std::sync::Arc;

/// The scope and role `grant` gives on chat `c` — or why it gives none.
fn scope_of(store: &Store, c: &ChatRecord, grant: ChatGrant) -> Result<(Scope, Role), String> {
    match (c.kind.outside(), grant) {
        (true, ChatGrant::Chat) => OrgId::new(&c.org).map(|o| (Scope::Org(o), Role::Chatter)).map_err(|e| e.to_string()),
        (true, ChatGrant::Ticket) => Err(format!("chat {} is on no ticket: grant chat", c.id)),
        (false, g) => {
            let key = store.ticket_id(&c.ticket).ok_or_else(|| format!("chat {}'s ticket is gone", c.id))?;
            let role = if g == ChatGrant::Ticket { Role::Viewer } else { Role::Chatter };
            Ok((Scope::Ticket(TicketId::new(key)), role))
        }
    }
}

/// What a grant does, in words: `viewer on KILN-7`.
fn words(scope: &Scope, role: Role) -> String {
    format!("{role} on {scope}")
}

/// `chats.add`.
pub(super) async fn add(store: Arc<Store>, access: Access, user: VerifiedUser, chat: u64, principal: String, grant: Option<ChatGrant>) -> ChatsEvent {
    let (on, fits) = match membership(&store, &user, chat) {
        Ok(m) => m,
        Err(ev) => return ev,
    };
    let principal = principal.trim().to_string();
    let Some(c) = store.chat_record(chat) else { return ChatsEvent::NotFound { chat: ChatId::new(chat) } };
    if c.has(&principal) || fits.admits(&access, &store, &principal) {
        let admits = |p: &str| fits.admits(&access, &store, p);
        return match store.add_to_chat(on, chat, Signer::of(&user), principal, admits).await {
            Ok(c) => viewed_or_record(&store, &user, &c, |chat| ChatsEvent::Added { chat, granted: None }),
            Err(e) => refusal(e),
        };
    }
    // One who can't read it: an admin's add, with the grant named.
    let choices: Vec<ChatGrant> = if c.kind.outside() { vec![ChatGrant::Chat] } else { ChatGrant::ALL.to_vec() };
    let ticket = (!c.kind.outside()).then(|| store.ticket_id(&c.ticket)).flatten();
    let place = ticket.clone().unwrap_or_else(|| c.org.clone());
    let Some(g) = grant else {
        return ChatsEvent::NeedsGrant { chat: ChatId::new(chat), principal, ticket, choices };
    };
    if !choices.contains(&g) {
        return ChatsEvent::Refused { reasons: vec![format!("chat {chat} is on no ticket: a grant here is chat (nothing written)")] };
    }
    let (scope, role) = match scope_of(&store, &c, g) {
        Ok(s) => s,
        Err(why) => return ChatsEvent::Refused { reasons: vec![why] },
    };
    let Ok(authority) = user.admin_of(&scope, &store) else {
        return ChatsEvent::Refused { reasons: vec![format!("{principal} can't read {place}: only an admin of {place} adds them, granting it (nothing written)")] };
    };
    let Ok(grantee) = Grantee::try_from(principal.as_str()) else {
        return ChatsEvent::Refused { reasons: vec![format!("{principal} is not a principal a grant names")] };
    };
    let said = words(&scope, role);
    let g = Grant { principal: grantee, scope, role, granted_by: GrantedBy::Principal(user.principal().clone()), at: chrono::Utc::now().timestamp() };
    match store.add_granting(on, chat, Signer::of(&user), principal, authority, g).await {
        Ok(c) => viewed_or_record(&store, &user, &c, |chat| ChatsEvent::Added { chat, granted: Some(said) }),
        Err(e) => refusal(e),
    }
}

/// `chats.remove`: `revoke` takes back the grant the seat's add made — an
/// admin of its scope's.
pub(super) async fn remove(store: Arc<Store>, user: VerifiedUser, chat: u64, principal: String, revoke: bool) -> ChatsEvent {
    let (on, _) = match membership(&store, &user, chat) {
        Ok(m) => m,
        Err(ev) => return ev,
    };
    let authority = if revoke {
        let made = store.chat_record(chat).and_then(|c| c.parts.iter().find(|p| p.principal == principal.trim()).and_then(|p| p.granted.clone()));
        let Some(scope) = made.and_then(|s| Scope::try_from(s.as_str()).ok()) else {
            return ChatsEvent::Refused { reasons: vec![format!("{principal}'s seat in chat {chat} made no grant to revoke (nothing written)")] };
        };
        match user.admin_of(&scope, &store) {
            Ok(a) => Some(a),
            Err(_) => return ChatsEvent::Refused { reasons: vec![format!("revoking {principal}'s grant on {scope} needs admin there (nothing written)")] },
        }
    } else {
        None
    };
    match store.remove_revoking(on, chat, Signer::of(&user), principal.clone(), authority).await {
        Ok((c, revoked)) => viewed_or_record(&store, &user, &c, |chat| ChatsEvent::Removed { chat, principal, revoked }),
        Err(e) => refusal(e),
    }
}
