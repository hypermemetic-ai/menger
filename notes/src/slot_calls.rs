//! NOTES-4: what the slot's methods do, beside the activation (MENGER-29:
//! its `impl` keeps only the methods). Each asks the gate on the note's
//! organization first ([`crate::streams::reached`]) — `notes.slot` a
//! viewer's, `fill`, `empty`, `building` and `props` an editor's — then the
//! store ([`crate::slot`]); a fill asks the ui service through the seam
//! whether its version holds its component ([`crate::seam::Seam::component`]).

use crate::caller::Caller;
use crate::events::{Building, NotesEvent};
use crate::seam::{Held, Seam};
use crate::slot::{FillOf, Hand};
use crate::store::{now_ms, NotesStore};
use crate::streams::{failed, reached};
use crate::vocab::BuildStep;
use serde_json::{Map, Value};
use std::sync::Arc;

fn hand(user: &Caller) -> Hand {
    Hand { name: user.name(), principal: user.principal().to_string() }
}

/// `notes.slot`: the slot whole.
pub(crate) async fn slot(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.slot").await {
        return ev;
    }
    match store.slot(&id).await {
        Ok(slot) => NotesEvent::Slot { slot },
        Err(e) => failed(e),
    }
}

/// What a fill sets, its version resolved — or the refusal naming the
/// component and the version.
async fn checked(seam: &Seam, component: String, version: String, props: Option<Map<String, Value>>, say: Option<String>) -> Result<FillOf, NotesEvent> {
    match seam.component(&version, &component).await {
        Ok(Held::Holds(version)) => Ok(FillOf { component, version, props, say }),
        Ok(Held::NoComponent(whole)) => Err(NotesEvent::Refused { reason: "unknown_component".into(), message: format!("version {whole} holds no component {component} (nothing was written)") }),
        Ok(Held::NoVersion) => Err(NotesEvent::Refused { reason: "unknown_version".into(), message: format!("the ui service keeps no version {version}, so none holds {component} (nothing was written)") }),
        Err(message) => Err(NotesEvent::Error { message }),
    }
}

/// `notes.fill`: the slot filled with `component` of `version`, once the ui
/// service says the version holds it.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fill(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, component: String, version: String, props: Option<Map<String, Value>>, say: Option<String>) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.fill").await {
        return ev;
    }
    let fill = match checked(&seam, component, version, props, say).await {
        Ok(f) => f,
        Err(ev) => return ev,
    };
    match store.fill(&id, fill, hand(&user)).await {
        Ok(slot) => NotesEvent::Filled { slot },
        Err(e) => failed(e),
    }
}

/// `notes.empty`: nothing drawn in the slot.
pub(crate) async fn empty(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.empty").await {
        return ev;
    }
    match store.empty(&id, hand(&user)).await {
        Ok(slot) => NotesEvent::Emptied { slot },
        Err(e) => failed(e),
    }
}

/// `notes.props`: a patch merged into the slot's props.
pub(crate) async fn props(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, patch: Map<String, Value>) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.props").await {
        return ev;
    }
    match store.props(&id, patch, hand(&user)).await {
        Ok(slot) => NotesEvent::Props { slot },
        Err(e) => failed(e),
    }
}

/// `notes.building`: the slot marked as being built — a step, a line, a failure.
pub(crate) async fn building(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, component: String, step: Option<BuildStep>, say: Option<String>, failed_: bool) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.building").await {
        return ev;
    }
    let at = now_ms();
    let b = Building { component, step, say, failed: failed_, by: user.name(), principal: user.principal().to_string(), started: at, at };
    match store.building(&id, b).await {
        Ok(slot) => NotesEvent::Building { slot },
        Err(e) => failed(e),
    }
}
