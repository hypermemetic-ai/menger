//! ORG-13: the epics vocabulary — the store's: what an epic is, its archive
//! mark, a move and the changes the store signals. V2-25: the `epics`
//! activation that said these on the wire went (owner, 2026-11-01); the
//! types stay the store's, and nothing narrows a read to a reader's epic.
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
//! organization — its choice (`epics switch`), else the organization's
//! **default** epic (ORG-23: never the newest, so making an epic moves
//! nobody) — and a filtered read answers there unless its filter names
//! `epic`. An admin **archives** an epic (ORG-23): everyone on it moves to
//! the next epic (the next newer, else the next older), and its tickets
//! leave every view unless a filter names the epic or `archived`.

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
    /// ORG-23: whether it is its organization's default — where a principal
    /// who never switched is (the first, until an admin sets another).
    #[serde(default)]
    pub default: bool,
    /// ORG-23: how many principals with a grant in the organization are on
    /// it now, those on it by default included.
    #[serde(default)]
    pub members_on: u32,
    /// ORG-23: the archive's mark — when and by whom; none for a live epic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<EpicArchived>,
}

/// ORG-23: an archived epic's mark — on the epic, and on each of its
/// tickets as `get` answers it (`epic_archived`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EpicArchived {
    /// The epic's name.
    pub name: EpicName,
    /// When (unix seconds).
    pub at_unix: i64,
    /// Who archived it.
    pub by: String,
}

/// ORG-23: a move of a principal's current epic it did not make itself —
/// an archive took its epic. Said once: `epics current` carries it until
/// `epics dismiss`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EpicMove {
    /// The epic it was on.
    pub from: EpicName,
    /// The epic it is on now.
    pub to: EpicName,
    /// Who moved it (the archive's admin).
    pub by: String,
    /// When (unix seconds).
    pub at_unix: i64,
    /// Why: `archived`.
    pub reason: String,
}

/// ORG-23: what changed of an organization's epics, as `epics watch` says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EpicChange {
    Created,
    Renamed,
    Deleted,
    Archived,
    Restored,
    DefaultMoved,
}

/// What the store signals of an organization's epics
/// (`Store::subscribe_epics`); V2-25: no method says these on the wire.
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
    /// `current` and `switch`: the caller's current epic in its
    /// organization — ORG-23: with `moved` when an archive moved the caller
    /// there and it has not dismissed it (`epics dismiss`).
    Current {
        epic: Epic,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        moved: Option<EpicMove>,
    },
    /// ORG-23: `archive`'s answer (and `--dry-run`'s, which wrote nothing):
    /// the epic, the principals moved off it, where they went, and how many
    /// tickets left the views.
    Archived { epic: Epic, moved: Vec<String>, to: EpicName, tickets: u32, dry_run: bool },
    /// ORG-23: `restore`'s answer: the epic, live again; nobody moved.
    Restored { epic: Epic },
    /// ORG-23: `set_default`'s answer: the organization's default epic now.
    DefaultSet { epic: Epic },
    /// ORG-23: `dismiss`'s answer: the move notice is gone (or there was none).
    Dismissed { org: OrgId },
    /// ORG-23: `watch` — something of the organization's epics changed;
    /// `list` again for the cards.
    EpicChanged { org: OrgId, name: EpicName, what: EpicChange },
    /// ORG-23: `watch`, to the principal an archive moved: from where, to
    /// where, by whom.
    Moved { org: OrgId, moved: EpicMove },
    /// `watch`: the subscriber fell behind and lost this many changes.
    Lagged { missed: u64 },
    /// `watch`: it ended — its caller was verified again and refused.
    WatchClosed { reason: crate::types::WatchEnd, message: String },
    /// `name` is no epic of the organization.
    NotFound { name: String },
    /// A refusal, naming the problem; nothing was changed.
    Error { message: String },
}

