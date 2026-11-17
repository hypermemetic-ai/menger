//! EDITOR-32: **`media.list`** — every kept file the caller may see, newest
//! first, a page at a time, each with the tickets and notes it is kept
//! for among those the caller reads (the owner, 2026-11-15: "a media page
//! that shows all saved media, should be able to get to the tickets they
//! are on too").
//!
//! The store answers rows ([`MediaStore::list`]: files kept for a ticket or
//! a note, never a putter's own or a link's preview — those are used
//! nowhere); whose each owner is, is asked of the seam as the caller —
//! `tickets.may { id, role: viewer }` for a ticket, `notes.may` for a
//! note — once per owner per call, a few at a time. An owner the caller
//! cannot read is left off its row, and a row left with none is left out
//! (ACCESS-10: not found ≡ not yours). A ticket is answered by the id the
//! tickets service names it by now (a ticket moved since keeps its file).
//!
//! A page reads at most [`SCAN_MAX`] rows: past that it answers what it
//! found and a `next` to go on from, so a reader who sees little of a large
//! store is never held while the whole file is read.

use crate::activation::MediaActivation;
use crate::caller::Caller;
use crate::image::MediaEvent;
use crate::seam::Answer;
use crate::store::{Listed, MediaStore, Owner, StoreError};
use crate::vocab::{MediaHash, MediaKind};
use futures::StreamExt;
use menger_notes::seam::Answer as NoteAnswer;
use menger_tickets_api::members::Role;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

/// A page when `limit` names none.
pub const LIST_DEFAULT: usize = 60;
/// The most one page holds.
pub const LIST_MAX: usize = 500;
/// The rows read from the store at a time.
const BATCH: usize = 100;
/// The most rows one call reads before it answers a `next`.
pub const SCAN_MAX: usize = 2000;
/// The owners asked of the seam at once.
const ASKING: usize = 16;

/// One shown row: the file, and the tickets and notes the caller reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub file: Listed,
    pub tickets: Vec<String>,
    pub notes: Vec<String>,
}

/// A page: its rows, and the `after` that asks the next (none: no more).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Page {
    pub rows: Vec<Shown>,
    pub next: Option<String>,
}

/// At most `limit` rows from `after`, of `kind`, each owner decided by
/// `decide` — `Some(id)`, the id it is read by now, for an owner the caller
/// reads; `None` for one it does not. A hash `after` that names no file is
/// [`StoreError::NotFound`].
pub async fn page<F, Fut>(store: &Arc<MediaStore>, kind: Option<MediaKind>, after: Option<&str>, limit: usize, decide: F) -> Result<Page, StoreError>
where
    F: Fn(Owner) -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let limit = limit.clamp(1, LIST_MAX);
    let mut known: HashMap<Owner, Option<String>> = HashMap::new();
    let mut out = Page::default();
    let mut cursor = after.map(str::to_string);
    let mut read = 0;
    loop {
        let batch = store.list(kind.map(MediaKind::as_str), cursor.as_deref(), BATCH).await?;
        let mut seen = std::collections::HashSet::new();
        let asked: Vec<Owner> = batch.iter().flat_map(|f| f.owners.iter()).filter(|o| !known.contains_key(*o) && seen.insert(*o)).cloned().collect();
        let answers: Vec<(Owner, Option<String>)> = futures::stream::iter(asked).map(|o| {
            let asking = decide(o.clone());
            async move { (o, asking.await) }
        }).buffer_unordered(ASKING).collect().await;
        known.extend(answers);
        let full = batch.len() == BATCH;
        for file in batch {
            read += 1;
            cursor = Some(file.info.hash.clone());
            let (mut tickets, mut notes) = (Vec::new(), Vec::new());
            for o in &file.owners {
                let into = match o {
                    Owner::Ticket(_) => &mut tickets,
                    Owner::Note(_) => &mut notes,
                    _ => continue,
                };
                if let Some(Some(id)) = known.get(o) {
                    if !into.contains(id) {
                        into.push(id.clone());
                    }
                }
            }
            if tickets.is_empty() && notes.is_empty() {
                continue;
            }
            out.rows.push(Shown { file, tickets, notes });
            if out.rows.len() == limit {
                out.next = cursor;
                return Ok(out);
            }
        }
        if !full {
            return Ok(out);
        }
        if read >= SCAN_MAX {
            out.next = cursor;
            return Ok(out);
        }
    }
}

/// Whether `user` reads `owner`, by the seam's word as the caller: the id it
/// is read by now, or none. A seam that fails on one owner leaves it out
/// (never shown on a doubt), said in the log.
async fn reads(me: &MediaActivation, user: &Caller, owner: Owner) -> Option<String> {
    let answer = match &owner {
        Owner::Ticket(id) => me.seam.may(user.context(), id, Role::Viewer).await.map(|a| match a {
            Answer::Held(now) => Some(now),
            _ => None,
        }),
        Owner::Note(id) => me.seam.note_may(user.context(), id, Role::Viewer).await.map(|a| match a {
            NoteAnswer::Held(now) => Some(now),
            _ => None,
        }),
        Owner::App(_) | Owner::Anyone => Ok(None),
    };
    answer.unwrap_or_else(|e| {
        tracing::warn!("media.list: {owner:?} left out: {e}");
        None
    })
}

/// `media.list`: a page of what `user` may see, then `page { next }`.
pub(crate) async fn list(me: MediaActivation, user: Caller, kind: Option<MediaKind>, after: Option<MediaHash>, limit: Option<u32>) -> Vec<MediaEvent> {
    if me.seam.service().is_none() {
        return vec![MediaEvent::Error { message: "the media service is not attached to a running service".into() }];
    }
    let limit = limit.map_or(LIST_DEFAULT, |l| l as usize);
    let asked = page(&me.store, kind, after.as_ref().map(MediaHash::as_str), limit, |o| reads(&me, &user, o)).await;
    match asked {
        Ok(Page { rows, next }) => {
            let mut events: Vec<MediaEvent> = rows.into_iter().map(listed).collect();
            events.push(MediaEvent::Page { next });
            events
        }
        Err(StoreError::NotFound(h)) => vec![MediaEvent::no_hash(&h)],
        Err(e) => vec![MediaEvent::Error { message: e.to_string() }],
    }
}

fn listed(s: Shown) -> MediaEvent {
    let Shown { file: Listed { info, at, .. }, tickets, notes } = s;
    let crate::MediaInfo { hash, mime, bytes, width, height, kind, name } = info;
    MediaEvent::Listed { hash, mime, bytes, width, height, kind, name, at, tickets, notes }
}

#[cfg(test)]
mod tests;
