//! APP-23: **`request_changes`** — hands a queued ticket back to whoever
//! queued it: what to change kept as the queue record's `reason`
//! (`changes_requested`), and the entry out, in one write. The agent
//! waiting on the ticket (`await_reply`) hears it. The ticket's inputs stay
//! as they are: it asks the same questions once it is queued again.

use super::history::Act;
use super::queue::{record_of, store as store_queue};
use super::{lineage, QueueChange, Store, StoreError};
use crate::access::{Editor, Permit};
use crate::queue::QueueAction;
use std::sync::Arc;

/// The longest change request the queue's log keeps, in bytes: it is read
/// whole at every start.
pub const CHANGES_MAX: usize = 64 * 1024;

impl Store {
    /// APP-23: requests changes on the queued ticket `id` — `body` kept on
    /// the queue's log, and its entry taken out, in one write. Answers the
    /// queue change. Refused, with nothing written: a ticket that is none,
    /// one not queued, a body blank or past [`CHANGES_MAX`].
    pub async fn request_changes_by(&self, on: Permit<Editor>, body: String, act: Act) -> Result<QueueChange, StoreError> {
        // ACCESS-11: of the ticket `on` is for.
        let id = self.keyed(on.id())?;
        if body.trim().is_empty() {
            return Err(StoreError::Invalid("a change request says what to change: `body` is blank".into()));
        }
        if body.len() > CHANGES_MAX {
            return Err(StoreError::Invalid(format!("`body` is {} bytes — a change request keeps at most {CHANGES_MAX}", body.len())));
        }
        let serial = self.serial().await;
        let now = chrono::Utc::now().timestamp();
        let (entry, record, key, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&id).ok_or_else(|| StoreError::NotFound(id.clone()))?;
            let entry = cache.queue.get(&id).cloned().ok_or_else(|| {
                StoreError::Invalid(format!("{id} is not queued — a change request hands a queued ticket back to whoever queued it"))
            })?;
            let mut record = record_of(cache.queue.next_id(), &entry, QueueAction::ChangesRequested, &act, now);
            record.reason = Some(body);
            let key = (row.shortcode.clone(), row.seq);
            let under = lineage::touch_or_queued_before_with(&self.unders, &cache, &[(&key.0, key.1)]);
            (entry, record, key, under)
        };
        let change = QueueChange::Unqueued { id: id.clone(), entry, action: QueueAction::ChangesRequested, by: act.actor, at_unix: now };
        let (stored, kept) = (record.clone(), record);
        let tickets = Arc::clone(&self.tickets);
        let shortcode = key.0.clone();
        self.write(
            serial,
            move |tx| {
                store_queue(tx, &stored.ticket, None, &stored)?;
                Ok(())
            },
            move |cache, _| {
                cache.log.touch(&key.0, key.1, kept.at_unix);
                let ticket = kept.ticket.clone();
                cache.queue.put(&ticket, None, kept);
            },
            move |(), (), tx| {
                tx.send(super::Change::Queue { shortcode, change: change.clone() });
                if let Some(c) = under.and_then(|u| u.after(&tickets)) {
                    tx.send(c);
                }
                change
            },
        )
        .await
    }
}
