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
pub use crate::types::{Kind, Severity, Status, Ticket};

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
/// costs the walk and not a test per ticket.
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
    /// An existing ticket's `unlocks` gained `added` because a newly
    /// created ticket named it in `blocked_by`. Carries the full updated
    /// ticket so watch-only clients stay coherent.
    UnlocksChanged { ticket: Ticket, added: String },
    /// AGS-19: an existing ticket's `children` gained `added` — a child
    /// was born naming it as `parent`. Same discipline as
    /// `UnlocksChanged`: the full updated parent rides along.
    ChildrenChanged { ticket: Ticket, added: String },
    /// MNG-12: a ticket's `parent` was set after birth (`set_parent`);
    /// `previous` is the parent it had, if any. Carries the full child.
    ParentChanged {
        ticket: Ticket,
        previous: Option<String>,
    },
    /// MNG-12: an existing ticket's `children` lost `removed` — that
    /// child was re-parented elsewhere. The full updated ex-parent rides
    /// along, as with `ChildrenChanged`.
    ChildRemoved { ticket: Ticket, removed: String },
}

impl Change {
    pub fn ticket(&self) -> &Ticket {
        match self {
            Self::Created { ticket }
            | Self::StatusChanged { ticket, .. }
            | Self::UnlocksChanged { ticket, .. }
            | Self::ChildrenChanged { ticket, .. }
            | Self::ParentChanged { ticket, .. }
            | Self::ChildRemoved { ticket, .. } => ticket,
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
fn shortcode_root<'a>(tickets: &'a Cache, epic: &str) -> Option<&'a Ticket> {
    // The cache is ordered by (epic, seq), so the prefix's tickets are one
    // contiguous range and the first scope met is the lowest-seq one.
    let lo = OrderKey { epic: epic.to_string(), seq: 0 };
    let hi = OrderKey { epic: epic.to_string(), seq: u32::MAX };
    tickets.order.range(lo..=hi).map(|(_, t)| t).find(|t| t.kind == Kind::Scope)
}

/// A blocker satisfies the ready-set rule when it is done with.
fn satisfied(status: Status) -> bool {
    matches!(status, Status::Complete | Status::Superseded)
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
/// disagree.
struct Cache {
    order: BTreeMap<OrderKey, Ticket>,
    at: HashMap<String, OrderKey>,
}

impl Cache {
    /// Builds the cache from the tickets loaded at startup. Collected,
    /// not inserted one at a time: `BTreeMap`'s `FromIterator` sorts once
    /// and bulk-builds the tree instead of rebalancing n times. Holding
    /// the cache in the store's order costs this once per start — 350 ms
    /// to ~545 ms at 100,000 tickets, measured — and every ordered read
    /// after it is a walk of what is already sorted.
    fn new(loaded: HashMap<String, Ticket>) -> Self {
        let order: BTreeMap<OrderKey, Ticket> = loaded
            .into_values()
            .map(|t| (OrderKey::of(&t), t))
            .collect();
        let at = order
            .values()
            .map(|t| (t.id.clone(), OrderKey::of(t)))
            .collect();
        Self { order, at }
    }

    /// Inserts or replaces one ticket. A ticket's `(epic, seq)` is fixed
    /// at birth — no operation re-epics or re-numbers one — so replacing
    /// an existing ticket rewrites the same entry, and the two halves
    /// cannot drift apart.
    fn insert(&mut self, ticket: Ticket) {
        let key = OrderKey::of(&ticket);
        self.at.insert(ticket.id.clone(), key.clone());
        self.order.insert(key, ticket);
    }

    fn get(&self, id: &str) -> Option<&Ticket> {
        self.order.get(self.at.get(id)?)
    }

    fn len(&self) -> usize {
        self.at.len()
    }

    fn tickets(&self) -> impl Iterator<Item = &Ticket> {
        self.order.values()
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
        // A database written before AGS-19 gains `parent`/`children` here,
        // once, durably — with typed parents backfilled from body
        // frontmatter. Never fatal: an unresolvable one is logged and
        // left empty.
        db::migrate_add_parenthood(&mut conn)?;
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
    /// Parenthood (AGS-19) is maintained the same way: a `parent` that
    /// names no ticket, or one that would make the newborn its own
    /// ancestor, is rejected before anything persists; otherwise the new
    /// id is appended to the parent's `children`. A ticket that is both
    /// parent and blocker is updated once, carrying both edges.
    ///
    /// The newborn and every updated ticket commit in one transaction;
    /// broadcasts follow the commit in acknowledgment order: `Created`,
    /// then `ChildrenChanged` for the parent, then one `UnlocksChanged`
    /// per blocker.
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
        // exactly, so max-existing-seq + 1 is the next id — monotonic
        // across restarts (the cache was loaded from the database) and
        // race-free (writers are serialized). This read phase computes
        // everything and mutates nothing: a rejection leaves no trace.
        let (ticket, touched, unlocks_added) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let tickets = &cache;
            let seq = tickets
                .tickets()
                .filter(|t| t.epic == epic)
                .map(|t| t.seq)
                .max()
                .unwrap_or(0)
                + 1;
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
            // owners are the newborn's `unlocks` from birth.
            let mut referrers: Vec<(String, u32, String)> = tickets
                .tickets()
                .filter(|t| t.blocked_by.iter().any(|b| b == &id))
                .map(|t| (t.epic.clone(), t.seq, t.id.clone()))
                .collect();
            referrers.sort();
            let unlocks: Vec<String> = referrers.into_iter().map(|(_, _, rid)| rid).collect();

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
                children: Vec::new(),
                body: new.body,
                created_unix: now,
                updated_unix: now,
            };

            // Symmetry, in one place: every existing ticket this create
            // touches is cloned once and updated in place, so a ticket
            // that is both the parent and a blocker carries both edges
            // into the commit. Updated clones only — the cache changes
            // after the database commit.
            let mut touched: HashMap<String, Ticket> = HashMap::new();
            if let Some(parent_id) = &parent {
                let mut updated = tickets.get(parent_id).expect("parent checked above").clone();
                updated.children.push(id.clone());
                updated.updated_unix = now;
                touched.insert(parent_id.clone(), updated);
            }
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
            (ticket, touched, unlocks_added)
        };

        let mut batch: Vec<Ticket> = touched.values().cloned().collect();
        batch.push(ticket.clone());
        self.persist(batch).await?;

        {
            let mut tickets = self.tickets.write().expect("store lock poisoned");
            for updated in touched.values() {
                tickets.insert(updated.clone());
            }
            tickets.insert(ticket.clone());
        }
        let _ = self.tx.send(Change::Created {
            ticket: ticket.clone(),
        });
        if let Some(parent_id) = &ticket.parent {
            let _ = self.tx.send(Change::ChildrenChanged {
                ticket: touched[parent_id].clone(),
                added: ticket.id.clone(),
            });
        }
        for blocker_id in unlocks_added {
            let _ = self.tx.send(Change::UnlocksChanged {
                ticket: touched[&blocker_id].clone(),
                added: ticket.id.clone(),
            });
        }
        Ok(ticket)
    }

    pub fn get(&self, id: &str) -> Option<Ticket> {
        self.tickets
            .read()
            .expect("store lock poisoned")
            .get(id)
            .cloned()
    }

    /// Tickets ordered by (epic, seq), optionally filtered. The `parent`
    /// filter (AGS-19) selects exactly that ticket's children. The
    /// unpaged shape, kept: [`Store::page`] is the same walk with a
    /// cursor and a limit (MNG-9).
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
    /// (plus the count of what remains), not a clone-and-sort of every
    /// ticket. `after` naming no ticket is [`StoreError::CursorNotFound`]:
    /// a cursor into nothing is a caller error, not an empty page.
    pub fn page(&self, q: PageQuery<'_>) -> Result<Page, StoreError> {
        let epic = q.epic.map(str::to_ascii_uppercase);
        let parent = q.parent.map(str::to_ascii_uppercase);
        let after = q.after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        let span = Span::of(&cache, after.as_deref(), epic.as_deref())?;
        // The epic is already spent — it bounded the span rather than
        // filtering inside it — so with no status or parent asked for,
        // there is nothing left to test on the way through.
        if q.status.is_none() && parent.is_none() {
            return Ok(Self::walk_cache(&cache, span, q.limit, Filter::All));
        }
        let keep = |t: &Ticket| {
            q.status.is_none_or(|s| t.status == s)
                && parent.as_deref().is_none_or(|p| t.parent.as_deref() == Some(p))
        };
        Ok(Self::walk_cache(&cache, span, q.limit, Filter::By(&keep)))
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

    /// The one walk both page methods run: the span, `limit` matches
    /// cloned into the page, then the rest of the span counted — because
    /// `remaining` is an exact number, not an estimate. That count is the
    /// O(what is left) the ticket's Risks section names and accepts;
    /// what keeps it cheap is that the span is already the right stretch
    /// of the order and its tickets are already in hand.
    fn walk_cache(cache: &Cache, span: Span, limit: Option<u32>, filter: Filter<'_>) -> Page {
        let take = limit.map_or(usize::MAX, |n| n as usize);
        let mut tickets = Vec::new();
        if let Some(n) = limit {
            tickets.reserve(n as usize);
        }
        let remaining = match (span.range(cache), filter) {
            (None, _) => 0,
            (Some(range), Filter::All) => {
                let mut it = range;
                tickets.extend(it.by_ref().take(take).map(|(_, t)| t.clone()));
                it.count()
            }
            (Some(range), Filter::By(keep)) => {
                let mut it = range.map(|(_, t)| t).filter(|t| keep(t));
                tickets.extend(it.by_ref().take(take).cloned());
                it.count()
            }
        } as u64;

        let next = if remaining == 0 {
            None
        } else {
            // The page's last id, or — for the degenerate empty page
            // (`limit: 0`) — the cursor it was cut with: either way, where
            // the next page resumes.
            tickets.last().map(|t| t.id.clone()).or(span.cursor)
        };
        Page {
            tickets,
            next,
            remaining,
        }
    }

    /// Changes the status, write-through: durable in the database before
    /// the cache is updated and subscribers are notified.
    pub async fn set_status(&self, id: &str, status: Status) -> Result<Change, StoreError> {
        let _serial = self.write_serial.lock().await;
        let mut ticket = self
            .get(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        let previous = ticket.status;
        ticket.status = status;
        ticket.updated_unix = chrono::Utc::now().timestamp();

        self.persist(vec![ticket.clone()]).await?;
        self.tickets
            .write()
            .expect("store lock poisoned")
            .insert(ticket.clone());
        let change = Change::StatusChanged { ticket, previous };
        let _ = self.tx.send(change.clone());
        Ok(change)
    }

    /// MNG-12: files an existing ticket as an output of `parent` — the
    /// verb AGS-19 left out, for adopting a ticket born parentless or
    /// moving one. Same guards as `create`: the parent must exist, a
    /// non-root's parent must be inside its own shortcode (MNG-14; the
    /// shortcode's scope root may be hosted anywhere), and the chain must
    /// stay acyclic (naming itself included); a rejection changes
    /// nothing. The child, the new parent (`children` gains the
    /// id) and the previous parent, if any (`children` loses it), commit
    /// in one transaction; then the cache; then, in order,
    /// `ParentChanged` (the child), `ChildrenChanged` (the new parent)
    /// and `ChildRemoved` (the old one). Setting the parent a ticket
    /// already has writes nothing and emits nothing: the returned
    /// `ParentChanged` then shows `previous == Some(parent)`.
    pub async fn set_parent(&self, id: &str, parent: &str) -> Result<Change, StoreError> {
        let id = id.trim().to_ascii_uppercase();
        let parent = parent.trim().to_ascii_uppercase();
        let _serial = self.write_serial.lock().await;
        let (child, new_parent, old_parent, previous) = {
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

            let mut new_parent = parent_ticket.clone();
            new_parent.children.push(id.clone());
            new_parent.updated_unix = now;

            // A previous parent that names no ticket (a hand-edited
            // database) has nothing to update; the child's edge still moves.
            let old_parent = previous.as_deref().and_then(|p| tickets.get(p)).map(|t| {
                let mut t = t.clone();
                t.children.retain(|c| c != &id);
                t.updated_unix = now;
                t
            });
            (child, new_parent, old_parent, previous)
        };

        let mut batch = vec![child.clone(), new_parent.clone()];
        batch.extend(old_parent.iter().cloned());
        self.persist(batch).await?;

        {
            let mut tickets = self.tickets.write().expect("store lock poisoned");
            tickets.insert(child.clone());
            tickets.insert(new_parent.clone());
            if let Some(old) = &old_parent {
                tickets.insert(old.clone());
            }
        }
        let change = Change::ParentChanged {
            ticket: child,
            previous,
        };
        let _ = self.tx.send(change.clone());
        let _ = self.tx.send(Change::ChildrenChanged {
            ticket: new_parent,
            added: id.clone(),
        });
        if let Some(old) = old_parent {
            let _ = self.tx.send(Change::ChildRemoved {
                ticket: old,
                removed: id,
            });
        }
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
            let mut max: HashMap<String, u32> = HashMap::new();
            for t in cache.tickets() {
                let e = max.entry(t.epic.clone()).or_insert(0);
                *e = (*e).max(t.seq);
            }
            max
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

/// PERF-2: the synthetic ticket generator. Private to the store: the
/// shape it produces mirrors what `create` + `set_status` would have
/// built one call at a time, so every invariant the DAG queries rely on
/// (ids, `blocked_by` -> `unlocks` symmetry, acyclicity) holds by
/// construction rather than by checking.
mod synthetic {
    use super::{Kind, SeedSpec, Severity, Status, Ticket};
    use std::collections::{HashMap, HashSet};

    /// Rows per write transaction: a few fsyncs for a million rows, and a
    /// WAL that gets checkpointed between chunks instead of growing to
    /// the size of the whole seed.
    pub(super) const ROWS_PER_TRANSACTION: usize = 50_000;

    /// SplitMix64 — tiny, well-distributed, and dependency-free; the
    /// point is reproducibility, not cryptography.
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: usize) -> usize {
            debug_assert!(n > 0);
            (self.next_u64() % n as u64) as usize
        }
        fn unit(&mut self) -> f64 {
            (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
        }
        fn chance(&mut self, percent: u32) -> bool {
            self.below(100) < percent as usize
        }
        fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
            &xs[self.below(xs.len())]
        }
        fn weighted<T: Copy>(&mut self, table: &[(T, u32)]) -> T {
            let total: u32 = table.iter().map(|(_, w)| w).sum();
            let mut roll = self.below(total as usize) as u32;
            for (v, w) in table {
                if roll < *w {
                    return *v;
                }
                roll -= w;
            }
            table[table.len() - 1].0
        }
    }

    // A realistic mix (weights are percentages of non-root tickets).
    const STATUSES: [(Status, u32); 7] = [
        (Status::Pending, 28),
        (Status::Ready, 18),
        (Status::Blocked, 8),
        (Status::Complete, 34),
        (Status::Idea, 6),
        (Status::Superseded, 4),
        (Status::Epic, 2),
    ];
    const KINDS: [(Kind, u32); 5] = [
        (Kind::Scope, 4),
        (Kind::Execution, 14),
        (Kind::Build, 52),
        (Kind::Gap, 16),
        (Kind::Spike, 14),
    ];
    const SEVERITIES: [(Option<Severity>, u32); 5] = [
        (None, 40),
        (Some(Severity::Medium), 24),
        (Some(Severity::High), 16),
        (Some(Severity::Low), 14),
        (Some(Severity::Critical), 6),
    ];

    const VERBS: [&str; 20] = [
        "Migrate", "Harden", "Wire", "Measure", "Split", "Cache", "Index", "Paginate",
        "Stream", "Validate", "Restyle", "Document", "Refuse", "Retry", "Bound", "Persist",
        "Render", "Resolve", "Throttle", "Audit",
    ];
    const OBJECTS: [&str; 20] = [
        "the watch stream", "id allocation", "the nav tree", "the board render",
        "startup load", "the REST bridge", "cycle detection", "the ticket page",
        "unlocks symmetry", "the seed path", "SSE reconnects", "the list payload",
        "the status control", "epic rollups", "the proxy preamble", "frontmatter folding",
        "diagram mounting", "the write-serial lock", "WAL checkpoints", "the health probe",
    ];
    const QUALIFIERS: [&str; 8] = [
        "under load", "at scale", "for the shell", "before serving",
        "on reconnect", "per epic", "without fsync", "in one transaction",
    ];
    const PROBLEMS: [&str; 6] = [
        "Today this path does the work one row at a time, which is fine at ten and hopeless at a million.",
        "The current behavior is correct but unobservable: nothing logs how long it took or how much it moved.",
        "A client that opens this before the snapshot lands can miss an event; the gap is small but real.",
        "The shape is right and the cost is wrong — every call re-derives what the previous call already knew.",
        "Two readers disagree about the order, and the tie-break lives in a comment nobody reads.",
        "The failure mode is a silent hang rather than a clear refusal, so the operator learns nothing.",
    ];
    const BEHAVIORS: [&str; 8] = [
        "One call, one transaction, one log line with count and elapsed time.",
        "Reads stay on the cache; nothing on this path touches storage.",
        "A refusal names the offending row and leaves the file untouched.",
        "The same input twice produces byte-identical output.",
        "Timeouts are recorded as failures, never as hangs.",
        "The wire surface does not grow; no new method, no new field.",
        "Every edge added is mirrored on the other side before the commit.",
        "The startup log reports what was loaded and how long it took.",
    ];
    const CRITERIA: [&str; 6] = [
        "`cargo test` passes with the new cases included.",
        "A sampled check agrees with the query's answer on a seeded store.",
        "The log line carries the count and the elapsed milliseconds.",
        "A second open of the same file finds the same rows.",
        "The measurement completes inside its budget at the target magnitude.",
        "The operator can reproduce the run from the recorded seed value.",
    ];

    /// Number of epics for `count` tickets: a few for a hundred, a couple
    /// of hundred for a million — a Zipf-weighted spread below, so the
    /// biggest epic is a real stress for "expand the largest group".
    fn epic_count(count: usize) -> usize {
        let k = (count as f64).powf(0.4).ceil() as usize;
        k.clamp(3, 300).min(count.max(1))
    }

    /// Pronounceable 3–4 letter uppercase codes (consonant/vowel
    /// alternation), unique within the run.
    fn epic_code(rng: &mut Rng, taken: &HashSet<String>) -> String {
        const C: &[u8] = b"BCDFGHKLMNPRSTVZ";
        const V: &[u8] = b"AEIOU";
        loop {
            let len = 3 + rng.below(2);
            let code: String = (0..len)
                .map(|i| {
                    let set = if i % 2 == 0 { C } else { V };
                    set[rng.below(set.len())] as char
                })
                .collect();
            if !taken.contains(&code) {
                return code;
            }
        }
    }

    fn title(rng: &mut Rng) -> String {
        let mut t = format!("{} {}", rng.pick(&VERBS), rng.pick(&OBJECTS));
        if rng.chance(40) {
            t.push(' ');
            t.push_str(rng.pick(&QUALIFIERS));
        }
        t
    }

    /// A short markdown body in the house shape (Problem / Required
    /// behavior / Acceptance criteria), ~400–600 bytes.
    fn body(rng: &mut Rng, id: &str, blocked_by: &[String]) -> String {
        let mut b = String::with_capacity(640);
        b.push_str("## Problem\n\n");
        b.push_str(rng.pick(&PROBLEMS));
        if let Some(first) = blocked_by.first() {
            b.push_str(&format!(" Blocked on {first} until it lands."));
        }
        b.push_str("\n\n## Required behavior\n\n");
        let n = 2 + rng.below(2);
        for _ in 0..n {
            b.push_str("- ");
            b.push_str(rng.pick(&BEHAVIORS));
            b.push('\n');
        }
        b.push_str("\n## Acceptance criteria\n\n");
        let n = 2 + rng.below(2);
        for i in 0..n {
            b.push_str(&format!("{}. {}\n", i + 1, rng.pick(&CRITERIA)));
        }
        b.push_str(&format!("\n_{id}: synthetic (PERF-2)._\n"));
        b
    }

    /// Generates `spec.count` tickets in creation order. `next_seq` is the
    /// per-epic maximum sequence already in the store (empty for a fresh
    /// one): a generated epic that collides with an existing one
    /// continues its numbering, as `create` would.
    pub(super) fn generate(spec: SeedSpec, existing_max: &HashMap<String, u32>) -> Vec<Ticket> {
        let mut rng = Rng(spec.seed ^ 0x5EED_0000_0000_0000);
        let count = spec.count;
        let mut out: Vec<Ticket> = Vec::with_capacity(count);
        if count == 0 {
            return out;
        }

        // Epics: Zipf-weighted (the biggest holds ~1/H(K) of everything).
        let k = epic_count(count);
        let mut codes: Vec<String> = Vec::with_capacity(k);
        let mut taken: HashSet<String> = HashSet::new();
        for _ in 0..k {
            let c = epic_code(&mut rng, &taken);
            taken.insert(c.clone());
            codes.push(c);
        }
        let mut cumulative: Vec<f64> = Vec::with_capacity(k);
        let mut acc = 0.0;
        for i in 0..k {
            acc += 1.0 / (i as f64 + 1.0);
            cumulative.push(acc);
        }
        let pick_epic = |rng: &mut Rng| -> usize {
            let r = rng.unit() * acc;
            cumulative.partition_point(|c| *c <= r).min(k - 1)
        };

        let mut seq: Vec<u32> = codes
            .iter()
            .map(|c| existing_max.get(c).copied().unwrap_or(0))
            .collect();
        // Per-epic creation-order index lists (for local blocker choice).
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); k];
        let mut index: HashMap<String, usize> = HashMap::with_capacity(count);

        const BASE_UNIX: i64 = 1_750_000_000; // 2025-06-15T13:20Z
        const STEP: i64 = 60;

        for i in 0..count {
            let e = pick_epic(&mut rng);
            seq[e] += 1;
            let epic = codes[e].clone();
            let id = format!("{epic}-{}", seq[e]);
            let root = members[e].is_empty();

            let (kind, status, severity, title_s) = if root {
                (
                    Kind::Scope,
                    Status::Epic,
                    None,
                    format!("{epic}: {}", title(&mut rng).to_lowercase()),
                )
            } else {
                (
                    rng.weighted(&KINDS),
                    rng.weighted(&STATUSES),
                    rng.weighted(&SEVERITIES),
                    title(&mut rng),
                )
            };

            // A modest fraction carries blockers: mostly recent siblings
            // in the same epic, sometimes anything created earlier
            // anywhere. Always earlier in creation order — acyclic.
            let mut blocked_by: Vec<String> = Vec::new();
            if !root && rng.chance(25) {
                let want = 1 + usize::from(rng.chance(30)) + usize::from(rng.chance(10));
                for _ in 0..want {
                    let candidate = if rng.chance(90) {
                        let mine = &members[e];
                        let window = mine.len().min(40);
                        mine.get(mine.len() - window + rng.below(window)).copied()
                    } else if i > 0 {
                        Some(rng.below(i))
                    } else {
                        None
                    };
                    if let Some(c) = candidate {
                        let bid = out[c].id.clone();
                        if !blocked_by.contains(&bid) {
                            blocked_by.push(bid);
                        }
                    }
                }
            }

            let created = BASE_UNIX + i as i64 * STEP;
            let updated = if status == Status::Pending {
                created
            } else {
                created + rng.below(30 * 86_400) as i64
            };
            let body_s = body(&mut rng, &id, &blocked_by);

            for bid in &blocked_by {
                let bi = index[bid];
                out[bi].unlocks.push(id.clone());
                out[bi].updated_unix = out[bi].updated_unix.max(created);
            }
            // MNG-14: the shape `create` would give it — every non-root
            // hangs under its epic's scope root, mirrored in `children`.
            let parent = if root {
                None
            } else {
                let ri = members[e][0];
                out[ri].children.push(id.clone());
                out[ri].updated_unix = out[ri].updated_unix.max(created);
                Some(out[ri].id.clone())
            };
            members[e].push(i);
            index.insert(id.clone(), i);
            out.push(Ticket {
                id,
                epic,
                seq: seq[e],
                title: title_s,
                status,
                kind,
                severity,
                blocked_by,
                unlocks: Vec::new(),
                parent,
                children: Vec::new(),
                body: body_s,
                created_unix: created,
                updated_unix: updated,
            });
        }
        out
    }
}

/// SQL for the system of record. Private: the rest of the application
/// only ever sees the cache above.
mod db {
    use super::Ticket;
    use rusqlite::types::Type;
    use rusqlite::{params, Connection};
    use std::collections::{HashMap, HashSet};
    use std::str::FromStr;

    pub(super) const SCHEMA: &str = "
        CREATE TABLE IF NOT EXISTS tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            kind         TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            parent       TEXT,
            children     TEXT NOT NULL DEFAULT '[]',
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

    /// The pre-AGS-13 column, and its vocabulary mapped onto kinds (the
    /// pinned migration). This is the only place the legacy vocabulary
    /// survives: it exists to be read once and rewritten, never written.
    const LEGACY_COLUMN: &str = "ticket_type";

    fn legacy_kind(ticket_type: &str) -> Option<&'static str> {
        match ticket_type {
            "epic" => Some("scope"),
            "implementation" => Some("build"),
            "analysis" => Some("spike"),
            "checkpoint" => Some("execution"),
            "spike" => Some("spike"),
            _ => None,
        }
    }

    pub(super) fn has_column(
        conn: &Connection,
        table: &str,
        column: &str,
    ) -> rusqlite::Result<bool> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for name in names {
            if name? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// One-shot, durable migration of a pre-AGS-13 database. If the
    /// `tickets` table still carries `ticket_type`, every row is checked
    /// against the pinned mapping first — one unmappable value refuses
    /// the open, naming the rows, with nothing changed — then, in a single
    /// transaction, the column is renamed to `kind` and each row's value
    /// is rewritten. Committed before the cache loads, so a restart (or a
    /// SIGKILL right after) finds kinds in the file, never the old
    /// values, and the check above is a no-op from then on. Returns how
    /// many rows were rewritten (0: nothing to migrate).
    pub(super) fn migrate_legacy_ticket_type(conn: &mut Connection) -> anyhow::Result<usize> {
        if !has_column(conn, "tickets", LEGACY_COLUMN)? {
            return Ok(0);
        }
        let tx = conn.transaction()?;
        let rows: Vec<(String, String)> = {
            let mut stmt = tx.prepare(&format!("SELECT id, {LEGACY_COLUMN} FROM tickets"))?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let unmappable: Vec<String> = rows
            .iter()
            .filter(|(_, tt)| legacy_kind(tt).is_none())
            .map(|(id, tt)| format!("{id} ({LEGACY_COLUMN} '{tt}')"))
            .collect();
        if !unmappable.is_empty() {
            anyhow::bail!(
                "refusing to open: {} ticket(s) carry a legacy {LEGACY_COLUMN} outside the pinned \
                 kind mapping (epic->scope, implementation->build, analysis->spike, \
                 checkpoint->execution, spike->spike): {}. Nothing was changed; fix those rows \
                 and reopen.",
                unmappable.len(),
                unmappable.join(", ")
            );
        }
        tx.execute(
            &format!("ALTER TABLE tickets RENAME COLUMN {LEGACY_COLUMN} TO kind"),
            [],
        )?;
        for (id, tt) in &rows {
            let kind = legacy_kind(tt).expect("checked above");
            tx.execute("UPDATE tickets SET kind = ?1 WHERE id = ?2", params![kind, id])?;
        }
        tx.commit()?;
        tracing::info!(
            "migrated {} tickets: {LEGACY_COLUMN} -> kind (AGS-13), durably",
            rows.len()
        );
        Ok(rows.len())
    }

    /// AGS-19: the `parent:` line of a body's YAML frontmatter, if the
    /// body opens with a frontmatter block and carries one. The body is
    /// the raw record (AGS-2 decides canonicality); this reads it, never
    /// writes it.
    pub(super) fn frontmatter_parent(body: &str) -> Option<String> {
        let mut lines = body.lines();
        if lines.next()?.trim_end() != "---" {
            return None;
        }
        for line in lines {
            let line = line.trim_end();
            if line == "---" {
                break;
            }
            if let Some(value) = line.strip_prefix("parent:") {
                let value = value.trim().trim_matches('"').trim();
                if value.is_empty() || value == "[]" || value == "null" {
                    return None;
                }
                return Some(value.to_ascii_uppercase());
            }
        }
        None
    }

    /// Ticket ids in (epic, seq) order — the order `create` would have
    /// appended them in, so a backfilled `children` list reads like a
    /// grown one. An id that is not `EPIC-N` sorts last, by name.
    fn by_epic_seq(id: &str) -> (String, u32, String) {
        match id.rsplit_once('-').and_then(|(e, n)| {
            n.parse::<u32>().ok().map(|n| (e.to_string(), n))
        }) {
            Some((epic, seq)) => (epic, seq, id.to_string()),
            None => (id.to_string(), u32::MAX, id.to_string()),
        }
    }

    /// True if making `parent` the parent of `child` would close a loop
    /// in `edges` (child -> parent), `child == parent` included.
    fn parent_edge_loops(edges: &HashMap<String, String>, child: &str, parent: &str) -> bool {
        let mut current = parent;
        for _ in 0..=edges.len() {
            if current == child {
                return true;
            }
            match edges.get(current) {
                Some(next) => current = next,
                None => return false,
            }
        }
        true
    }

    /// One-shot, durable migration of a pre-AGS-19 database (AGS-19).
    ///
    /// If the `tickets` table has no `parent` column, this adds `parent`
    /// and `children` and backfills the typed parent from each body's
    /// frontmatter `parent: <id>`, in one transaction: an id that names
    /// an existing ticket is written through (and mirrored in that
    /// ticket's `children`), logged per ticket; one that resolves to
    /// nothing — or that would make a ticket its own ancestor — is
    /// logged and left empty. Never fatal: an unresolvable reference is a
    /// dangling note in prose, not a corrupt store, and a later create
    /// can still establish the relationship.
    ///
    /// Committed before the cache loads, so a restart (or a SIGKILL right
    /// after) finds the typed parents in the file; the column check above
    /// makes every later open a no-op, which also keeps startup off the
    /// bodies of a million synthetic rows. Returns how many parents were
    /// backfilled (0: nothing to migrate).
    pub(super) fn migrate_add_parenthood(conn: &mut Connection) -> anyhow::Result<usize> {
        if has_column(conn, "tickets", "parent")? {
            return Ok(0);
        }
        let tx = conn.transaction()?;
        tx.execute("ALTER TABLE tickets ADD COLUMN parent TEXT", [])?;
        tx.execute(
            "ALTER TABLE tickets ADD COLUMN children TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;

        let rows: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT id, body FROM tickets")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let known: HashSet<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
        let mut proposed: Vec<(String, String)> = rows
            .iter()
            .filter_map(|(id, body)| frontmatter_parent(body).map(|p| (id.clone(), p)))
            .collect();
        proposed.sort_by_key(|(id, _)| by_epic_seq(id));

        let mut edges: HashMap<String, String> = HashMap::new();
        for (id, parent) in proposed {
            if !known.contains(parent.as_str()) {
                tracing::warn!(
                    "AGS-19 migration: {id} names parent '{parent}' in its frontmatter, \
                     which is not a ticket — left empty"
                );
                continue;
            }
            if parent_edge_loops(&edges, &id, &parent) {
                tracing::warn!(
                    "AGS-19 migration: {id} -> parent '{parent}' would make a ticket its own \
                     ancestor — left empty"
                );
                continue;
            }
            tracing::info!("AGS-19 migration: {id}.parent = {parent} (from body frontmatter)");
            edges.insert(id, parent);
        }

        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        for (child, parent) in &edges {
            children.entry(parent.as_str()).or_default().push(child);
        }
        for (child, parent) in &edges {
            tx.execute(
                "UPDATE tickets SET parent = ?1 WHERE id = ?2",
                params![parent, child],
            )?;
        }
        for (parent, mut kids) in children {
            kids.sort_by_key(|id| by_epic_seq(id));
            tx.execute(
                "UPDATE tickets SET children = ?1 WHERE id = ?2",
                params![
                    serde_json::to_string(&kids).expect("serialize ids"),
                    parent
                ],
            )?;
        }
        tx.commit()?;
        tracing::info!(
            "AGS-19: added parent/children to {} tickets, backfilling {} typed parent(s) \
             from body frontmatter, durably",
            rows.len(),
            edges.len()
        );
        Ok(edges.len())
    }

    /// Serializes a unit enum to its serde string form (`Pending`,
    /// `build`, `Critical`) — the same casing the wire uses, and what the
    /// `FromStr` impls parse back.
    pub(super) fn enum_str<T: serde::Serialize>(v: &T) -> String {
        match serde_json::to_value(v) {
            Ok(serde_json::Value::String(s)) => s,
            other => unreachable!("unit enum must serialize to a string, got {other:?}"),
        }
    }

    fn parse_col<T: FromStr>(idx: usize, s: &str) -> rusqlite::Result<T>
    where
        T::Err: std::fmt::Display,
    {
        s.parse().map_err(|e: T::Err| {
            rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, format!("{e}").into())
        })
    }

    fn parse_ids(idx: usize, s: &str) -> rusqlite::Result<Vec<String>> {
        serde_json::from_str(s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, format!("{e}").into())
        })
    }

    /// Inserts or replaces one ticket. A single implicit transaction:
    /// when this returns Ok, the row is durable.
    pub(super) fn upsert(conn: &Connection, t: &Ticket) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO tickets
                 (id, epic, seq, title, status, kind, severity,
                  blocked_by, unlocks, parent, children, body,
                  created_unix, updated_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (id) DO UPDATE SET
                 epic = excluded.epic,
                 seq = excluded.seq,
                 title = excluded.title,
                 status = excluded.status,
                 kind = excluded.kind,
                 severity = excluded.severity,
                 blocked_by = excluded.blocked_by,
                 unlocks = excluded.unlocks,
                 parent = excluded.parent,
                 children = excluded.children,
                 body = excluded.body,
                 created_unix = excluded.created_unix,
                 updated_unix = excluded.updated_unix",
            params![
                t.id,
                t.epic,
                t.seq,
                t.title,
                enum_str(&t.status),
                enum_str(&t.kind),
                t.severity.as_ref().map(enum_str),
                serde_json::to_string(&t.blocked_by).expect("serialize ids"),
                serde_json::to_string(&t.unlocks).expect("serialize ids"),
                t.parent,
                serde_json::to_string(&t.children).expect("serialize ids"),
                t.body,
                t.created_unix,
                t.updated_unix,
            ],
        )?;
        Ok(())
    }

    /// PERF-2: inserts many fresh rows inside the caller's transaction
    /// through one prepared statement. Plain INSERT (no upsert): seeded
    /// ids are allocated above existing maxima, so a conflict here would
    /// be a generator bug and must surface, not overwrite.
    pub(super) fn insert_many(conn: &Connection, tickets: &[Ticket]) -> rusqlite::Result<()> {
        let mut stmt = conn.prepare_cached(
            "INSERT INTO tickets
                 (id, epic, seq, title, status, kind, severity,
                  blocked_by, unlocks, parent, children, body,
                  created_unix, updated_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )?;
        for t in tickets {
            stmt.execute(params![
                t.id,
                t.epic,
                t.seq,
                t.title,
                enum_str(&t.status),
                enum_str(&t.kind),
                t.severity.as_ref().map(enum_str),
                serde_json::to_string(&t.blocked_by).expect("serialize ids"),
                serde_json::to_string(&t.unlocks).expect("serialize ids"),
                t.parent,
                serde_json::to_string(&t.children).expect("serialize ids"),
                t.body,
                t.created_unix,
                t.updated_unix,
            ])?;
        }
        Ok(())
    }

    pub(super) fn load_all(conn: &Connection) -> anyhow::Result<HashMap<String, Ticket>> {
        let mut stmt = conn.prepare(
            "SELECT id, epic, seq, title, status, kind, severity,
                    blocked_by, unlocks, body, created_unix, updated_unix,
                    parent, children
             FROM tickets",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Ticket {
                id: row.get(0)?,
                epic: row.get(1)?,
                seq: row.get(2)?,
                title: row.get(3)?,
                status: parse_col(4, &row.get::<_, String>(4)?)?,
                kind: parse_col(5, &row.get::<_, String>(5)?)?,
                severity: row
                    .get::<_, Option<String>>(6)?
                    .as_deref()
                    .map(|s| parse_col(6, s))
                    .transpose()?,
                blocked_by: parse_ids(7, &row.get::<_, String>(7)?)?,
                unlocks: parse_ids(8, &row.get::<_, String>(8)?)?,
                body: row.get(9)?,
                created_unix: row.get(10)?,
                updated_unix: row.get(11)?,
                parent: row.get::<_, Option<String>>(12)?,
                children: parse_ids(
                    13,
                    row.get::<_, Option<String>>(13)?.as_deref().unwrap_or("[]"),
                )?,
            })
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let ticket = row?;
            out.insert(ticket.id.clone(), ticket);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_ticket(epic: &str, title: &str) -> NewTicket {
        NewTicket {
            epic: epic.into(),
            title: title.into(),
            kind: Kind::Build,
            severity: None,
            blocked_by: Vec::new(),
            parent: None,
            body: String::new(),
        }
    }

    /// `new_ticket` as a shortcode's scope root — every prefix starts
    /// with one (MNG-12).
    fn scope(epic: &str, title: &str) -> NewTicket {
        NewTicket {
            kind: Kind::Scope,
            ..new_ticket(epic, title)
        }
    }

    /// `new_ticket`, filed as an output of `parent`.
    fn child_of(epic: &str, title: &str, parent: &str) -> NewTicket {
        NewTicket {
            parent: Some(parent.into()),
            ..new_ticket(epic, title)
        }
    }

    fn ids(page: &Page) -> Vec<&str> {
        page.tickets.iter().map(|t| t.id.as_str()).collect()
    }

    /// MNG-9: the sorted index is the order `list` always had — walking it
    /// in pages of one, from no cursor to a null `next`, visits every
    /// ticket exactly once, in `(epic, seq)` order, and `remaining` counts
    /// down exactly. Ids are created out of order on purpose, and one epic
    /// passes 9 so the difference between numeric `seq` and a string
    /// compare on the id shows (`A-10` must follow `A-9`, not `A-1`).
    #[tokio::test]
    async fn the_index_pages_the_whole_store_in_order_exactly_once() {
        let store = Store::new();
        store.create(scope("A", "a")).await.unwrap(); // MNG-12: the root first
        for _ in 0..10 {
            store.create(new_ticket("A", "a")).await.unwrap();
        }
        store.create(scope("B", "b")).await.unwrap();
        store.create(scope("AB", "ab")).await.unwrap();

        let all: Vec<String> = store.list(None, None, None).iter().map(|t| t.id.clone()).collect();
        assert_eq!(
            all,
            ["A-1", "A-2", "A-3", "A-4", "A-5", "A-6", "A-7", "A-8", "A-9", "A-10", "A-11",
             "AB-1", "B-1"],
            "seq is numeric (A-10 after A-9), epic is lexicographic"
        );

        let mut walked: Vec<String> = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = store
                .page(PageQuery {
                    after: after.as_deref(),
                    limit: Some(1),
                    ..PageQuery::default()
                })
                .unwrap();
            assert_eq!(page.remaining as usize, all.len() - walked.len() - page.tickets.len());
            walked.extend(page.tickets.iter().map(|t| t.id.clone()));
            match page.next {
                Some(next) => {
                    assert_eq!(Some(&next), walked.last(), "the cursor is the last row");
                    after = Some(next);
                }
                None => break,
            }
        }
        assert_eq!(walked, all, "one page at a time == the whole list, in order");
    }

    /// MNG-9: `after` composes with the filters, `remaining` counts under
    /// them, and a cursor naming no ticket is the cursor error (not an
    /// empty page) — the message the wire shows.
    #[tokio::test]
    async fn a_page_composes_with_filters_and_a_bad_cursor_is_an_error() {
        let store = Store::new();
        for i in 0..6 {
            let nt = if i == 0 { scope("PG", "p") } else { new_ticket("PG", "p") };
            let id = store.create(nt).await.unwrap().id;
            if i % 2 == 0 {
                store.set_status(&id, Status::Ready).await.unwrap();
            }
        }
        store.create(scope("ZZ", "z")).await.unwrap();

        let page = store
            .page(PageQuery {
                status: Some(Status::Ready),
                after: Some("pg-1"),
                limit: Some(1),
                ..PageQuery::default()
            })
            .unwrap();
        assert_eq!(ids(&page), ["PG-3"], "the cursor is case-insensitive, like every id");
        assert_eq!((page.next.as_deref(), page.remaining), (Some("PG-3"), 1));

        let page = store
            .page(PageQuery {
                epic: Some("PG"),
                after: Some("PG-6"),
                ..PageQuery::default()
            })
            .unwrap();
        assert!(page.tickets.is_empty(), "ZZ-1 is past the epic filter");
        assert_eq!((page.next, page.remaining), (None, 0));

        match store.page(PageQuery {
            after: Some("ZZZ-9"),
            ..PageQuery::default()
        }) {
            Err(StoreError::CursorNotFound(id)) => {
                assert_eq!(id, "ZZZ-9");
                assert_eq!(
                    StoreError::CursorNotFound(id).to_string(),
                    "after: no ticket ZZZ-9"
                );
            }
            other => panic!("expected CursorNotFound, got {other:?}"),
        }
    }

    /// MNG-9: an epic is a *position*, not a predicate — `(epic, seq)`
    /// puts an epic's tickets in one contiguous stretch of the index, so
    /// an epic-filtered page is bounded by that stretch. The cursor can
    /// sit below it, inside it, or past it; all three answer correctly.
    #[tokio::test]
    async fn an_epic_filter_bounds_the_walk_wherever_the_cursor_sits() {
        let store = Store::new();
        for epic in ["AA", "MM", "ZZ"] {
            store.create(scope(epic, "t")).await.unwrap();
            for _ in 0..2 {
                store.create(new_ticket(epic, "t")).await.unwrap();
            }
        }
        let epic_page = |after: Option<&'static str>, limit| {
            store.page(PageQuery {
                epic: Some("MM"),
                after,
                limit,
                ..PageQuery::default()
            })
        };

        // Cursor below the epic: the epic's own lower bound stands.
        let page = epic_page(Some("AA-2"), Some(2)).unwrap();
        assert_eq!(ids(&page), ["MM-1", "MM-2"]);
        assert_eq!((page.next.as_deref(), page.remaining), (Some("MM-2"), 1));

        // Cursor inside the epic: it wins.
        let page = epic_page(Some("MM-2"), None).unwrap();
        assert_eq!(ids(&page), ["MM-3"]);
        assert_eq!((page.next, page.remaining), (None, 0));

        // Cursor past the epic: nothing is left, and nothing from ZZ leaks.
        let page = epic_page(Some("ZZ-1"), None).unwrap();
        assert!(page.tickets.is_empty());
        assert_eq!((page.next, page.remaining), (None, 0));

        // And the whole epic, uncursored, is still just the epic.
        let page = epic_page(None, None).unwrap();
        assert_eq!(ids(&page), ["MM-1", "MM-2", "MM-3"]);

        // A cursor that names nothing still fails, epic or no epic.
        assert!(matches!(
            epic_page(Some("QQ-1"), None),
            Err(StoreError::CursorNotFound(_))
        ));
    }

    /// MNG-9: a ticket created above a walk's cursor is visited by the
    /// walk (the index is live — the cursor is a position in the order,
    /// not a snapshot); one below it is not, which is what `watch` is for.
    #[tokio::test]
    async fn a_ticket_born_above_the_cursor_is_still_walked() {
        let store = Store::new();
        store.create(scope("MID", "m")).await.unwrap();
        for _ in 0..2 {
            store.create(new_ticket("MID", "m")).await.unwrap();
        }
        store.create(scope("AAA", "below")).await.unwrap();

        let first = store
            .page(PageQuery {
                limit: Some(2),
                ..PageQuery::default()
            })
            .unwrap();
        assert_eq!(ids(&first), ["AAA-1", "MID-1"]);

        // Mid-walk: one above the cursor (MID-4), one below it (AAA-2).
        store.create(new_ticket("MID", "above")).await.unwrap();
        store.create(new_ticket("AAA", "below again")).await.unwrap();

        let rest = store
            .page(PageQuery {
                after: first.next.as_deref(),
                ..PageQuery::default()
            })
            .unwrap();
        assert_eq!(ids(&rest), ["MID-2", "MID-3", "MID-4"], "MID-4 joins the walk");
        assert_eq!((rest.next, rest.remaining), (None, 0));
    }

    /// MNG-9: `ready_set` pages the same way, on the ready-set membership
    /// rule — blocked and non-Ready tickets are not in the order it walks.
    #[tokio::test]
    async fn the_ready_set_pages_on_its_own_membership() {
        let store = Store::new();
        for i in 0..4 {
            let nt = if i == 0 { scope("RD", "r") } else { new_ticket("RD", "r") };
            let id = store.create(nt).await.unwrap().id;
            store.set_status(&id, Status::Ready).await.unwrap();
        }
        store
            .create(NewTicket {
                blocked_by: vec!["RD-1".into()],
                ..new_ticket("RD", "blocked")
            })
            .await
            .unwrap();
        store.set_status("RD-5", Status::Ready).await.unwrap();

        let page = store.ready_page(None, None, Some(2)).unwrap();
        assert_eq!(ids(&page), ["RD-1", "RD-2"]);
        assert_eq!((page.next.as_deref(), page.remaining), (Some("RD-2"), 2));

        let page = store.ready_page(None, page.next.as_deref(), Some(2)).unwrap();
        assert_eq!(ids(&page), ["RD-3", "RD-4"], "RD-5 is blocked by RD-1");
        assert_eq!((page.next, page.remaining), (None, 0));
    }

    /// The store's schema as it was before AGS-13 (`ticket_type`), so a
    /// test can build "a database created before this ticket" by hand.
    const LEGACY_SCHEMA: &str = "
        CREATE TABLE tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            ticket_type  TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

    /// Writes a pre-AGS-13 database at `path` holding one row per
    /// `(seq, ticket_type)` in `rows`, epic `LEG`, bodies with frontmatter.
    fn write_legacy_db(path: &Path, rows: &[(u32, &str)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(LEGACY_SCHEMA).unwrap();
        for (seq, tt) in rows {
            conn.execute(
                "INSERT INTO tickets VALUES (?1, 'LEG', ?2, ?3, 'Pending', ?4, NULL, '[]', '[]', ?5, 1, 1)",
                rusqlite::params![
                    format!("LEG-{seq}"),
                    seq,
                    format!("legacy {tt}"),
                    tt,
                    format!("---\nid: LEG-{seq}\ntype: {tt}\n---\n\nbody"),
                ],
            )
            .unwrap();
        }
    }

    fn columns(path: &Path) -> Vec<String> {
        let conn = Connection::open(path).unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(tickets)").unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// AGS-13 criterion 1 at the store level: every legacy value maps per
    /// the pinned table, the rows are rewritten in the file (not remapped
    /// per load), and the body's frontmatter is left exactly as it was.
    #[tokio::test]
    async fn legacy_ticket_type_migrates_durably_on_first_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        write_legacy_db(
            &path,
            &[
                (1, "epic"),
                (2, "implementation"),
                (3, "analysis"),
                (4, "checkpoint"),
                (5, "spike"),
            ],
        );
        assert!(columns(&path).contains(&"ticket_type".to_string()));

        let expected = [
            ("LEG-1", Kind::Scope),
            ("LEG-2", Kind::Build),
            ("LEG-3", Kind::Spike),
            ("LEG-4", Kind::Execution),
            ("LEG-5", Kind::Spike),
        ];
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.count(), 5);
            for (id, kind) in expected {
                let t = store.get(id).unwrap();
                assert_eq!(t.kind, kind, "{id}");
                assert!(t.body.contains("type: "), "{id}: frontmatter untouched");
            }
        }

        // The file itself now speaks kinds: column renamed, values rewritten.
        let cols = columns(&path);
        assert!(cols.contains(&"kind".to_string()), "{cols:?}");
        assert!(!cols.contains(&"ticket_type".to_string()), "{cols:?}");
        {
            let conn = Connection::open(&path).unwrap();
            let raw: Vec<(String, String)> = conn
                .prepare("SELECT id, kind FROM tickets ORDER BY seq")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                raw,
                vec![
                    ("LEG-1".into(), "scope".into()),
                    ("LEG-2".into(), "build".into()),
                    ("LEG-3".into(), "spike".into()),
                    ("LEG-4".into(), "execution".into()),
                    ("LEG-5".into(), "spike".into()),
                ]
            );
            assert!(!db::has_column(&conn, "tickets", "ticket_type").unwrap());
        }

        // Second open: nothing to migrate, same kinds, still writable.
        let store = Store::open(&path).unwrap();
        for (id, kind) in expected {
            assert_eq!(store.get(id).unwrap().kind, kind, "{id} after reopen");
        }
        let next = store.create(new_ticket("LEG", "born after the migration")).await.unwrap();
        assert_eq!(next.id, "LEG-6");
        assert_eq!(next.kind, Kind::Build);
    }

    /// A legacy value outside the pinned mapping refuses the open,
    /// naming the row, and leaves the database exactly as it was.
    #[test]
    fn legacy_value_outside_the_mapping_fails_the_open_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("odd.db");
        write_legacy_db(&path, &[(1, "implementation"), (2, "wibble")]);

        let err = match Store::open(&path) {
            Ok(_) => panic!("an unmappable legacy value must refuse the open"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("LEG-2"), "names the row: {err}");
        assert!(err.contains("'wibble'"), "names the value: {err}");
        assert!(err.contains("implementation->build"), "shows the mapping: {err}");

        // Untouched: the legacy column and both values are still there.
        let cols = columns(&path);
        assert!(cols.contains(&"ticket_type".to_string()), "{cols:?}");
        assert!(!cols.contains(&"kind".to_string()), "{cols:?}");
        let conn = Connection::open(&path).unwrap();
        let tt: String = conn
            .query_row("SELECT ticket_type FROM tickets WHERE id = 'LEG-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tt, "implementation", "the good row was not rewritten either");
    }

    // ---- AGS-19: parenthood --------------------------------------------

    /// The store's schema as it was before AGS-19 (no `parent`, no
    /// `children`), so a test can build "a database created before this
    /// ticket" by hand — the black-box counterpart is
    /// `tests/fixtures/pre-ags19.db`, written by the pre-AGS-19 binary.
    const PRE_AGS19_SCHEMA: &str = "
        CREATE TABLE tickets (
            id           TEXT PRIMARY KEY,
            epic         TEXT NOT NULL,
            seq          INTEGER NOT NULL,
            title        TEXT NOT NULL,
            status       TEXT NOT NULL,
            kind         TEXT NOT NULL,
            severity     TEXT,
            blocked_by   TEXT NOT NULL,
            unlocks      TEXT NOT NULL,
            body         TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL,
            UNIQUE (epic, seq)
        );
    ";

    /// Writes a pre-AGS-19 database at `path`, epic `OLD`, one row per
    /// `(seq, frontmatter parent)` — `None` writes a body with no
    /// frontmatter at all.
    fn write_pre_ags19_db(path: &Path, rows: &[(u32, Option<&str>)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(PRE_AGS19_SCHEMA).unwrap();
        for (seq, parent) in rows {
            let body = match parent {
                Some(p) => format!(
                    "---\nid: OLD-{seq}\ntitle: \"old {seq}\"\nstatus: Ready\nparent: {p}\n---\n\nbody"
                ),
                None => format!("no frontmatter, just prose for OLD-{seq}"),
            };
            conn.execute(
                "INSERT INTO tickets VALUES (?1, 'OLD', ?2, ?3, 'Pending', 'build', NULL, '[]', '[]', ?4, 1, 1)",
                rusqlite::params![format!("OLD-{seq}"), seq, format!("old {seq}"), body],
            )
            .unwrap();
        }
    }

    /// Acceptance 1 at the store level: a child names its parent, the
    /// parent gains it in `children`, a subscriber sees the parent's
    /// change as `ChildrenChanged` carrying the full ticket, and both
    /// edges are durable.
    #[tokio::test]
    async fn a_childs_birth_updates_its_parent_and_is_broadcast() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("family.db");
        let store = Store::open(&path).unwrap();
        let parent = store.create(scope("FAM", "the parent")).await.unwrap();
        assert_eq!(parent.parent, None);
        assert!(parent.children.is_empty());

        let mut rx = store.subscribe();
        let child = store
            .create(child_of("FAM", "the output", "FAM-1"))
            .await
            .unwrap();
        assert_eq!(child.parent.as_deref(), Some("FAM-1"));
        assert!(child.children.is_empty());

        match rx.recv().await.unwrap() {
            Change::Created { ticket } => assert_eq!(ticket.id, "FAM-2"),
            other => panic!("expected Created, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            Change::ChildrenChanged { ticket, added } => {
                assert_eq!(ticket.id, "FAM-1");
                assert_eq!(added, "FAM-2");
                // The full ticket, not just the delta.
                assert_eq!(ticket.children, vec!["FAM-2"]);
                assert_eq!(ticket.title, "the parent");
                assert_eq!(ticket.status, Status::Pending);
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        assert_eq!(store.get("FAM-1").unwrap().children, vec!["FAM-2"]);

        // Durable: a reopen finds both halves of the edge.
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.get("FAM-1").unwrap().children, vec!["FAM-2"]);
        assert_eq!(store.get("FAM-2").unwrap().parent.as_deref(), Some("FAM-1"));
    }

    /// Acceptance 2 at the store level: a parent that names no ticket is
    /// refused with the reason and nothing is stored; self-ancestry is
    /// impossible by construction (the allocated id cannot already be an
    /// ancestor, and there is no re-parent method), and a 3-deep chain —
    /// crossing epics — reads back correctly.
    #[tokio::test]
    async fn nonexistent_parent_is_refused_and_a_three_deep_chain_reads_back() {
        let store = Store::new();
        // MNG-12: the shortcode's scope root comes first, so the orphan
        // would be CHAIN-2.
        store.create(scope("CHAIN", "root")).await.unwrap();
        let err = store
            .create(child_of("CHAIN", "orphan", "CHAIN-9"))
            .await
            .unwrap_err();
        match &err {
            StoreError::ParentNotFound { id, parent } => {
                assert_eq!(id, "CHAIN-2");
                assert_eq!(parent, "CHAIN-9");
            }
            other => panic!("expected ParentNotFound, got {other:?}"),
        }
        assert!(err.to_string().contains("CHAIN-9"), "{err}");
        assert!(err.to_string().contains("must exist"), "{err}");
        assert_eq!(store.count(), 1, "nothing was created");

        // A ticket cannot name itself: the id is allocated by this very
        // create, so it cannot exist yet — the store refuses it as a
        // missing parent, before any cycle can be formed. The id stays
        // unallocated: the next successful create still takes CHAIN-2.
        let err = store
            .create(child_of("CHAIN", "myself", "CHAIN-2"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, StoreError::ParentNotFound { .. }),
            "self-parenthood is impossible by construction, got {err:?}"
        );

        // Three deep, the middle link in another epic (its scope root).
        store
            .create(NewTicket {
                kind: Kind::Scope,
                ..child_of("OTHER", "middle", "CHAIN-1")
            })
            .await
            .unwrap();
        // MNG-14: a non-root stays inside its shortcode — the leaf under
        // OTHER-1 is OTHER-2, and a CHAIN ticket naming OTHER-1 is refused.
        let err = store
            .create(child_of("CHAIN", "leaf", "OTHER-1"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, StoreError::ParentOutsideShortcode { .. }),
            "cross-shortcode parent on a non-root, got {err:?}"
        );
        assert!(err.to_string().contains("OTHER-1") && err.to_string().contains("outside"), "{err}");
        store
            .create(child_of("OTHER", "leaf", "OTHER-1"))
            .await
            .unwrap();
        assert_eq!(store.get("CHAIN-1").unwrap().parent, None);
        assert_eq!(store.get("CHAIN-1").unwrap().children, vec!["OTHER-1"]);
        assert_eq!(store.get("OTHER-1").unwrap().parent.as_deref(), Some("CHAIN-1"));
        assert_eq!(store.get("OTHER-1").unwrap().children, vec!["OTHER-2"]);
        assert_eq!(store.get("OTHER-2").unwrap().parent.as_deref(), Some("OTHER-1"));
        assert!(store.get("OTHER-2").unwrap().children.is_empty());

        // And the guard itself: an ancestry walk that would come back
        // around is caught before anything persists.
        let tickets = Cache::new(
            store
                .list(None, None, None)
                .into_iter()
                .map(|t| (t.id.clone(), t))
                .collect(),
        );
        assert_eq!(
            find_parent_cycle(&tickets, "CHAIN-1", "OTHER-2"),
            Some(vec!["OTHER-2".into(), "OTHER-1".into(), "CHAIN-1".into()]),
            "a chain that reaches the ticket itself is a cycle"
        );
        assert_eq!(find_parent_cycle(&tickets, "CHAIN-3", "OTHER-2"), None);
    }

    /// A ticket that is both the parent and a blocker of the newborn is
    /// updated once and carries both edges — the two events that follow
    /// both show the fully updated ticket (PERF-4 is exactly this shape).
    #[tokio::test]
    async fn a_parent_that_is_also_a_blocker_gets_both_edges() {
        let store = Store::new();
        store.create(scope("BOTH", "the parent")).await.unwrap();
        let mut rx = store.subscribe();
        let child = store
            .create(NewTicket {
                blocked_by: vec!["BOTH-1".into()],
                ..child_of("BOTH", "report", "BOTH-1")
            })
            .await
            .unwrap();
        assert_eq!(child.parent.as_deref(), Some("BOTH-1"));
        assert_eq!(child.blocked_by, vec!["BOTH-1"]);

        let parent = store.get("BOTH-1").unwrap();
        assert_eq!(parent.children, vec!["BOTH-2"]);
        assert_eq!(parent.unlocks, vec!["BOTH-2"], "neither edge overwrote the other");

        assert!(matches!(rx.recv().await.unwrap(), Change::Created { .. }));
        match rx.recv().await.unwrap() {
            Change::ChildrenChanged { ticket, .. } => assert_eq!(ticket, parent),
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            Change::UnlocksChanged { ticket, .. } => assert_eq!(ticket, parent),
            other => panic!("expected UnlocksChanged, got {other:?}"),
        }
    }

    /// Acceptance 4 at the store level: `list` filtered by parent returns
    /// exactly that ticket's children, ordered (epic, seq), and composes
    /// with the other filters.
    #[tokio::test]
    async fn list_filters_by_parent() {
        let store = Store::new();
        store.create(scope("LP", "parent")).await.unwrap();
        store.create(new_ticket("LP", "unrelated")).await.unwrap();
        store.create(child_of("LP", "child a", "LP-1")).await.unwrap();
        store
            .create(NewTicket {
                kind: Kind::Scope,
                ..child_of("ZZ", "child b", "LP-1")
            })
            .await
            .unwrap();
        store.create(child_of("LP", "grandchild", "LP-3")).await.unwrap();
        store.set_status("LP-3", Status::Ready).await.unwrap();

        // MNG-14: "unrelated" (LP-2) named no parent, so it defaulted to
        // the shortcode root and is LP-1's child too.
        let ids = |ts: Vec<Ticket>| ts.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(
            ids(store.list(None, None, Some("LP-1"))),
            vec!["LP-2", "LP-3", "ZZ-1"],
            "exactly LP-1's children, ordered (epic, seq)"
        );
        assert_eq!(ids(store.list(None, None, Some("lp-1"))), vec!["LP-2", "LP-3", "ZZ-1"]);
        assert_eq!(ids(store.list(None, None, Some("LP-3"))), vec!["LP-4"]);
        assert!(store.list(None, None, Some("LP-2")).is_empty(), "no children");
        assert!(store.list(None, None, Some("NOPE-1")).is_empty());
        // Composes with the existing filters.
        assert_eq!(ids(store.list(Some("LP"), None, Some("LP-1"))), vec!["LP-2", "LP-3"]);
        assert_eq!(
            ids(store.list(None, Some(Status::Ready), Some("LP-1"))),
            vec!["LP-3"]
        );
        assert_eq!(store.list(None, None, None).len(), 5, "unfiltered is unchanged");
    }

    /// Acceptance 3 at the store level: a pre-AGS-19 database gains the
    /// columns and its typed parents on first open, from body
    /// frontmatter — resolvable ones written through (and mirrored in
    /// `children`), an unresolvable one left empty, a frontmatter cycle
    /// refused, a body without frontmatter untouched — and the rewrite is
    /// durable, once. Nothing about it is fatal.
    #[tokio::test]
    async fn pre_ags19_database_backfills_typed_parents_from_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        write_pre_ags19_db(
            &path,
            &[
                (1, None),            // no frontmatter at all
                (2, Some("OLD-1")),   // resolvable
                (3, Some("OLD-99")),  // names nothing: left empty
                (4, Some("OLD-2")),   // resolvable, three deep
                (5, Some("OLD-6")),   // a two-cycle with OLD-6 …
                (6, Some("OLD-5")),   // … one half of which is refused
            ],
        );
        assert!(!columns(&path).contains(&"parent".to_string()));

        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.count(), 6);
            let p = |id: &str| store.get(id).unwrap().parent;
            assert_eq!(p("OLD-1"), None);
            assert_eq!(p("OLD-2").as_deref(), Some("OLD-1"));
            assert_eq!(p("OLD-3"), None, "an unresolvable parent is left empty");
            assert_eq!(p("OLD-4").as_deref(), Some("OLD-2"));
            // The chain is acyclic: the first half of the OLD-5/OLD-6
            // pair is taken, the half that would close it is refused.
            assert_eq!(p("OLD-5").as_deref(), Some("OLD-6"));
            assert_eq!(p("OLD-6"), None);
            // …and `children` mirrors every accepted edge.
            assert_eq!(store.get("OLD-1").unwrap().children, vec!["OLD-2"]);
            assert_eq!(store.get("OLD-2").unwrap().children, vec!["OLD-4"]);
            assert_eq!(store.get("OLD-6").unwrap().children, vec!["OLD-5"]);
            assert!(store.get("OLD-3").unwrap().children.is_empty());
            // The body is the raw record, untouched by the migration.
            assert!(store.get("OLD-3").unwrap().body.contains("parent: OLD-99"));
        }

        // The file itself carries the typed parents now.
        let cols = columns(&path);
        assert!(cols.contains(&"parent".to_string()), "{cols:?}");
        assert!(cols.contains(&"children".to_string()), "{cols:?}");
        {
            let conn = Connection::open(&path).unwrap();
            let raw: Vec<(String, Option<String>)> = conn
                .prepare("SELECT id, parent FROM tickets ORDER BY seq")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                raw,
                vec![
                    ("OLD-1".into(), None),
                    ("OLD-2".into(), Some("OLD-1".into())),
                    ("OLD-3".into(), None),
                    ("OLD-4".into(), Some("OLD-2".into())),
                    ("OLD-5".into(), Some("OLD-6".into())),
                    ("OLD-6".into(), None),
                ]
            );
        }

        // Second open: nothing to migrate, same parents, still writable —
        // and the newborn's own parent goes through the normal path.
        let store = Store::open(&path).unwrap();
        assert_eq!(db::migrate_add_parenthood(&mut Connection::open(&path).unwrap()).unwrap(), 0);
        assert_eq!(store.get("OLD-4").unwrap().parent.as_deref(), Some("OLD-2"));
        let next = store.create(child_of("OLD", "born after", "OLD-4")).await.unwrap();
        assert_eq!(next.id, "OLD-7");
        assert_eq!(next.parent.as_deref(), Some("OLD-4"));
        assert_eq!(store.get("OLD-4").unwrap().children, vec!["OLD-7"]);
    }

    /// The frontmatter reader: only a leading block counts, only the
    /// `parent:` key, and anything empty means "no parent".
    #[test]
    fn frontmatter_parent_reads_only_a_leading_block() {
        let p = db::frontmatter_parent;
        assert_eq!(p("---\nid: A-1\nparent: B-2\n---\n\nbody"), Some("B-2".into()));
        assert_eq!(p("---\nparent: \"B-2\"\n---\n"), Some("B-2".into()));
        assert_eq!(p("---\nparent: b-2\n---\n"), Some("B-2".into()));
        assert_eq!(p("---\nid: A-1\n---\n\nparent: B-2 in the prose"), None);
        assert_eq!(p("no frontmatter\nparent: B-2\n"), None);
        assert_eq!(p("---\nparent:\n---\n"), None);
        assert_eq!(p("---\nparent: []\n---\n"), None);
        assert_eq!(p("---\nblocked_by: [B-2]\n---\n"), None);
        assert_eq!(p(""), None);
    }

    #[test]
    fn unknown_kind_error_names_the_five() {
        let err = "bug".parse::<Kind>().unwrap_err();
        for name in ["scope", "execution", "build", "gap", "spike"] {
            assert!(err.contains(name), "{err}");
        }
        assert!(err.contains("'bug'"), "{err}");
        assert_eq!("GAP".parse::<Kind>(), Ok(Kind::Gap), "case-insensitive");
    }

    #[tokio::test]
    async fn reopen_round_trips_every_field_and_continues_seq() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tickets.db");

        let (after_flip, child) = {
            let store = Store::open(&path).unwrap();
            store
                .create(NewTicket {
                    epic: "rt".into(),
                    title: "first — ünïcode ✓".into(),
                    kind: Kind::Scope,
                    severity: Some(Severity::Critical),
                    blocked_by: vec!["RT-0".into(), "X-9".into()],
                    parent: None,
                    body: "## Problem\n\nlines\nand more".into(),
                })
                .await
                .unwrap();
            // AGS-19: the parent edge and its mirror ride the same path.
            let child = store
                .create(child_of("RT", "its output", "RT-1"))
                .await
                .unwrap();
            let parent = match store.set_status("RT-1", Status::Ready).await.unwrap() {
                Change::StatusChanged { ticket, previous } => {
                    assert_eq!(previous, Status::Pending);
                    ticket
                }
                other => panic!("expected StatusChanged, got {other:?}"),
            };
            assert_eq!(parent.children, vec!["RT-2"]);
            (parent, child)
        };

        // Restart: same location, everything identical.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.count(), 2);
        assert_eq!(store.get("RT-1").unwrap(), after_flip);
        assert_eq!(store.get("RT-2").unwrap(), child);
        assert_eq!(store.get("RT-2").unwrap().parent.as_deref(), Some("RT-1"));

        // Id allocation continues from the persisted max, not from 1.
        let next = store.create(new_ticket("RT", "second")).await.unwrap();
        assert_eq!(next.id, "RT-3");
        assert_eq!(next.seq, 3);
        assert_eq!(next.parent.as_deref(), Some("RT-1"), "MNG-14: defaulted to the root");
        assert!(next.children.is_empty());
    }

    #[tokio::test]
    async fn fresh_location_is_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("brand-new.db")).unwrap();
        assert_eq!(store.count(), 0);
        assert!(store.create(scope("NEW", "works")).await.is_ok());
    }

    // ---- PERF-2: synthetic seeding -------------------------------------

    fn spec(count: usize, seed: u64) -> SeedSpec {
        SeedSpec {
            count,
            seed,
            into_existing: false,
        }
    }

    /// Iterative DFS over `blocked_by`, reporting the first back edge.
    fn has_cycle(tickets: &HashMap<String, Ticket>) -> Option<String> {
        // 0 = unvisited, 1 = on the stack, 2 = done.
        let mut color: HashMap<&str, u8> = HashMap::with_capacity(tickets.len());
        for start in tickets.keys() {
            if color.get(start.as_str()).copied().unwrap_or(0) != 0 {
                continue;
            }
            let mut stack: Vec<(&str, usize)> = vec![(start.as_str(), 0)];
            color.insert(start.as_str(), 1);
            while let Some((id, next)) = stack.pop() {
                let edges = &tickets[id].blocked_by;
                if next < edges.len() {
                    stack.push((id, next + 1));
                    let b = edges[next].as_str();
                    match color.get(b).copied().unwrap_or(0) {
                        0 => {
                            if tickets.contains_key(b) {
                                color.insert(b, 1);
                                stack.push((b, 0));
                            }
                        }
                        1 => return Some(format!("{id} -> {b}")),
                        _ => {}
                    }
                } else {
                    color.insert(id, 2);
                }
            }
        }
        None
    }

    /// PERF-2 criterion 1 at the store level, on 10⁴: the count, a
    /// realistic spread (several epics, every status and kind present),
    /// a modest fraction with blockers, and the DAG invariants — no
    /// dangling blocker, `unlocks` mirrors every `blocked_by` exactly,
    /// no cycle — plus `ready_set` and `blockers` answering correctly
    /// against an independent computation. And it all persisted: a
    /// reopen of the file loads the same tickets.
    #[tokio::test]
    async fn seed_ten_thousand_has_a_valid_dag_and_correct_queries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seeded.db");
        let store = Store::open(&path).unwrap();
        let report = store.seed_synthetic(spec(10_000, 42)).unwrap();
        assert_eq!(report.count, 10_000);
        assert_eq!(store.count(), 10_000);
        assert!(report.epics >= 10, "several epics, got {}", report.epics);
        assert!(report.edges > 0);

        let all = store.list(None, None, None);
        assert_eq!(all.len(), 10_000);
        let by_id: HashMap<String, Ticket> = all.iter().map(|t| (t.id.clone(), t.clone())).collect();

        // Shape: every status and kind occurs; ids are EPIC-N with N
        // contiguous per epic from 1.
        for s in [
            Status::Pending,
            Status::Ready,
            Status::Blocked,
            Status::Complete,
            Status::Idea,
            Status::Epic,
            Status::Superseded,
        ] {
            assert!(all.iter().any(|t| t.status == s), "status {s:?} present");
        }
        for k in [Kind::Scope, Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike] {
            assert!(all.iter().any(|t| t.kind == k), "kind {k:?} present");
        }
        let mut per_epic: HashMap<&str, Vec<u32>> = HashMap::new();
        for t in &all {
            assert_eq!(t.id, format!("{}-{}", t.epic, t.seq));
            per_epic.entry(t.epic.as_str()).or_default().push(t.seq);
        }
        assert_eq!(per_epic.len(), report.epics);
        for (epic, mut seqs) in per_epic {
            seqs.sort_unstable();
            assert_eq!(seqs, (1..=seqs.len() as u32).collect::<Vec<_>>(), "{epic} ids contiguous");
        }
        let with_blockers = all.iter().filter(|t| !t.blocked_by.is_empty()).count();
        assert!(
            with_blockers * 100 / all.len() >= 10 && with_blockers * 100 / all.len() <= 40,
            "a modest fraction carries blockers, got {with_blockers}/10000"
        );
        assert!(all.iter().all(|t| !t.body.is_empty() && t.body.starts_with("## Problem")));

        // DAG invariants.
        let mut edges = 0;
        for t in &all {
            for b in &t.blocked_by {
                let blocker = by_id.get(b).unwrap_or_else(|| panic!("{}: dangling blocker {b}", t.id));
                assert!(blocker.unlocks.contains(&t.id), "{b}.unlocks lacks {}", t.id);
                edges += 1;
            }
            for u in &t.unlocks {
                let dependent = by_id.get(u).unwrap_or_else(|| panic!("{}: unlocks names {u}", t.id));
                assert!(dependent.blocked_by.contains(&t.id), "{u}.blocked_by lacks {}", t.id);
            }
        }
        assert_eq!(edges, report.edges);
        assert_eq!(has_cycle(&by_id), None);

        // ready_set against an independent computation over `list`.
        let expected: Vec<String> = all
            .iter()
            .filter(|t| t.status == Status::Ready)
            .filter(|t| t.blocked_by.iter().all(|b| satisfied(by_id[b].status)))
            .map(|t| t.id.clone())
            .collect();
        let got: Vec<String> = store.ready_set(None).into_iter().map(|t| t.id).collect();
        assert!(!got.is_empty());
        assert_eq!(got, expected);
        // ...and it is doing real work: some Ready tickets are held back.
        assert!(
            all.iter().filter(|t| t.status == Status::Ready).count() > got.len(),
            "some Ready tickets have unsatisfied blockers"
        );

        // blockers on a sampled ticket with several blockers.
        let sample = all.iter().filter(|t| t.blocked_by.len() >= 2).nth(7).expect("a multi-blocker ticket");
        let refs = store.blockers(&sample.id).unwrap();
        assert_eq!(refs.len(), sample.blocked_by.len());
        for (r, b) in refs.iter().zip(&sample.blocked_by) {
            match r {
                BlockerRef::Resolved(t) => assert_eq!(&t.id, b),
                BlockerRef::Dangling(d) => panic!("{}: {d} dangling", sample.id),
            }
        }

        // Persisted: a reopen finds the same tickets (write-through happened).
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.count(), 10_000);
        assert_eq!(reopened.list(None, None, None), all);
    }

    /// Criterion 4: the same seed value produces byte-identical tickets;
    /// a different one produces different tickets.
    #[test]
    fn seed_is_deterministic_per_seed_value() {
        let a = Store::new();
        let b = Store::new();
        let c = Store::new();
        a.seed_synthetic(spec(500, 7)).unwrap();
        b.seed_synthetic(spec(500, 7)).unwrap();
        c.seed_synthetic(spec(500, 8)).unwrap();
        let la = a.list(None, None, None);
        assert_eq!(la, b.list(None, None, None));
        assert_ne!(la, c.list(None, None, None));
        assert_eq!(serde_json::to_string(&la).unwrap(), serde_json::to_string(&b.list(None, None, None)).unwrap());
    }

    /// Criterion 3: a store that already holds tickets refuses the seed
    /// (nothing written) unless explicitly allowed — and then ids
    /// continue after the existing maxima, never colliding.
    #[tokio::test]
    async fn seed_refuses_an_existing_store_unless_allowed_then_continues_ids() {
        let store = Store::new();
        store.create(scope("REAL", "already here")).await.unwrap();

        let err = store.seed_synthetic(spec(100, 1)).unwrap_err();
        assert!(matches!(err, StoreError::SeedRefused { existing: 1 }), "{err}");
        assert!(err.to_string().contains("already holds 1 ticket"), "{err}");
        assert_eq!(store.count(), 1, "nothing was written");

        let allowed = SeedSpec {
            into_existing: true,
            ..spec(100, 1)
        };
        store.seed_synthetic(allowed).unwrap();
        assert_eq!(store.count(), 101);
        // Seeding the same spec again regenerates the same epics: the
        // second batch must continue their numbering (the normal
        // allocation), which the UNIQUE (epic, seq) constraint would
        // otherwise reject.
        store.seed_synthetic(allowed).unwrap();
        assert_eq!(store.count(), 201);
        let all = store.list(None, None, None);
        let mut ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 201);
        assert!(store.get("REAL-1").is_some());
        // The normal write path still works afterwards.
        let next = store.create(new_ticket("REAL", "after the seed")).await.unwrap();
        assert_eq!(next.id, "REAL-2");
    }

    #[test]
    fn seed_of_zero_is_a_no_op_and_small_counts_work() {
        let store = Store::new();
        let r = store.seed_synthetic(spec(0, 1)).unwrap();
        assert_eq!((r.count, r.epics, r.edges), (0, 0, 0));
        assert_eq!(store.count(), 0);
        for n in [1, 2, 3, 10] {
            let s = Store::new();
            s.seed_synthetic(spec(n, 1)).unwrap();
            assert_eq!(s.count(), n);
        }
    }

    #[tokio::test]
    async fn concurrent_creates_never_collide_on_ids() {
        let store = Arc::new(Store::new());
        store.create(scope("RACE", "root")).await.unwrap();
        let mut handles = Vec::new();
        for i in 0..16 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                store
                    .create(new_ticket("RACE", &format!("t{i}")))
                    .await
                    .unwrap()
                    .id
            }));
        }
        let mut ids = Vec::new();
        for h in handles {
            ids.push(h.await.unwrap());
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 16, "every concurrent create got a unique id");
    }
    // ---- MNG-12: every shortcode starts with a scope; set_parent --------

    /// MNG-12 criterion 1: the first ticket of a prefix must be a scope —
    /// anything else is refused naming the rule and nothing is allocated;
    /// once the root exists, any kind follows and, naming no parent,
    /// hangs under the root (MNG-14).
    #[tokio::test]
    async fn first_ticket_of_a_shortcode_must_be_a_scope() {
        let store = Store::new();
        let err = store.create(new_ticket("FRESH", "a build first")).await.unwrap_err();
        match &err {
            StoreError::FirstMustBeScope { epic, kind } => {
                assert_eq!(epic, "FRESH");
                assert_eq!(*kind, Kind::Build);
            }
            other => panic!("expected FirstMustBeScope, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("FRESH") && msg.contains("scope") && msg.contains("build"), "{msg}");
        assert_eq!(store.count(), 0, "nothing was created");

        let root = store.create(scope("fresh", "the root")).await.unwrap();
        assert_eq!(root.id, "FRESH-1", "the refused create allocated nothing");
        assert_eq!(root.kind, Kind::Scope);
        let next = store.create(new_ticket("FRESH", "a build second")).await.unwrap();
        assert_eq!(next.id, "FRESH-2");
        assert_eq!(next.kind, Kind::Build);
        assert_eq!(next.parent.as_deref(), Some("FRESH-1"), "defaults to the shortcode root");
        assert_eq!(store.get("FRESH-1").unwrap().children, vec!["FRESH-2"]);
    }

    /// MNG-14: a parent named must be inside the ticket's own shortcode;
    /// the shortcode's scope root is the exception — its parent is the
    /// shortcode it hangs under, in any prefix (a sub-shortcode).
    #[tokio::test]
    async fn a_parent_stays_inside_the_shortcode_except_for_the_root() {
        let store = Store::new();
        store.create(scope("HOST", "the host")).await.unwrap();
        // A new shortcode's root may hang under HOST-1 …
        let sub = store
            .create(NewTicket {
                kind: Kind::Scope,
                ..child_of("SUB", "a sub-shortcode", "HOST-1")
            })
            .await
            .unwrap();
        assert_eq!(sub.parent.as_deref(), Some("HOST-1"));
        // … but its non-root tickets may not.
        let err = store.create(child_of("SUB", "escapes", "HOST-1")).await.unwrap_err();
        match &err {
            StoreError::ParentOutsideShortcode { id, parent } => {
                assert_eq!(id, "SUB-2");
                assert_eq!(parent, "HOST-1");
            }
            other => panic!("expected ParentOutsideShortcode, got {other:?}"),
        }
        assert_eq!(store.count(), 2, "nothing was created");
        // Nor be moved out afterwards; the root can be re-hosted.
        store.create(new_ticket("SUB", "inside")).await.unwrap(); // SUB-2 → SUB-1
        assert!(matches!(
            store.set_parent("SUB-2", "HOST-1").await.unwrap_err(),
            StoreError::ParentOutsideShortcode { .. }
        ));
        store.create(scope("HOST2", "another host")).await.unwrap();
        store.set_parent("SUB-1", "HOST2-1").await.unwrap();
        assert_eq!(store.get("SUB-1").unwrap().parent.as_deref(), Some("HOST2-1"));
        assert_eq!(store.get("HOST-1").unwrap().children, Vec::<String>::new());
        assert_eq!(store.get("HOST2-1").unwrap().children, vec!["SUB-1"]);
    }

    /// MNG-12 criterion 2: `set_parent` moves a ticket — the child, the
    /// new parent and the old parent move together, the three events carry
    /// full tickets, the edge survives a reopen, and repeating it is a
    /// no-op. (Adopting a *parentless* non-root is the MNG-12 backfill's
    /// case; since MNG-14 none is born that way.)
    #[tokio::test]
    async fn set_parent_moves_a_ticket_broadcasts_and_is_durable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("move.db");
        let store = Store::open(&path).unwrap();
        store.create(scope("AD", "the root")).await.unwrap(); // AD-1
        store.create(new_ticket("AD", "a")).await.unwrap(); // AD-2 → AD-1
        store.create(new_ticket("AD", "b")).await.unwrap(); // AD-3 → AD-1
        assert_eq!(store.get("AD-1").unwrap().children, vec!["AD-2", "AD-3"]);
        let mut rx = store.subscribe();

        match store.set_parent("ad-3", "ad-2").await.unwrap() {
            Change::ParentChanged { ticket, previous } => {
                assert_eq!(ticket.id, "AD-3");
                assert_eq!(ticket.parent.as_deref(), Some("AD-2"));
                assert_eq!(previous.as_deref(), Some("AD-1"));
            }
            other => panic!("expected ParentChanged, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            Change::ParentChanged { ticket, .. } => {
                assert_eq!(ticket.parent.as_deref(), Some("AD-2"))
            }
            other => panic!("expected ParentChanged, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            Change::ChildrenChanged { ticket, added } => {
                assert_eq!(ticket.id, "AD-2");
                assert_eq!(added, "AD-3");
                assert_eq!(ticket.children, vec!["AD-3"]);
                assert_eq!(ticket.title, "a", "the full parent, not a delta");
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            Change::ChildRemoved { ticket, removed } => {
                assert_eq!(ticket.id, "AD-1");
                assert_eq!(removed, "AD-3");
                assert_eq!(ticket.children, vec!["AD-2"], "the full ex-parent");
            }
            other => panic!("expected ChildRemoved, got {other:?}"),
        }
        assert!(rx.try_recv().is_err());

        // Idempotent: the parent it already has writes and emits nothing.
        match store.set_parent("AD-3", "AD-2").await.unwrap() {
            Change::ParentChanged { previous, .. } => assert_eq!(previous.as_deref(), Some("AD-2")),
            other => panic!("expected ParentChanged, got {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "a no-op emits nothing");

        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.get("AD-3").unwrap().parent.as_deref(), Some("AD-2"));
        assert_eq!(store.get("AD-2").unwrap().children, vec!["AD-3"]);
        assert_eq!(store.get("AD-1").unwrap().children, vec!["AD-2"]);
    }

    /// MNG-12 criterion 3: the guards, and a move. A missing child, a
    /// parent that names no ticket, and any chain that comes back around
    /// (itself included) are refused with nothing changed; moving a child
    /// removes it from the old parent and says so.
    #[tokio::test]
    async fn set_parent_guards_ancestry_and_a_move_removes_from_the_old_parent() {
        let store = Store::new();
        store.create(scope("MV", "root")).await.unwrap(); // MV-1
        store.create(child_of("MV", "a", "MV-1")).await.unwrap(); // MV-2
        store.create(child_of("MV", "b", "MV-2")).await.unwrap(); // MV-3
        store.create(new_ticket("MV", "loose")).await.unwrap(); // MV-4 → MV-1 (MNG-14)

        assert!(matches!(
            store.set_parent("MV-9", "MV-1").await.unwrap_err(),
            StoreError::NotFound(_)
        ));
        match store.set_parent("MV-4", "MV-9").await.unwrap_err() {
            StoreError::ParentNotFound { id, parent } => {
                assert_eq!(id, "MV-4");
                assert_eq!(parent, "MV-9");
            }
            other => panic!("expected ParentNotFound, got {other:?}"),
        }
        match store.set_parent("MV-2", "MV-2").await.unwrap_err() {
            StoreError::ParentCycle { id, path } => {
                assert_eq!(id, "MV-2");
                assert_eq!(path, vec!["MV-2"]);
            }
            other => panic!("expected ParentCycle, got {other:?}"),
        }
        match store.set_parent("MV-1", "MV-3").await.unwrap_err() {
            StoreError::ParentCycle { id, path } => {
                assert_eq!(id, "MV-1");
                assert_eq!(path, vec!["MV-3", "MV-2", "MV-1"]);
                let msg = StoreError::ParentCycle { id, path }.to_string();
                assert!(msg.contains("own ancestor") && msg.contains("MV-3 -> MV-2 -> MV-1"), "{msg}");
            }
            other => panic!("expected ParentCycle, got {other:?}"),
        }
        assert_eq!(store.get("MV-1").unwrap().parent, None, "nothing changed");
        assert_eq!(store.get("MV-2").unwrap().children, vec!["MV-3"]);

        // The move: MV-3 from under MV-2 to under MV-1.
        let mut rx = store.subscribe();
        store.set_parent("MV-3", "MV-1").await.unwrap();
        assert!(matches!(
            rx.recv().await.unwrap(),
            Change::ParentChanged { previous: Some(p), .. } if p == "MV-2"
        ));
        match rx.recv().await.unwrap() {
            Change::ChildrenChanged { ticket, added } => {
                assert_eq!((ticket.id.as_str(), added.as_str()), ("MV-1", "MV-3"));
                assert_eq!(ticket.children, vec!["MV-2", "MV-4", "MV-3"]);
            }
            other => panic!("expected ChildrenChanged, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            Change::ChildRemoved { ticket, removed } => {
                assert_eq!((ticket.id.as_str(), removed.as_str()), ("MV-2", "MV-3"));
                assert!(ticket.children.is_empty(), "the full ex-parent");
            }
            other => panic!("expected ChildRemoved, got {other:?}"),
        }
        assert_eq!(store.get("MV-3").unwrap().parent.as_deref(), Some("MV-1"));
        assert_eq!(store.get("MV-2").unwrap().children, Vec::<String>::new());
    }

}
