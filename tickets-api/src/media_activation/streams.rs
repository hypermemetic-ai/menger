//! COMMENT-5: what the `media` methods do, beside the activation (its
//! `impl` keeps only the methods — MENGER-29, ACCESS-13's frame). Each
//! takes the [`VerifiedUser`] the dispatch resolved; nothing here admits by
//! hand.

use crate::access::{Editor, Hidden, VerifiedUser};
use crate::address::TicketId;
use crate::media::MediaEvent;
use crate::store::{OnMedia, Put, Store, StoreError};
use crate::vocab::{MediaData, MediaHash, MediaType, TicketRef};
use std::sync::Arc;

/// `not_found` for the ticket `t` names.
fn no_ticket(t: &TicketRef) -> MediaEvent {
    MediaEvent::NotFound { hash: None, ticket: Some(t.to_string()) }
}

/// ACCESS-11: what a put for `ticket` takes — an editor's permit on it (a
/// ticket out of reach answered as one that is not there), or, with none,
/// the call's grant held anywhere: an image on the app.
fn proof(store: &Store, user: &VerifiedUser, ticket: Option<&TicketRef>) -> Result<OnMedia, MediaEvent> {
    let Some(t) = ticket else {
        return user.reach_filter().map(|_| OnMedia::app()).map_err(|message| MediaEvent::Error { message });
    };
    let key = match store.resolve(t) {
        Ok(k) => k,
        Err(StoreError::Ambiguous { text, .. }) => {
            return Err(MediaEvent::Error { message: format!("'{text}' names more than one ticket — name it in full (nothing was kept)") })
        }
        Err(_) => return Err(no_ticket(t)),
    };
    user.permit::<Editor>(&TicketId::new(key), store).map(OnMedia::from).map_err(|Hidden| no_ticket(t))
}

/// `media.put`: the image checked and kept by the store, for the ticket the
/// caller may edit — signed by its principal.
pub(super) async fn put(store: Arc<Store>, user: VerifiedUser, ticket: Option<TicketRef>, mime: MediaType, data: MediaData) -> MediaEvent {
    let on = match proof(&store, &user, ticket.as_ref()) {
        Ok(on) => on,
        Err(ev) => return ev,
    };
    match store.put_media(on, mime.as_str(), data.into_string(), Some(user.principal_text())).await {
        Ok(Put { info, duplicate }) => MediaEvent::stored(info, duplicate),
        Err(StoreError::MediaRefused(problem)) => problem.into(),
        Err(StoreError::NotFound(_)) => ticket.as_ref().map(no_ticket).unwrap_or(MediaEvent::NotFound { hash: None, ticket: None }),
        Err(e) => MediaEvent::Error { message: e.to_string() },
    }
}

/// `media.get`: ACCESS-10 — the image, to a reader who sees a ticket it is
/// for; `not_found` alike for none and for one out of reach.
pub(super) async fn get(store: Arc<Store>, user: VerifiedUser, hash: MediaHash) -> MediaEvent {
    use base64::Engine;
    match store.media(&user, hash.as_str()).await {
        Ok((info, bytes)) => {
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            let crate::media::MediaInfo { hash, mime, bytes, width, height } = info;
            MediaEvent::Media { hash, mime, bytes, width, height, data }
        }
        Err(StoreError::MediaNotFound(h)) => MediaEvent::NotFound { hash: Some(h), ticket: None },
        Err(e) => MediaEvent::Error { message: e.to_string() },
    }
}
