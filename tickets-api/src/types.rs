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
    /// SDLC-11: a doc's, and only a doc's — set at its birth, never changed.
    Doc,
}

impl Status {
    /// Every status a ticket can hold — the lifecycle's nine, then a doc's
    /// one (SDLC-11). What a status count keeps a bucket for, what `list`
    /// filters on, and what the browser app's status table paints.
    pub const ALL: [Status; 10] = [
        Status::Pending,
        Status::Ready,
        Status::Coding,
        Status::InReview,
        Status::Blocked,
        Status::Complete,
        Status::Idea,
        Status::Epic,
        Status::Superseded,
        Status::Doc,
    ];

    /// The lifecycle: the statuses a ticket that has one moves between, in
    /// the order a chooser should offer them. The browser app's status
    /// control and its menu render exactly this — so a status added here
    /// appears in the UI without a second edit, and one removed cannot
    /// linger in it. SDLC-7: the worker states sit where a ticket passes
    /// through them, between `Ready` and `Complete`. SDLC-11: `Doc` is not
    /// here — no ticket moves into it or out of it, so no chooser offers it.
    pub const LIFECYCLE: [Status; 9] = [
        Status::Pending,
        Status::Ready,
        Status::Coding,
        Status::InReview,
        Status::Blocked,
        Status::Complete,
        Status::Idea,
        Status::Epic,
        Status::Superseded,
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
            "doc" => Ok(Self::Doc),
            other => Err(format!(
                "unknown status '{other}' (expected Pending, Ready, Coding, In Review, Blocked, \
                 Complete, Idea, Epic, or Superseded — or Doc, a doc's own)"
            )),
        }
    }
}

/// What shape of work a ticket is (AGS-13): the owner's closed five-value
/// set, one vocabulary owning the slot — and, since SDLC-11 (owner,
/// 2026-10-02), a sixth that is not work at all: `doc`. Serialized
/// lowercase (`kind: build`) on the wire and in the store.
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
            created_unix: t.created_unix,
            updated_unix: t.updated_unix,
        }
    }
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
/// kept current the way the counts are: a change to the root's title or
/// status is a change to its group, and `group_changed` carries it (a kind
/// never changes). `None` exactly when `root` is.
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
/// MNG-4's rule (a blocker is satisfied iff it is a `Complete` or
/// `Superseded` ticket; an id naming no ticket never is).
///
/// - `Wave { n }` — unfinished, and `n` is 1 when every blocker is
///   satisfied, else 1 + the highest wave among its unsatisfied blockers.
/// - `Done` — the ticket itself is `Complete` or `Superseded`.
/// - `MissingBlocker` — it, or something it waits on, names a blocker that
///   is no ticket: no wave can be given until that id exists.
///
/// Internally tagged like the events (`{"type": "wave", "n": 2}`,
/// `{"type": "done"}`, `{"type": "missing_blocker"}`). Nothing stores it:
/// the store computes it on demand, per call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Placement {
    Wave { n: u32 },
    Done,
    MissingBlocker,
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
    Page {
        next: Option<String>,
        remaining: u64,
    },
    /// A ticket's status changed (yielded by `set_status`, and by `watch`).
    StatusChanged { ticket: Ticket, previous: Status },
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
    /// An existing ticket's `unlocks` gained `added`: a newly created
    /// ticket named it in `blocked_by` (yielded by `watch`). Carries the
    /// full updated ticket so a watch-only client stays coherent.
    UnlocksChanged { ticket: Ticket, added: String },
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
    /// One resolved blocker of the queried ticket, at its current status
    /// (yielded per-entry by `blockers`).
    Blocker { ticket: Ticket },
    /// A blocker id that names no existing ticket — a dangling forward
    /// reference (yielded per-entry by `blockers`).
    DanglingBlocker { id: String },
    /// Lookup miss.
    NotFound { id: String },
    /// In-band error (bad enum value, invalid epic, blocked_by cycle, …).
    /// A *data* event: the call succeeded and the activation said no,
    /// which is not the same thing as a transport or remote failure.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

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
    /// that writes them canonically. The refusal names all nine.
    #[test]
    fn the_nine_statuses_round_trip_and_the_worker_states_parse_leniently() {
        assert_eq!(Status::LIFECYCLE.len(), 9);
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
        // serde is the strict reader: only the canonical spelling.
        assert!(serde_json::from_str::<Status>("\"InReview\"").is_err());

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
        assert_eq!(Status::ALL.len(), 10);
        assert_eq!(Status::ALL[..9], Status::LIFECYCLE, "the lifecycle, then Doc");
        assert_eq!(Status::ALL[9], Status::Doc);
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
