//! menger's ticket vocabulary — the one declaration.
//!
//! Moved here from `menger/src/store.rs` (`Status`, `Kind`, `Severity`,
//! `Ticket`) and `menger/src/activations/tickets/types.rs`
//! (`TicketSummary`, `TicketsEvent`) by MFE-9, unchanged in shape: the
//! server re-exports them from `menger::store` / the tickets activation, so
//! every existing call site still reads `crate::store::Status`, and the
//! browser app reads *these* types rather than a hand-written mirror of the
//! JSON they arrive as.
//!
//! What that buys is one property, and it is the reason the module moved:
//! **rename a field here and the browser app stops compiling.** The old
//! frontend mirror was `#[derive(Deserialize)]` with `#[serde(default)]` on
//! most fields, so a renamed field became an empty string at runtime.
//!
//! Wire casing is load-bearing and is *not* uniform: `Status` and
//! `Severity` serialize in frontmatter casing (`"Pending"`, `"Critical"`)
//! so a ticket round-trips into a `plans/` file unchanged, while `Kind`
//! serializes lowercase (`"build"`). The `rename_all` attributes below are
//! the wire contract, not style.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Lifecycle status. Serialized with frontmatter casing (`Pending`, `Ready`, …)
/// so a ticket round-trips into a plans/ file unchanged.
///
/// SDLC-7 adds the two **worker states**, the middle of rim's lifecycle
/// under the house casing: a worker flips its ticket `Ready → Coding` the
/// moment it starts (so no leaf is double-claimed) and `Coding → In Review`
/// in the commit that finishes the work; the merge sets `Complete`. The
/// interim answer, not the final vocabulary — SDLC-2 owns that, and must
/// keep or explicitly map these two. `set_status` stays a free write: no
/// transition is gated (SDLC-4).
///
/// SDLC-11 adds `Doc`, the one status outside the lifecycle: the state a
/// [`Kind::Doc`] is born in and never leaves, and that no other kind ever
/// holds. It is in [`Status::ALL`] (a ticket can hold it, `list` filters on
/// it, every count has a bucket for it) and not in [`Status::LIFECYCLE`]
/// (nothing moves a ticket into or out of it, so no chooser offers it).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub enum Status {
    Pending,
    Ready,
    /// SDLC-7: a worker has claimed the ticket and work is under way.
    Coding,
    /// SDLC-7: the work is committed, gate green, and waits on the merge.
    /// The one status whose wire spelling is not its variant's name: it
    /// carries a space on the wire and in frontmatter, as written.
    #[serde(rename = "In Review")]
    InReview,
    Blocked,
    Complete,
    Idea,
    Epic,
    Superseded,
    /// SDLC-15: work that will not be done and that nothing replaced —
    /// `Superseded` without the successor. Done with, as `Superseded` is.
    Canceled,
    /// SDLC-11: a doc's, and only a doc's — set at its birth, never moved by
    /// `set_status`. SDLC-13: `set_kind` gives it to a ticket that becomes a
    /// doc and takes it from one that stops being one.
    Doc,
}

impl Status {
    /// Every status a ticket can hold — the lifecycle's ten, then a doc's
    /// one (SDLC-11). What a status count keeps a bucket for, what `list`
    /// filters on, and what the browser app's status table paints.
    pub const ALL: [Status; 11] = [
        Status::Pending,
        Status::Ready,
        Status::Coding,
        Status::InReview,
        Status::Blocked,
        Status::Complete,
        Status::Idea,
        Status::Epic,
        Status::Superseded,
        Status::Canceled,
        Status::Doc,
    ];

    /// The lifecycle: the statuses a ticket that has one moves between, in
    /// the order a chooser should offer them. The browser app's status
    /// control and its menu render exactly this — so a status added here
    /// appears in the UI without a second edit, and one removed cannot
    /// linger in it. SDLC-7: the worker states sit where a ticket passes
    /// through them, between `Ready` and `Complete`. SDLC-11: `Doc` is not
    /// here — no ticket moves into it or out of it, so no chooser offers it.
    pub const LIFECYCLE: [Status; 10] = [
        Status::Pending,
        Status::Ready,
        Status::Coding,
        Status::InReview,
        Status::Blocked,
        Status::Complete,
        Status::Idea,
        Status::Epic,
        Status::Superseded,
        Status::Canceled,
    ];

    /// The wire spelling — what serde writes and what `set_status` accepts.
    pub const fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "Pending",
            Status::Ready => "Ready",
            Status::Coding => "Coding",
            Status::InReview => "In Review",
            Status::Blocked => "Blocked",
            Status::Complete => "Complete",
            Status::Idea => "Idea",
            Status::Epic => "Epic",
            Status::Superseded => "Superseded",
            Status::Canceled => "Canceled",
            Status::Doc => "Doc",
        }
    }

    /// The status a ticket of `kind` is born in (SDLC-11): a doc in `Doc`,
    /// which it never leaves; anything else in `Pending`, which only a human
    /// moves on (to `Ready`).
    pub const fn at_birth(kind: Kind) -> Status {
        if kind.has_lifecycle() {
            Status::Pending
        } else {
            Status::Doc
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Case-insensitive, and lenient about the worker states (SDLC-7) — a
/// worker types them by hand, so the drift the ticket's Risks name (`In
/// review`, `in-review`, `in_review`) is accepted rather than refused, and
/// so is `active`: the house methodology (and CLAUDE.md, which quotes it)
/// calls the claimed state that, and a worker following it to the letter
/// must not be refused. What is *written* is single-sourced in the enum:
/// every spelling parses to one variant, which serializes as `Coding` or
/// `In Review`. SDLC-11: `Doc` parses too — `list` filters on it; that
/// `set_status` never writes it is the store's rule, not the parser's.
impl FromStr for Status {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "coding" | "active" => Ok(Self::Coding),
            "in review" | "in_review" | "in-review" | "inreview" => Ok(Self::InReview),
            "blocked" => Ok(Self::Blocked),
            "complete" => Ok(Self::Complete),
            "idea" => Ok(Self::Idea),
            "epic" => Ok(Self::Epic),
            "superseded" => Ok(Self::Superseded),
            "canceled" | "cancelled" => Ok(Self::Canceled),
            "doc" => Ok(Self::Doc),
            other => Err(format!(
                "unknown status '{other}' (expected Pending, Ready, Coding, In Review, Blocked, \
                 Complete, Idea, Epic, Superseded or Canceled — or Doc, a doc's own)"
            )),
        }
    }
}

/// What shape of work a ticket is (AGS-13): the owner's closed five-value
/// set, one vocabulary owning the slot — and, since SDLC-11 (owner,
/// 2026-10-02), a sixth that is not work at all: `doc`. Serialized
/// lowercase (`kind: build`) on the wire and in the store.
///
/// SDLC-13: set at birth, and changed only by `set_kind`, which moves
/// everything kept by kind in the same write. Its rules: to a doc, the
/// status becomes [`Status::Doc`] — refused while the ticket waits on a
/// blocker or a ticket waits on it; from a doc, the status becomes the one
/// given (never `Doc`), else `Epic` for a scope and `Pending` for any other
/// kind; between two kinds with a lifecycle the status stays unless one is
/// given. A shortcode's root never becomes a kind that roots nothing
/// (doc ↔ scope it may), and a lower-seq ticket that becomes a scope or a
/// doc becomes its shortcode's root.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A container that decomposes into children; done only when they
    /// are. Its acceptance is the operator's gate.
    Scope,
    /// Carries out a defined procedure (verify, run, migrate, release)
    /// without producing new capability.
    Execution,
    /// Produces new capability; gated on acceptance criteria.
    Build,
    /// A defect or missing piece discovered in shipped behavior (rim's
    /// "bug").
    Gap,
    /// A binary investigation that answers a question.
    Spike,
    /// SDLC-11: a document — a finding, a manifest, a reference. Outside the
    /// lifecycle: born in [`Status::Doc`] and never moved from it, never
    /// blocked and never a blocker, never ready, never done, and not
    /// counted in a roll-up's `done/total`. It keeps everything else a
    /// ticket has: a body and `amend`, a parent and children (docs or work),
    /// commits. It may root a shortcode, as a scope may.
    Doc,
}

impl Kind {
    /// Every kind — the owner's five, then the doc (SDLC-11).
    pub const ALL: [Kind; 6] = [
        Kind::Scope,
        Kind::Execution,
        Kind::Build,
        Kind::Gap,
        Kind::Spike,
        Kind::Doc,
    ];

    /// The phrase every "unknown kind" error ends with — the six, named.
    pub const EXPECTED: &'static str = "scope, execution, build, gap, spike, or doc";

    /// The wire spelling (lowercase).
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Scope => "scope",
            Kind::Execution => "execution",
            Kind::Build => "build",
            Kind::Gap => "gap",
            Kind::Spike => "spike",
            Kind::Doc => "doc",
        }
    }

    /// SDLC-11: whether a ticket of this kind moves through the lifecycle
    /// ([`Status::LIFECYCLE`]) — every kind but `doc`, which simply exists
    /// in its state.
    pub const fn has_lifecycle(self) -> bool {
        !matches!(self, Kind::Doc)
    }

    /// MNG-12 / SDLC-11: whether a ticket of this kind may be the first of
    /// a shortcode, and so its root: a scope, or a doc (a doc collection's
    /// root).
    pub const fn roots_a_shortcode(self) -> bool {
        matches!(self, Kind::Scope | Kind::Doc)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "scope" => Ok(Self::Scope),
            "execution" => Ok(Self::Execution),
            "build" => Ok(Self::Build),
            "gap" => Ok(Self::Gap),
            "spike" => Ok(Self::Spike),
            "doc" => Ok(Self::Doc),
            other => Err(format!(
                "unknown kind '{other}' (expected {})",
                Kind::EXPECTED
            )),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl Severity {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Critical => "Critical",
            Severity::High => "High",
            Severity::Medium => "Medium",
            Severity::Low => "Low",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Severity {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "critical" => Ok(Self::Critical),
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            other => Err(format!(
                "unknown severity '{other}' (expected Critical, High, Medium, or Low)"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Ticket {
    /// `EPIC-N`, allocated by the store.
    pub id: String,
    /// Epic prefix, uppercase (e.g. `MNG`).
    pub epic: String,
    /// Sequence number within the epic (the `N` in `EPIC-N`).
    pub seq: u32,
    pub title: String,
    pub status: Status,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Ticket ids that must complete before this one starts.
    pub blocked_by: Vec<String>,
    /// Ticket ids that can start once this one completes.
    pub unlocks: Vec<String>,
    /// AGS-19: the ticket this one is an output of, if any. Any epic; the
    /// parent must exist at creation and the chain is acyclic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// SCALE-4: how many tickets name this one as their `parent` — derived
    /// and kept by the store. It replaces AGS-19's `children`, the list of
    /// every child id, which made a shortcode root's row (every ticket of
    /// the shortcode hangs under it, MNG-14) as large as the shortcode. The
    /// children themselves come by page: `list { parent }`.
    pub child_count: u64,
    /// SDLC-9: the commits that finished this ticket, in the order they
    /// were linked (`tickets.link_commits`) — each a commit reference, a
    /// hexadecimal object name of 7 to 64 characters stored lowercase,
    /// optionally `repository@` before it. `[]` when none. Recorded, not
    /// verified: the server has no repository. At most 256 (SCALE-4's
    /// lesson — a list on a row is bounded).
    pub commits: Vec<String>,
    /// WAVE-10: the ticket's **path address** inside its shortcode, if it
    /// has one — `<SHORTCODE>/<path>` names it (`get { id:
    /// "PLEXUS/_MENGER/board" }`), and no other ticket of the shortcode
    /// holds the same path. An address, not an id: the ticket keeps its
    /// `EPIC-N`, and the store's order, extents, paging and readiness never
    /// read it. Only the reserved prefix `_MENGER/` is accepted, on a doc
    /// filed under its shortcode's scope root — menger's own documents
    /// (WAVE-1 decision 11). Omitted on the wire when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// WAVE-6: where the ticket sits in the order of work ([`Placement`]),
    /// kept by the store — equal at every moment to WAVE-4's rule over the
    /// whole `blocked_by` graph, moved by the write that moves it (a birth
    /// that resolves a forward reference, a status crossing `Complete` /
    /// `Superseded` / `Canceled`), and announced by `placements_changed`. `None` —
    /// omitted on the wire — for a doc, which has no place in the order of
    /// work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    /// MENGER-12: the ticket's labels — free-form tags (`needs-you`, …),
    /// each lower-case letters, digits and `-` ([`crate::labels`]), sorted,
    /// each once, at most 16; `[]` when none. Moved by `tickets.label`
    /// alone, and asked by the filter cell `label <name>`. `default`: a
    /// server from before MENGER-12 sends none.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Markdown body (Problem / Required behavior / Acceptance criteria …).
    pub body: String,
    pub created_unix: i64,
    pub updated_unix: i64,
}

/// MNG-9: a ticket row without its body — every field of [`Ticket`]
/// except `body`.
///
/// The body is ~70 % of a ticket's weight (PERF-4: 98 MiB of `list` at
/// 100,000 tickets, against ~30 MiB of everything-but-bodies), and a
/// board, a nav or a picker shows none of it — they show what is here.
/// A consumer that needs one body calls `get` for that one ticket.
///
/// Unlike `Ticket`, `severity` and `parent` are written out as `null`
/// rather than skipped: a row shape a client destructures should have
/// the same keys in every row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TicketSummary {
    pub id: String,
    pub epic: String,
    pub seq: u32,
    pub title: String,
    pub status: Status,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub parent: Option<String>,
    pub child_count: u64,
    pub blocked_by: Vec<String>,
    pub unlocks: Vec<String>,
    /// SDLC-9: as on [`Ticket`] — on the row, so the board, the nav and
    /// the ticket page all have it without a `get`.
    pub commits: Vec<String>,
    /// WAVE-10: as on [`Ticket`] — on the row, so the nav can hide
    /// `_MENGER/` documents without a `get`. Unlike `severity` and
    /// `parent`, omitted when there is none (the ticket's contract): every
    /// ticket but menger's own documents would carry a `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// WAVE-6: as on [`Ticket`] — the kept placement, on the row, so the
    /// board, the nav and a body's `{wave: ID}` have it without a `get`;
    /// omitted for a doc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    /// MENGER-12: as on [`Ticket`] — on the row, so the board, the nav and
    /// Home's "Needs you" draw a ticket's label chips without a `get`.
    #[serde(default)]
    pub labels: Vec<String>,
    pub created_unix: i64,
    pub updated_unix: i64,
}

impl From<Ticket> for TicketSummary {
    fn from(t: Ticket) -> Self {
        Self {
            id: t.id,
            epic: t.epic,
            seq: t.seq,
            title: t.title,
            status: t.status,
            kind: t.kind,
            severity: t.severity,
            parent: t.parent,
            child_count: t.child_count,
            blocked_by: t.blocked_by,
            unlocks: t.unlocks,
            commits: t.commits,
            path: t.path,
            placement: t.placement,
            labels: t.labels,
            created_unix: t.created_unix,
            updated_unix: t.updated_unix,
        }
    }
}

/// WAVE-10: the reserved prefix of every path menger accepts — the
/// documents menger itself keeps inside a shortcode (WAVE-1 decision 11),
/// hidden from the nav unless asked for.
pub const MENGER_PATH_PREFIX: &str = "_MENGER/";

/// WAVE-10: whether `path` is one menger keeps for itself — `_MENGER/` and
/// at least one character after it. The one rule `create { path }` accepts
/// and the nav hides by.
pub fn is_menger_path(path: &str) -> bool {
    path.len() > MENGER_PATH_PREFIX.len() && path.starts_with(MENGER_PATH_PREFIX)
}

/// FILTER-21: whether a ticket with this path is one of menger's own
/// documents — its path starts with `_MENGER/`. What the filter's `menger
/// doc` flag admits, and what a ticket's cell keeps.
pub fn is_menger_doc(path: Option<&str>) -> bool {
    path.is_some_and(|p| p.starts_with(MENGER_PATH_PREFIX))
}

/// WAVE-10: a **path address** — `<SHORTCODE>/<path>` — split into the
/// shortcode (upper-cased: shortcodes are case-insensitive) and the path
/// (exactly as written). `None` for anything else — an `EPIC-N` id has no
/// `/`, and an address needs a shortcode and a path on either side of the
/// first one.
pub fn path_address(id: &str) -> Option<(String, &str)> {
    let (shortcode, path) = id.split_once('/')?;
    let shortcode = shortcode.trim();
    if shortcode.is_empty() || path.is_empty() || !shortcode.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some((shortcode.to_ascii_uppercase(), path))
}

/// MFE-9: the same narrowing from a borrowed ticket, for a consumer that
/// keeps summaries and holds the body elsewhere (the browser app's row
/// store: a `watch` event carries a whole `Ticket`, the row wants the
/// summary of it and nothing more).
impl From<&Ticket> for TicketSummary {
    fn from(t: &Ticket) -> Self {
        t.clone().into()
    }
}

/// MNG-10: one shortcode's roll-up — the row AGS-20's nav draws for it,
/// computed by the server so a client can draw the tree without holding
/// every ticket.
///
/// The numbers are the nav's, exactly (`frontend/src/nav.rs`, `plan()` and
/// the group row's rollup): `total` counts every ticket (SDLC-11: but the
/// docs) in the shortcode
/// row's subtree as the nav nests it — its own tickets, a ticket of another
/// shortcode filed under one of them, and every sub-shortcode hosted
/// beneath it, at any depth — and `done` counts the `Complete` ones among
/// them (`Superseded` is not done; the nav's rule). `root` is the
/// shortcode's lowest-seq `scope` ticket (`None` only for a prefix that
/// never had one). `host` is set when the shortcode is a **sub-shortcode**
/// (AGS-20): the ticket, in another shortcode, that its root is filed
/// under and that its row nests beneath — `None` for a top-level row.
///
/// Like [`TicketSummary`], the optional fields are written out as `null`
/// rather than skipped: every group has the same keys.
///
/// SDLC-11: a doc has no completion, so `done/total` is the work alone —
/// `total` counts every ticket of the subtree but its docs, and a scope's
/// done-ness ignores doc children. `docs` counts the docs beside it, so a
/// shortcode that holds only docs (a doc collection, rooted at a doc) is
/// still a row: a group is drawn when it holds anything ([`Group::drawn`]),
/// and one that arrives with `total: 0` and `docs: 0` is a row no longer
/// drawn. `root` is the shortcode's lowest-seq scope — or doc: a doc may
/// root a shortcode as a scope may.
///
/// AGS-23: the nav draws a shortcode that has a root *as* its root ticket's
/// row, so the group carries what that row shows — `root_row`, the root's
/// title, status and kind, read from the server's cache — and the top level
/// still paints from `groups {}` alone, with no `get` per shortcode. It is
/// kept current the way the counts are: a change to the root's title,
/// status or kind (SDLC-13: `set_kind`) is a change to its group, and
/// `group_changed` carries it. `None` exactly when `root` is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Group {
    /// The shortcode (epic prefix), uppercase.
    pub epic: String,
    pub total: u64,
    pub done: u64,
    /// SDLC-11: the docs in the row's subtree, which `total` leaves out.
    pub docs: u64,
    pub root: Option<String>,
    /// AGS-23: what the root's own row shows.
    pub root_row: Option<RootRow>,
    pub host: Option<String>,
}

/// AGS-23: a shortcode root's row as the nav draws it at the top of its
/// shortcode — the three fields of the root ticket a row shows beside its
/// id ([`Group::root`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RootRow {
    pub title: String,
    pub status: Status,
    pub kind: Kind,
}

impl Group {
    /// Whether the nav draws this row: it holds a ticket, work or doc.
    pub fn drawn(&self) -> bool {
        self.total > 0 || self.docs > 0
    }
}

/// WAVE-4: where a ticket sits in the order of work — what `waves` says of
/// each ticket it places, derived from `blocked_by` over the whole graph by
/// MNG-4's rule (a blocker is satisfied iff it is a `Complete`,
/// `Superseded` or `Canceled` ticket; an id naming no ticket never is).
///
/// - `Wave { n }` — unfinished, and `n` is 1 when every blocker is
///   satisfied, else 1 + the highest wave among its unsatisfied blockers.
/// - `Done` — the ticket itself is `Complete` or `Superseded`.
/// - `MissingBlocker` — it, or something it waits on, names a blocker that
///   is no ticket: no wave can be given until that id exists.
///
/// Internally tagged like the events (`{"type": "wave", "n": 2}`,
/// `{"type": "done"}`, `{"type": "missing_blocker"}`). WAVE-6: the store
/// keeps it — on every row ([`Ticket::placement`]), current after every
/// write — and says when one moves ([`PlacementChange`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Placement {
    Wave { n: u32 },
    Done,
    MissingBlocker,
}

/// WAVE-6: one ticket whose kept placement a write moved — its id and the
/// placement it has now (a `placements_changed` carries one per ticket).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlacementChange {
    pub id: String,
    pub placement: Placement,
}

/// SEARCH-2: a stretch of a search hit's title or passage, in order — `hit`
/// when it is a word the query matched, which a client marks; the stretches
/// of one title or passage, joined, are its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Marked {
    pub text: String,
    pub hit: bool,
}

/// SEARCH-2: one word of a `search`, as the query was read — and the
/// index's words it also matched, being one or two edits away (a typo).
/// `also` is empty for a word the index holds — as itself, or as the start
/// of its words — in more than a few tickets: such a word is never
/// corrected (one it holds in a few, only to far commoner words). A phrase (`"…"`, or a word joined by `-`, `_`, `.`,
/// `/`) is one entry, its words joined by spaces, never corrected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchWord {
    pub word: String,
    pub also: Vec<String>,
}

/// COMP-3: one entry of the **component registry** — a custom element some
/// ticket defined in a ` ```component <name> ` block (COMP-1 decisions 2, 3,
/// pins 6–8), as the server keeps it.
///
/// - `name` — the tag: a custom element's name, lowercase, with a hyphen
///   (`m-wave-summary`).
/// - `source` — the block's content, exactly as written between its fences
///   (a `<template>`, an optional `<style>`, an optional `<script>`).
/// - `owner` — the ticket whose body defines it; `null` for an **orphan**: a
///   component whose definition left its ticket while something still
///   references it. An orphan is read-only; a ticket that defines its name
///   again **adopts** it.
/// - `references` — what uses it: each ticket whose body holds `<name` as an
///   HTML tag (outside code spans and fences, once per ticket), in the
///   store's `(epic, seq)` order, then each component whose source holds it,
///   by name.
///
/// - `docs` (COMP-8) — the source's `<docs>` element, parsed
///   ([`ComponentDocs`]); `null` when the source holds none.
/// - `preview` (COMP-8) — the attributes written after the tag on the
///   block's opening line (` ```component m-x board=RETURNS label="Platform" `),
///   `[name, value]` in the order written; a value may be double-quoted, and
///   a bare name's value is `""`. What a live preview of the definition is
///   rendered with.
///
/// Unlike [`Ticket`], `owner` and `docs` are written out as `null` rather
/// than skipped: every component has the same keys, and `null` is what
/// "orphan" (or "no docs yet") looks like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Component {
    pub name: String,
    pub source: String,
    pub owner: Option<String>,
    pub references: Vec<String>,
    pub docs: Option<ComponentDocs>,
    pub preview: Vec<(String, String)>,
}

/// COMP-8: a component's help — its source's one `<docs>` element, parsed.
///
/// ```html
/// <docs>
///   <summary>How many of a board's tickets are workable now.</summary>
///   <attr name="board" type="shortcode" required>The shortcode to count.</attr>
///   <slot name="title">Optional title markup.</slot>
///   <example><m-wave-summary board="RETURNS"></m-wave-summary></example>
///   Longer help, as markdown prose.
/// </docs>
/// ```
///
/// - `summary` — the `<summary>`'s text, tags dropped, whitespace collapsed,
///   trimmed (`""` when there is none).
/// - `attrs` — one [`AttrDoc`] per `<attr>`, in the order written.
/// - `slots` — one [`SlotDoc`] per `<slot>`, in the order written.
/// - `parts` — THEME-5: one [`PartDoc`] per `<part>`, in the order written:
///   what a theme may skin with `m-name::part(name)`. Each names a `part`
///   the component's `<template>` carries (or, marked `scripted`, one its
///   script makes) — a documented part the template lacks is refused.
/// - `examples` — each `<example>`'s inner HTML as written, its common
///   indentation and the blank lines around it removed.
/// - `help` — everything else inside `<docs>`, as markdown: the lines the
///   elements above stood on removed, the rest de-indented and trimmed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ComponentDocs {
    pub summary: String,
    pub attrs: Vec<AttrDoc>,
    pub slots: Vec<SlotDoc>,
    #[serde(default)]
    pub parts: Vec<PartDoc>,
    pub examples: Vec<String>,
    pub help: String,
}

/// COMP-8: one declared attribute — `<attr name="board" type="shortcode"
/// required>The shortcode to count.</attr>`. `name` is lowercased (HTML
/// folds attribute names); `kind` is the `type` (`string` when omitted);
/// `required` is whether the `required` attribute is present;
/// `description` the element's text, as [`ComponentDocs::summary`] is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AttrDoc {
    pub name: String,
    pub kind: AttrKind,
    pub required: bool,
    pub description: String,
}

/// COMP-8: one declared slot — `<slot name="title">Optional title
/// markup.</slot>`; `name` is `null` for the default slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SlotDoc {
    pub name: Option<String>,
    pub description: String,
}

/// THEME-5: one declared part — `<part name="label">The field's
/// label.</part>` — a piece of the component's shadow tree that a theme may
/// skin with `m-name::part(label)` (looks only, THEME-1's `skins`).
/// `scripted`: the component's script makes it (`el.part = …`), so the
/// template is not asked for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PartDoc {
    pub name: String,
    #[serde(default)]
    pub scripted: bool,
    pub description: String,
}

/// COMP-8: what an attribute's value must be — an `<attr type=…>`, written
/// `string` (the default), `number`, `boolean`, `shortcode` (an existing
/// shortcode, e.g. `RETURNS`), `ticket` (an existing ticket id) or
/// `enum(a|b|c)` (one of those words). Internally tagged on the wire, as
/// [`Placement`] is: `{"type": "shortcode"}`, `{"type": "enum", "values":
/// ["a", "b"]}`. [`FromStr`] reads the written form and [`fmt::Display`]
/// writes it back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AttrKind {
    #[default]
    String,
    Number,
    Boolean,
    Shortcode,
    Ticket,
    Enum { values: Vec<String> },
}

impl FromStr for AttrKind {
    type Err = String;

    /// The written form, exactly (`enum(` … `)` with its values split on
    /// `|` and trimmed). An unknown word, or an `enum` naming no value, is
    /// refused with the reason.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        Ok(match s {
            "string" => Self::String,
            "number" => Self::Number,
            "boolean" => Self::Boolean,
            "shortcode" => Self::Shortcode,
            "ticket" => Self::Ticket,
            _ => {
                let Some(inner) = s.strip_prefix("enum(").and_then(|r| r.strip_suffix(')')) else {
                    return Err(format!(
                        "unknown type '{s}' — one of string, number, boolean, shortcode, ticket, enum(a|b|c)"
                    ));
                };
                let values: Vec<String> = inner
                    .split('|')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
                    .collect();
                if values.is_empty() {
                    return Err(format!("the type '{s}' names no value — enum(a|b|c)"));
                }
                Self::Enum { values }
            }
        })
    }
}

/// COMP-9: what a caller knows exists, for checking a `shortcode` or
/// `ticket` value — `None` when it cannot say (nothing loaded yet), and the
/// check then passes the value as far as the form goes.
pub trait Known {
    /// Whether a shortcode holds a ticket (`epic` upper case).
    fn shortcode(&self, epic: &str) -> Option<bool>;
    /// Whether a ticket has this id (upper case).
    fn ticket(&self, id: &str) -> Option<bool>;
}

/// COMP-9: knowing nothing — only the form of a value is checked.
pub struct KnowNothing;

impl Known for KnowNothing {
    fn shortcode(&self, _: &str) -> Option<bool> {
        None
    }
    fn ticket(&self, _: &str) -> Option<bool> {
        None
    }
}

/// A shortcode's form: ASCII letters and digits (the server folds case).
fn is_shortcode(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
}

impl AttrKind {
    /// COMP-9: whether an instance's attribute `value` fits — `Err` says
    /// why not. `string`: anything. `number`: a finite decimal number.
    /// `boolean`: present is true, so `""`, `true` or `false` (any case).
    /// `shortcode`: letters and digits naming a shortcode that holds a
    /// ticket; `ticket`: `SHORTCODE-N` naming a ticket that exists (both
    /// case-insensitive, as the server reads them; existence as `known`
    /// says). `enum(a|b)`: exactly one of the values.
    pub fn check(&self, value: &str, known: &dyn Known) -> Result<(), String> {
        let v = value.trim();
        match self {
            Self::String => Ok(()),
            Self::Number => match v.parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(()),
                _ => Err(format!("'{value}' is not a number")),
            },
            Self::Boolean => match v.to_ascii_lowercase().as_str() {
                "" | "true" | "false" => Ok(()),
                _ => Err(format!("'{value}' is not a boolean — write the attribute alone, or true / false")),
            },
            Self::Shortcode => {
                if !is_shortcode(v) {
                    return Err(format!("'{value}' is not a shortcode (letters and digits, e.g. RETURNS)"));
                }
                match known.shortcode(&v.to_ascii_uppercase()) {
                    Some(false) => Err(format!("no shortcode '{}'", v.to_ascii_uppercase())),
                    _ => Ok(()),
                }
            }
            Self::Ticket => {
                let id = v.to_ascii_uppercase();
                let formed = id
                    .rsplit_once('-')
                    .is_some_and(|(epic, seq)| is_shortcode(epic) && !seq.is_empty() && seq.chars().all(|c| c.is_ascii_digit()));
                if !formed {
                    return Err(format!("'{value}' is not a ticket id (e.g. MNG-2)"));
                }
                match known.ticket(&id) {
                    Some(false) => Err(format!("no ticket '{id}'")),
                    _ => Ok(()),
                }
            }
            Self::Enum { values } => {
                if values.iter().any(|x| x == v) {
                    Ok(())
                } else {
                    Err(format!("'{value}' is not one of {}", values.join(" | ")))
                }
            }
        }
    }
}

/// COMP-9: one declared attribute on one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrState {
    /// Set, and its value fits.
    Fits,
    /// Not set, and not required.
    Unset,
    /// Required, and not set.
    Missing,
    /// Set to a value that does not fit — why.
    Unfit(String),
}

impl AttrDoc {
    /// COMP-9: this attribute on an instance that sets it to `value`
    /// (`None`: not set). A boolean set to its own name
    /// (`compact="compact"`) is true, as HTML has it.
    pub fn state(&self, value: Option<&str>, known: &dyn Known) -> AttrState {
        match value {
            None if self.required => AttrState::Missing,
            None => AttrState::Unset,
            Some(v) if self.kind == AttrKind::Boolean && v.trim().eq_ignore_ascii_case(&self.name) => AttrState::Fits,
            Some(v) => match self.kind.check(v, known) {
                Ok(()) => AttrState::Fits,
                Err(why) => AttrState::Unfit(why),
            },
        }
    }
}

impl fmt::Display for AttrKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String => f.write_str("string"),
            Self::Number => f.write_str("number"),
            Self::Boolean => f.write_str("boolean"),
            Self::Shortcode => f.write_str("shortcode"),
            Self::Ticket => f.write_str("ticket"),
            Self::Enum { values } => write!(f, "enum({})", values.join("|")),
        }
    }
}

/// INPUT-3: what an **input** asks for (INPUT-1 decision 2, the owner's
/// fixed set): `choice` — one of its options (an `<m-decision>`); `multi` —
/// any of its options, a set; `text` — a free reply (both an `<m-input>`).
/// Lowercase on the wire, as [`Kind`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Choice,
    Multi,
    Text,
}

impl InputKind {
    /// The wire spelling — also how a body's `kind="…"` writes it.
    pub const fn as_str(self) -> &'static str {
        match self {
            InputKind::Choice => "choice",
            InputKind::Multi => "multi",
            InputKind::Text => "text",
        }
    }
}

impl fmt::Display for InputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Case-insensitive and trimmed, as a body's `kind` attribute is read.
impl FromStr for InputKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "choice" => Ok(Self::Choice),
            "multi" => Ok(Self::Multi),
            "text" => Ok(Self::Text),
            other => Err(format!("unknown input kind '{other}' (expected choice, multi, or text)")),
        }
    }
}

/// INPUT-3: where an input stands (INPUT-1 decision 4): `open` — waiting
/// for an answer; `answered` — it holds one, which may change; `closed` —
/// frozen, with or without an answer (DECIDED, for a decision); `removed` —
/// its body no longer declares it, and its history is kept. The state is
/// what the history says (the last answer or clear, the last close or
/// reopen) while the body declares the input, and `removed` when it does not.
///
/// MENGER-13: `withdrawn` — its ticket was canceled while it waited (open):
/// nobody is asked any more, it takes no answer, and its history is kept;
/// the ticket leaving `Canceled` restores it. "Has an input in state `open`"
/// is therefore "is asking someone", with no case for a canceled ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputState {
    Open,
    Answered,
    Closed,
    Removed,
    Withdrawn,
}

impl InputState {
    pub const ALL: [InputState; 5] =
        [InputState::Open, InputState::Answered, InputState::Closed, InputState::Removed, InputState::Withdrawn];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            InputState::Open => "open",
            InputState::Answered => "answered",
            InputState::Closed => "closed",
            InputState::Removed => "removed",
            InputState::Withdrawn => "withdrawn",
        }
    }
}

impl fmt::Display for InputState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InputState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "answered" => Ok(Self::Answered),
            "closed" => Ok(Self::Closed),
            "removed" => Ok(Self::Removed),
            "withdrawn" => Ok(Self::Withdrawn),
            other => Err(format!("unknown input state '{other}' (expected open, answered, closed, removed, or withdrawn)")),
        }
    }
}

/// INPUT-3: who made a change to an input (INPUT-1's language) — `app`, a
/// viewer's click, which the host says on its `answer`; `api`, everything
/// else (synapse, an agent). The only "who" until CONTEXT adds identity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnswerSource {
    App,
    #[default]
    Api,
    /// MENGER-13: the store itself — an input withdrawn because its ticket
    /// was canceled, or restored when it was not any more. Never a caller's
    /// to say: `answer` and `clear_answer` refuse it.
    System,
}

impl AnswerSource {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            AnswerSource::App => "app",
            AnswerSource::Api => "api",
            AnswerSource::System => "system",
        }
    }
}

impl fmt::Display for AnswerSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AnswerSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "app" => Ok(Self::App),
            "api" => Ok(Self::Api),
            "system" => Ok(Self::System),
            other => Err(format!("unknown answer source '{other}' (expected app or api)")),
        }
    }
}

/// INPUT-3: an answer's value, typed by its input's kind — internally
/// tagged, as [`Placement`] is: `{"type": "choice", "option": "archive"}`
/// (one option's `value`), `{"type": "multi", "options": ["a", "b"]}` (a set
/// of them — stored in the order the body declares them, each once; it may
/// be empty: none ticked), `{"type": "text", "text": "…"}` (a reply, not
/// blank). Through synapse, a param's fields are dotted flags:
/// `--value.type choice --value.option archive`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputValue {
    Choice { option: String },
    Multi { options: Vec<String> },
    Text { text: String },
}

impl InputValue {
    /// The kind of input this value answers.
    pub const fn kind(&self) -> InputKind {
        match self {
            InputValue::Choice { .. } => InputKind::Choice,
            InputValue::Multi { .. } => InputKind::Multi,
            InputValue::Text { .. } => InputKind::Text,
        }
    }
}

/// INPUT-3: one option an input declares — `value` is what an answer names
/// (an `<m-option>`'s `value`, else its `name`; an `<m-choice>`'s `value`),
/// `label` what a reader sees (the `name`, else the value; an `<m-choice>`'s
/// text, else its value), `recommended` whether the option is marked so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InputOption {
    pub value: String,
    pub label: String,
    pub recommended: bool,
}

/// INPUT-3: an input's current answer — the value, its rationale (`null`
/// when none was given), who gave it and when (unix seconds). The latest
/// `answered` of its history, unless a `cleared` came after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Answer {
    pub value: InputValue,
    pub rationale: Option<String>,
    pub source: AnswerSource,
    pub at_unix: i64,
}

/// INPUT-3: what one entry of an input's history did — an answer recorded
/// (`answered`), the answer cleared (`cleared`), the input frozen
/// (`closed`) or unfrozen (`reopened`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputAction {
    Answered,
    Cleared,
    Closed,
    Reopened,
    /// MENGER-13: its ticket was canceled while it was open — it waits on
    /// no one now (`withdrawn`); the rationale says why.
    Withdrawn,
    /// MENGER-13: its ticket left `Canceled` — it is what its history says
    /// again.
    Restored,
}

impl InputAction {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            InputAction::Answered => "answered",
            InputAction::Cleared => "cleared",
            InputAction::Closed => "closed",
            InputAction::Reopened => "reopened",
            InputAction::Withdrawn => "withdrawn",
            InputAction::Restored => "restored",
        }
    }
}

impl FromStr for InputAction {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "answered" => Ok(Self::Answered),
            "cleared" => Ok(Self::Cleared),
            "closed" => Ok(Self::Closed),
            "reopened" => Ok(Self::Reopened),
            "withdrawn" => Ok(Self::Withdrawn),
            "restored" => Ok(Self::Restored),
            other => Err(format!("unknown input action '{other}'")),
        }
    }
}

/// INPUT-3: one change an input went through, oldest first — the action,
/// the value and rationale of an `answered` (`null` for the others), who
/// made it and when (unix seconds). Every change is kept, a removed input's
/// included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InputHistoryEntry {
    pub action: InputAction,
    pub value: Option<InputValue>,
    pub rationale: Option<String>,
    pub source: AnswerSource,
    pub at_unix: i64,
}

/// INPUT-3: one **input** — a component in a ticket's body that asks for an
/// answer (INPUT-1), as the store keeps it: the ticket it is in, its `id`
/// (unique in that ticket), its kind, what the body declares of it — the
/// question (`null` without an `<m-question>`), the options in the body's
/// order, the recommendation (the `<m-recommendation>`'s text, else the
/// label of the option marked `recommended`; `null` with neither) — and
/// what has been answered: its state, its current answer (`null` when it
/// holds none) and its whole history, oldest first.
///
/// The declaration follows the body: every write of the body re-reads it.
/// An answer stays what was recorded when a later body changes the options
/// it named — the history is a record, never rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Input {
    pub ticket: String,
    pub id: String,
    pub kind: InputKind,
    pub question: Option<String>,
    pub options: Vec<InputOption>,
    pub recommendation: Option<String>,
    pub state: InputState,
    pub answer: Option<Answer>,
    pub history: Vec<InputHistoryEntry>,
}

/// MENGER-13: which of a ticket's fields a transition moved — its status, or
/// its kind (SDLC-13's `set_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionField {
    Status,
    Kind,
}

impl TransitionField {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            TransitionField::Status => "status",
            TransitionField::Kind => "kind",
        }
    }
}

impl FromStr for TransitionField {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "status" => Ok(Self::Status),
            "kind" => Ok(Self::Kind),
            other => Err(format!("unknown transition field '{other}' (expected status or kind)")),
        }
    }
}

/// MENGER-13: where a transition came from — `app` (a viewer in the app,
/// which says so), `api` (synapse, an agent, a script: the default),
/// `pipeline` (the merge, which says so), `cascade` (the store itself: a
/// cancellation carried down a canceled ticket's subtree, or undone).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionSource {
    App,
    #[default]
    Api,
    Pipeline,
    Cascade,
}

impl TransitionSource {
    /// The four, in the order above.
    pub const ALL: [TransitionSource; 4] =
        [TransitionSource::App, TransitionSource::Api, TransitionSource::Pipeline, TransitionSource::Cascade];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            TransitionSource::App => "app",
            TransitionSource::Api => "api",
            TransitionSource::Pipeline => "pipeline",
            TransitionSource::Cascade => "cascade",
        }
    }
}

impl fmt::Display for TransitionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Case-insensitive; `synapse` is `api`.
impl FromStr for TransitionSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "app" => Ok(Self::App),
            "api" | "synapse" => Ok(Self::Api),
            "pipeline" | "merge" => Ok(Self::Pipeline),
            "cascade" => Ok(Self::Cascade),
            other => Err(format!("unknown source '{other}' (expected app, api, pipeline or cascade)")),
        }
    }
}

/// MENGER-13: who a transition's actor is when no one signed in made it:
/// the store's own cascades and restores.
pub const SYSTEM_ACTOR: &str = "system";

/// MENGER-13: the actor of a transition made with login off — nobody signed
/// in to name (its `source` still says where it came from).
pub const ANONYMOUS_ACTOR: &str = "anonymous";

/// MENGER-13: one **transition** — a change of one ticket's status or kind,
/// as the store keeps it, append-only: `id` (the log's own, from 1, in the
/// order written), the `ticket`, the `field` it moved, `from` → `to` (the
/// wire spellings: `In Review`, `build`; `from` is `null` at a ticket's
/// birth), when (`at_unix`), who (`actor`: the caller's principal, `system`
/// for the store's own, `anonymous` with login off; `name` the caller's
/// display name, when it has one), where it came from (`source`), why
/// (`message`, always given for `system`) and what caused it (`cause`: the
/// transition a cascade carried down, or undid; `null` for a call's own).
/// The log starts when a file gains it (MENGER-13): a ticket written before
/// has no history before then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Transition {
    pub id: u64,
    pub ticket: String,
    pub field: TransitionField,
    pub from: Option<String>,
    pub to: String,
    pub at_unix: i64,
    pub actor: String,
    pub name: Option<String>,
    pub source: TransitionSource,
    pub message: Option<String>,
    pub cause: Option<u64>,
    /// The ticket `cause` is a transition of — the canceled (or restored)
    /// ticket a cascade carried down from; `null` without a cause.
    pub cause_ticket: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TicketsEvent {
    /// Heartbeat.
    Health {
        version: String,
        unix_now: i64,
        ticket_count: u64,
    },
    /// A ticket was created (yielded by `create`, and by `watch` on every creation).
    Created { ticket: Ticket },
    /// One ticket, as it exists now (yielded by `get`, and per-row by
    /// `list` and `ready_set`).
    Ticket { ticket: Ticket },
    /// MNG-9: one ticket row without its body — what `list` and
    /// `ready_set` yield per row instead of `ticket` when the call asked
    /// for `summary`, and what `find` always yields (SCALE-6).
    Summary { summary: TicketSummary },
    /// MNG-9: where a `list` / `ready_set` / `find` walk stands, yielded once as
    /// the last data event of the call. `next` is the cursor to pass as
    /// the next call's `after` — `null` when nothing more matches, which
    /// is how a client knows the walk is over — and `remaining` counts
    /// the matches beyond this page under the same filters.
    ///
    /// FILTER-4: `first` is the position of the page's first row among the
    /// matches, 0-based — for an empty page, the position it would start
    /// at — given by `list` when it was asked a `filter` or an `at`, and
    /// omitted (not null) otherwise, so every other page is byte for byte
    /// what it was. A position plus the page is how a windowed client
    /// knows where the rows it holds sit.
    Page {
        next: Option<String>,
        remaining: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first: Option<u64>,
    },
    /// A ticket's status changed (yielded by `set_status`, and by `watch`).
    StatusChanged { ticket: Ticket, previous: Status },
    /// SDLC-13: a ticket's kind changed (yielded by `set_kind`, and by
    /// `watch`) — and its status with it, when the change crossed the doc
    /// line or the call gave one; `previous` is the kind it had. The whole
    /// ticket rides along, its new placement included, so applying it is
    /// one upsert, as `status_changed` is. First of its write's events: the
    /// `group_changed`, `placements_changed` and (on a filtered watch)
    /// `entered` / `left` it caused follow, as they follow a status change.
    /// `set_kind`'s own answer to a call that changed nothing has `previous`
    /// equal to the ticket's kind; `watch` never carries that one.
    KindChanged { ticket: Ticket, previous: Kind },
    /// MNG-16: a ticket's title and/or body was replaced (yielded by
    /// `amend`, and by `watch` for every amend that changed something).
    /// `changed` names what moved — `"title"`, `"body"`, both in that
    /// order, or nothing: `amend`'s own answer to a no-op, which `watch`
    /// never carries. The full ticket rides along, body included, so a
    /// watch-only client — and an open ticket page — needs no `get`.
    Amended { ticket: Ticket, changed: Vec<String> },
    /// SDLC-9: commits were linked to a ticket (yielded by `link_commits`,
    /// and by `watch` for every link that changed something). `added` is
    /// what was newly linked, in the order given — the ticket's `commits`
    /// end with it — or nothing: `link_commits`' own answer when every
    /// entry was already linked, which `watch` never carries. The whole
    /// ticket rides along, as with every write after birth.
    CommitsLinked { ticket: Ticket, added: Vec<String> },
    /// SDLC-9: commits were unlinked from a ticket (yielded by
    /// `unlink_commits`, and by `watch` for every unlink that changed
    /// something). `removed` is what came off, in the order given; empty
    /// when none of it was linked, which `watch` never carries.
    CommitsUnlinked { ticket: Ticket, removed: Vec<String> },
    /// MENGER-12: a ticket's labels moved (yielded by `label`, and by
    /// `watch` for every `label` that changed something). `added` and
    /// `removed` are what moved, in the order given — both empty is
    /// `label`'s own answer to a call that moved nothing, which `watch`
    /// never carries. The whole ticket rides along, its `labels` as the
    /// write left them.
    LabelsChanged { ticket: Ticket, added: Vec<String>, removed: Vec<String> },
    /// An existing ticket's `unlocks` gained `added`: a newly created
    /// ticket named it in `blocked_by` (yielded by `watch`). Carries the
    /// full updated ticket so a watch-only client stays coherent.
    UnlocksChanged { ticket: Ticket, added: String },
    /// MENGER-30: an existing ticket's `unlocks` lost `removed`, which no
    /// longer waits on it (`update { unblock }`; yielded by `watch`). The
    /// full updated ticket, as `unlocks_changed`.
    UnlocksRemoved { ticket: Ticket, removed: String },
    /// MENGER-30: a ticket's `blocked_by` moved after birth (`update`;
    /// yielded by `watch`) — `added` and `removed` as they moved. The whole
    /// ticket rides along; each blocker's side follows, `unlocks_changed` or
    /// `unlocks_removed`, then the placements it moved.
    BlockersChanged { ticket: Ticket, added: Vec<String>, removed: Vec<String> },
    /// MENGER-30: a ticket's severity moved (`update`; yielded by `watch`);
    /// `previous` the one it had (`null`: none). The whole ticket rides along.
    SeverityChanged { ticket: Ticket, previous: Option<Severity> },
    /// MENGER-30: `update`'s answer — the ticket as the write left it, and
    /// the aspects that moved, in this order: `title`, `body`, `kind`,
    /// `status`, `parent`, `severity`, `blocked_by`. Empty when nothing
    /// would change, which wrote nothing; `watch` never carries it (it
    /// carries each aspect's own event).
    Updated { ticket: Ticket, changed: Vec<String> },
    /// AGS-19: the ticket `id` gained the child `added` — a child was born
    /// naming it as `parent`, or was moved under it by `set_parent`
    /// (yielded by `watch`). SCALE-4: the parent's id and its new
    /// `child_count`, not the parent: the whole parent carried its whole
    /// list of children, 2 MB under a root of 163,530. A client that
    /// mirrors rows sets the count; one that shows the parent's children
    /// adds `added` to what it holds.
    ChildrenChanged {
        id: String,
        added: String,
        child_count: u64,
    },
    /// MNG-12: a ticket's `parent` was set after birth (yielded by
    /// `set_parent`, and by `watch`); `previous` is the parent it had, if
    /// any. Carries the full child.
    ParentChanged {
        ticket: Ticket,
        previous: Option<String>,
    },
    /// MNG-12: the ticket `id` lost the child `removed` — that child was
    /// re-parented elsewhere (yielded by `watch`). SCALE-4: the ex-parent's
    /// id and its new `child_count`, as `ChildrenChanged`.
    ChildRemoved {
        id: String,
        removed: String,
        child_count: u64,
    },
    /// SCALE-5: this `watch` fell more than the store's broadcast holds
    /// behind, and the channel dropped `missed` changes before it could
    /// read them (`missed` ≥ 1: the count the channel reports). Yielded
    /// only by `watch`, exactly once per gap, in place of the changes lost
    /// — whatever its `epic` filter, since the subscriber cannot know
    /// whether a lost change was in its epic — and then the stream goes on
    /// with the oldest change still held. Not an error and not an end:
    /// it means "what you hold may be stale — fetch again what you show".
    Lagged { missed: u64 },
    /// MNG-10: one shortcode's roll-up (yielded per shortcode by `groups`,
    /// in shortcode order).
    Group { group: Group },
    /// MNG-10: a shortcode's roll-up moved (yielded by `watch`) — a status
    /// change crossed `Complete`, a ticket was born into its subtree, or a
    /// `set_parent` reshaped it; AGS-23: or its root's row did — the root's
    /// status changed (any change, not only across `Complete`) or `amend`
    /// changed its title. Carries the whole group as it is now; it
    /// follows the ticket events of the same mutation, innermost group
    /// first. A group whose shortcode no longer draws a row of its own
    /// arrives once with `total: 0` (SDLC-11: and `docs: 0`).
    GroupChanged { group: Group },
    /// SCALE-16: where one shortcode ends in the store's order — its
    /// highest sequence (yielded per shortcode by `extents`, in shortcode
    /// order). Ids are allocated highest-plus-one and nothing deletes, so
    /// the shortcode holds `last_seq` tickets and its row k is `EPIC-k`;
    /// a database edited by hand may hold a hole, and then this is still
    /// the highest sequence — a page after the row before the hole starts
    /// at the next ticket, while `after` naming the missing id itself is
    /// `list`'s in-band error for a cursor that names no ticket. Not a
    /// roll-up: unlike `Group::total` it counts only the
    /// shortcode's own tickets, and every shortcode that holds one has an
    /// extent whether or not the nav draws it a row. No watch event keeps
    /// it: `created` carries the newborn's `epic` and `seq`, which is its
    /// shortcode's new `last_seq`.
    Extent { epic: String, last_seq: u32 },
    /// FILTER-4: how many tickets of one shortcode match a filter (yielded
    /// per shortcode by `counts`, in shortcode order, only for a shortcode
    /// that holds a match) — the filtered `extents`. A sum over the kept
    /// counts of the cells the filter admits, never a test of a ticket.
    Count { epic: String, held: u64 },
    /// FILTER-4: one menu count (yielded by `facets`): how many tickets
    /// match `base` and `filter` once `filter`'s own groups for `field` are
    /// set aside, among those whose `field` is `value`. `field` is spelled
    /// as the language spells it (`status`, `category`, `kind`, `severity`,
    /// `blocked`, `ready`, `has commits`, `in`); `value` is canonical — a
    /// status as `set_status` stores it, a kind, a severity or a category in
    /// lower case, `yes` / `no` for the three flags, a shortcode for `in`.
    Facet { field: String, value: String, held: u64 },
    /// FILTER-4: a write moved this ticket into a `watch { filter }`'s
    /// filter — born into it, or its status, its blockers' or its commits
    /// moved it there (yielded by `watch { filter }` only, after every other
    /// event of the write).
    Entered { id: String, epic: String, seq: u32 },
    /// FILTER-4: a write moved this ticket out of a `watch { filter }`'s
    /// filter (yielded by `watch { filter }` only, after every other event
    /// of the write).
    Left { id: String, epic: String, seq: u32 },
    /// FILTER-4: one write moved more than 256 tickets into or out of a
    /// `watch { filter }`'s filter: instead of those `entered` and `left`,
    /// one of these per shortcode it moved, with the filter's count of that
    /// shortcode as the write left it. A client sets its count and asks for
    /// its window again, as after `lagged`.
    Recount { epic: String, held: u64 },
    /// WAVE-4: one ticket's place in the order of work (yielded per ticket
    /// by `waves`): its row, its [`Placement`] — computed over the whole
    /// `blocked_by` graph, whatever shortcodes were asked for — and
    /// `waiting_on`, its unsatisfied direct blockers (ids that name no
    /// ticket included) in `blocked_by` order, distinct; empty for `Done`.
    Placed {
        summary: TicketSummary,
        placement: Placement,
        waiting_on: Vec<String>,
    },
    /// WAVE-4: the tally that closes a `waves` answer, exactly once, after
    /// every `placed`: `waves[i]` is how many of the placed tickets sit in
    /// wave `i + 1` (as long as the highest wave placed; `[]` when none is),
    /// `missing` how many are `missing_blocker`, `done` how many are done.
    WaveCounts {
        waves: Vec<u64>,
        missing: u64,
        done: u64,
    },
    /// MENGER-31: a `blocked_by` cycle among what a `waves_to` target waits
    /// on (yielded after the `placed`, before the `wave_counts`, one per
    /// cycle): its ids in the store's order. Every ticket on it — and the
    /// target behind it — is `missing_blocker`: no status change ends it.
    WaveCycle { ids: Vec<String> },
    /// WAVE-6: the kept placements one write moved (yielded by `watch`) —
    /// one event per write that moved any, after that write's other events,
    /// each change the ticket's id and its placement now: the ticket a
    /// status change crossed `Complete` / `Superseded` with, and every
    /// ticket downstream of it along `unlocks` whose placement changed; or,
    /// for a birth that resolved forward references, the tickets that were
    /// waiting on its id and theirs (the newborn's own is on its `created`).
    /// `watch { epic }` keeps only the changes in that shortcode, and the
    /// event only when one is left.
    PlacementsChanged { changes: Vec<PlacementChange> },
    /// COMP-3: one entry of the component registry (yielded per component by
    /// `components`, in name order) — orphans included (`owner: null`).
    Component { component: Component },
    /// COMP-3: a component was registered, or its source, owner or
    /// references moved (yielded by `watch`) — a `create` or `amend` whose
    /// body gained, changed or lost a ` ```component ` block or a `<name`
    /// tag. Carries the whole component as it is now, so applying it is one
    /// upsert; one per component the write moved, in name order, after that
    /// write's other events. A component belongs to no shortcode — any
    /// ticket may use it — so `watch { epic }` passes it on whatever the
    /// filter.
    ComponentChanged { component: Component },
    /// COMP-3: a component left the registry (yielded by `watch`) — an
    /// orphan whose last reference went, in the write that took it. After
    /// the write's `component_changed` events; past any `epic` filter, as
    /// they are.
    ComponentRemoved { name: String },
    /// INPUT-3: one input, as it is now (yielded per input by `inputs`, in
    /// the store's order of tickets and the body's order inside one — a
    /// removed input after the ones its body declares; and by a
    /// `clear_answer`, `close_input` or `reopen_input` that changed nothing).
    Input { input: Input },
    /// INPUT-3: an input changed (yielded by `answer`, `clear_answer`,
    /// `close_input` and `reopen_input`, and by `watch`) — answered, cleared,
    /// closed, reopened; or, by a `create` or `amend` of its ticket's body,
    /// declared, re-declared (its kind, question, options or recommendation),
    /// removed from the body, or declared again. Carries the whole input —
    /// its state, its answer and its history — so applying it is one upsert;
    /// after every other event of its write. `watch { epic }` filters it by
    /// its ticket's shortcode.
    InputChanged { input: Input },
    /// One resolved blocker of the queried ticket, at its current status
    /// (yielded per-entry by `blockers`).
    Blocker { ticket: Ticket },
    /// A blocker id that names no existing ticket — a dangling forward
    /// reference (yielded per-entry by `blockers`).
    DanglingBlocker { id: String },
    /// FILTER-15: whether a filter admits one ticket (yielded by `member`,
    /// one per id asked, in the order asked): `id` as it was asked,
    /// `matched` whether the filter admits the ticket it names — `false`
    /// for an id that names no ticket. One lookup in the filter's admitted
    /// cells, never a test of the ticket.
    Member { id: String, matched: bool },
    /// Lookup miss.
    NotFound { id: String },
    /// In-band error (bad enum value, invalid epic, blocked_by cycle, …).
    /// A *data* event: the call succeeded and the activation said no,
    /// which is not the same thing as a transport or remote failure.
    Error { message: String },
    /// MENGER-15: a `create` or `amend` refused for its body's size — the
    /// body was `bytes` bytes, past the server's cap, `max`
    /// (`--max-body-bytes`, 8 MiB by default); `id` the amended ticket
    /// (`None` for a `create`, which allocated no id). Typed, so a client can
    /// say so without reading `message`; `message` says it in words. Nothing
    /// was written.
    BodyTooLarge { id: Option<String>, bytes: u64, max: u64, message: String },
    /// FILTER-13: one row of `tree` — a parent's child, or a shortcode's top
    /// row, that matches the filter or holds a match below it: `matched`
    /// whether it matches itself, `below` how many matches are strictly
    /// under it (its whole subtree, whatever the nav shows). A row that only
    /// holds matches has `matched: false` and `below` above 0. Yielded in
    /// the store's order, then one `page`.
    Node {
        summary: TicketSummary,
        matched: bool,
        below: u64,
    },
    /// SEARCH-2: one ticket a `search` matched, best first — its row; its
    /// title and a passage of its body's text (at most 24 words, around the
    /// matches), each as stretches with the matched words marked; and
    /// `corrected` when it matched only through a word's correction (those
    /// follow every ticket matched without one).
    Hit {
        summary: TicketSummary,
        title: Vec<Marked>,
        snippet: Vec<Marked>,
        corrected: bool,
    },
    /// SEARCH-2: the last data event of a `search` — `matched`, how many
    /// tickets match (under its filter); `next`, the `offset` of the next
    /// page, or null when this page holds the last match; `words`, the
    /// query's words as they were read, with their corrections.
    Searched {
        matched: u64,
        next: Option<u32>,
        words: Vec<SearchWord>,
    },
    /// MENGER-13: one transition (yielded per transition by `history`: the
    /// ticket's own, oldest first, then those they caused in other tickets;
    /// and by `watch`, right after the event of the write that made it — a
    /// `created`, `status_changed` or `kind_changed` — so a cascade is its
    /// cause's events, then each canceled ticket's `status_changed` and
    /// `transition`, in the store's order).
    Transition { transition: Transition },
    /// MENGER-13: the last event of `history` — when the log began
    /// (`began_unix`: nothing before it has a history; `null` for a log no
    /// file began), and, for a canceled ticket, `restorable`: the tickets its
    /// latest cancel canceled and nothing has moved since, which `set_status
    /// { restore_children: true }` would put back, in the store's order
    /// (empty otherwise). FILTER-27: and since when the ticket has held its
    /// current status (`status_since_unix`) — its latest status transition,
    /// `status_since_exact: true`; or, for a ticket the log says nothing of
    /// (from before it began), a bound — "status since at least …": its last
    /// update or the log's beginning, whichever is earlier — `false`.
    HistoryEnd {
        began_unix: Option<i64>,
        restorable: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status_since_unix: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status_since_exact: Option<bool>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// COMP-9 acceptance 1: an instance's attribute checked against each
    /// `AttrKind` — the form, and existence as the caller knows it.
    #[test]
    fn attribute_values_are_checked_per_kind() {
        struct Board;
        impl Known for Board {
            fn shortcode(&self, epic: &str) -> Option<bool> {
                Some(matches!(epic, "RETURNS" | "MNG"))
            }
            fn ticket(&self, id: &str) -> Option<bool> {
                Some(matches!(id, "MNG-2" | "RETURNS-6"))
            }
        }
        let fits = |k: &AttrKind, v: &str| k.check(v, &Board).is_ok();
        let why = |k: &AttrKind, v: &str| k.check(v, &Board).unwrap_err();
        assert!(fits(&AttrKind::String, "") && fits(&AttrKind::String, "anything <at> all"));
        for n in ["0", "-3", "2.5", " 7 ", "1e3"] {
            assert!(fits(&AttrKind::Number, n), "{n}");
        }
        for n in ["", "seven", "NaN", "inf", "1,5"] {
            assert!(why(&AttrKind::Number, n).contains("is not a number"), "{n}");
        }
        for b in ["", "true", "FALSE", "False"] {
            assert!(fits(&AttrKind::Boolean, b), "{b}");
        }
        assert!(why(&AttrKind::Boolean, "yes").contains("not a boolean"));
        assert!(fits(&AttrKind::Shortcode, "RETURNS") && fits(&AttrKind::Shortcode, "mng"));
        assert_eq!(why(&AttrKind::Shortcode, "NOPE"), "no shortcode 'NOPE'");
        assert!(why(&AttrKind::Shortcode, "RE TURNS").contains("is not a shortcode"));
        assert!(why(&AttrKind::Shortcode, "").contains("is not a shortcode"));
        assert!(fits(&AttrKind::Ticket, "MNG-2") && fits(&AttrKind::Ticket, "returns-6"));
        assert_eq!(why(&AttrKind::Ticket, "MNG-99"), "no ticket 'MNG-99'");
        for bad in ["MNG", "MNG-", "-2", "MNG-x", "M N-2"] {
            assert!(why(&AttrKind::Ticket, bad).contains("is not a ticket id"), "{bad}");
        }
        let size = AttrKind::Enum { values: vec!["s".into(), "m".into(), "l".into()] };
        assert!(fits(&size, "m") && fits(&size, " l "));
        assert_eq!(why(&size, "xl"), "'xl' is not one of s | m | l");
        assert!(why(&size, "M").contains("not one of"), "enum values are exact");
        // Knowing nothing, existence is not checked — only the form.
        assert!(AttrKind::Shortcode.check("NOPE", &KnowNothing).is_ok());
        assert!(AttrKind::Ticket.check("NOPE-1", &KnowNothing).is_ok());
        assert!(AttrKind::Ticket.check("NOPE", &KnowNothing).is_err());
    }

    /// COMP-9: an attribute on an instance — set and fitting, unset,
    /// required and missing, unfit; a boolean set to its own name is true.
    #[test]
    fn an_instances_attribute_state() {
        let attr = |name: &str, kind: AttrKind, required: bool| AttrDoc { name: name.into(), kind, required, description: String::new() };
        let board = attr("board", AttrKind::Shortcode, true);
        let label = attr("label", AttrKind::String, false);
        let compact = attr("compact", AttrKind::Boolean, false);
        assert_eq!(board.state(None, &KnowNothing), AttrState::Missing);
        assert_eq!(label.state(None, &KnowNothing), AttrState::Unset);
        assert_eq!(board.state(Some("RETURNS"), &KnowNothing), AttrState::Fits);
        assert_eq!(board.state(Some("no way"), &KnowNothing), AttrState::Unfit("'no way' is not a shortcode (letters and digits, e.g. RETURNS)".into()));
        assert_eq!(compact.state(Some("compact"), &KnowNothing), AttrState::Fits);
        assert_eq!(compact.state(Some(""), &KnowNothing), AttrState::Fits);
        assert!(matches!(compact.state(Some("very"), &KnowNothing), AttrState::Unfit(_)));
    }

    /// MFE-9: the browser app renders a status by its wire spelling and
    /// sends that spelling straight back to `set_status`. If `as_str` and
    /// serde ever disagreed the UI would show one word and change the
    /// ticket to another — so they are pinned to each other here, for
    /// every member of the closed vocabulary.
    #[test]
    fn every_status_renders_as_its_wire_spelling() {
        for s in Status::ALL {
            assert_eq!(
                serde_json::to_value(s).unwrap(),
                serde_json::Value::String(s.as_str().to_string()),
                "{s:?}"
            );
            assert_eq!(s.as_str().parse::<Status>().unwrap(), s);
        }
    }

    /// SDLC-7 criterion 5: all nine round-trip — through serde both ways
    /// (`In Review` with its space, byte-exact), through `FromStr` from the
    /// wire spelling and from its lowercase — and the worker states parse
    /// from every spelling a worker is likely to type, to the one variant
    /// that writes them canonically. The refusal names all ten. SDLC-15:
    /// `Canceled` reads the British `cancelled` too, and writes one spelling.
    #[test]
    fn the_ten_statuses_round_trip_and_the_worker_states_parse_leniently() {
        assert_eq!(Status::LIFECYCLE.len(), 10);
        for s in Status::LIFECYCLE {
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(json, format!("\"{}\"", s.as_str()), "{s:?}");
            assert_eq!(serde_json::from_str::<Status>(&json).unwrap(), s, "{json}");
            assert_eq!(s.as_str().to_lowercase().parse::<Status>().unwrap(), s, "{s:?}");
            assert_eq!(s.to_string(), s.as_str());
        }
        assert_eq!(serde_json::to_string(&Status::InReview).unwrap(), "\"In Review\"");
        for spelling in ["Coding", "coding", "CODING", "active", "Active"] {
            assert_eq!(spelling.parse::<Status>(), Ok(Status::Coding), "{spelling}");
        }
        for spelling in [
            "In Review", "in review", "In review", "IN REVIEW", "in_review", "IN_REVIEW",
            "inreview", "InReview", "in-review",
        ] {
            assert_eq!(spelling.parse::<Status>(), Ok(Status::InReview), "{spelling}");
        }
        for spelling in ["Canceled", "canceled", "CANCELED", "cancelled", "Cancelled"] {
            assert_eq!(spelling.parse::<Status>(), Ok(Status::Canceled), "{spelling}");
        }
        // serde is the strict reader: only the canonical spelling.
        assert!(serde_json::from_str::<Status>("\"InReview\"").is_err());
        assert!(serde_json::from_str::<Status>("\"Cancelled\"").is_err());

        let err = "review".parse::<Status>().unwrap_err();
        assert!(err.contains("'review'"), "{err}");
        for s in Status::ALL {
            assert!(err.contains(s.as_str()), "names {s}: {err}");
        }
    }

    /// SDLC-11: `Doc` is a status a ticket holds — every status, `Doc`
    /// included, round-trips through serde and `FromStr` — and is not one a
    /// ticket moves to: `ALL` is the lifecycle and then `Doc`, and the
    /// lifecycle (what a chooser offers) does not hold it. A doc is born in
    /// it and every other kind in `Pending`.
    #[test]
    fn doc_is_a_status_outside_the_lifecycle_and_a_docs_birth_status() {
        assert_eq!(Status::ALL.len(), 11);
        assert_eq!(Status::ALL[..10], Status::LIFECYCLE, "the lifecycle, then Doc");
        assert_eq!(Status::ALL[10], Status::Doc);
        assert!(!Status::LIFECYCLE.contains(&Status::Doc), "no chooser offers Doc");
        for s in Status::ALL {
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(serde_json::from_str::<Status>(&json).unwrap(), s, "{json}");
            assert_eq!(s.as_str().parse::<Status>(), Ok(s), "{s:?}");
        }
        assert_eq!(serde_json::to_string(&Status::Doc).unwrap(), "\"Doc\"");
        for spelling in ["Doc", "doc", "DOC"] {
            assert_eq!(spelling.parse::<Status>(), Ok(Status::Doc), "{spelling}");
        }
        for k in Kind::ALL {
            let want = if k == Kind::Doc { Status::Doc } else { Status::Pending };
            assert_eq!(Status::at_birth(k), want, "{k:?}");
            assert_eq!(k.has_lifecycle(), k != Kind::Doc, "{k:?}");
            assert_eq!(k.roots_a_shortcode(), matches!(k, Kind::Scope | Kind::Doc), "{k:?}");
        }
    }

    /// SDLC-11: the sixth kind, `doc`, on the wire as `doc`; the error for
    /// an unknown kind names all six.
    #[test]
    fn doc_is_the_sixth_kind_and_the_unknown_kind_error_names_it() {
        assert_eq!(serde_json::to_string(&Kind::Doc).unwrap(), "\"doc\"");
        assert_eq!(serde_json::from_str::<Kind>("\"doc\"").unwrap(), Kind::Doc);
        assert_eq!("Doc".parse::<Kind>(), Ok(Kind::Doc));
        let err = "memo".parse::<Kind>().unwrap_err();
        for k in Kind::ALL {
            assert!(err.contains(k.as_str()), "names {k}: {err}");
        }
    }

    /// SDLC-11: a group is drawn when it holds anything — work or docs — so
    /// a doc collection is a row; `total: 0, docs: 0` is a row no longer
    /// drawn.
    #[test]
    fn a_group_is_drawn_when_it_holds_work_or_docs() {
        let g = |total, docs| Group { epic: "G".into(), total, done: 0, docs, root: None, root_row: None, host: None };
        assert!(g(1, 0).drawn());
        assert!(g(0, 3).drawn(), "a doc collection");
        assert!(!g(0, 0).drawn());
    }

    /// WAVE-4: `Placement` and the two `waves` events on the wire — tagged
    /// like every event, snake_case, and back again.
    #[test]
    fn placements_and_the_waves_events_render_tagged_and_snake_cased() {
        use serde_json::json;
        for (p, want) in [
            (Placement::Wave { n: 3 }, json!({"type": "wave", "n": 3})),
            (Placement::Done, json!({"type": "done"})),
            (Placement::MissingBlocker, json!({"type": "missing_blocker"})),
        ] {
            assert_eq!(serde_json::to_value(p).unwrap(), want, "{p:?}");
            assert_eq!(serde_json::from_value::<Placement>(want).unwrap(), p);
        }
        let counts = TicketsEvent::WaveCounts { waves: vec![2, 0, 1], missing: 1, done: 4 };
        assert_eq!(
            serde_json::to_value(&counts).unwrap(),
            json!({"type": "wave_counts", "waves": [2, 0, 1], "missing": 1, "done": 4})
        );
        let summary = TicketSummary {
            labels: Vec::new(),
            id: "W-2".into(),
            epic: "W".into(),
            seq: 2,
            title: "t".into(),
            status: Status::Pending,
            kind: Kind::Build,
            severity: None,
            parent: Some("W-1".into()),
            child_count: 0,
            blocked_by: vec!["W-3".into()],
            unlocks: vec![],
            commits: vec![],
            path: None,
            placement: Some(Placement::Wave { n: 2 }),
            created_unix: 1,
            updated_unix: 1,
        };
        let placed = TicketsEvent::Placed {
            summary,
            placement: Placement::Wave { n: 2 },
            waiting_on: vec!["W-3".into()],
        };
        let wire = serde_json::to_value(&placed).unwrap();
        assert_eq!(wire["type"], "placed");
        assert_eq!(wire["summary"]["id"], "W-2");
        assert_eq!(wire["placement"], json!({"type": "wave", "n": 2}));
        assert_eq!(wire["waiting_on"], json!(["W-3"]));
        match serde_json::from_value::<TicketsEvent>(wire).unwrap() {
            TicketsEvent::Placed { placement, waiting_on, .. } => {
                assert_eq!((placement, waiting_on), (Placement::Wave { n: 2 }, vec!["W-3".to_string()]));
            }
            other => panic!("expected Placed, got {other:?}"),
        }
    }

    #[test]
    fn kind_and_severity_render_as_their_wire_spellings() {
        for k in Kind::ALL {
            assert_eq!(
                serde_json::to_value(k).unwrap(),
                serde_json::Value::String(k.as_str().to_string()),
                "{k:?}"
            );
            assert_eq!(k.as_str().parse::<Kind>(), Ok(k), "{k:?}");
        }
        for s in [
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
        ] {
            assert_eq!(
                serde_json::to_value(s).unwrap(),
                serde_json::Value::String(s.as_str().to_string()),
                "{s:?}"
            );
        }
    }

    /// MNG-9: "every field of `Ticket` except `body`" is the contract
    /// MFE-9's rows are typed against, and `From<Ticket>` is written out
    /// field by field — so a field added to `Ticket` would silently not
    /// reach a summary row. This is the guard: the two shapes differ by
    /// exactly `body`.
    #[test]
    fn a_summary_is_a_ticket_minus_exactly_the_body() {
        // Every optional field set, so `Ticket`'s skip_serializing_if
        // does not hide one from the comparison.
        let ticket = Ticket {
            id: "SUM-1".into(),
            epic: "SUM".into(),
            seq: 1,
            title: "every field populated".into(),
            status: Status::Ready,
            kind: Kind::Build,
            severity: Some(Severity::High),
            blocked_by: vec!["SUM-2".into()],
            unlocks: vec!["SUM-3".into()],
            parent: Some("SUM-4".into()),
            child_count: 1,
            commits: vec!["77df9a0".into()],
            path: Some("_MENGER/board".into()),
            placement: Some(Placement::MissingBlocker),
            labels: vec!["needs-you".into()],
            body: "## Problem\n\nthe weight of a list".into(),
            created_unix: 1,
            updated_unix: 2,
        };
        let summary = TicketSummary::from(ticket.clone());

        let keys = |v: &serde_json::Value| -> Vec<String> {
            let mut k: Vec<String> = v.as_object().expect("object").keys().cloned().collect();
            k.sort();
            k
        };
        let full = serde_json::to_value(&ticket).unwrap();
        let slim = serde_json::to_value(&summary).unwrap();
        let mut expected = keys(&full);
        expected.retain(|k| k != "body");
        assert_eq!(keys(&slim), expected, "a summary is a ticket minus the body");

        // …and every shared field carries the same value.
        for k in keys(&slim) {
            assert_eq!(slim[&k], full[&k], "field {k}");
        }
    }
}
