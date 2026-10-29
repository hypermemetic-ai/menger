//! UI-9: `ui.watch` as its stream — the store's changes (a `live` on every
//! pointer move, a `slot_changed` on every set and clear), the caller held
//! as `tickets.watch` holds it (ACCESS-12, `access::Following`): verified
//! again when the grants move, a sign-out or the expiry comes; refused, the
//! stream ends with one `watch_closed { reason, message }`.

use crate::events::UiEvent;
use crate::store::UiStore;
use futures::Stream;
use menger_tickets_api::access::{Access, VerifiedUser};
use menger_tickets_api::store::Store;
use std::sync::Arc;

/// The stream. UI-9 phase 1: unimplemented, in band.
pub(super) fn watch_stream(store: Arc<UiStore>, access: Access, tickets: Option<Arc<Store>>, user: VerifiedUser) -> impl Stream<Item = UiEvent> + Send + 'static {
    let _ = (store, access, tickets, user);
    futures::stream::once(std::future::ready(UiEvent::Error { message: "unimplemented: ui.watch (UI-9)".into() }))
}
