//! CHAT-18: a participant's **live work** in a chat — the bridge's "on it",
//! each tool call, a heartbeat, "ended". Not a message and never stored:
//! broadcast on the chats' channel ([`ChatChange::Activity`]) to the
//! participants' `watch`es, and the newest [`ACTIVITY_KEPT`] of each chat
//! held in the cache, so a viewer opening mid-turn reads them
//! (`activity_log`). A restart forgets them, as it ends every wait.

use super::{ChatChange, ChatError};
use crate::access::VerifiedUser;
use crate::chats::{activity_line, Activity, ActivityState, ACTIVITY_KEPT};
use crate::store::{Signer, Store};
use crate::vocab::ChatId;

impl Store {
    /// CHAT-18: `user`'s live work in chat `chat` — `state`, with `line` cut
    /// to [`crate::chats::ACTIVITY_LINE_MAX`] — kept in the chat's ring and
    /// said to its participants. Only a participant works in a chat; to
    /// anyone else it is not there.
    pub fn chat_activity(&self, user: &VerifiedUser, chat: u64, state: ActivityState, line: Option<&str>) -> Result<Activity, ChatError> {
        let (_, s) = self.chat_seen(user, chat)?;
        if !s.participant {
            return Err(ChatError::NotFound(chat));
        }
        let by = Signer::of(user);
        let activity = {
            let mut cache = self.tickets.write().expect("store lock poisoned");
            cache.chats.activity_seq += 1;
            let a = Activity { seq: cache.chats.activity_seq, chat: ChatId::new(chat), by: by.principal, name: by.name, at: chrono::Utc::now().timestamp(), state, line: activity_line(line) };
            let ring = cache.chats.activity.entry(chat).or_default();
            if ring.len() >= ACTIVITY_KEPT {
                ring.pop_front();
            }
            ring.push_back(a.clone());
            a
        };
        self.tx.chat(ChatChange::Activity(activity.clone()));
        Ok(activity)
    }

    /// CHAT-18: chat `chat`'s kept activity, oldest first — for a
    /// participant; to anyone else it is not there.
    pub fn chat_activity_log(&self, user: &VerifiedUser, chat: u64) -> Result<Vec<Activity>, ChatError> {
        let (_, s) = self.chat_seen(user, chat)?;
        if !s.participant {
            return Err(ChatError::NotFound(chat));
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        Ok(cache.chats.activity.get(&chat).map(|r| r.iter().cloned().collect()).unwrap_or_default())
    }
}
