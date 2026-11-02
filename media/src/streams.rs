//! What the `media` methods do, beside the activation (its `impl` keeps
//! only the methods — MENGER-29's rule, tickets-api/tests/sizes.rs). Each
//! takes the [`Caller`] the dispatch resolved; whose a ticket is, is asked
//! of the tickets service over the [`Seam`] as that caller — never read
//! from its store.

use crate::caller::Caller;
use crate::image::MediaEvent;
use crate::seam::{Answer, Seam};
use crate::store::{MediaStore, Owner, StoreError};
use crate::vocab::{MediaData, MediaHash, MediaType};
use menger_tickets_api::members::Role;
use menger_tickets_api::vocab::TicketRef;
use std::sync::Arc;

/// `not_found` for the ticket `t` names.
fn no_ticket(t: &TicketRef) -> MediaEvent {
    MediaEvent::NotFound { hash: None, ticket: Some(t.to_string()) }
}

/// ACCESS-11, over the seam: what a put for `ticket` takes — the caller's
/// editor permit on it, the tickets service's word (`tickets.may { id,
/// role: editor }` as the caller); a ticket out of reach, or none, is
/// answered as one that is not there. With no ticket, the image is the
/// putter's.
async fn owner(seam: &Seam, user: &Caller, ticket: Option<&TicketRef>) -> Result<Owner, MediaEvent> {
    let Some(t) = ticket else {
        return Ok(Owner::App(Some(user.principal().to_string())));
    };
    match seam.may(user.context(), t.as_str(), Role::Editor).await {
        Ok(Answer::Held(id)) => Ok(Owner::Ticket(id)),
        Ok(Answer::NotHeld | Answer::NotFound) => Err(no_ticket(t)),
        Err(message) => Err(MediaEvent::Error { message: format!("{message} (nothing was kept)") }),
    }
}

/// `media.put`: the image checked and kept by the store, for the ticket the
/// caller may edit — or, with none, as the caller's own.
pub(crate) async fn put(store: Arc<MediaStore>, seam: Seam, user: Caller, ticket: Option<TicketRef>, mime: MediaType, data: MediaData) -> MediaEvent {
    let on = match owner(&seam, &user, ticket.as_ref()).await {
        Ok(on) => on,
        Err(ev) => return ev,
    };
    match store.put(on, mime.as_str(), data.into_string(), Some(user.principal().to_string())).await {
        Ok(crate::store::Put { info, duplicate }) => MediaEvent::stored(info, duplicate),
        Err(StoreError::Refused(problem)) => problem.into(),
        Err(e) => MediaEvent::Error { message: e.to_string() },
    }
}

/// ACCESS-10, over the seam: whether `user` sees an image kept for `owners`
/// — a ticket it was put for that the tickets service says the caller sees
/// (`tickets.may { id, role: viewer }` as the caller), or, put with no
/// ticket, its putter, and anyone holding `media.get` on `*`.
async fn sees(seam: &Seam, user: &Caller, owners: &[Owner]) -> Result<bool, String> {
    for owner in owners {
        let seen = match owner {
            Owner::App(p) => p.as_deref() == Some(user.principal()) || seam.held_everywhere(user.context(), "media.get") == Some(true),
            Owner::Ticket(id) => matches!(seam.may(user.context(), id, Role::Viewer).await?, Answer::Held(_)),
        };
        if seen {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `media.get`: the image, to a reader who sees a ticket it is for;
/// `not_found` alike for none and for one out of reach.
pub(crate) async fn get(store: Arc<MediaStore>, seam: Seam, user: Caller, hash: MediaHash) -> MediaEvent {
    use base64::Engine;
    let not_found = || MediaEvent::NotFound { hash: Some(hash.as_str().to_string()), ticket: None };
    let kept = match store.kept(hash.as_str()).await {
        Ok(k) => k,
        Err(StoreError::NotFound(_)) => return not_found(),
        Err(e) => return MediaEvent::Error { message: e.to_string() },
    };
    match sees(&seam, &user, &kept.owners).await {
        Ok(true) => {}
        Ok(false) => return not_found(),
        Err(message) => return MediaEvent::Error { message },
    }
    match store.bytes(hash.as_str()).await {
        Ok((info, bytes)) => {
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            let crate::image::MediaInfo { hash, mime, bytes, width, height } = info;
            MediaEvent::Media { hash, mime, bytes, width, height, data }
        }
        Err(StoreError::NotFound(_)) => not_found(),
        Err(e) => MediaEvent::Error { message: e.to_string() },
    }
}
