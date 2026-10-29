//! UI-9: `ui slots` — what fills each section of each layout: `list`
//! (viewer), `set` and `clear` (admin's alone: unlisted in `roles`, so only
//! `admin`, which holds every grant, holds them).
//!
//! A `#[plexus_macros::child]` of `ui` on the frame (ACCESS-13), mounted by
//! `Ui::slots`, so its methods are `ui.slots.list|set|clear` on the wire and
//! the gate asks a call by that whole path (MACRO-20), as `tickets`'
//! children are.

use crate::events::UiEvent;
use super::{answer, refusal};
use crate::store::{Assignment, UiError, UiStore};
use crate::vocab::{ComponentName, LayoutName, SlotName, VersionHash};
use futures::{Stream, StreamExt};
use menger_tickets_api::access::{Access, MengerRequest, VerifiedUser};
use std::sync::Arc;

/// The sections of the UI's layouts: their assignments, set and cleared.
#[derive(Clone)]
pub struct SlotsActivation {
    pub(super) store: Arc<UiStore>,
    pub(super) access: Access,
}

/// `list`'s answer: one `assignment` each, or the refusal.
fn listed(r: Result<Vec<Assignment>, UiError>) -> Vec<UiEvent> {
    match r {
        Ok(rows) => rows
            .into_iter()
            .map(|a| UiEvent::Assignment { layout: a.layout, slot: a.slot, component: a.component, version: a.version, default: a.default })
            .collect(),
        Err(e) => vec![refusal(e)],
    }
}

/// What fills each section of each layout — list the assignments, set one, clear one.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(viewer = ["list"], editor = [viewer])
)]
impl SlotsActivation {
    /// Every assignment (of one layout, when named), each with the default it overrides; yields one assignment { layout, slot, component, version, default } each.
    ///
    /// `default` is the component the live manifest's `slots` names for the
    /// section (`null` when it names none, or nothing is live). None at all
    /// when no section is assigned.
    #[plexus_macros::method(group = "read")]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only this layout's sections, e.g. board-layout (optional).
        layout: Option<LayoutName>,
    ) -> impl Stream<Item = UiEvent> + Send + 'static {
        let _ = user;
        let store = self.store.clone();
        let read = async move { tokio::task::spawn_blocking(move || store.slots_list(layout.as_ref().map(LayoutName::as_str))).await };
        futures::stream::once(read).flat_map(|r| {
            let events = match r {
                Ok(r) => listed(r),
                Err(e) => vec![UiEvent::Error { message: format!("the ui store failed: {e}") }],
            };
            futures::stream::iter(events)
        })
    }

    /// Fill a section with a component of a stored version (none: the live one); yields set { layout, slot, component, version } and says slot_changed on every ui.watch.
    ///
    /// Refused in band, nothing written, naming which: `unknown_version` (a
    /// hash not stored, or none named and nothing live),
    /// `unknown_component` (the version's manifest holds no such
    /// component), `unknown_slot` (the live manifest's `slots` declares no
    /// such slot for the layout). Admin only.
    #[plexus_macros::method(group = "write")]
    pub async fn set(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The layout, e.g. board-layout.
        layout: LayoutName,
        /// The section: its slot's name in the layout, e.g. main.
        slot: SlotName,
        /// The component to fill it with, e.g. ticket-row.
        component: ComponentName,
        /// The stored version the component is of, by hash (optional: the live version's).
        version: Option<VersionHash>,
    ) -> impl Stream<Item = UiEvent> + Send + 'static {
        let _ = user;
        let store = self.store.clone();
        answer(move || match store.slots_set(layout.as_str(), slot.as_str(), component.as_str(), version.as_ref().map(VersionHash::as_str)) {
            Ok(a) => UiEvent::Set { layout: a.layout, slot: a.slot, component: a.component, version: a.version },
            Err(e) => refusal(e),
        })
    }

    /// Return a section to its default; yields cleared { layout, slot, component, version } (the default it shows now) and says slot_changed on every ui.watch.
    ///
    /// Admin only.
    #[plexus_macros::method(group = "write")]
    pub async fn clear(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The layout, e.g. board-layout.
        layout: LayoutName,
        /// The section: its slot's name in the layout, e.g. main.
        slot: SlotName,
    ) -> impl Stream<Item = UiEvent> + Send + 'static {
        let _ = user;
        let store = self.store.clone();
        answer(move || match store.slots_clear(layout.as_str(), slot.as_str()) {
            Ok(s) => UiEvent::Cleared { layout: s.layout, slot: s.slot, component: s.component, version: s.version },
            Err(e) => refusal(e),
        })
    }
}
