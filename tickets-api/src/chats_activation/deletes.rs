//! CHAT-44: `chats.delete` and `chats.restore`, beside the activation. The
//! caller writes as an admin when it is one — of the chat's ticket, or,
//! outside tickets, of its organization (who need not read the chat) — and
//! otherwise with its seat, where only its own messages are its to delete
//! ([`Store::delete_messages`] decides, under its write lock).

use super::streams::refusal;
use crate::access::{Admin, VerifiedUser};
use crate::address::{OrgId, TicketId};
use crate::chats::ChatsEvent;
use crate::members::Scope;
use crate::store::{ChatOn, Pick, Signer, Store};
use crate::vocab::{ChatId, MessageId};
use std::sync::Arc;

/// What `delete` / `restore` name: `ids`, or `from` … `to`; never both.
pub(super) fn pick(ids: Option<Vec<MessageId>>, from: Option<MessageId>, to: Option<MessageId>) -> Result<Pick, String> {
    match (ids.filter(|i| !i.is_empty()), from, to) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => Err("name messages by --ids or by --from and --to, not both".into()),
        (Some(ids), None, None) => Ok(Pick::Ids(ids.into_iter().map(MessageId::get).collect())),
        (None, Some(from), Some(to)) => Ok(Pick::Range { from: from.get(), to: to.get() }),
        (None, Some(_), None) | (None, None, Some(_)) => Err("a range takes both --from and --to".into()),
        (None, None, None) => Err("no message named: give --ids, or --from and --to".into()),
    }
}

/// What a delete in `chat` writes with: an admin's proof when the caller is
/// one, else its standing (a seat, or a viewer's permit on the ticket).
fn authority(store: &Store, user: &VerifiedUser, chat: u64) -> Result<ChatOn, ChatsEvent> {
    match store.chat_seen(user, chat) {
        Ok((c, s)) => {
            if let Some(k) = s.key.as_ref().filter(|_| s.admin) {
                if let Ok(p) = user.permit::<Admin>(&TicketId::new(k.clone()), store) {
                    return Ok(ChatOn::TicketAdmin(p));
                }
            }
            let org = c.kind.outside().then(|| OrgId::new(&c.org).ok()).flatten();
            Ok(org.and_then(|o| user.admin_of(&Scope::Org(o), store).ok()).map_or(s.on, ChatOn::Admin))
        }
        Err(e) => {
            // An admin of a chat's organization deletes without reading it.
            let hidden = || ChatsEvent::NotFound { chat: ChatId::new(chat) };
            let c = store.chat_record(chat).filter(|c| c.kind.outside()).ok_or_else(|| refusal(e))?;
            let org = OrgId::new(&c.org).map_err(|_| hidden())?;
            user.admin_of(&Scope::Org(org), store).map(ChatOn::Admin).map_err(|_| hidden())
        }
    }
}

/// `chats.delete` (`restore: false`) and `chats.restore`.
pub(super) async fn delete(store: Arc<Store>, user: VerifiedUser, chat: u64, pick: Result<Pick, String>, restore: bool) -> ChatsEvent {
    let pick = match pick {
        Ok(p) => p,
        Err(why) => return ChatsEvent::Refused { reasons: vec![why] },
    };
    let on = match authority(&store, &user, chat) {
        Ok(on) => on,
        Err(ev) => return ev,
    };
    match store.delete_messages(on, chat, Signer::of(&user), pick, restore).await {
        Ok(d) => {
            let unread = store.chat_view(&user, chat).map_or(0, |c| c.unread);
            let (chat, ids) = (ChatId::new(d.chat), d.ids.into_iter().map(MessageId::new).collect());
            match restore {
                false => ChatsEvent::MessagesDeleted { chat, ids, by: d.by, at: d.at, unread },
                true => ChatsEvent::MessagesRestored { chat, ids, by: d.by, at: d.at, unread },
            }
        }
        Err(e) => refusal(e),
    }
}
