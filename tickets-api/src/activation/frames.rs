//! COMP-16: a body's frame, beside the activation (MENGER-29) —
//! `frame_reach`'s answer, and the reader a read naming `frame` is made
//! through: the caller narrowed to that body's bound (the store's
//! [`crate::store::Framed`]: the caller's reach ∩ the body's author's ∩
//! what the body asks). The author's reach is the access layer's, by the
//! author's grants alone ([`Access::grants_reach`]).

use super::*;
use crate::store::Framed;
use crate::types::FrameReach;

/// `frame_reach` — the bound, and what it reads by shortcode.
pub(super) async fn frame_reach_of(store: &Store, access: &Access, user: &VerifiedUser, id: &str) -> Result<TicketsEvent, Failed> {
    let (frame, framed) = store.frame(user, id, |author| access.grants_reach(author)).await?;
    let extent = store.extent(&framed).unwrap_or_default();
    let tickets = extent.iter().map(|(_, n)| n).sum();
    let reach = FrameReach {
        asked: frame.asked_text(),
        alone: frame.alone(),
        ticket: frame.ticket,
        author: frame.author,
        shortcodes: extent.into_iter().map(|(sc, _)| sc).collect(),
        tickets,
    };
    Ok(TicketsEvent::FrameReach { reach })
}

/// The reader a read answers through: the caller, or — `frame` named — the
/// caller narrowed to that ticket's body's bound. A frame the caller cannot
/// see is `not_found`, naming it; nothing is read wider than the caller.
pub(super) async fn reader_for(store: &Store, access: &Access, user: &VerifiedUser, frame: Option<&str>) -> Result<Framed, TicketsEvent> {
    let Some(id) = frame else { return Ok(Framed::plain(user)) };
    match store.frame(user, id, |author| access.grants_reach(author)).await {
        Ok((_, framed)) => Ok(framed),
        Err(StoreError::NotFound(_)) => Err(TicketsEvent::NotFound { id: Asked::stored(id.trim()) }),
        Err(e) => Err(veiled(store, user, e)),
    }
}
