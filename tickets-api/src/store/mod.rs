//! The service-wide ticket cache, with the database behind it.
//!
//! This layer is the primary interface through which the whole
//! application touches tickets — activations, DAG queries, and the shell
//! all go through here, never to storage directly. An embedded SQLite
//! database (the system of record) sits behind the cache: every ticket
//! loads into memory at startup, reads are served from the cache only
//! (async-safe: no lock held across an await, no I/O on the read path),
//! and writes go through write-through — durable in the database before
//! the cache is updated, the event emitted, and `watch` subscribers
//! notified. Nothing outside this module holds a database handle.
//! The status vocabulary matches the house ticketing convention; the kind
//! vocabulary is the owner's closed five (AGS-13), and a database written
//! before AGS-13 is migrated to it, durably, the first time it is opened.
//! AGS-19 adds parenthood: a ticket may name a `parent` (the ticket it is
//! an output of), `children` is the mirror the store maintains, and a
//! database written before AGS-19 gains both columns — with typed parents
//! backfilled from body frontmatter — durably, once, on first open.
//! MNG-9 names the store's total order — [`OrderKey`], `(epic, seq)` —
//! and holds the cache in it: the tickets live in a sorted map, and a
//! hash map says where an id sits in it. So the order costs nothing to
//! walk, an id is a cursor, and a page (`after` + `limit`) is a range
//! scan rather than a sort of everything.
//! SCALE-4 takes AGS-19's `children` off the row: a ticket carries
//! `child_count`, and its children are an index the cache keeps from each
//! ticket's `parent` ([`children::Children`]) — so a birth under a root of
//! 163,530 rewrites no list, stores none and broadcasts none
//! (`ChildrenChanged` is the parent's id and new count), and a page of
//! `list { parent }` costs the page. The parent's stored row is not
//! written by its child's birth or move at all. A database written before
//! SCALE-4 loses its `children` column, durably, once, on first open; the
//! `parent` column was always the edge, and the dropped lists are checked
//! against it on the way out.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use tokio::sync::broadcast;
use tokio::sync::Mutex as AsyncMutex;

/// MFE-9: the ticket vocabulary itself now lives in `menger-tickets-api`
/// so the browser app can consume the same declarations instead of a
/// hand-written mirror of the JSON they arrive as. Re-exported here
/// unchanged — every `crate::store::Status` in the server still resolves,
/// and the store remains the only thing that *writes* them.
pub use crate::types::{Group, Kind, Severity, Status, Ticket};

/// MNG-9: the store's total order over tickets — `(epic, seq)`, which the
/// schema keeps unique (`UNIQUE (epic, seq)`). Every list has been in this
/// order since MNG-2; naming it makes it a *cursor* as well: because it is
/// total and unique, a ticket id names one position in it, so "the next
/// `limit` tickets after this id" needs no state beyond the id itself.
/// Derived `Ord` is epic-then-seq, byte-lexicographic on the epic — the
/// same comparison `list` sorted with before the index existed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderKey {
    pub epic: String,
    pub seq: u32,
}

impl OrderKey {
    fn of(ticket: &Ticket) -> Self {
        Self {
            epic: ticket.epic.clone(),
            seq: ticket.seq,
        }
    }
}

/// MNG-9: what one page of the store's order is cut with. `after` is the
/// exclusive cursor (a ticket id); `limit` caps the page; the three
/// filters are the ones `list` has always had, and they compose with the
/// cursor — `remaining` counts under the same filters.
#[derive(Debug, Clone, Copy, Default)]
pub struct PageQuery<'a> {
    pub epic: Option<&'a str>,
    pub status: Option<Status>,
    pub parent: Option<&'a str>,
    /// The id the page starts strictly after. `None` starts at the top.
    pub after: Option<&'a str>,
    /// At most this many tickets. `None` is "every match" — what `list`
    /// answered before MNG-9, and still does.
    pub limit: Option<u32>,
}

/// MNG-9: what a page walk keeps. `All` is not "a predicate that always
/// says yes": it is the evidence that there is nothing left to test —
/// the filters that *can* be answered by position (the epic, the cursor)
/// having already been spent on the [`Span`] — so counting what remains
/// costs the walk and not a test per ticket. SCALE-19: and then not the
/// walk either — with nothing to test, what remains is the span's length
/// less the page, which the cache knows per shortcode ([`Span::len`]).
enum Filter<'a> {
    All,
    By(&'a dyn Fn(&Ticket) -> bool),
}

/// MNG-9: the stretch of the sorted index a page is cut from — the two
/// things that are *positions* rather than predicates.
///
/// `after` is a lower bound, exclusive. An `epic` is one too: because the
/// order is `(epic, seq)`, an epic's tickets are contiguous, so filtering
/// by epic is bounding the walk rather than testing every ticket in the
/// store — which is what keeps a per-epic page O(the epic) instead of
/// O(everything).
struct Span {
    start: std::ops::Bound<OrderKey>,
    end: std::ops::Bound<OrderKey>,
    /// The cursor this span was cut with, kept so an empty page can still
    /// say where to resume.
    cursor: Option<String>,
    /// The cursor sits at or past the end of the span: nothing can match.
    empty: bool,
}

impl Span {
    fn of(cache: &Cache, after: Option<&str>, epic: Option<&str>) -> Result<Self, StoreError> {
        use std::ops::Bound::{Excluded, Included, Unbounded};
        let (mut start, end) = match epic {
            None => (Unbounded, Unbounded),
            Some(e) => (
                Included(OrderKey {
                    epic: e.to_string(),
                    seq: 0,
                }),
                Included(OrderKey {
                    epic: e.to_string(),
                    seq: u32::MAX,
                }),
            ),
        };
        let mut empty = false;
        if let Some(id) = after {
            let key = cache
                .at
                .get(id)
                .cloned()
                .ok_or_else(|| StoreError::CursorNotFound(id.to_string()))?;
            match (&start, &end) {
                // The cursor is at or past the epic's last ticket.
                (_, Included(last)) if key >= *last => empty = true,
                // Below the epic's first: the epic's bound already covers it.
                (Included(first), _) if key < *first => {}
                _ => start = Excluded(key),
            }
        }
        Ok(Self {
            start,
            end,
            cursor: after.map(str::to_string),
            empty,
        })
    }

    /// The tickets in this span, in order, or `None` when there are none.
    fn range<'c>(
        &self,
        cache: &'c Cache,
    ) -> Option<std::collections::btree_map::Range<'c, OrderKey, Ticket>> {
        if self.empty {
            return None;
        }
        Some(cache.order.range((self.start.clone(), self.end.clone())))
    }

    /// SCALE-19: how many tickets this span holds, from what the cache
    /// keeps about each shortcode ([`ids::Ids`]) rather than by walking
    /// them — so an unfiltered page costs the page, not what is left
    /// after it.
    ///
    /// A span is at most one cut shortcode and then whole ones: the
    /// cursor's shortcode from the cursor on, and every shortcode after
    /// it (or, for an epic, that one shortcode). A whole shortcode is its
    /// kept count. The cut one is `last − seq` when its sequences are
    /// dense — every one the server made — and is counted, by walking
    /// the rest of that shortcode, when a hand edit left a hole in it.
    /// The value is the walk's either way.
    fn len(&self, cache: &Cache) -> u64 {
        use std::ops::Bound::{Excluded, Included, Unbounded};
        if self.empty {
            return 0;
        }
        let rest_of_run = |key: &OrderKey| {
            cache.ids.after_in_run(key).unwrap_or_else(|| {
                let end = OrderKey {
                    epic: key.epic.clone(),
                    seq: u32::MAX,
                };
                cache.order.range((Excluded(key), Included(&end))).count() as u64
            })
        };
        match (&self.start, &self.end) {
            // The whole store.
            (Unbounded, Unbounded) => cache.len() as u64,
            // After a cursor, to the end of the store.
            (Excluded(key), Unbounded) => rest_of_run(key) + cache.ids.held_after(&key.epic),
            // One epic, whole — the cursor below it, or none.
            (Included(first), Included(_)) => cache.ids.held(&first.epic),
            // One epic, after a cursor inside it.
            (Excluded(key), Included(_)) => rest_of_run(key),
            // No other shape is cut (`Span::of`); were one, the walk is
            // still the answer.
            _ => self.range(cache).map_or(0, |range| range.count() as u64),
        }
    }
}

/// MNG-9: one page of the store's order, and where the walk stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// The page itself, in `(epic, seq)` order.
    pub tickets: Vec<Ticket>,
    /// The cursor for the next page — the last id on this page, or the
    /// cursor this page was cut with when the page came back empty.
    /// `None` means the walk is over: nothing matches beyond this page.
    pub next: Option<String>,
    /// How many further tickets match the same filters beyond this page.
    pub remaining: u64,
}

/// A mutation, broadcast to every `watch` subscriber. (Internal — the
/// wire form is `TicketsEvent`, same `type` tag; `kind` is a ticket's.)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Change {
    Created { ticket: Ticket },
    StatusChanged { ticket: Ticket, previous: Status },
    /// MNG-16: a ticket's title and/or body was replaced (`amend`);
    /// `changed` names which, in that order. Carries the full ticket — body
    /// included, so an open page can show the new one without a `get`.
    /// Only broadcast when something changed: a no-op amend writes and
    /// emits nothing.
    Amended { ticket: Ticket, changed: Vec<String> },
    /// An existing ticket's `unlocks` gained `added` because a newly
    /// created ticket named it in `blocked_by`. Carries the full updated
    /// ticket so watch-only clients stay coherent.
    UnlocksChanged { ticket: Ticket, added: String },
    /// AGS-19: the ticket `id` gained the child `added` — born naming it
    /// as `parent`, or moved under it. SCALE-4: the parent's id and new
    /// `child_count`, not the full parent (whose list of children made the
    /// event as large as the shortcode). `epic` is the parent's shortcode,
    /// what `watch { epic }` filters on; it does not cross the wire.
    ChildrenChanged {
        id: String,
        epic: String,
        added: String,
        child_count: u64,
    },
    /// MNG-12: a ticket's `parent` was set after birth (`set_parent`);
    /// `previous` is the parent it had, if any. Carries the full child.
    ParentChanged {
        ticket: Ticket,
        previous: Option<String>,
    },
    /// MNG-12: the ticket `id` lost the child `removed` — that child was
    /// re-parented elsewhere. SCALE-4: the ex-parent's id and new count,
    /// as with `ChildrenChanged`.
    ChildRemoved {
        id: String,
        epic: String,
        removed: String,
        child_count: u64,
    },
    /// MNG-10: a shortcode's roll-up moved. Broadcast after the ticket
    /// changes of the same mutation — the counts are derived from them, so
    /// a client applying events in order meets the cause before the
    /// effect — one per group whose row changed, innermost first.
    GroupChanged { group: Group },
}

impl Change {
    /// The shortcode a change belongs to — what `watch { epic }` filters
    /// on. A ticket change belongs to its ticket's epic; a roll-up change
    /// to the group's own shortcode (MNG-10), so a watcher of `PLEXUS`
    /// hears PLEXUS's count move when a sub-shortcode's ticket completes,
    /// without hearing the ticket itself.
    pub fn epic(&self) -> &str {
        match self {
            Self::Created { ticket }
            | Self::StatusChanged { ticket, .. }
            | Self::Amended { ticket, .. }
            | Self::UnlocksChanged { ticket, .. }
            | Self::ParentChanged { ticket, .. } => &ticket.epic,
            Self::ChildrenChanged { epic, .. } | Self::ChildRemoved { epic, .. } => epic,
            Self::GroupChanged { group } => &group.epic,
        }
    }
}

/// One entry of a ticket's `blocked_by`, resolved against current state.
#[derive(Debug, Clone)]
pub enum BlockerRef {
    /// The blocker exists; carries it at its current status.
    Resolved(Ticket),
    /// The id names no existing ticket (a dangling forward reference).
    Dangling(String),
}

pub struct NewTicket {
    pub epic: String,
    pub title: String,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub blocked_by: Vec<String>,
    /// AGS-19: the ticket this one is an output of. Must already exist
    /// (any epic) and must not make the newborn its own ancestor.
    pub parent: Option<String>,
    pub body: String,
}

/// PERF-2: a request to bulk-seed synthetic tickets through the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeedSpec {
    /// How many tickets to generate.
    pub count: usize,
    /// The RNG seed: the same value generates the same tickets, so a
    /// magnitude is reproducible run to run.
    pub seed: u64,
    /// Seed even though the store already holds tickets. Off by default:
    /// seeding is for fresh (measurement) databases, and adding a million
    /// synthetic rows to someone's real one must be an explicit choice.
    /// When on, ids continue after the existing per-epic maxima exactly
    /// as `create` would allocate them.
    pub into_existing: bool,
}

/// What a seed run did, for the startup log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeedReport {
    pub count: usize,
    pub epics: usize,
    /// Total `blocked_by` entries (each mirrored by one `unlocks` entry).
    pub edges: usize,
    pub generate: std::time::Duration,
    pub persist: std::time::Duration,
    pub elapsed: std::time::Duration,
}

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    InvalidEpic(String),
    /// Creating `id` with the requested `blocked_by` would close a cycle;
    /// `path` is the blocked_by chain from `id` back to `id`.
    Cycle { id: String, path: Vec<String> },
    /// AGS-19: the requested `parent` names no existing ticket. Nothing
    /// was stored and the id stays unallocated.
    ParentNotFound { id: String, parent: String },
    /// AGS-19: the requested parent chain would make `id` its own
    /// ancestor; `path` is the chain from the parent back to `id`.
    ParentCycle { id: String, path: Vec<String> },
    /// MNG-12: every shortcode starts with a scope. The first ticket of a
    /// prefix that has none yet must be `kind: scope`; `kind` is what was
    /// asked for instead. Nothing was stored and no id was allocated.
    FirstMustBeScope { epic: String, kind: Kind },
    /// MNG-14: a ticket's parent must be inside its own shortcode; only a
    /// shortcode's scope root may hang under another shortcode (its host).
    /// Nothing changed.
    ParentOutsideShortcode { id: String, parent: String },
    /// MNG-16: an `amend` asked for a title that is empty after trimming.
    /// A ticket always has one; nothing was written.
    EmptyTitle { id: String },
    /// The write could not be made durable; nothing was acknowledged.
    Db(String),
    /// PERF-2: a seed was requested into a store that already holds
    /// `existing` tickets without `SeedSpec::into_existing`. Nothing was
    /// generated or written.
    SeedRefused { existing: usize },
    /// MNG-9: the `after` cursor names no ticket, so there is no position
    /// to resume from. Distinct from `NotFound`: the miss is the cursor,
    /// not the thing asked for, and the message says so.
    CursorNotFound(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "no ticket with id '{id}'"),
            Self::InvalidEpic(e) => write!(
                f,
                "invalid epic '{e}' (expected a short alphanumeric prefix like MNG)"
            ),
            Self::Cycle { id, path } => write!(
                f,
                "creating '{id}' would close a blocked_by cycle: {id} -> {}",
                path.join(" -> ")
            ),
            Self::ParentNotFound { id, parent } => write!(
                f,
                "no ticket with id '{parent}': a parent must exist before its child \
                 (nothing changed for '{id}')"
            ),
            Self::ParentCycle { id, path } => write!(
                f,
                "'{id}' would become its own ancestor: parent chain {id} -> {}",
                path.join(" -> ")
            ),
            Self::FirstMustBeScope { epic, kind } => write!(
                f,
                "every shortcode starts with a scope: '{epic}' has no tickets yet, so its \
                 first ticket must be kind 'scope' (got '{}'); create the scope root first",
                db::enum_str(kind)
            ),
            Self::ParentOutsideShortcode { id, parent } => write!(
                f,
                "'{parent}' is outside the shortcode of '{id}': a ticket's parent must be in \
                 its own shortcode — only a shortcode's scope root may hang under another \
                 shortcode (nothing changed for '{id}')"
            ),
            Self::EmptyTitle { id } => write!(
                f,
                "a ticket's title cannot be empty: the title given for '{id}' is blank \
                 (nothing changed for '{id}')"
            ),
            Self::Db(msg) => write!(f, "database error: {msg}"),
            Self::SeedRefused { existing } => write!(
                f,
                "refusing to seed synthetic tickets: the store already holds {existing} ticket(s) \
                 and seeding into an existing database was not explicitly allowed"
            ),
            Self::CursorNotFound(id) => write!(f, "after: no ticket {id}"),
        }
    }
}

/// Walks `blocked_by` edges from the proposed blockers through existing
/// tickets; if the allocated `new_id` is reachable (self-reference
/// included), returns the blocked_by chain that closes the cycle.
/// Dangling ids other than `new_id` terminate their branch.
fn find_cycle(
    tickets: &Cache,
    new_id: &str,
    blocked_by: &[String],
) -> Option<Vec<String>> {
    let mut stack: Vec<Vec<String>> = blocked_by.iter().map(|b| vec![b.clone()]).collect();
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(path) = stack.pop() {
        let last = path.last().expect("paths start non-empty");
        if last == new_id {
            return Some(path);
        }
        if !visited.insert(last.clone()) {
            continue;
        }
        if let Some(t) = tickets.get(last) {
            for b in &t.blocked_by {
                let mut next = path.clone();
                next.push(b.clone());
                stack.push(next);
            }
        }
    }
    None
}

/// AGS-19: walks the parent chain upward from `parent`; if the allocated
/// `new_id` is reachable (naming itself included), returns the chain from
/// `parent` up to `new_id` — the ticket would be its own ancestor. A
/// chain that leaves the store (a parent with no parent) or revisits a
/// ticket terminates; the walk cannot loop.
fn find_parent_cycle(
    tickets: &Cache,
    new_id: &str,
    parent: &str,
) -> Option<Vec<String>> {
    let mut path = vec![parent.to_string()];
    let mut seen: HashSet<String> = HashSet::new();
    let mut current = parent.to_string();
    loop {
        if current == new_id {
            return Some(path);
        }
        if !seen.insert(current.clone()) {
            return None;
        }
        let next = tickets.get(&current).and_then(|t| t.parent.clone())?;
        path.push(next.clone());
        current = next;
    }
}

/// MNG-12/14: a shortcode's root is its lowest-seq `scope` ticket — the
/// first ticket of every prefix created since MNG-12, and whatever scope
/// came first in a prefix older than that (DEMO's is DEMO-4). `None` only
/// for a legacy prefix that never had a scope.
///
/// SCALE-3: read from the roll-ups, which keep exactly that per shortcode
/// (MNG-10 — fixed once set: kinds never change and every later ticket has
/// a higher seq), rather than found by walking the prefix to its first
/// scope. That walk was one step for every prefix made since MNG-12, but
/// the whole prefix for a legacy one that never had a scope — and `create`
/// took it, under the write lock, for every ticket filed there.
fn shortcode_root<'a>(tickets: &'a Cache, epic: &str) -> Option<&'a Ticket> {
    tickets.rollups.root(epic).and_then(|id| tickets.get(id))
}

/// SCALE-4: every parent's `child_count`, from a freshly built children
/// index — at load, and after a seed. A step per parent, not per ticket:
/// a ticket starts at 0 (the database stores no count) and a build only
/// ever finds more children than a store had, never fewer, so the tickets
/// it does not name are already right. Returns how many parents it set.
fn count_children(
    order: &mut BTreeMap<OrderKey, Ticket>,
    at: &HashMap<String, OrderKey>,
    children: &children::Children,
) -> usize {
    let mut parents = 0;
    for (parent, count) in children.counts() {
        if let Some(t) = at.get(parent).and_then(|key| order.get_mut(key)) {
            t.child_count = count;
            parents += 1;
        }
    }
    parents
}

/// A blocker satisfies the ready-set rule when it is done with.
fn satisfied(status: Status) -> bool {
    matches!(status, Status::Complete | Status::Superseded)
}

/// SCALE-6: what `find` matches with — the jump palette's rule (AGS-18,
/// `palette_match` in `frontend/src/keys.rs`), moved to where the rows
/// are. A ticket matches when the query, trimmed, is a substring of its id
/// or of its title, ASCII case folded. A blank query is no needle at all:
/// it matches every ticket, and `find` answers it by position, as `list`.
///
/// Folding is ASCII only, as the palette's is — ids are ASCII by
/// construction, and a title's other bytes compare as they are (UTF-8 is
/// self-synchronizing, so a byte window never matches across a character
/// boundary). The tests hold this to every case the palette's own tests
/// name, read from that file.
struct Needle {
    /// The trimmed query, ASCII-lowercased once — so a window compares a
    /// folded byte against a folded byte without folding the query again.
    folded: Vec<u8>,
}

impl Needle {
    /// `None` for an empty or blank query: it matches every ticket.
    fn of(query: &str) -> Option<Self> {
        let query = query.trim();
        (!query.is_empty()).then(|| Self {
            folded: query.as_bytes().to_ascii_lowercase(),
        })
    }

    fn matches(&self, t: &Ticket) -> bool {
        self.within(&t.id) || self.within(&t.title)
    }

    /// The palette's `h.windows(n).any(|w| w.eq_ignore_ascii_case(n))`,
    /// with the needle folded once and a window looked at only where its
    /// first byte already matches — what keeps a query that matches
    /// nothing (the whole store tested, for an exact `remaining`) inside a
    /// keystroke's budget.
    fn within(&self, haystack: &str) -> bool {
        let (h, n) = (haystack.as_bytes(), self.folded.as_slice());
        if n.len() > h.len() {
            return false;
        }
        let (first, rest) = (n[0], &n[1..]);
        (0..=h.len() - n.len()).any(|i| {
            h[i].to_ascii_lowercase() == first
                && h[i + 1..i + n.len()]
                    .iter()
                    .zip(rest)
                    .all(|(b, q)| b.to_ascii_lowercase() == *q)
        })
    }
}

/// The cache, under one lock (MNG-9). The tickets live in `order` — the
/// sorted `(epic, seq)` map — and `at` says where an id sits in it, so:
///
/// - an **ordered** read (`list`, `ready_set`, any page) is a range walk
///   over tickets that are already in order and already in hand: no sort,
///   and no lookup per ticket. That last part is what a paging index is
///   for; an index of *ids* would put a random hash lookup between the
///   walk and every ticket it has to look at.
/// - a **point** read (`get`, a blocker) is `at` then `order`: a hash
///   lookup and a 17-deep descent at 100,000 tickets, both flat in N.
///
/// One lock over both halves is the point: a reader can never see the two
/// disagree. MNG-10 adds a third half, [`rollup::Rollups`] — the per-
/// shortcode roll-ups `groups` answers from — under the same lock for the
/// same reason: every write that changes a count changes it in the same
/// critical section as the tickets it counts. SCALE-3 adds a fourth,
/// [`ids::Ids`] — each shortcode's highest sequence, and the ids named in
/// some `blocked_by` before they exist — which is what `create` reads to
/// allocate an id and to resolve forward references, instead of walking
/// every ticket twice; a birth moves it in the same critical section as
/// the ticket born. SCALE-4 adds a fifth, [`children::Children`] — each
/// parent's children in the store's order, from every ticket's `parent` —
/// which is what `list { parent }` pages from and what each ticket's
/// `child_count` is; every path that sets a parent moves it in the same
/// critical section as the ticket whose parent it is, and sets the
/// parent's count from it, so the two cannot disagree.
struct Cache {
    order: BTreeMap<OrderKey, Ticket>,
    at: HashMap<String, OrderKey>,
    rollups: rollup::Rollups,
    ids: ids::Ids,
    children: children::Children,
}

/// MNG-10: the tickets half of the cache, borrowed on its own — what the
/// roll-ups walk while they update their own half under the same lock.
/// SCALE-4: with the children index beside them, which is how the roll-ups
/// find a ticket's children now that the row no longer lists them.
#[derive(Clone, Copy)]
struct Rows<'a> {
    order: &'a BTreeMap<OrderKey, Ticket>,
    at: &'a HashMap<String, OrderKey>,
    children: &'a children::Children,
}

impl<'a> Rows<'a> {
    fn get(&self, id: &str) -> Option<&'a Ticket> {
        self.order.get(self.at.get(id)?)
    }

    /// SCALE-4: `t`'s children in its own shortcode, in the store's order
    /// — what a subtree walk follows.
    fn children_in_shortcode(&self, t: &'a Ticket) -> impl Iterator<Item = &'a Ticket> + 'a {
        let order = self.order;
        self.children
            .in_shortcode(&t.id, &t.epic)
            .iter()
            .filter_map(move |&seq| {
                order.get(&OrderKey {
                    epic: t.epic.clone(),
                    seq,
                })
            })
    }

    /// Whether `id` names a ticket — the hash half alone, no descent.
    fn contains(&self, id: &str) -> bool {
        self.at.contains_key(id)
    }

    fn len(&self) -> usize {
        self.at.len()
    }

    fn tickets(&self) -> impl Iterator<Item = &'a Ticket> {
        self.order.values()
    }
}

impl Cache {
    /// Builds the cache from the tickets loaded at startup. Collected,
    /// not inserted one at a time: `BTreeMap`'s `FromIterator` sorts once
    /// and bulk-builds the tree instead of rebalancing n times. Holding
    /// the cache in the store's order costs this once per start — 350 ms
    /// to ~545 ms at 100,000 tickets, measured — and every ordered read
    /// after it is a walk of what is already sorted. The roll-ups are
    /// built here once, from the sorted tickets (MNG-10), and from then on
    /// only kept; so are the id indexes (SCALE-3) — derived state, never
    /// persisted, so a reopened or migrated store gets them from what it
    /// loaded. SCALE-4: and so is the children index, and with it every
    /// ticket's `child_count` (the database stores neither).
    fn new(loaded: HashMap<String, Ticket>) -> Self {
        let mut order: BTreeMap<OrderKey, Ticket> = loaded
            .into_values()
            .map(|t| (OrderKey::of(&t), t))
            .collect();
        let at: HashMap<String, OrderKey> = order
            .values()
            .map(|t| (t.id.clone(), OrderKey::of(t)))
            .collect();
        let started = std::time::Instant::now();
        let children = children::Children::build(order.values());
        let parents = count_children(&mut order, &at, &children);
        tracing::info!(
            "indexed the children of {parents} parents over {} tickets in {:.1} ms",
            order.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        let rows = Rows {
            order: &order,
            at: &at,
            children: &children,
        };
        let started = std::time::Instant::now();
        let rollups = rollup::Rollups::build(rows);
        tracing::info!(
            "rolled up {} shortcodes over {} tickets in {:.1} ms",
            rollups.len(),
            order.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        let started = std::time::Instant::now();
        let ids = ids::Ids::build(rows);
        tracing::info!(
            "indexed {} shortcodes' sequences and {} ids named before birth over {} tickets in {:.1} ms",
            ids.last_seqs().count(),
            ids.unborn_count(),
            order.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        Self {
            order,
            at,
            rollups,
            ids,
            children,
        }
    }

    fn rows(&self) -> Rows<'_> {
        Rows {
            order: &self.order,
            at: &self.at,
            children: &self.children,
        }
    }

    /// SCALE-4: `id`'s kept `child_count` — set from the children index,
    /// on the one row it describes, every time the index moves for it. A
    /// parent that names no ticket (only a hand edit makes one) has no row
    /// to carry it.
    fn set_child_count(&mut self, id: &str, count: u64) {
        if let Some(t) = self.at.get(id).and_then(|key| self.order.get_mut(key)) {
            t.child_count = count;
        }
    }

    /// Inserts or replaces one ticket. A ticket's `(epic, seq)` is fixed
    /// at birth — no operation re-epics or re-numbers one — so replacing
    /// an existing ticket rewrites the same entry, and the two halves
    /// cannot drift apart. Rows only: the roll-ups move through the
    /// mutation-shaped methods below, which call this.
    fn insert(&mut self, ticket: Ticket) {
        let key = OrderKey::of(&ticket);
        self.at.insert(ticket.id.clone(), key.clone());
        self.order.insert(key, ticket);
    }

    /// MNG-10: a create's rows — the newborn and every ticket it touched —
    /// go in, then the newborn is counted. Returns the groups whose row
    /// moved, innermost first. SCALE-3: and it is born into the id
    /// indexes — its shortcode's sequence, the forward references it
    /// resolved, the ones it makes. SCALE-4: and into its parent's
    /// children, whose count it moves; returns that count too.
    fn created(
        &mut self,
        ticket: Ticket,
        touched: impl IntoIterator<Item = Ticket>,
    ) -> (Vec<Group>, Option<u64>) {
        for updated in touched {
            self.insert(updated);
        }
        let id = ticket.id.clone();
        let key = OrderKey::of(&ticket);
        let parent = ticket.parent.clone();
        self.insert(ticket);
        let count = parent.map(|p| {
            let count = self.children.add(&p, &key);
            self.set_child_count(&p, count);
            count
        });
        let rows = Rows {
            order: &self.order,
            at: &self.at,
            children: &self.children,
        };
        let ticket = rows.get(&id).expect("just inserted");
        self.ids.born(rows, ticket);
        (self.rollups.created(rows, ticket), count)
    }

    /// MNG-10: a status change; only a crossing of `Complete` moves a count.
    fn status_changed(&mut self, ticket: Ticket, previous: Status) -> Vec<Group> {
        let id = ticket.id.clone();
        self.insert(ticket);
        let rows = Rows {
            order: &self.order,
            at: &self.at,
            children: &self.children,
        };
        let ticket = rows.get(&id).expect("just inserted");
        self.rollups.status_changed(rows, ticket, previous)
    }

    /// MNG-10: a `set_parent`. The moved tree's count is taken from where
    /// it is counted *before* the rows change (that is where the old
    /// parent still is) and given to where it will be; the rows go in;
    /// then the shape is settled against the new rows.
    ///
    /// SCALE-4: the child is the only row that changes. It leaves
    /// `previous`'s children and joins its new parent's, and each parent's
    /// count is set from the index; returns the groups, then the new
    /// parent's count and the old one's.
    fn reparented(&mut self, child: Ticket, previous: Option<&str>) -> (Vec<Group>, u64, u64) {
        let parent = child.parent.clone().expect("set_parent sets a parent");
        let rows = Rows {
            order: &self.order,
            at: &self.at,
            children: &self.children,
        };
        let moving = self.rollups.reparenting(
            rows,
            rows.get(&child.id).expect("the child exists"),
            rows.get(&parent).expect("the parent exists"),
        );
        let key = OrderKey::of(&child);
        self.insert(child);
        let left = previous.map_or(0, |p| {
            let count = self.children.remove(p, &key);
            self.set_child_count(p, count);
            count
        });
        let joined = self.children.add(&parent, &key);
        self.set_child_count(&parent, joined);
        let rows = Rows {
            order: &self.order,
            at: &self.at,
            children: &self.children,
        };
        (self.rollups.reparented(rows, moving), joined, left)
    }

    fn get(&self, id: &str) -> Option<&Ticket> {
        self.order.get(self.at.get(id)?)
    }

    fn len(&self) -> usize {
        self.at.len()
    }
}

pub struct Store {
    /// The cache. Every read in the process is answered from here; the
    /// lock is synchronous and never held across an await.
    tickets: RwLock<Cache>,
    tx: broadcast::Sender<Change>,
    /// The system of record. Private to this module — nothing else in the
    /// application holds a database handle. Only touched off-runtime, via
    /// `spawn_blocking`.
    db: Arc<StdMutex<Connection>>,
    /// Serializes mutations end to end: id allocation, the durable write,
    /// the cache update, and the broadcast happen atomically with respect
    /// to other writers, so ids are unique under concurrent creates and
    /// `watch` subscribers see changes in acknowledgment order. Readers
    /// never take this lock.
    write_serial: AsyncMutex<()>,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    /// An in-memory store (SQLite `:memory:`): the same write-through path
    /// with no file behind it. For unit tests; the server always opens a
    /// real location via [`Store::open`].
    pub fn new() -> Self {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        Self::with_connection(conn).expect("init in-memory schema")
    }

    /// Opens (creating if absent) the database at `path` and loads every
    /// ticket into the cache. A fresh location is an empty store, not an
    /// error. Does blocking I/O — call from a blocking context (the
    /// builder wraps this in `spawn_blocking`).
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        // WAL + synchronous=FULL: an acknowledged commit survives SIGKILL.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA synchronous = FULL;")?;
        Self::with_connection(conn)
    }

    fn with_connection(mut conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(db::SCHEMA)?;
        // A database written before AGS-13 is rewritten to kinds here, once,
        // durably — before anything is loaded. An unmappable row refuses
        // the open (loudly, nothing changed) rather than loading a guess.
        db::migrate_legacy_ticket_type(&mut conn)?;
        // A database written before AGS-19 gains `parent` here, once,
        // durably — with typed parents backfilled from body frontmatter.
        // Never fatal: an unresolvable one is logged and left empty.
        db::migrate_add_parenthood(&mut conn)?;
        // A database written between AGS-19 and SCALE-4 loses `children`
        // here, once, durably: each list is checked against the `parent`
        // column it mirrored (the edge, then and now) and dropped. Never
        // fatal: a list that disagreed is logged, and `parent` stands.
        db::migrate_drop_children(&mut conn)?;
        let tickets = db::load_all(&conn)?;
        let (tx, _) = broadcast::channel(256);
        Ok(Self {
            tickets: RwLock::new(Cache::new(tickets)),
            tx,
            db: Arc::new(StdMutex::new(conn)),
            write_serial: AsyncMutex::new(()),
        })
    }

    /// Writes a batch of tickets durably in one transaction, off the
    /// async runtime. On `Err` nothing has been acknowledged: the cache
    /// is untouched and no event is emitted.
    async fn persist(&self, batch: Vec<Ticket>) -> Result<(), StoreError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
            let mut conn = db.lock().expect("db lock poisoned");
            let tx = conn.transaction()?;
            for t in &batch {
                db::upsert(&tx, t)?;
            }
            tx.commit()
        })
        .await
        .map_err(|e| StoreError::Db(format!("write task failed: {e}")))?
        .map_err(|e| StoreError::Db(e.to_string()))
    }

    /// Allocates the next `EPIC-N` id, maintains the DAG, writes through
    /// to the database, and only then updates the cache and notifies
    /// `watch` subscribers.
    ///
    /// DAG maintenance: a create whose allocated id would close a
    /// `blocked_by` cycle (self-reference included) is rejected before
    /// anything persists — nothing stored, the id stays unallocated.
    /// Otherwise the new id is appended to each existing blocker's
    /// `unlocks`, and any existing tickets whose `blocked_by` already
    /// named the allocated id (dangling forward references) become the
    /// newborn's `unlocks` from birth.
    ///
    /// Parenthood (AGS-19) is checked the same way: a `parent` that names
    /// no ticket, or one that would make the newborn its own ancestor, is
    /// rejected before anything persists. SCALE-4: otherwise the edge is
    /// the newborn's own `parent` and nothing else — the parent's row is
    /// not rewritten (it holds no list of children to grow), and the cache
    /// adds the newborn to the parent's children and counts it. A parent
    /// that is also a blocker is written once, for its `unlocks`, carrying
    /// the count it will have.
    ///
    /// The newborn and every updated blocker commit in one transaction;
    /// broadcasts follow the commit in acknowledgment order: `Created`,
    /// then `ChildrenChanged` for the parent (its id and new count), then
    /// one `UnlocksChanged` per blocker.
    pub async fn create(&self, new: NewTicket) -> Result<Ticket, StoreError> {
        let epic = new.epic.trim().to_ascii_uppercase();
        if epic.is_empty() || !epic.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(StoreError::InvalidEpic(new.epic));
        }
        let requested_parent = new
            .parent
            .map(|p| p.trim().to_ascii_uppercase())
            .filter(|p| !p.is_empty());

        let _serial = self.write_serial.lock().await;
        // Under the write-serial lock the cache mirrors the database
        // exactly, so the shortcode's highest seq + 1 is the next id —
        // monotonic across restarts (the cache was loaded from the
        // database) and race-free (writers are serialized). SCALE-3: that
        // highest seq is kept in the cache, beside the tickets and moved
        // by every birth, so allocating is a lookup rather than a walk of
        // every ticket in the store. This read phase computes everything
        // and mutates nothing: a rejection leaves no trace, in the
        // tickets or in what is kept about them.
        let (ticket, touched, unlocks_added, parent_epic) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let tickets = &cache;
            let seq = tickets.ids.next_seq(&epic);
            let id = format!("{epic}-{seq}");

            // MNG-12: every shortcode starts with a scope. `seq == 1` is
            // exactly "this prefix has no tickets yet" (ids are never
            // reused), so the newborn would be the shortcode's root.
            if seq == 1 && new.kind != Kind::Scope {
                return Err(StoreError::FirstMustBeScope {
                    epic,
                    kind: new.kind,
                });
            }

            if let Some(path) = find_cycle(tickets, &id, &new.blocked_by) {
                return Err(StoreError::Cycle { id, path });
            }
            // MNG-14 (owner, 2026-09-06): a parent named must exist; a
            // non-root's parent must be inside its own shortcode, and when
            // omitted it defaults to the shortcode's scope root. The root
            // itself (seq == 1) is the exception: its parent, if any, is
            // the shortcode it hangs under — a sub-shortcode — in any
            // prefix; omitted, it stays a top-level root.
            let parent = match (&requested_parent, seq) {
                (Some(p), _) => {
                    let Some(pt) = tickets.get(p) else {
                        return Err(StoreError::ParentNotFound {
                            id,
                            parent: p.clone(),
                        });
                    };
                    if seq > 1 && pt.epic != epic {
                        return Err(StoreError::ParentOutsideShortcode {
                            id,
                            parent: p.clone(),
                        });
                    }
                    if let Some(path) = find_parent_cycle(tickets, &id, p) {
                        return Err(StoreError::ParentCycle { id, path });
                    }
                    Some(p.clone())
                }
                (None, 1) => None,
                (None, _) => shortcode_root(tickets, &epic).map(|r| r.id.clone()),
            };

            // Dangling forward references to this id resolve now: their
            // owners are the newborn's `unlocks` from birth. SCALE-3: the
            // cache keeps them by the id they name, already in (epic, seq)
            // order — only ids that do not exist yet, so it holds the few
            // forward references there are, never every edge.
            let unlocks = tickets.ids.waiting_on(&id);

            let now = chrono::Utc::now().timestamp();
            let ticket = Ticket {
                id: id.clone(),
                epic,
                seq,
                title: new.title,
                status: Status::Pending,
                kind: new.kind,
                severity: new.severity,
                blocked_by: new.blocked_by,
                unlocks,
                parent: parent.clone(),
                // SCALE-4: none, unless a hand-edited database already
                // names this id as some ticket's parent — the index has
                // kept those under the id, as `list { parent }` finds them.
                child_count: tickets.children.count(&id),
                body: new.body,
                created_unix: now,
                updated_unix: now,
            };

            // Symmetry, in one place: every existing ticket this create
            // touches is cloned once and updated in place, so a ticket
            // that is blocker twice over is written once. Updated clones
            // only — the cache changes after the database commit.
            // SCALE-4: the parent is not one of them — its edge is the
            // newborn's `parent`, and the count is the cache's.
            let mut touched: HashMap<String, Ticket> = HashMap::new();
            let mut unlocks_added: Vec<String> = Vec::new();
            for blocker_id in &ticket.blocked_by {
                let mirrored = touched
                    .get(blocker_id)
                    .or_else(|| tickets.get(blocker_id))
                    .map(|b| b.unlocks.contains(&id));
                // None: a dangling blocker — nothing exists to update.
                if mirrored != Some(false) {
                    continue;
                }
                let updated = touched
                    .entry(blocker_id.clone())
                    .or_insert_with(|| {
                        tickets.get(blocker_id).expect("blocker resolved above").clone()
                    });
                updated.unlocks.push(id.clone());
                updated.updated_unix = now;
                unlocks_added.push(blocker_id.clone());
            }
            // A parent that is also a blocker rides out whole in its
            // `UnlocksChanged`, after the `ChildrenChanged` that counts the
            // newborn: it carries the count it is about to have, or a
            // client applying the two in order would undo the first.
            if let Some(both) = parent.as_ref().and_then(|p| touched.get_mut(p)) {
                both.child_count += 1;
            }
            // The parent's shortcode, which `watch { epic }` files its
            // `ChildrenChanged` under — another one only for a scope root
            // hosted by another shortcode.
            let parent_epic = parent
                .as_deref()
                .map(|p| tickets.get(p).expect("parent checked above").epic.clone());
            (ticket, touched, unlocks_added, parent_epic)
        };

        let mut batch: Vec<Ticket> = touched.values().cloned().collect();
        batch.push(ticket.clone());
        self.persist(batch).await?;

        let (groups, child_count) = self
            .tickets
            .write()
            .expect("store lock poisoned")
            .created(ticket.clone(), touched.values().cloned());
        let _ = self.tx.send(Change::Created {
            ticket: ticket.clone(),
        });
        if let (Some(parent_id), Some(epic), Some(child_count)) =
            (&ticket.parent, parent_epic, child_count)
        {
            let _ = self.tx.send(Change::ChildrenChanged {
                id: parent_id.clone(),
                epic,
                added: ticket.id.clone(),
                child_count,
            });
        }
        for blocker_id in unlocks_added {
            let _ = self.tx.send(Change::UnlocksChanged {
                ticket: touched[&blocker_id].clone(),
                added: ticket.id.clone(),
            });
        }
        self.broadcast_groups(groups);
        Ok(ticket)
    }

    /// MNG-10: the roll-up half of a mutation's broadcast, sent last — after
    /// every ticket change of the same mutation, so the established order
    /// (`Created` first, …) is untouched and a client meets the cause
    /// before the count it moved.
    fn broadcast_groups(&self, groups: Vec<Group>) {
        for group in groups {
            let _ = self.tx.send(Change::GroupChanged { group });
        }
    }

    /// MNG-10: one roll-up per shortcode row the nav draws, in shortcode
    /// order — `total`, `done`, `root` and `host` exactly as AGS-20's
    /// `plan()` computes them from every row. Answered from the roll-ups
    /// the cache keeps beside the tickets, so the cost is the number of
    /// shortcodes, never the number of tickets.
    ///
    /// A prefix whose tickets are all drawn under other shortcodes' tickets
    /// (only a legacy, pre-MNG-14 shape: a cross-shortcode child with no
    /// scope root of its own) has no row in the nav and so no group here;
    /// those tickets count in the groups they are drawn in.
    pub fn groups(&self) -> Vec<Group> {
        self.tickets
            .read()
            .expect("store lock poisoned")
            .rollups
            .groups()
    }

    /// SCALE-16: every shortcode that holds a ticket, with its highest
    /// sequence, in shortcode order — the per-shortcode sequence `create`
    /// allocates from (SCALE-3), read as it stands, so the cost is the
    /// number of shortcodes, never the number of tickets.
    ///
    /// Because ids are allocated highest-plus-one and nothing deletes, a
    /// shortcode's sequences are dense and it holds exactly that many
    /// tickets. A database edited by hand can hold a hole; the answer is
    /// still the highest sequence, never a count.
    pub fn extents(&self) -> Vec<(String, u32)> {
        self.tickets
            .read()
            .expect("store lock poisoned")
            .ids
            .last_seqs()
            .map(|(epic, seq)| (epic.to_string(), seq))
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<Ticket> {
        self.tickets
            .read()
            .expect("store lock poisoned")
            .get(id)
            .cloned()
    }

    /// Tickets ordered by (epic, seq), optionally filtered. The `parent`
    /// filter (AGS-19) selects exactly that ticket's children (SCALE-4:
    /// from the children index, still in the store's order). The unpaged
    /// shape, kept: [`Store::page`] is the same walk with a cursor and a
    /// limit (MNG-9).
    pub fn list(
        &self,
        epic: Option<&str>,
        status: Option<Status>,
        parent: Option<&str>,
    ) -> Vec<Ticket> {
        self.page(PageQuery {
            epic,
            status,
            parent,
            ..PageQuery::default()
        })
        // Only an `after` cursor can fail, and there is none here.
        .expect("an uncursored page cannot fail")
        .tickets
    }

    /// MNG-9: one page of the store's order — at most `limit` tickets
    /// strictly after the `after` cursor, under the same filters `list`
    /// has always taken, plus how many further tickets match.
    ///
    /// The walk is over the cache's sorted half, so the cost is the page
    /// (plus the count of what remains — SCALE-19: arithmetic, with no
    /// `status` to test), not a clone-and-sort of every ticket. SCALE-4:
    /// a `parent` is a position too — the walk is that ticket's children
    /// in the children index, cut by the same span, so a page of a root's
    /// 163,530 children costs the page and its `remaining` is arithmetic;
    /// only a `status` beside it is tested, child by child. `after` naming
    /// no ticket is [`StoreError::CursorNotFound`]: a cursor into nothing
    /// is a caller error, not an empty page.
    pub fn page(&self, q: PageQuery<'_>) -> Result<Page, StoreError> {
        let epic = q.epic.map(str::to_ascii_uppercase);
        let parent = q.parent.map(str::to_ascii_uppercase);
        let after = q.after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        let span = Span::of(&cache, after.as_deref(), epic.as_deref())?;
        let keep = |t: &Ticket| q.status.is_none_or(|s| t.status == s);
        // The epic is already spent — it bounded the span rather than
        // filtering inside it — so with no status asked for, there is
        // nothing left to test on the way through.
        let filter = match q.status {
            None => Filter::All,
            Some(_) => Filter::By(&keep),
        };
        Ok(match parent.as_deref() {
            Some(p) => Self::walk_children(&cache, p, span, q.limit, filter),
            None => Self::walk_cache(&cache, span, q.limit, filter),
        })
    }

    /// MNG-9: one page of the ready set — the [`Store::ready_set`]
    /// membership rule, cut with the same cursor and limit as
    /// [`Store::page`].
    pub fn ready_page(
        &self,
        epic: Option<&str>,
        after: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Page, StoreError> {
        let epic = epic.map(str::to_ascii_uppercase);
        let after = after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        let span = Span::of(&cache, after.as_deref(), epic.as_deref())?;
        let keep = |t: &Ticket| {
            t.status == Status::Ready
                && t.blocked_by
                    .iter()
                    .all(|b| cache.get(b).is_some_and(|bt| satisfied(bt.status)))
        };
        Ok(Self::walk_cache(&cache, span, limit, Filter::By(&keep)))
    }

    /// SCALE-6: one page of the tickets whose id or title contains
    /// `query` — the jump palette's rule ([`Needle`]) — cut with the same
    /// cursor and limit as [`Store::page`] (MNG-9: `after` is exclusive and
    /// a position, so it may name a ticket that does not match; one naming
    /// no ticket is [`StoreError::CursorNotFound`]), `remaining` counting
    /// the matches beyond the page.
    ///
    /// An empty or blank query is the store's order unfiltered — the page
    /// the palette opens on costs the page, `remaining` by arithmetic
    /// (SCALE-19). Any other query tests the tickets after the cursor: the
    /// rows stop at `limit`, the count does not, so an exact `remaining` is
    /// a scan of every id and title past the page — the price the ticket's
    /// Risks section accepts while the cache is a full mirror (an index is
    /// SCALE-13's B5).
    pub fn find_page(
        &self,
        query: &str,
        after: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Page, StoreError> {
        let after = after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        let span = Span::of(&cache, after.as_deref(), None)?;
        let Some(needle) = Needle::of(query) else {
            return Ok(Self::walk_cache(&cache, span, limit, Filter::All));
        };
        let keep = |t: &Ticket| needle.matches(t);
        Ok(Self::walk_cache(&cache, span, limit, Filter::By(&keep)))
    }

    /// The one walk every page method runs: the span, `limit` matches
    /// cloned into the page, then the rest of the span counted — because
    /// `remaining` is an exact number, not an estimate. That count is the
    /// O(what is left) the ticket's Risks section names and accepts;
    /// what keeps it cheap is that the span is already the right stretch
    /// of the order and its tickets are already in hand.
    ///
    /// SCALE-19: except where nothing is filtered. There the page is the
    /// span's first tickets, so what is left is the span's length less
    /// the page — [`Span::len`], arithmetic over the shortcodes — and the
    /// count, which made a paged walk of the store quadratic (15.7 s at
    /// 10⁶), is not taken at all. A predicate still costs its walk.
    fn walk_cache(cache: &Cache, span: Span, limit: Option<u32>, filter: Filter<'_>) -> Page {
        let rows = span.range(cache).map(|range| range.map(|(_, t)| t));
        let cursor = span.cursor.clone();
        Self::walk_rows(rows, || span.len(cache), cursor, limit, filter)
    }

    /// SCALE-4: the same walk over `parent`'s children — the children
    /// index cut by the span (the epic and the cursor are positions in it
    /// as they are in the store), each child looked up by its key. With no
    /// status to test, what is left is the index's arithmetic
    /// ([`children::Children::len`]): the page costs the page, wherever
    /// the cursor sits in a list of any length.
    fn walk_children(
        cache: &Cache,
        parent: &str,
        span: Span,
        limit: Option<u32>,
        filter: Filter<'_>,
    ) -> Page {
        let (start, end) = (span.start.as_ref(), span.end.as_ref());
        let rows = (!span.empty).then(|| {
            cache
                .children
                .range(parent, start, end)
                .filter_map(|key| cache.order.get(&key))
        });
        let cursor = span.cursor.clone();
        Self::walk_rows(
            rows,
            || cache.children.len(parent, start, end),
            cursor,
            limit,
            filter,
        )
    }

    /// What every walk shares: `limit` matches cloned into the page, then
    /// `remaining` — `len` less the page when nothing is filtered (the
    /// page is a prefix of what `rows` holds), or the rest of the matches
    /// counted when something is.
    fn walk_rows<'c>(
        rows: Option<impl Iterator<Item = &'c Ticket>>,
        len: impl FnOnce() -> u64,
        cursor: Option<String>,
        limit: Option<u32>,
        filter: Filter<'_>,
    ) -> Page {
        let take = limit.map_or(usize::MAX, |n| n as usize);
        let mut tickets = Vec::new();
        if let Some(n) = limit {
            tickets.reserve(n as usize);
        }
        let remaining = match (rows, filter) {
            (None, _) => 0,
            (Some(rows), Filter::All) => {
                tickets.extend(rows.take(take).cloned());
                // The page is a prefix of the span: it cannot be longer.
                len().saturating_sub(tickets.len() as u64)
            }
            (Some(rows), Filter::By(keep)) => {
                let mut it = rows.filter(|t| keep(t));
                tickets.extend(it.by_ref().take(take).cloned());
                it.count() as u64
            }
        };

        let next = if remaining == 0 {
            None
        } else {
            // The page's last id, or — for the degenerate empty page
            // (`limit: 0`) — the cursor it was cut with: either way, where
            // the next page resumes.
            tickets.last().map(|t| t.id.clone()).or(cursor)
        };
        Page {
            tickets,
            next,
            remaining,
        }
    }

    /// Changes the status, write-through: durable in the database before
    /// the cache is updated and subscribers are notified. A change into or
    /// out of `Complete` moves `done` for the group the ticket is drawn in
    /// and every group around it (MNG-10): `StatusChanged`, then one
    /// `GroupChanged` per group, innermost first. Any other change moves
    /// no count and broadcasts no group.
    pub async fn set_status(&self, id: &str, status: Status) -> Result<Change, StoreError> {
        let _serial = self.write_serial.lock().await;
        let mut ticket = self
            .get(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        let previous = ticket.status;
        ticket.status = status;
        ticket.updated_unix = chrono::Utc::now().timestamp();

        self.persist(vec![ticket.clone()]).await?;
        let groups = self
            .tickets
            .write()
            .expect("store lock poisoned")
            .status_changed(ticket.clone(), previous);
        let change = Change::StatusChanged { ticket, previous };
        let _ = self.tx.send(change.clone());
        self.broadcast_groups(groups);
        Ok(change)
    }

    /// MNG-12: files an existing ticket as an output of `parent` — the
    /// verb AGS-19 left out, for adopting a ticket born parentless or
    /// moving one. Same guards as `create`: the parent must exist, a
    /// non-root's parent must be inside its own shortcode (MNG-14; the
    /// shortcode's scope root may be hosted anywhere), and the chain must
    /// stay acyclic (naming itself included); a rejection changes
    /// nothing. The child's row commits; then the cache, where it leaves
    /// the previous parent's children and joins the new one's (SCALE-4:
    /// neither parent's row is written — the edge is the child's `parent`,
    /// and the counts are the cache's); then, in order, `ParentChanged`
    /// (the child), `ChildrenChanged` (the new parent's id and count) and
    /// `ChildRemoved` (the old one's, when it names a ticket). Setting the
    /// parent a ticket already has writes nothing and emits nothing: the
    /// returned `ParentChanged` then shows `previous == Some(parent)`.
    pub async fn set_parent(&self, id: &str, parent: &str) -> Result<Change, StoreError> {
        let id = id.trim().to_ascii_uppercase();
        let parent = parent.trim().to_ascii_uppercase();
        let _serial = self.write_serial.lock().await;
        let (child, new_epic, old_epic, previous) = {
            let tickets = self.tickets.read().expect("store lock poisoned");
            let mut child = tickets
                .get(&id)
                .ok_or_else(|| StoreError::NotFound(id.clone()))?
                .clone();
            let Some(parent_ticket) = tickets.get(&parent) else {
                return Err(StoreError::ParentNotFound { id, parent });
            };
            if child.parent.as_deref() == Some(parent.as_str()) {
                return Ok(Change::ParentChanged {
                    ticket: child,
                    previous: Some(parent),
                });
            }
            // MNG-14: only the shortcode's scope root may hang under
            // another shortcode; everything else stays inside its own.
            let is_root = shortcode_root(&tickets, &child.epic).is_some_and(|r| r.id == child.id);
            if !is_root && parent_ticket.epic != child.epic {
                return Err(StoreError::ParentOutsideShortcode { id, parent });
            }
            if let Some(path) = find_parent_cycle(&tickets, &id, &parent) {
                return Err(StoreError::ParentCycle { id, path });
            }
            let now = chrono::Utc::now().timestamp();
            let previous = child.parent.take();
            child.parent = Some(parent.clone());
            child.updated_unix = now;

            // A previous parent that names no ticket (a hand-edited
            // database) has no row to tell; the child's edge still moves.
            let old_epic = previous
                .as_deref()
                .and_then(|p| tickets.get(p))
                .map(|t| t.epic.clone());
            (child, parent_ticket.epic.clone(), old_epic, previous)
        };

        self.persist(vec![child.clone()]).await?;

        let (groups, joined, left) = self
            .tickets
            .write()
            .expect("store lock poisoned")
            .reparented(child.clone(), previous.as_deref());
        let change = Change::ParentChanged {
            ticket: child,
            previous: previous.clone(),
        };
        let _ = self.tx.send(change.clone());
        let _ = self.tx.send(Change::ChildrenChanged {
            id: parent,
            epic: new_epic,
            added: id.clone(),
            child_count: joined,
        });
        if let (Some(old), Some(epic)) = (previous, old_epic) {
            let _ = self.tx.send(Change::ChildRemoved {
                id: old,
                epic,
                removed: id,
                child_count: left,
            });
        }
        self.broadcast_groups(groups);
        Ok(change)
    }

    /// MNG-16: replaces a ticket's title and/or body — whole values, last
    /// writer wins (AGS-3 owns live, contended text). The one write after
    /// birth that touches neither status nor shape: `id`, status, kind,
    /// both edge lists and parenthood are carried over untouched, so no
    /// roll-up moves and no id index changes — the row goes into the cache
    /// and that is all.
    ///
    /// Each value given is compared with the current one; only a different
    /// one is written, and `changed` names what was (`"title"`, then
    /// `"body"`). Nothing different — neither given, or both equal — is no
    /// write and no broadcast, and the returned `Amended` says `changed:
    /// []`, as `set_parent` answers a no-op. Otherwise `updated_unix` moves
    /// to now, the row commits, then the cache, then one `Amended`. A title
    /// that is empty after trimming is refused before anything is read; a
    /// title or body that is given is stored exactly as given, byte for
    /// byte (like `create`, which does not trim either).
    pub async fn amend(
        &self,
        id: &str,
        title: Option<String>,
        body: Option<String>,
    ) -> Result<Change, StoreError> {
        let id = id.trim().to_ascii_uppercase();
        if title.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err(StoreError::EmptyTitle { id });
        }
        let _serial = self.write_serial.lock().await;
        let mut ticket = self
            .get(&id)
            .ok_or_else(|| StoreError::NotFound(id.clone()))?;
        let mut changed: Vec<String> = Vec::new();
        if let Some(title) = title.filter(|t| *t != ticket.title) {
            ticket.title = title;
            changed.push("title".into());
        }
        if let Some(body) = body.filter(|b| *b != ticket.body) {
            ticket.body = body;
            changed.push("body".into());
        }
        if changed.is_empty() {
            return Ok(Change::Amended { ticket, changed });
        }
        ticket.updated_unix = chrono::Utc::now().timestamp();

        self.persist(vec![ticket.clone()]).await?;
        // Rows only, deliberately: what an amend changes is counted by no
        // roll-up and indexed by no id index (see above), so there is no
        // mutation-shaped `Cache` method to go through.
        self.tickets
            .write()
            .expect("store lock poisoned")
            .insert(ticket.clone());
        let change = Change::Amended { ticket, changed };
        let _ = self.tx.send(change.clone());
        Ok(change)
    }

    /// Tickets that can start now: status `Ready` and every `blocked_by`
    /// entry resolves to a ticket whose status is `Complete` or
    /// `Superseded`. A dangling blocker (id naming no existing ticket)
    /// keeps its ticket out. The epic filter applies to membership;
    /// blockers resolve globally. Ordered by (epic, seq).
    pub fn ready_set(&self, epic: Option<&str>) -> Vec<Ticket> {
        self.ready_page(epic, None, None)
            .expect("an uncursored page cannot fail")
            .tickets
    }

    /// One ticket's `blocked_by`, each entry resolved to the current
    /// ticket or marked dangling, in `blocked_by` order.
    pub fn blockers(&self, id: &str) -> Result<Vec<BlockerRef>, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let ticket = cache
            .get(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        Ok(ticket
            .blocked_by
            .iter()
            .map(|b| match cache.get(b) {
                Some(t) => BlockerRef::Resolved(t.clone()),
                None => BlockerRef::Dangling(b.clone()),
            })
            .collect())
    }

    pub fn count(&self) -> usize {
        self.tickets.read().expect("store lock poisoned").len()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.tx.subscribe()
    }

    /// PERF-2: bulk-seeds `spec.count` synthetic tickets, through this
    /// layer and nothing else. The tickets are generated in memory first
    /// (ids allocated the way `create` allocates them — max existing seq
    /// per epic, plus one — every `blocked_by` edge pointing at an
    /// already-generated ticket so the DAG is acyclic by construction,
    /// and each mirrored in its blocker's `unlocks`), then written
    /// through to the database in a few large transactions (one fsync
    /// per ~50k rows, never one per row), and only then inserted into
    /// the cache. Deterministic given `spec.seed`.
    ///
    /// Refuses a store that already holds tickets unless
    /// `spec.into_existing` — nothing is generated or written then.
    /// Nothing is broadcast: seeding runs before the server serves, so
    /// there is no subscriber to tell, and a million `Created` events
    /// would only lag the channel. Blocking I/O: call from a blocking
    /// context (the builder wraps it in `spawn_blocking`).
    pub fn seed_synthetic(&self, spec: SeedSpec) -> Result<SeedReport, StoreError> {
        // Mutations stay serialized even here: a concurrent writer means
        // this was called while serving, which it must not be.
        let _serial = self
            .write_serial
            .try_lock()
            .map_err(|_| StoreError::Db("seed requested while another write is in flight".into()))?;
        let started = std::time::Instant::now();

        let next_seq: HashMap<String, u32> = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if cache.len() > 0 && !spec.into_existing {
                return Err(StoreError::SeedRefused {
                    existing: cache.len(),
                });
            }
            // SCALE-3: the per-shortcode sequence `create` allocates from,
            // so seeded ids continue exactly where a create's would.
            cache
                .ids
                .last_seqs()
                .map(|(epic, seq)| (epic.to_string(), seq))
                .collect()
        };

        let generated = synthetic::generate(spec, &next_seq);
        let generate = started.elapsed();
        let epics = {
            let mut seen: HashSet<&str> = HashSet::new();
            generated.iter().for_each(|t| {
                seen.insert(t.epic.as_str());
            });
            seen.len()
        };
        let edges: usize = generated.iter().map(|t| t.blocked_by.len()).sum();

        let persist_started = std::time::Instant::now();
        {
            let mut conn = self.db.lock().expect("db lock poisoned");
            for chunk in generated.chunks(synthetic::ROWS_PER_TRANSACTION) {
                let tx = conn
                    .transaction()
                    .map_err(|e| StoreError::Db(e.to_string()))?;
                db::insert_many(&tx, chunk).map_err(|e| StoreError::Db(e.to_string()))?;
                tx.commit().map_err(|e| StoreError::Db(e.to_string()))?;
            }
        }
        let persist = persist_started.elapsed();

        {
            let mut tickets = self.tickets.write().expect("store lock poisoned");
            tickets.at.reserve(generated.len());
            for t in generated {
                tickets.insert(t);
            }
            // SCALE-4: the children index, and every parent's count from
            // it, built over the seeded store as at load — first, because
            // the roll-ups walk it. A seed only ever adds children, so a
            // count set before it is either set again here or still right.
            let children = children::Children::build(tickets.order.values());
            let cache = &mut *tickets;
            count_children(&mut cache.order, &cache.at, &children);
            cache.children = children;
            // MNG-10: built once over the seeded store, as at load — not
            // kept per row: a seed into an existing store can add tickets
            // to its prefixes in any shape, and nothing is broadcast here.
            let rollups = rollup::Rollups::build(tickets.rows());
            tickets.rollups = rollups;
            // SCALE-3: the id indexes too, for the same reason — the seed
            // went into the cache without passing through `create`, and
            // one of its tickets may be an id the store already named.
            let ids = ids::Ids::build(tickets.rows());
            tickets.ids = ids;
        }
        Ok(SeedReport {
            count: spec.count,
            epics,
            edges,
            generate,
            persist,
            elapsed: started.elapsed(),
        })
    }
}

/// MNG-10: the per-shortcode roll-ups — AGS-20's nav tree, computed by the
/// server and *kept*, so `groups` costs the number of shortcodes and never
/// the number of tickets.
///
/// What is counted is exactly what `frontend/src/nav.rs`'s `plan()` draws:
/// a ticket nests under its parent's row (any shortcode); a ticket with no
/// parent is a row of its own shortcode's group; in a **sub-shortcode** —
/// one whose scope root's parent is in another shortcode, its *host* — a
/// ticket whose parent is that host is a row of the sub-shortcode's group
/// instead (the *cut*); and a group's `done/total` counts every ticket in
/// its subtree, nested groups included.
///
/// The shape that makes this cheap to keep: a cut is always an edge
/// *between* shortcodes (the host is in another one), so following
/// `parent` while it stays inside the shortcode never meets one. Every
/// ticket therefore belongs to a **tree** — the ancestor where that walk
/// stops is its *top* — and a whole tree is drawn wherever its top is:
///
/// - a top with no parent (or none plan() honours: one that names no
///   ticket, or a parent cycle) is drawn in its own shortcode's row,
///   whatever else ever moves. Those trees are pooled per shortcode
///   (`base`).
/// - a top whose parent is in another shortcode (a **cross** top: every
///   sub-shortcode's root, plus any legacy cross-shortcode child) is drawn
///   in its own row when that parent is the host, and under that parent —
///   in whatever group holds it — otherwise. Each keeps its own tally.
///
/// The shape lives in those few places — one pool per shortcode, one
/// entry per cross top — and [`Rollups::settle`] re-derives it from them
/// alone, in plan()'s own order: O(shortcodes + cross tops), never
/// O(tickets). A status change or a create costs a walk up to its tree's
/// top plus the chain of groups around it; a `set_parent`, and a create
/// that starts a sub-shortcode, settle and diff. A `set_parent` that moves
/// a subtree from one tree to another inside a shortcode walks that
/// subtree to count it — only possible in a legacy prefix with several
/// parentless tickets; since MNG-14 a shortcode is one tree.
///
/// A database edited by hand can hold what the server never makes — a
/// parent that names no ticket, a parent cycle. It is loaded and rolled up
/// as plan() would draw it; the two writes that can *repair* such a thing
/// (a create of the id a dangling parent names, a `set_parent` of a ticket
/// on a cycle) rebuild the roll-ups from the cache, with a warning.
mod rollup;

/// SCALE-3: what `create` reads instead of walking every ticket twice
/// under the write lock — each shortcode's highest sequence (the next id
/// is it plus one) and the dangling forward references, kept from the end
/// they name (an id some `blocked_by` names that is no ticket yet → who
/// names it, in the store's order: the newborn's `unlocks` from birth).
///
/// Only the *dangling* references, never every edge: an edge to a ticket
/// that exists is already on both tickets (`blocked_by` and `unlocks`), and
/// a bounded store (SCALE-10) cannot hold O(edges) beside the rows. So the
/// index is O(shortcodes + forward references) — tiny by definition — and
/// a birth is O(the newborn's own `blocked_by`).
///
/// Beside MNG-10's per-shortcode roll-ups, not inside them: those are the
/// nav's drawing — which shortcodes get a row, rebuilt whole when a hand
/// edit is repaired — while a shortcode's sequence is a fact about the
/// store, read before the newborn exists, for every shortcode that holds a
/// ticket whether the nav draws it or not. An id must never move because
/// a drawing did. The one per-shortcode fact both need, the scope root,
/// stays where MNG-10 keeps it, and `create` reads it from there.
///
/// Derived, never persisted: built in one pass at load (after both
/// in-place migrations) and after a seed, and kept by every birth through
/// `create`. `set_parent` and `set_status` touch neither a sequence nor a
/// `blocked_by`, so they leave it as it is.
///
/// SCALE-19: beside each shortcode's sequence, how many tickets it holds —
/// kept by the same paths, so a shortcode is known to be *dense* (every
/// sequence from 1 to its highest: all the server ever makes) by comparing
/// two numbers. That is what lets `page.remaining` be arithmetic instead of
/// a count of everything after the page; a shortcode a hand edit left a
/// hole in is counted, as before, where a page cuts it.
mod ids;

/// PERF-2: the synthetic ticket generator. Private to the store: the
/// shape it produces mirrors what `create` + `set_status` would have
/// built one call at a time, so every invariant the DAG queries rely on
/// (ids, `blocked_by` -> `unlocks` symmetry, acyclicity) holds by
/// construction rather than by checking.
mod synthetic;

/// SCALE-4: each parent's children, kept from the end that names them —
/// every ticket's `parent` — in the store's order, which is what
/// `list { parent }` pages from and what a ticket's `child_count` is.
///
/// AGS-19 kept the other end: a `children` list on the parent's row,
/// mirrored like `unlocks`. MNG-14 made every ticket of a shortcode its
/// root's child, so a root's row grew with its shortcode — 163,530 ids,
/// 2 MB at 10⁶ — and every birth under it rewrote that row, stored it and
/// broadcast it whole, while `list { parent }` walked the store testing
/// every ticket's `parent`. Now the row carries a count, a birth is one
/// push onto the parent's vector here, and a page of children is a binary
/// search to the cursor and then the page; `remaining` is arithmetic.
///
/// Per parent: per shortcode, the sequences of its children, ascending —
/// a `u32` a child, the shortcode's name once. Keyed by the id `parent`
/// names whether or not it is a ticket, as `list { parent }` always
/// answered. Derived, never persisted: built in one pass at load (after
/// every in-place migration) and after a seed, and moved by `create` and
/// `set_parent` in the same critical section as the row whose parent it
/// is; each move sets the parent's `child_count` from it.
mod children;

/// SQL for the system of record. Private: the rest of the application
/// only ever sees the cache above.
mod db;

#[cfg(test)]
mod tests;

/// SCALE-4: the children index against a scan of every ticket's `parent`
/// after every path that sets one, `child_count` against both, pages of
/// `list { parent }` against the old filter, and the migration that drops
/// the `children` column.
#[cfg(test)]
mod tests_children;

/// SCALE-19: `page.remaining` is arithmetic now; these prove it is the
/// count, page for page, against MNG-9's rule applied to a plain list.
#[cfg(test)]
mod tests_paging;

/// SCALE-6: `find` — the server's rule held to the palette's own tests and
/// code, and its pages to MNG-9's rules.
#[cfg(test)]
mod tests_find;

/// SDLC-7: all nine statuses through the system of record, and what the
/// two worker states are not (ready, done, a satisfied blocker).
#[cfg(test)]
mod tests_worker_states;

/// MNG-16: `amend` — every row of its table, byte-exact bodies through the
/// file, and nothing else about the ticket moving.
#[cfg(test)]
mod tests_amend;
