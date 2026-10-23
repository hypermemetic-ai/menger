//! ORG-13: the epics vocabulary — what the `epics` activation says.
//! Wasm-safe, like [`crate::orgs`]: the app reads it through the generated
//! `EpicsClient` (the nav's epic switcher).
//!
//! An **epic** is a level of the organization that holds shortcodes:
//! organization → epic → shortcode → ticket (owner, 2026-10-23: *"another
//! mechanism in the organization structure. it should contain scopes and
//! they should be renumbered such that we can start completely fresh in a
//! new epic"*). Every shortcode is in exactly one; a shortcode's code is
//! unique within (organization, epic, host), so `MENGER-1` can be born again
//! in a new epic. Each organization starts with `genesis`, which holds
//! everything made before epics; its addresses name no epic
//! (`hypermemetic:MENGER/ORG-3`), every other epic's do
//! (`hypermemetic:fresh:MENGER-1`). A reader is *in* one epic per
//! organization — its choice (`epics switch`), else the newest — and a
//! filtered read answers there unless its filter names `epic`.

use crate::address::{EpicName, OrgId};
use serde::{Deserialize, Serialize};

/// One epic, as a reader of its organization sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Epic {
    /// Its organization.
    pub org: OrgId,
    /// Its name, unique in its organization: `genesis`, `fresh`.
    pub name: EpicName,
    /// Its organization's first — everything made before epics is in it,
    /// and its addresses name no epic.
    pub first: bool,
    /// Whether it is the caller's current epic in its organization.
    pub current: bool,
    /// How many shortcodes it holds, hosted ones included.
    pub shortcodes: u32,
    /// How many tickets it holds.
    pub tickets: u32,
    /// When it was made (unix seconds; the first's: when the file gained epics).
    pub created_unix: i64,
}

/// What the `epics` activation's methods yield.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EpicsEvent {
    /// `list` and `get`: one epic.
    Epic { epic: Epic },
    /// `create`'s answer: the epic, empty.
    Created { epic: Epic },
    /// `rename`'s answer: the epic under its new name (every address that
    /// named the old one now names the new).
    Renamed { from: EpicName, epic: Epic },
    /// `delete`'s answer: the epic is gone.
    Deleted { org: OrgId, name: EpicName },
    /// `current` and `switch`: the caller's current epic in its organization.
    Current { epic: Epic },
    /// `name` is no epic of the organization.
    NotFound { name: String },
    /// A refusal, naming the problem; nothing was changed.
    Error { message: String },
}

/// MACRO-21: the emitted client implements the activation's trait; a client
/// failure is the in-band error, the last item of its stream.
impl plexus_client::FromClientError for EpicsEvent {
    fn from_client_error(error: plexus_client::ClientError) -> Self {
        EpicsEvent::Error { message: error.to_string() }
    }
}
