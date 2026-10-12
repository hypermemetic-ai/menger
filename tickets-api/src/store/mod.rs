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
//! SCALE-14 takes the body out of the cache — the first step of SCALE-10's
//! bounded store, and the end of "no I/O on the read path": the cache holds
//! each ticket's row ([`Row`], every field but the body), and anything that
//! answers a whole [`Ticket`] reads the body from the database, inside this
//! module and off the runtime: `get`, `blockers` and a full-ticket walk on
//! read-only connections of their own ([`Readers`]), so that a read never
//! queues behind a write's fsync; a write, for the tickets its events
//! carry whole, in its own transaction before it commits. What
//! is kept: nothing outside this module holds a database handle; a write
//! is durable before the cache changes and before anything is broadcast;
//! no lock is held across an await; and a read sees every write that was
//! acknowledged before it began, as one version of each ticket — never a
//! body older, or newer, than the row beside it ([`Store::full`]).
//! SDLC-9 puts on each row the commits that finished it — `commits`, in
//! link order, bounded ([`commits`]) — written by `link_commits` and
//! `unlink_commits` through the same `commit()` then `apply()` as every
//! other write. A database written before SDLC-9 gains the column, durably,
//! once, on first open, every ticket with none linked.
//! MNG-20 makes a write run to its end whatever becomes of its caller:
//! commit, apply and broadcast run together in one task nothing cancels
//! ([`Store::write`]), holding the write-serial lock until the broadcast,
//! and the caller only waits for the answer. Before it, a caller that went
//! away while its write was committing left the database ahead of the
//! cache — and the next `create` in that shortcode overwrote the row.
//! SCALE-28 stores what the cache derives from the tickets — each
//! shortcode's run, the dangling forward references, the roll-ups, and each
//! ticket's children (the file's index on `(parent, epic, seq)`) with its
//! `child_count` (a column on its row) — beside them, in the file, written
//! by every write in the transaction that writes its tickets ([`Derived`]):
//! a SIGKILL at any instant leaves the two agreeing. A start reads the four
//! structures instead of rebuilding them by a pass over every ticket (the
//! rows still load, until B4 stops that too); a file written before
//! SCALE-28 has them built once, durably, on first open ([`stored::build`]);
//! and `menger --verify-derived` rebuilds them by a scan and compares
//! ([`Store::verify_derived`]). The cache's own copies stay, and answer, as before.
//! SCALE-32 keeps the two answers that were still a walk of the tickets
//! ([`ready::Ready`]): each ticket's count of unsatisfied blockers, the ready
//! set it decides — in the store's order, so a page of `ready_set` is a range
//! of it and its `remaining` counts at most the one shortcode a cursor cuts —
//! and each shortcode's count of each status, which `list { status }` sums
//! whole shortcodes from. A write moves them where it moves what they count:
//! a birth, and a status change — which, across the line MNG-4 draws, moves
//! the count of every ticket its `unlocks` names. Stored as SCALE-28 stores
//! the rest: a column, an index over it, two tables, in the write's
//! transaction, compared by `--verify-derived`.
//! SDLC-11 holds a ticket outside the lifecycle: a doc ([`Kind::Doc`]) is
//! born in [`Status::Doc`] and `set_status` refuses to move it, or to move
//! anything else into `Doc` — so a doc's status is fixed at birth and is
//! never `Ready`: the ready set never holds one, by the rule it already has.
//! `create` keeps docs out of the `blocked_by` graph at both ends (a doc is
//! never blocked and never a blocker), so no count of unsatisfied blockers
//! hangs on one. The status counts keep `Doc` as a bucket of its own; the
//! roll-ups count a doc in `docs`, never in `done/total` ([`rollup`]) — the
//! derived state's form 3, built once on a file's first open. Everything
//! else is a doc's as any ticket's: amend, parenthood, commits, the pages.
//! WAVE-4 reads the order of work off the rows ([`waves`]): each ticket's
//! wave, derived from `blocked_by` over the whole graph by MNG-4's rule, on
//! demand — computed under the read lock for the one call, memoized for it,
//! and kept nowhere: no write moves it, nothing stores it.
//! WAVE-6 keeps them ([`placements`]): every non-doc row carries its
//! placement, equal at every moment to that rule — placed from the rows at
//! open (nothing stores one, so none is stale after a restart), and moved by
//! the two writes that can move one, in the write, before it commits: a
//! birth (the newborn, and the tickets a forward reference to its id held
//! missing) and a status change across `Complete` / `Superseded` (the ticket,
//! and what moves downstream of it along `unlocks` — the walk stops where
//! nothing moves) — SDLC-13: and a kind change across the doc line, or with
//! such a status. The moves are broadcast last, one `PlacementsChanged`
//! per write that made any; `waves` reads the kept placements.
//! WAVE-10 gives a ticket an optional **path address**: a `path` on its row,
//! unique within its shortcode, which `get` resolves from `<SHORTCODE>/<path>`
//! through an index the cache keeps beside the hash half ([`Paths`]) and the
//! file keeps as a unique index on `(epic, path)`. An address, not an id —
//! the order, extents, paging and readiness never read it — set at birth and
//! never moved; only `_MENGER/…`, on a doc directly under its shortcode's
//! scope root, for the documents menger keeps inside a shortcode. A database
//! written before WAVE-10 gains the column, durably, once, on first open.
//! COMP-3 keeps the **component registry** ([`components`]): every custom
//! element a ticket defines in a ` ```component <tag> ` block — its source,
//! its owner (or none: an orphan, kept while something live still uses it)
//! and what references it. The two writes that change a body move it —
//! `create` and an `amend` of the body — worked out under the write-serial
//! lock from the new body in hand and the registry's own record of what the
//! ticket defined and referenced (the old body is never read), stored in the
//! write's transaction, taken by the cache with its rows, and announced
//! last (`ComponentChanged`, `ComponentRemoved`). A definition of a name
//! another ticket owns refuses the write. Held in the cache, sources and
//! all (a component is a few KB, and there are few); stored in its own
//! table, which a database written before COMP-3 gains — the registry built
//! from every body — durably, once, on first open.
//! SDLC-13 makes a kind changeable — by one write, `set_kind`
//! ([`Store::set_kind`]), and only by it. Everything the cache keeps by kind
//! moves in that write's own [`Derived`] and transaction, worked out before it
//! commits as a status change's is ([`Cache::kind_change`]): the roll-ups (a
//! ticket's count moving between work and docs, a lower-seq ticket made a
//! scope or a doc re-rooting its shortcode, the shape settled again), the
//! status counts and the ready set (the status crosses the doc line with the
//! kind), the placements (none for a doc), the cells (kind is in the cell) and
//! the kept tree's tallies and holders. `--verify-derived` holds it to a scan.
//! INPUT-3 keeps the **inputs store** ([`inputs`]): every input a body
//! declares (`<m-decision id>`, `<m-input id kind>` — [`crate::inputs`]) and
//! every answer and state change it has been through, with who and when. The
//! same two writes move the declarations — `create` and an `amend` of the
//! body, worked out under the write-serial lock from the new body and the
//! store's own record (an input the body no longer declares is `removed`,
//! history kept), stored in the write's transaction, taken by the cache with
//! its rows, and announced last of all (`InputChanged`, after the
//! components'). Four writes of their own move the answers — `answer`,
//! `clear_answer`, `close_input`, `reopen_input`: one history entry each,
//! never a body. Held in the cache, histories and all; stored in two tables
//! of its own, which a database written before INPUT-3 gains — every body's
//! inputs read in — durably, once, on first open.
//! COMMENT-2 keeps the **comments** ([`comments`]): a reader's notes on the
//! app and on ticket bodies, records of their own beside the tickets. Held in
//! the cache whole, read from it for every `list`; three writes of their own
//! — `add_comment`, `resolve_comment`, `reopen_comment` — take the write path
//! every write takes and broadcast on a channel of their own
//! ([`Store::subscribe_comments`]); no ticket's write touches one, so a
//! comment keeps its anchor. Stored in one table, which is stored form 7: a
//! file in form 6 gains it, empty, in place, once, on first open
//! (`stored::upgrade`).

use crate::types::{
    is_menger_path, path_address, AnswerSource, Component, Input, InputState, InputValue, Placement, PlacementChange,
    TicketSummary, MENGER_PATH_PREFIX,
};
use rusqlite::{Connection, OpenFlags};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use tokio::sync::broadcast;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::{OwnedMutexGuard, Semaphore};

/// MFE-9: the ticket vocabulary itself now lives in `menger-tickets-api`
/// so the browser app can consume the same declarations instead of a
/// hand-written mirror of the JSON they arrive as. Re-exported here
/// unchanged — every `crate::store::Status` in the server still resolves,
/// and the store remains the only thing that *writes* them.
pub use crate::types::{Group, Kind, RootRow, Severity, Status, Ticket};

/// SCALE-14: what the cache holds of a ticket — every field but the body,
/// which is MNG-9's summary row exactly (the guard test
/// `a_summary_is_a_ticket_minus_exactly_the_body` holds [`TicketSummary`]
/// to "a [`Ticket`] minus its body"). The body is half of what a ticket
/// cost resident (PERF-4) and one kind of answer shows it; the row is what
/// every walk, filter, roll-up and index reads. A summary page is rows as
/// they are; a full ticket is a row and a body ([`whole`]).
type Row = TicketSummary;

/// SCALE-14: a full ticket from its row and its body. Destructured, not
/// copied field by field from memory: a field added to the row is a
/// compile error here until it is carried over.
fn whole(row: Row, body: String) -> Ticket {
    let Row {
        id,
        epic,
        seq,
        title,
        status,
        kind,
        severity,
        parent,
        child_count,
        blocked_by,
        unlocks,
        commits,
        path,
        placement,
        labels,
        created_unix,
        updated_unix,
    } = row;
    Ticket {
        id,
        epic,
        seq,
        title,
        status,
        kind,
        severity,
        blocked_by,
        unlocks,
        parent,
        child_count,
        commits,
        path,
        placement,
        labels,
        body,
        created_unix,
        updated_unix,
    }
}

/// SCALE-14: a row the cache holds and the database does not. Nothing
/// deletes a ticket, so only a file changed under a running server makes
/// one; the read says so rather than answer a ticket without its body.
fn missing(id: &str) -> StoreError {
    StoreError::Db(format!("{id} is in the cache but not in the database"))
}

/// SCALE-14: puts a committed write into the cache — and counts it there,
/// in the same critical section, so a reader that sees the rows sees the
/// count that says they are this write's. Every write applies through
/// here, once per commit ([`Store::write`]); so does a seed.
fn apply<T>(tickets: &RwLock<Cache>, f: impl FnOnce(&mut Cache) -> T) -> T {
    let mut cache = tickets.write().expect("store lock poisoned");
    let out = f(&mut cache);
    cache.written += 1;
    out
}

/// MNG-10: the roll-up half of a mutation's broadcast, sent last — after
/// every ticket change of the same mutation, so the established order
/// (`Created` first, …) is untouched and a client meets the cause before
/// the count it moved.
fn broadcast_groups(tx: &Broadcast, groups: Vec<Group>) {
    for group in groups {
        let _ = tx.send(Change::GroupChanged { group });
    }
}

/// The store's broadcast, on two channels (FILTER-4). `plain` is `watch`'s
/// — every change, exactly as before FILTER-4, so a `watch` without a
/// filter is today's stream, lags where it lagged and loses what it lost.
/// `filtered` is `watch { filter }`'s: the same changes in the same order
/// and, last in each write that moves any cell, [`Change::CellsMoved`] —
/// fed only while a filtered subscriber is open, so a store with none pays
/// nothing for it. Each has the capacity [`BROADCAST_CAPACITY`].
#[derive(Clone)]
pub(crate) struct Broadcast {
    plain: broadcast::Sender<Change>,
    filtered: broadcast::Sender<Change>,
    /// COMMENT-2: `comments.watch`'s — the comments' own changes, which no
    /// ticket's `watch` carries.
    comments: broadcast::Sender<CommentChange>,
}

/// How many changes a subscriber may fall behind before it lags (SCALE-5).
pub const BROADCAST_CAPACITY: usize = 256;

impl Broadcast {
    fn new() -> Self {
        Self {
            plain: broadcast::channel(BROADCAST_CAPACITY).0,
            filtered: broadcast::channel(BROADCAST_CAPACITY).0,
            comments: broadcast::channel(BROADCAST_CAPACITY).0,
        }
    }

    /// COMMENT-2: a comment's change, to `comments.watch`.
    fn comment(&self, change: CommentChange) {
        let _ = self.comments.send(change);
    }

    /// Sends `change` to every subscriber it is for: a cell move to the
    /// filtered channel alone; anything else to both.
    fn send(&self, change: Change) {
        let watched = self.filtered.receiver_count() > 0;
        if let Change::CellsMoved { .. } | Change::UnderMoved { .. } = change {
            if watched {
                let _ = self.filtered.send(change);
            }
            return;
        }
        if watched {
            let _ = self.filtered.send(change.clone());
        }
        let _ = self.plain.send(change);
    }
}

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
    fn of(ticket: &Row) -> Self {
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
///
/// SCALE-32: `Kept` is a predicate the cache keeps counts of — a status, so
/// far ([`Span::len_of`]) — so its page is still tested row by row, and its
/// `remaining` is that count less the page, as `All`'s is. `By` is one the
/// cache keeps nothing for (`find`'s needle, a status among one parent's
/// children): everything past the page is tested and counted.
enum Filter<'a> {
    All,
    Kept(&'a dyn Fn(&Row) -> bool),
    By(&'a dyn Fn(&Row) -> bool),
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
    ) -> Option<std::collections::btree_map::Range<'c, OrderKey, Row>> {
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

    /// SCALE-32: how many tickets in this span hold `status` — [`Span::len`]
    /// for `list { status }`: every whole shortcode from its kept count of
    /// the status ([`ready::Ready::held`]), and the one the cursor cuts into
    /// by walking the rest of it and testing each row — the rest of one
    /// shortcode, never the rest of the store. The value is the walk's.
    fn len_of(&self, cache: &Cache, status: Status) -> u64 {
        use std::ops::Bound::{Excluded, Included, Unbounded};
        if self.empty {
            return 0;
        }
        let rest_of_run = |key: &OrderKey| {
            let end = OrderKey {
                epic: key.epic.clone(),
                seq: u32::MAX,
            };
            cache
                .order
                .range((Excluded(key), Included(&end)))
                .filter(|(_, t)| t.status == status)
                .count() as u64
        };
        match (&self.start, &self.end) {
            (Unbounded, Unbounded) => cache.ready.held_after(None, status),
            (Excluded(key), Unbounded) => rest_of_run(key) + cache.ready.held_after(Some(&key.epic), status),
            (Included(first), Included(_)) => cache.ready.held(&first.epic, status),
            (Excluded(key), Included(_)) => rest_of_run(key),
            _ => self
                .range(cache)
                .map_or(0, |range| range.filter(|(_, t)| t.status == status).count() as u64),
        }
    }
}

/// MNG-9: one page of the store's order, and where the walk stands.
/// SCALE-14: a page of rows — what the cache holds, without bodies: the
/// `summary` page, `find`'s, and what a [`Walk`] reads bodies for, a page
/// at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// The page itself, in `(epic, seq)` order.
    pub tickets: Vec<TicketSummary>,
    /// The cursor for the next page — the last id on this page, or the
    /// cursor this page was cut with when the page came back empty.
    /// `None` means the walk is over: nothing matches beyond this page.
    pub next: Option<String>,
    /// How many further tickets match the same filters beyond this page.
    pub remaining: u64,
    /// FILTER-4: the position of the page's first row among the matches —
    /// for an empty page, where it would start — on a filtered page
    /// ([`Store::filtered_page`]); `None` on every other.
    pub first: Option<u64>,
}

/// SCALE-14: what a walk of whole tickets selects — the two selections the
/// wire answers with bodies, `list {}` and `ready_set {}`.
#[derive(Debug, Clone, PartialEq)]
pub enum Select {
    /// `list`'s filters, as [`PageQuery`] takes them.
    List {
        epic: Option<String>,
        status: Option<Status>,
        parent: Option<String>,
    },
    /// The ready set's rule ([`Store::ready_set`]), optionally in one epic.
    Ready { epic: Option<String> },
    /// FILTER-4: a filter, with `list`'s old parameters beside it.
    Filtered(Filtered),
}

/// SCALE-14: whether a cut counts what it leaves behind. A page's
/// `remaining` is exact (MNG-9) — with a predicate, a test of everything
/// after the page. A [`Walk`] cuts a step at a time and counts once, where
/// it ends: counting at every step would make one `list { status }` of
/// the store quadratic.
#[derive(Debug, Clone, Copy)]
enum Rest {
    Count,
    /// The rows alone; the page's `next` and `remaining` are not worked out.
    Skip,
}

/// SCALE-14: a walk of whole tickets — how `list` and `ready_set` answer
/// with bodies without holding more than a page of them. A step cuts the
/// next [`BODIES_PER_STEP`] rows from the cache, lets the lock go, and
/// sets their bodies reading off the runtime; [`Walk::next`] hands out one
/// step's tickets with the next step's already reading, so the database
/// works while the transport sends. The walk keeps where it stands (the
/// last id it cut) and how much of its `limit` is left. As any page walk
/// (MNG-9), it sees tickets born ahead of its cursor while it goes.
#[derive(Debug)]
pub struct Walk {
    select: Select,
    after: Option<String>,
    /// FILTER-4: where a filtered walk starts, as a position among the
    /// matches — spent by its first cut, after which it goes on by `after`.
    at: Option<u64>,
    /// FILTER-4: a filtered walk's first position, from its first cut.
    first: Option<u64>,
    left: Option<u32>,
    /// Set once the selection is spent: its `next` and `remaining`.
    over: Option<(Option<String>, u64)>,
    /// The step cut and reading ahead of the one handed out.
    ahead: Option<Reading>,
}

/// One step's whole tickets, being read off the runtime.
type Reading = tokio::task::JoinHandle<Result<Vec<Ticket>, StoreError>>;

/// SCALE-14: one step of a [`Walk`].
#[derive(Debug)]
pub enum Step {
    /// The next tickets, whole, in the store's order — at most
    /// [`BODIES_PER_STEP`] of them.
    Tickets(Vec<Ticket>),
    /// The walk is over, and this is where it stands: what MNG-9's page
    /// event says — exactly what one page of the walk's `limit` says.
    /// FILTER-4: `first`, a filtered walk's first position.
    End {
        next: Option<String>,
        remaining: u64,
        first: Option<u64>,
    },
}

impl Walk {
    /// The next tickets of the walk, or its end. A cursor naming no ticket
    /// is [`StoreError::CursorNotFound`], at the first step: nothing walked.
    pub async fn next(&mut self, store: &Store) -> Result<Step, StoreError> {
        let current = match self.ahead.take() {
            Some(reading) => Some(reading),
            None => self.step(store)?,
        };
        let Some(current) = current else {
            return self.end(store);
        };
        self.ahead = self.step(store)?;
        let tickets = current
            .await
            .map_err(|e| StoreError::Db(format!("read task failed: {e}")))??;
        Ok(Step::Tickets(tickets))
    }

    /// Cuts the next step's rows from where the walk stands and sets their
    /// bodies reading; `None` when there is nothing left to cut — the
    /// selection is spent, or the limit is.
    fn step(&mut self, store: &Store) -> Result<Option<Reading>, StoreError> {
        let take = self.left.map_or(BODIES_PER_STEP, |n| n.min(BODIES_PER_STEP));
        if self.over.is_some() || take == 0 {
            return Ok(None);
        }
        let (rows, written) = {
            let cache = store.tickets.read().expect("store lock poisoned");
            let page = self.cut(&cache, Some(take), Rest::Skip)?;
            (page.tickets, cache.written)
        };
        if rows.len() < take as usize {
            // Fewer than asked for: the selection is spent, nothing remains.
            self.over = Some((None, 0));
        }
        let Some(last) = rows.last() else {
            return Ok(None);
        };
        self.after = Some(last.id.clone());
        self.left = self.left.map(|n| n - rows.len() as u32);
        Ok(Some(tokio::spawn(store.full_owned(rows, written))))
    }

    /// Where the walk stands once nothing is left to cut. The limit spent
    /// before the selection was, what is left is counted — once, from the
    /// last id cut, the cursor a single page would hand back.
    fn end(&mut self, store: &Store) -> Result<Step, StoreError> {
        let (next, remaining) = match &self.over {
            Some(over) => over.clone(),
            None => {
                let page = {
                    let cache = store.tickets.read().expect("store lock poisoned");
                    self.cut(&cache, Some(0), Rest::Count)?
                };
                self.over = Some((page.next.clone(), page.remaining));
                (page.next, page.remaining)
            }
        };
        Ok(Step::End {
            next,
            remaining,
            first: self.first,
        })
    }

    /// One cut from where the walk stands: a filtered walk's first by its
    /// `at` when it has one (and its first position kept), every other by
    /// `after`.
    fn cut(&mut self, cache: &Cache, limit: Option<u32>, rest: Rest) -> Result<Page, StoreError> {
        let Select::Filtered(q) = &self.select else {
            return Store::cut(cache, &self.select, self.after.as_deref(), limit, rest);
        };
        let page = Store::cut_filtered(cache, q, self.after.as_deref(), self.at.take(), limit, rest)?;
        self.first = self.first.or(page.first);
        Ok(page)
    }

    /// FILTER-4: a filtered walk that starts at the `at`-th match.
    pub fn at(mut self, at: Option<u64>) -> Self {
        self.at = at;
        self
    }
}

/// A mutation, broadcast to every `watch` subscriber. (Internal — the
/// wire form is `TicketsEvent`, same `type` tag; `kind` is a ticket's.)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Change {
    Created { ticket: Ticket },
    StatusChanged { ticket: Ticket, previous: Status },
    /// SDLC-13: a ticket's kind changed (`set_kind`) — and its status with
    /// it, when the change crossed the doc line or a status was given;
    /// `previous` is the kind it had. Carries the full ticket, as every write
    /// after birth does; first of its write's events, before the groups, the
    /// placements and the cell moves it caused.
    KindChanged { ticket: Ticket, previous: Kind },
    /// MNG-16: a ticket's title and/or body was replaced (`amend`);
    /// `changed` names which, in that order. Carries the full ticket — body
    /// included, so an open page can show the new one without a `get`.
    /// Only broadcast when something changed: a no-op amend writes and
    /// emits nothing.
    Amended { ticket: Ticket, changed: Vec<String> },
    /// SDLC-9: commits were linked to a ticket (`link_commits`); `added` is
    /// what was newly linked, in the order given. Carries the full ticket,
    /// as every write after birth does. Only broadcast when something was
    /// added: a no-op link writes and emits nothing.
    CommitsLinked { ticket: Ticket, added: Vec<String> },
    /// SDLC-9: commits were unlinked (`unlink_commits`); `removed` is what
    /// came off, in the order given. Only broadcast when something did.
    CommitsUnlinked { ticket: Ticket, removed: Vec<String> },
    /// MENGER-12: a ticket's labels moved (`label`); `added` and `removed`
    /// are what moved, in the order given. Only broadcast when something
    /// did. Carries the full ticket.
    LabelsChanged { ticket: Ticket, added: Vec<String>, removed: Vec<String> },
    /// An existing ticket's `unlocks` gained `added` because a newly
    /// created ticket named it in `blocked_by`. Carries the full updated
    /// ticket so watch-only clients stay coherent.
    UnlocksChanged { ticket: Ticket, added: String },
    /// MENGER-30: an existing ticket's `unlocks` lost `removed`, which no
    /// longer names it in `blocked_by` (`update { unblock }`). Carries the
    /// full updated ticket, as `UnlocksChanged` does.
    UnlocksRemoved { ticket: Ticket, removed: String },
    /// MENGER-30: a ticket's `blocked_by` moved after birth (`update`) —
    /// `added` and `removed` as they moved. Carries the full ticket; each
    /// blocker's side follows as `UnlocksChanged` / `UnlocksRemoved`.
    BlockersChanged { ticket: Ticket, added: Vec<String>, removed: Vec<String> },
    /// MENGER-30: a ticket's severity moved (`update`); `previous` is the
    /// one it had (`None`: none). Carries the full ticket.
    SeverityChanged { ticket: Ticket, previous: Option<Severity> },
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
    /// WAVE-6: the kept placements one write moved, each with its ticket's
    /// shortcode (what `watch { epic }` filters by; it does not cross the
    /// wire) — broadcast last, after the write's other events, once per
    /// write that moved any.
    PlacementsChanged { changes: Vec<(String, PlacementChange)> },
    /// COMP-3: a component a write registered or moved — its source, owner
    /// or references — as the write leaves it; one per component, in name
    /// order, after every other event of the write (placements included).
    ComponentChanged { component: Component },
    /// COMP-3: a component the write took out of the registry (an orphan
    /// nothing live uses any more) — after the write's `ComponentChanged`.
    ComponentRemoved { name: String },
    /// INPUT-3: an input a write moved — answered, cleared, closed, reopened;
    /// or declared, re-declared, removed or restored by a body — as it leaves
    /// it, after every other event of the write. `epic` is its ticket's
    /// shortcode, what `watch { epic }` filters by; it does not cross the wire.
    InputChanged { epic: String, input: Input },
    /// MENGER-13: a transition the write made — right after the event of the
    /// change it records (`Created`, `StatusChanged`, `KindChanged`). `epic`
    /// is its ticket's shortcode, what `watch { epic }` filters by; it does
    /// not cross the wire.
    Transition { epic: String, transition: crate::types::Transition },
    /// FILTER-4: every ticket whose cell the write moved — a birth, a status
    /// change and the blocker crossings it fans out to, a first commit or a
    /// last, SDLC-13: a kind changed — worked out once per write before it
    /// committed, and sent last, after every other event of the write.
    /// Never on the wire as itself:
    /// `watch { filter }` turns it into `entered`, `left` or `recount` for
    /// its own filter; a `watch` without one passes it over.
    #[serde(skip)]
    #[schemars(skip)]
    CellsMoved { moves: std::sync::Arc<cells::Moves> },
    /// FILTER-13: what the write moved for each open `watch { filter }`
    /// whose filter asks `under` — worked out by the write, before it
    /// committed and after it applied — sent last, after its `CellsMoved`.
    /// Never on the wire as itself: each such watch takes its own events.
    #[serde(skip)]
    #[schemars(skip)]
    UnderMoved { moves: std::sync::Arc<lineage::UnderMoves> },
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
            | Self::KindChanged { ticket, .. }
            | Self::Amended { ticket, .. }
            | Self::CommitsLinked { ticket, .. }
            | Self::CommitsUnlinked { ticket, .. }
            | Self::LabelsChanged { ticket, .. }
            | Self::UnlocksChanged { ticket, .. }
            | Self::UnlocksRemoved { ticket, .. }
            | Self::BlockersChanged { ticket, .. }
            | Self::SeverityChanged { ticket, .. }
            | Self::ParentChanged { ticket, .. } => &ticket.epic,
            Self::ChildrenChanged { epic, .. } | Self::ChildRemoved { epic, .. } => epic,
            Self::GroupChanged { group } => &group.epic,
            // WAVE-6: one per change, not per event — see `in_epic`.
            Self::PlacementsChanged { .. } => "",
            // COMP-3: in no shortcode — see `in_epic`.
            Self::ComponentChanged { .. } | Self::ComponentRemoved { .. } => "",
            // INPUT-3: its ticket's.
            Self::InputChanged { epic, .. } => epic,
            // MENGER-13: its ticket's.
            Self::Transition { epic, .. } => epic,
            // FILTER-4: one per write, its moves in any shortcode.
            Self::CellsMoved { .. } => "",
            // FILTER-13: one per write, each watch's events already in its epic.
            Self::UnderMoved { .. } => "",
        }
    }

    /// What `watch { epic }` passes on of this change: the change itself
    /// when it is in `epic` (or no epic is asked for) — WAVE-6: and of a
    /// `PlacementsChanged`, the changes in `epic`, when any is. COMP-3: a
    /// component's change whatever the epic — a component belongs to no
    /// shortcode, and a ticket of any shortcode may use it.
    pub fn in_epic(self, epic: Option<&str>) -> Option<Change> {
        let Some(epic) = epic else { return Some(self) };
        match self {
            Self::PlacementsChanged { changes } => {
                let changes: Vec<_> = changes.into_iter().filter(|(e, _)| e == epic).collect();
                (!changes.is_empty()).then_some(Self::PlacementsChanged { changes })
            }
            Self::ComponentChanged { .. } | Self::ComponentRemoved { .. } => Some(self),
            // FILTER-4: each move filtered where it is turned into events.
            Self::CellsMoved { .. } | Self::UnderMoved { .. } => Some(self),
            other => (other.epic() == epic).then_some(other),
        }
    }

    /// COMP-3: a write's component events, in the order they are sent —
    /// the components it moved, then the ones it removed, each in name order.
    fn components(plan: components::Plan) -> Vec<Change> {
        plan.changed
            .into_iter()
            .map(|component| Self::ComponentChanged { component })
            .chain(plan.removed.into_iter().map(|name| Self::ComponentRemoved { name }))
            .collect()
    }

    /// INPUT-3: a write's input events, in the order they are sent — the
    /// plan's: the body's inputs it moved, in the body's order, then the ones
    /// it removed.
    fn inputs(epic: &str, plan: inputs::Plan) -> Vec<Change> {
        plan.changed
            .into_iter()
            .map(|input| Self::InputChanged {
                epic: epic.to_string(),
                input,
            })
            .collect()
    }

    /// WAVE-6: the `PlacementsChanged` a write's pass makes — `None` when it
    /// moved no placement.
    fn placed(replaced: &placements::Replaced) -> Option<Change> {
        (!replaced.shifts.is_empty()).then(|| Self::PlacementsChanged {
            changes: replaced.shifts.iter().map(placements::Shift::change).collect(),
        })
    }
}

/// INPUT-3: what a write to one input did — `Changed`: the input as it
/// leaves it, broadcast as `InputChanged`; `Unchanged`: nothing to do
/// (closing a closed input, reopening one not closed, clearing one with no
/// answer) — the input as it is, nothing written or broadcast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputWrite {
    Changed(Input),
    Unchanged(Input),
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
    /// WAVE-10: the newborn's path address in its shortcode — only a
    /// `_MENGER/…` path, only on a doc, only directly under the shortcode's
    /// scope root, and only one no ticket of the shortcode already holds.
    pub path: Option<String>,
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

/// FILTER-13: where a seed hangs its tickets ([`Store::seed_synthetic_shaped`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeedShape {
    /// PERF-2's: every non-root under its shortcode's root — one level.
    #[default]
    Flat,
    /// FILTER-3 §6.5's deeper tree: executions under the root, builds under
    /// executions, docs under builds (`synthetic::generate`).
    Nested,
}

impl std::str::FromStr for SeedShape {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "flat" => Ok(Self::Flat),
            "nested" => Ok(Self::Nested),
            other => Err(format!("a seed shape is flat or nested, not {other:?}")),
        }
    }
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
    /// MNG-12: every shortcode starts with a scope — SDLC-11: or a doc. The
    /// first ticket of a prefix that has none yet must be `kind: scope` or
    /// `kind: doc`; `kind` is what was asked for instead. Nothing was stored
    /// and no id was allocated.
    FirstMustBeScope { epic: String, kind: Kind },
    /// SDLC-11: a doc was asked to be born with a `blocked_by`. A doc has no
    /// lifecycle — nothing for a blocker to hold back. Nothing was stored.
    DocBlocked { epic: String, blocked_by: Vec<String> },
    /// SDLC-11: the `blocked_by` of `id` names `docs`, each a doc. A doc
    /// never completes, so it can never unblock anything. Nothing was stored.
    BlockedByDoc { id: String, docs: Vec<String> },
    /// SDLC-11: a doc would be born as `id`, which tickets already name in
    /// their `blocked_by` (`by`, a forward reference) — they would wait on
    /// it forever. Nothing was stored and the id stays unallocated.
    DocAwaited { id: String, by: Vec<String> },
    /// SDLC-11: `set_status` on a doc. A doc has no lifecycle: it exists in
    /// its state, `Doc`, and nothing moves it. Nothing changed.
    DocHasNoLifecycle { id: String },
    /// SDLC-11: `set_status … Doc` on a ticket of another kind. `Doc` is a
    /// doc's own status and no other kind's. Nothing changed.
    DocStatusOnWork { id: String, kind: Kind },
    /// SDLC-13: `set_kind … doc` with a status other than `Doc` — a doc's
    /// status is `Doc`, whatever it was before. Nothing changed.
    DocStatusForDoc { id: String, status: Status },
    /// SDLC-13: `set_kind` to a kind with a lifecycle, with the status `Doc`
    /// — `Doc` is a doc's own. Nothing changed.
    DocStatusForWork { id: String, kind: Kind },
    /// SDLC-13: `set_kind … doc` on a ticket in the `blocked_by` graph — it
    /// waits on `blocked_by`, or `unlocks` wait on it — where a doc is never
    /// blocked and never a blocker. Both named. Nothing changed.
    DocInGraph { id: String, blocked_by: Vec<String>, unlocks: Vec<String> },
    /// SDLC-13: `id` is the root of `epic` — its lowest-seq scope or doc —
    /// and `kind` roots nothing. Nothing changed.
    RootKind { id: String, epic: String, kind: Kind },
    /// SDLC-13 (WAVE-10): `id` holds the path address `path`, which only a
    /// doc carries, and `kind` is work. Nothing changed.
    PathKind { id: String, path: String, kind: Kind },
    /// MNG-14: a ticket's parent must be inside its own shortcode; only a
    /// shortcode's scope root may hang under another shortcode (its host).
    /// Nothing changed.
    ParentOutsideShortcode { id: String, parent: String },
    /// MNG-16: an `amend` asked for a title that is empty after trimming.
    /// A ticket always has one; nothing was written.
    EmptyTitle { id: String },
    /// SDLC-9: a `link_commits` or `unlink_commits` named entries that are
    /// not commit references ([`commits::reference`]) — every one of them,
    /// as given. A call is all or nothing: nothing was written.
    NotACommit { id: String, entries: Vec<String> },
    /// SDLC-9: the link would put `linked + adding` commits on `id`, past
    /// [`commits::MAX_PER_TICKET`]. Nothing was written.
    TooManyCommits { id: String, linked: usize, adding: usize },
    /// MENGER-12: a `label` named entries that are not labels, or would
    /// leave more than [`crate::labels::MAX_PER_TICKET`] — `why` says which.
    /// A call is all or nothing: nothing was written.
    Labels { id: String, why: String },
    /// MENGER-15: a `create` or `amend` gave a body of `bytes` bytes, past
    /// the store's cap, `max` ([`Store::body_max`]). `id` is the amended
    /// ticket — `None` for a `create`, which stored nothing and allocated no
    /// id. Nothing was written.
    BodyTooLarge { id: Option<String>, bytes: usize, max: usize },
    /// The write could not be made durable; nothing was acknowledged.
    /// SCALE-14: or a read could not get a body from the database.
    Db(String),
    /// PERF-2: a seed was requested into a store that already holds
    /// `existing` tickets without `SeedSpec::into_existing`. Nothing was
    /// generated or written.
    SeedRefused { existing: usize },
    /// MNG-9: the `after` cursor names no ticket, so there is no position
    /// to resume from. Distinct from `NotFound`: the miss is the cursor,
    /// not the thing asked for, and the message says so.
    CursorNotFound(String),
    /// WAVE-10: `create { path }` with a path outside `_MENGER/` (or
    /// `_MENGER/` and nothing after it) — the one prefix menger accepts.
    /// Nothing was stored.
    PathNotReserved { path: String },
    /// WAVE-10: `create { path }` for a ticket that is not a doc — a path
    /// names one of menger's own documents. Nothing was stored.
    PathOnWork { path: String, kind: Kind },
    /// WAVE-10: `create { path }` with a `parent` other than the shortcode's
    /// scope root, `root`, which is where a `_MENGER/` document lives.
    /// Nothing was stored.
    PathParent { path: String, parent: String, root: String },
    /// WAVE-10: `create { path }` in a shortcode with no scope root yet (its
    /// first ticket, or a legacy prefix that never had one) — nowhere for
    /// the document to live. Nothing was stored.
    PathNoRoot { epic: String, path: String },
    /// WAVE-10: `create { path }` with a path `holder` already holds in
    /// `epic`. Nothing was stored and no id was allocated.
    PathTaken { epic: String, path: String, holder: String },
    /// COMP-3: the body holds a ` ```component ` block whose tag, `name`, is
    /// not a component name ([`crate::component_docs::is_component_name`]; `""`: it
    /// names none). `id` is the amended ticket — `None` for a `create`, which
    /// stored nothing and allocated no id. Nothing changed.
    ComponentName { id: Option<String>, name: String },
    /// COMP-3: the body defines `name` in two blocks. Nothing changed.
    ComponentTwice { id: Option<String>, name: String },
    /// COMP-3: the body defines `name`, which `owner` already defines — a
    /// component has one owner. Nothing changed.
    ComponentOwned { id: Option<String>, name: String, owner: String },
    /// COMP-8: the body's definition of `name` holds a malformed `<docs>` —
    /// `problem` says what. Nothing changed.
    ComponentDocs { id: Option<String>, name: String, problem: String },
    /// INPUT-3: the body's input markup is refused — two inputs with one id,
    /// a kind the element does not take, an option without a value or
    /// declared twice, a text input with options ([`crate::inputs::Problem`],
    /// which names the input). `id` as for a component's refusal. Nothing
    /// changed.
    InputMarkup { id: Option<String>, problem: crate::inputs::Problem },
    /// INPUT-3: `ticket` declares no input `input` — `known` the ids it does
    /// declare (removed ones included). Nothing changed.
    InputUnknown { ticket: String, input: String, known: Vec<String> },
    /// INPUT-3: the value does not answer the input — `reason` says why
    /// ([`crate::inputs::check`]). Nothing changed.
    InputInvalid { ticket: String, input: String, reason: String },
    /// INPUT-3: the input is closed — an answer or a clear waits for
    /// `reopen_input`. Nothing changed.
    InputClosed { ticket: String, input: String },
    /// INPUT-3: the input's body no longer declares it — it takes no change
    /// until the body declares it again. Nothing changed.
    InputRemoved { ticket: String, input: String },
    /// MENGER-13: the input is withdrawn — its ticket is canceled, and it
    /// asks no one until the ticket is not. Nothing changed.
    InputWithdrawn { ticket: String, input: String },
    /// FILTER-4: a `list` given both `at` and `after` — two answers to where
    /// its page starts.
    AtAndAfter,
    /// FILTER-4: a filter the store cannot answer — not understood, or a
    /// predicate that lives elsewhere; the message names every such chunk.
    Filter(String),
    /// FILTER-13: `tree` asked with both a `parent` and an `epic`, or with
    /// neither — it answers one address.
    TreeAddress,
    /// COMMENT-2: `add` refused the comment — an empty body, a blank author,
    /// a malformed anchor ([`crate::comments::Problem`], which names it).
    /// Nothing was kept.
    CommentRefused(crate::comments::Problem),
    /// COMMENT-2: the anchor's `ticket` names no ticket. Nothing was kept.
    CommentTicket { ticket: String },
    /// COMMENT-2: `resolve` or `reopen` named no comment — `newest` the
    /// newest id there is. Nothing changed.
    CommentNotFound { id: u64, newest: Option<u64> },
    /// COMMENT-2: `list`'s `after` names no comment, so there is no position
    /// to resume from.
    CommentCursor(u64),
    /// NOTE-2: `add` refused the note — a blank or oversized body or author
    /// ([`crate::notes::Problem`], which names it). Nothing was kept.
    NoteRefused(crate::notes::Problem),
    /// NOTE-2: an id that names no note — `newest` the newest id there is.
    /// Nothing changed.
    NoteNotFound { id: u64, newest: Option<u64> },
    /// NOTE-2: `upgrade` of a note already a ticket. Nothing changed.
    NoteUpgraded { id: u64, ticket: String },
    /// NOTE-2: `list`'s `after` names no note.
    NoteCursor(u64),
    /// MENGER-30: an `update`'s `block` / `unblock` named a blank id, or one
    /// id in both — `why` says which. Nothing was written.
    Blockers { id: String, why: String },
    /// MENGER-30: `update { block }` would close a `blocked_by` cycle; `path`
    /// is the chain from a new blocker back to `id`. Nothing was written.
    BlockCycle { id: String, path: Vec<String> },
    /// MENGER-30: what the caller refused before the store saw the call — an
    /// unknown kind, status or severity — named with the store's refusals.
    Invalid(String),
    /// MENGER-30: an `update` refused for more than one reason — every one,
    /// in the order the write would meet them. Nothing was written.
    Refused { id: String, refusals: Vec<StoreError> },
}

impl StoreError {
    /// COMP-3: a body's refused ` ```component ` block, as the write that
    /// carried it says so — `id` the amended ticket, `None` for a create.
    fn component(id: Option<&str>, problem: components::Problem) -> Self {
        let id = id.map(str::to_string);
        match problem {
            components::Problem::Name(name) => Self::ComponentName { id, name },
            components::Problem::Twice(name) => Self::ComponentTwice { id, name },
            components::Problem::Docs { name, problem } => Self::ComponentDocs { id, name, problem },
        }
    }
}

/// COMP-3: the end of a component refusal — what did not happen.
fn nothing(id: &Option<String>) -> String {
    match id {
        Some(id) => format!("nothing changed for '{id}'"),
        None => "nothing was created".to_string(),
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "no ticket with id '{id}'"),
            Self::BodyTooLarge { id, bytes, max } => {
                let whose = id.as_deref().map_or_else(|| "the body".to_string(), |i| format!("{i}'s new body"));
                write!(f, "{whose} is {bytes} bytes, past the {max}-byte cap on a ticket body (--max-body-bytes); nothing was written")
            }
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
                "every shortcode starts with a scope or a doc: '{epic}' has no tickets yet, so \
                 its first ticket must be kind 'scope' (or 'doc', for a doc collection) (got \
                 '{}'); create the root first",
                db::enum_str(kind)
            ),
            Self::DocBlocked { epic, blocked_by } => write!(
                f,
                "a doc is never blocked: it has no lifecycle, so there is nothing for {} to hold \
                 back — create the doc without blocked_by (nothing was created in '{epic}')",
                blocked_by.join(", ")
            ),
            Self::BlockedByDoc { id, docs } => {
                let named: Vec<String> = docs.iter().map(|d| format!("'{d}'")).collect();
                let verb = if docs.len() == 1 { "is a doc" } else { "are docs" };
                write!(
                    f,
                    "{} {verb}: a doc never completes, so it can never unblock anything — \
                     '{id}' cannot be blocked by it (nothing was created)",
                    named.join(", ")
                )
            }
            Self::DocAwaited { id, by } => write!(
                f,
                "'{id}' cannot be born a doc: {} already name it in blocked_by, and a doc never \
                 completes, so it would never unblock them (nothing was created)",
                by.join(", ")
            ),
            Self::DocHasNoLifecycle { id } => write!(
                f,
                "'{id}' is a doc: a doc has no lifecycle — it exists in its state, Doc, and no \
                 status is set on it (nothing changed)"
            ),
            Self::DocStatusOnWork { id, kind } => write!(
                f,
                "Doc is a doc's own status: '{id}' is a {}, so it cannot be set to Doc (nothing \
                 changed)",
                kind.as_str()
            ),
            Self::DocStatusForDoc { id, status } => write!(
                f,
                "a doc's status is Doc: '{id}' would be a doc, which has no lifecycle, so it cannot be {status} — \
                 give no status, or Doc (nothing changed)"
            ),
            Self::DocStatusForWork { id, kind } => write!(
                f,
                "Doc is a doc's own status: '{id}' would be a {}, so it cannot be Doc — give a status of the \
                 lifecycle, or none (nothing changed)",
                kind.as_str()
            ),
            Self::DocInGraph { id, blocked_by, unlocks } => {
                let named = |ids: &[String]| ids.iter().map(|i| format!("'{i}'")).collect::<Vec<_>>().join(", ");
                let mut why: Vec<String> = Vec::new();
                if !blocked_by.is_empty() {
                    why.push(format!("it is blocked by {}", named(blocked_by)));
                }
                if !unlocks.is_empty() {
                    let verb = if unlocks.len() == 1 { "waits" } else { "wait" };
                    why.push(format!("{} {verb} on it", named(unlocks)));
                }
                write!(
                    f,
                    "'{id}' cannot become a doc: a doc is never blocked and never a blocker, and {} (nothing \
                     changed)",
                    why.join(", and ")
                )
            }
            Self::RootKind { id, epic, kind } => write!(
                f,
                "'{id}' is the root of {epic}: a shortcode's root is its lowest-seq scope or doc, so it cannot \
                 become a {} — make it a scope or a doc (nothing changed)",
                kind.as_str()
            ),
            Self::PathKind { id, path, kind } => write!(
                f,
                "'{id}' holds the path '{path}': only a doc carries a path (menger's own documents), so it \
                 cannot become a {} (nothing changed)",
                kind.as_str()
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
            Self::NotACommit { id, entries } => {
                let named: Vec<String> = entries.iter().map(|e| format!("'{e}'")).collect();
                let verb = if entries.len() == 1 { "is not a commit reference" } else { "are not commit references" };
                write!(
                    f,
                    "{} {verb}: a commit reference is a hexadecimal object name of 7 to 64 \
                     characters, optionally after a repository (letters, digits, '.', '_', '-') \
                     and '@' — e.g. 77df9a0, plexus-macros@4f7114a (nothing changed for '{id}')",
                    named.join(", ")
                )
            }
            Self::TooManyCommits { id, linked, adding } => write!(
                f,
                "a ticket holds at most {} commits: '{id}' has {linked}, and linking {adding} more \
                 would make {} (nothing changed for '{id}')",
                commits::MAX_PER_TICKET,
                linked + adding
            ),
            Self::Labels { id, why } => write!(f, "{why} (nothing changed for '{id}')"),
            Self::Blockers { id, why } => write!(f, "{why} (nothing changed for '{id}')"),
            Self::BlockCycle { id, path } => write!(
                f,
                "'{id}' cannot wait on that: it would close a blocked_by cycle: {id} -> {} (nothing changed)",
                path.join(" -> ")
            ),
            Self::Invalid(why) => write!(f, "{why}"),
            Self::Refused { id, refusals } => {
                let each: Vec<String> = refusals.iter().map(|r| r.to_string()).collect();
                write!(f, "update of '{id}' refused, nothing written — {}", each.join("; "))
            }
            Self::Db(msg) => write!(f, "database error: {msg}"),
            Self::SeedRefused { existing } => write!(
                f,
                "refusing to seed synthetic tickets: the store already holds {existing} ticket(s) \
                 and seeding into an existing database was not explicitly allowed"
            ),
            Self::CursorNotFound(id) => write!(f, "after: no ticket {id}"),
            Self::PathNotReserved { path } => write!(
                f,
                "a path must start with {MENGER_PATH_PREFIX} — the prefix menger reserves for its own \
                 documents — and name something after it (got '{path}'); nothing was created"
            ),
            Self::PathOnWork { path, kind } => write!(
                f,
                "only a doc carries a path: '{path}' names one of menger's own documents, and a {} is \
                 work — create it with kind doc (nothing was created)",
                kind.as_str()
            ),
            Self::PathParent { path, parent, root } => write!(
                f,
                "a {MENGER_PATH_PREFIX} document lives directly under its shortcode's scope root, {root}: \
                 '{path}' cannot be filed under {parent} — omit parent, or name {root} (nothing was \
                 created)"
            ),
            Self::PathNoRoot { epic, path } => write!(
                f,
                "'{epic}' has no scope root for '{path}' to live under — create the shortcode's root \
                 first (nothing was created)"
            ),
            Self::PathTaken { epic, path, holder } => write!(
                f,
                "'{epic}/{path}' is taken: {holder} already holds the path '{path}' in {epic} (nothing \
                 was created)"
            ),
            Self::ComponentName { id, name } if name.is_empty() => write!(
                f,
                "a ```component block names its tag — ```component m-name — and this one names none ({})",
                nothing(id)
            ),
            Self::ComponentName { id, name } => write!(
                f,
                "'{name}' is not a component name: a component is a custom element, named in lowercase \
                 ASCII, starting with a letter and holding a hyphen — e.g. m-chart ({})",
                nothing(id)
            ),
            Self::ComponentTwice { id, name } => write!(
                f,
                "the body defines the component '{name}' twice: one ```component block per name ({})",
                nothing(id)
            ),
            Self::ComponentOwned { id, name, owner } => write!(
                f,
                "the component '{name}' is defined by {owner}: a component has one owner — change it in \
                 {owner}, or give this one another name ({})",
                nothing(id)
            ),
            Self::ComponentDocs { id, name, problem } => write!(
                f,
                "the component '{name}' has a malformed <docs>: {problem} ({})",
                nothing(id)
            ),
            Self::InputMarkup { id, problem } => write!(f, "{problem} ({})", nothing(id)),
            Self::InputUnknown { ticket, input, known } if known.is_empty() => write!(
                f,
                "{ticket} declares no input '{input}' — its body holds none (an input is an <m-decision id> or an \
                 <m-input id kind>; nothing changed)"
            ),
            Self::InputUnknown { ticket, input, known } => write!(
                f,
                "{ticket} declares no input '{input}' — its inputs are {} (nothing changed)",
                known.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ")
            ),
            Self::InputInvalid { ticket, input, reason } => write!(
                f,
                "not an answer to {ticket}'s '{input}': {reason} (nothing changed)"
            ),
            Self::InputClosed { ticket, input } => write!(
                f,
                "{ticket}'s '{input}' is closed: its answer is frozen — reopen_input it first (nothing changed)"
            ),
            Self::InputRemoved { ticket, input } => write!(
                f,
                "{ticket}'s '{input}' was removed from its body: its history is kept, and it takes no change until \
                 the body declares it again (nothing changed)"
            ),
            Self::InputWithdrawn { ticket, input } => write!(
                f,
                "{ticket}'s '{input}' is withdrawn: {ticket} is canceled, so it asks no one — it takes no change \
                 until {ticket} is not canceled (nothing changed)"
            ),
            Self::AtAndAfter => write!(
                f,
                "list: give `at` (a position among the matches) or `after` (a ticket id), not both — each says \
                 where the page starts"
            ),
            Self::Filter(message) => f.write_str(message),
            Self::TreeAddress => write!(
                f,
                "tree: give `parent` (a ticket's children) or `epic` (a shortcode's top level), exactly one of them"
            ),
            Self::CommentRefused(problem) => write!(f, "{problem} (nothing was kept)"),
            Self::CommentTicket { ticket } => write!(
                f,
                "malformed anchor: ticket {ticket} names no ticket — leave it out when the element belongs to none \
                 (nothing was kept)"
            ),
            Self::CommentNotFound { id, newest: None } => write!(f, "no comment {id}: there are no comments yet (nothing changed)"),
            Self::CommentNotFound { id, newest: Some(n) } => write!(
                f,
                "no comment {id}: comment ids run from 1 to {n} (nothing changed)"
            ),
            Self::CommentCursor(id) => write!(f, "after: no comment {id} to resume after"),
            Self::NoteRefused(problem) => write!(f, "{problem} (nothing was kept)"),
            Self::NoteNotFound { id, newest: None } => write!(f, "no note {id}: there are no notes yet (nothing changed)"),
            Self::NoteNotFound { id, newest: Some(n) } => write!(f, "no note {id}: note ids run from 1 to {n} (nothing changed)"),
            Self::NoteUpgraded { id, ticket } => write!(f, "note {id} is already a ticket: {ticket} (nothing changed)"),
            Self::NoteCursor(id) => write!(f, "after: no note {id} to resume after"),
        }
    }
}

/// SCALE-14: what goes wrong on a connection is a database error, read or
/// write — so a closure run on one can use `?` on both kinds.
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
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
/// (MNG-10 — a birth never moves it once set, every later ticket having a
/// higher seq; SDLC-13: a `set_kind` that makes a lower-seq ticket a scope
/// or a doc does, in its write), rather than found by walking the prefix to
/// its first scope. That walk was one step for every prefix made since MNG-12, but
/// the whole prefix for a legacy one that never had a scope — and `create`
/// took it, under the write lock, for every ticket filed there.
fn shortcode_root<'a>(tickets: &'a Cache, epic: &str) -> Option<&'a Row> {
    tickets.rollups.root(epic).and_then(|id| tickets.get(id))
}

/// SCALE-4: every parent's `child_count`, from a freshly built children
/// index — at a build by scan, and after a seed. A step per parent, not per
/// ticket: a scan starts every ticket at 0 (SCALE-28: whatever its stored
/// row says — [`Cache::new`]) and a seed only ever finds more children than
/// a store had, never fewer, so the tickets it does not name are already
/// right. Returns how many parents it set.
fn count_children(
    order: &mut BTreeMap<OrderKey, Row>,
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
    matches!(status, Status::Complete | Status::Superseded | Status::Canceled)
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

    fn matches(&self, t: &Row) -> bool {
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
///
/// SCALE-14: `order` holds rows, not tickets — no body is held — and the
/// cache counts the writes it holds, `written`, in the critical section
/// that applies each one: a reader takes the count with the rows, and that
/// is how it knows whether the database it reads a body from is at the
/// same write ([`Store::full`]).
///
/// SCALE-28: the last three halves are stored too, and a write moves the
/// file's copy in its transaction and this one in the same critical section
/// as its rows — from one worked-out change ([`Derived`]), so the two copies
/// are one value in two places. A start reads them from the file
/// ([`stored::load`]); only a build ([`Cache::new`]) computes them.
///
/// SCALE-32 adds a sixth, [`ready::Ready`] — each ticket's count of
/// unsatisfied blockers, the ready set they decide, each shortcode's count
/// of each status — which is what `ready_set` and `list { status }` page
/// and count from; moved and stored the same way.
struct Cache {
    order: BTreeMap<OrderKey, Row>,
    at: HashMap<String, OrderKey>,
    /// WAVE-10: the path addresses ([`Paths`]).
    paths: Paths,
    rollups: rollup::Rollups,
    ids: ids::Ids,
    children: children::Children,
    ready: ready::Ready,
    /// COMP-3: the component registry ([`components::Registry`]) — read from
    /// its table at open, moved by a body's write in the same critical
    /// section as the ticket's row.
    components: components::Registry,
    /// INPUT-3: every ticket's inputs, histories and all ([`inputs::Inputs`])
    /// — read from their tables at open, moved by a body's write with its
    /// row and by an answer's write alone.
    inputs: inputs::Inputs,
    /// FILTER-4: the cell table and every shortcode's bitmap of each cell it
    /// holds ([`cells::Cells`]) — what a filter is compiled against and
    /// answered from. Moved and stored as SCALE-32's state is.
    cells: cells::Cells,
    /// COMMENT-2: every comment ([`comments::Comments`]) — read from its
    /// table at open, moved by a comment's own write and by nothing else.
    comments: comments::Comments,
    /// NOTE-2: every note ([`notes::Notes`]) — read at open, moved by `add`
    /// and by the birth of the ticket a note becomes.
    notes: notes::Notes,
    /// IDENTITY-21: every grant ([`members::Members`]) — read at open, moved
    /// by `grant` and `revoke` alone.
    members: members::Members,
    /// AUTH-7: an admin's edits of a role's grants ([`roles::RoleEdits`]) —
    /// read at open, moved by `set_role` alone.
    roles: roles::RoleEdits,
    /// MENGER-13: the transition log and its kept indexes
    /// ([`history::Log`]) — read at open, appended to by every write that
    /// changes a status or a kind, in the same critical section as its rows.
    log: history::Log,
    /// THEME-8: every theme and what the site and each shortcode say.
    themes: themes::Themes,
    written: u64,
}

/// MNG-10: the tickets half of the cache, borrowed on its own — what the
/// roll-ups walk while they update their own half under the same lock.
/// SCALE-4: with the children index beside them, which is how the roll-ups
/// find a ticket's children now that the row no longer lists them.
#[derive(Clone, Copy)]
struct Rows<'a> {
    order: &'a BTreeMap<OrderKey, Row>,
    at: &'a HashMap<String, OrderKey>,
    children: &'a children::Children,
    /// SCALE-28: one row as the write in hand will leave it — the newborn,
    /// or a ticket with its new status or parent — standing in for the
    /// cache's own (beside them, for a newborn). A write works out what it
    /// does to the derived state before it commits, so the rows it reads
    /// are the ones it is about to make, while the cache still holds the
    /// old; every lookup, test of existence and walk in the store's order
    /// sees the patch. The children index does not: only a subtree walk
    /// reads it, and only on the rows before a move
    /// ([`rollup::Rollups::reparenting`]).
    patch: Option<&'a Row>,
}

impl<'a> Rows<'a> {
    /// These rows with `row` as the write will leave it ([`Rows::patch`]).
    fn with(self, row: &'a Row) -> Self {
        Self {
            patch: Some(row),
            ..self
        }
    }

    fn get(&self, id: &str) -> Option<&'a Row> {
        match self.patch {
            Some(p) if p.id == id => Some(p),
            _ => self.order.get(self.at.get(id)?),
        }
    }

    /// SCALE-4: `t`'s children in its own shortcode, in the store's order
    /// — what a subtree walk follows.
    fn children_in_shortcode(&self, t: &'a Row) -> impl Iterator<Item = &'a Row> + 'a {
        debug_assert!(self.patch.is_none(), "the children index does not see a patch");
        let order = self.order;
        self.children
            .in_shortcode(&t.id, &t.epic)
            .into_iter()
            .flat_map(|seqs| seqs.iter())
            .filter_map(move |seq| {
                order.get(&OrderKey {
                    epic: t.epic.clone(),
                    seq,
                })
            })
    }

    /// Whether `id` names a ticket — the hash half alone, no descent.
    fn contains(&self, id: &str) -> bool {
        self.patch.is_some_and(|p| p.id == id) || self.at.contains_key(id)
    }

    fn len(&self) -> usize {
        self.at.len() + usize::from(self.patch.is_some_and(|p| !self.at.contains_key(&p.id)))
    }

    /// Every row in the store's order — SCALE-28: the patch in its place,
    /// replacing the cache's own or between its neighbours.
    fn tickets(&self) -> impl Iterator<Item = &'a Row> + 'a {
        let mut entries = self.order.iter().peekable();
        let mut patch = self.patch.map(|p| (OrderKey::of(p), p));
        std::iter::from_fn(move || {
            if let Some((key, _)) = &patch {
                if !entries.peek().is_some_and(|(k, _)| *k < key) {
                    if entries.peek().is_some_and(|(k, _)| *k == key) {
                        entries.next();
                    }
                    return patch.take().map(|(_, p)| p);
                }
            }
            entries.next().map(|(_, t)| t)
        })
    }
}

/// SCALE-28: one write's change to what the cache derives from the
/// tickets, worked out before the write commits — from the cache as it
/// stands and the rows the write will leave ([`Rows::with`]); the
/// write-serial lock is held, so nothing else moves either — and carried in
/// two halves: `stored`, the rows the write's own transaction writes beside
/// its tickets, and `moved`, what the cache becomes once that transaction
/// has committed. One value in two places, so the file and the cache are
/// never told different things; and a write that fails to commit has
/// changed neither.
///
/// Before SCALE-28 the cache worked the change out in place, after the
/// commit, and nothing stored it: a start rebuilt it all by a pass over the
/// tickets. The roll-ups are worked out on a copy now ([`rollup::Rollups`]
/// is a row per shortcode and per cross top: the cost of `groups`), and the
/// copy is what the cache takes.
struct Derived {
    stored: stored::Changes,
    moved: Moved,
}

/// The cache's half of a [`Derived`].
#[derive(Default)]
struct Moved {
    /// A create's birth into the id indexes (SCALE-3).
    birth: Option<ids::Birth>,
    /// The roll-ups as the write leaves them; `None` when it moves none.
    rollups: Option<rollup::Rollups>,
    /// The groups whose row moved, innermost first — the write's
    /// `group_changed` events (MNG-10).
    groups: Vec<Group>,
    /// SCALE-32: the counts, the ready set and the status counts as a birth
    /// or a status change moves them.
    ready: Option<ready::Move>,
    /// WAVE-6: the placements a birth or a status change across the
    /// satisfied line moves, and the written ticket's own.
    placed: Option<placements::Replaced>,
    /// FILTER-4: the cells a birth, a status change (and its fan-out) or a
    /// first or last commit moves.
    cells: Option<cells::Move>,
    /// FILTER-13: what those moves, and a `set_parent`, do to the kept
    /// tree's tallies and holders.
    tree: Option<tree::Move>,
}

impl Moved {
    /// WAVE-6: the written ticket's placement as the write leaves it, when
    /// the write re-placed it.
    fn placement(&self) -> Option<Option<Placement>> {
        self.placed.as_ref().map(|r| r.placement)
    }
}

impl Derived {
    /// A write that moves nothing derived: `amend`, `link_commits`,
    /// `unlink_commits` — and, before SCALE-32, a status change that did not
    /// cross `Complete` (every status change now moves a status count).
    fn none() -> Self {
        Self {
            stored: stored::Changes::default(),
            moved: Moved::default(),
        }
    }

    /// The roll-ups as a write leaves them, and the rows of them it stores.
    fn rolled(cache: &Cache, rollups: rollup::Rollups, groups: Vec<Group>) -> Self {
        let mut stored = stored::Changes::default();
        cache.rollups.changes(&rollups, &mut stored);
        Self {
            stored,
            moved: Moved {
                birth: None,
                rollups: Some(rollups),
                groups,
                ready: None,
                placed: None,
                cells: None,
                tree: None,
            },
        }
    }

    /// SCALE-32: and what it does to the counts, the ready set and the
    /// status counts — stored in the same transaction, taken by the cache
    /// with the rest.
    fn readied(mut self, ready: ready::Move) -> Self {
        ready.store(&mut self.stored);
        self.moved.ready = Some(ready);
        self
    }

    /// FILTER-4: and what it does to the cells — the containers and counts
    /// stored in the same transaction, the moves taken by the cache with
    /// the rest and broadcast last.
    fn celled(mut self, cells: cells::Move) -> Self {
        cells.store(&mut self.stored);
        self.moved.cells = Some(cells);
        self
    }

    /// FILTER-13: and what it does to the kept tree — its tallies and
    /// holders stored in the same transaction, taken with the rest.
    fn treed(mut self, tree: tree::Move) -> Self {
        tree.store(&mut self.stored);
        self.moved.tree = Some(tree);
        self
    }

    /// FILTER-4: what the write's broadcast says of its cell moves.
    fn announced(&self) -> Option<std::sync::Arc<cells::Moves>> {
        self.moved.cells.as_ref().and_then(cells::Move::announced)
    }
}

/// The tickets as a plain list, put in the store's order: sorted once
/// rather than inserted one at a time — `BTreeMap`'s `FromIterator` sorts
/// and bulk-builds the tree instead of rebalancing n times — and the hash
/// half that says where each id sits.
///
/// WAVE-10: and the path addresses, from the same pass as the hash half —
/// the few tickets that carry a path ([`Paths`]).
fn ordered(loaded: HashMap<String, Row>) -> (BTreeMap<OrderKey, Row>, HashMap<String, OrderKey>, Paths) {
    let order: BTreeMap<OrderKey, Row> = loaded.into_values().map(|t| (OrderKey::of(&t), t)).collect();
    let mut paths = Paths::new();
    let at = order
        .values()
        .map(|t| {
            if let Some(p) = &t.path {
                paths.insert((t.epic.clone(), p.clone()), t.id.clone());
            }
            (t.id.clone(), OrderKey::of(t))
        })
        .collect();
    (order, at, paths)
}

/// WAVE-10: every path address the store holds — `(shortcode, path)` to
/// the id that holds it. Only menger's own documents carry a path, so this
/// is a few entries a shortcode at most; a path is set at birth and never
/// moves, so only a birth adds one and nothing takes one away.
type Paths = HashMap<(String, String), String>;

impl Cache {
    /// Builds the cache from the tickets alone: their rows put in order
    /// ([`ordered`]) — 350 ms to ~545 ms at 100,000 tickets, measured, and
    /// every ordered read after it is a walk of what is already sorted —
    /// then the derived state computed by a pass over them: the children
    /// index and every `child_count` (SCALE-4), the roll-ups (MNG-10), the
    /// id indexes (SCALE-3).
    ///
    /// SCALE-28: a *build*, no longer every start's — a start reads the
    /// derived state from the file ([`stored::load`]). This is what a file
    /// that holds none gets once ([`stored::build`]), and what
    /// `--verify-derived` compares the stored state with: so it is a scan
    /// and nothing else, every count included — a row's stored
    /// `child_count` is set aside, and the count is the parents'.
    ///
    /// MENGER-12: with the inputs as stored — whether a ticket is `asking`
    /// is in its cell, and the inputs say it, not the row.
    fn new(loaded: HashMap<String, Row>, inputs: inputs::Inputs) -> Self {
        let (mut order, at, paths) = ordered(loaded);
        for t in order.values_mut() {
            t.child_count = 0;
        }
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
            patch: None,
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
        let started = std::time::Instant::now();
        let ready = ready::Ready::build(rows);
        tracing::info!(
            "counted the unsatisfied blockers of {} tickets, a ready set of {} and the status counts \
             of {} shortcodes over {} tickets in {:.1} ms",
            ready.blocked(),
            ready.size(),
            ready.shortcodes(),
            order.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        let started = std::time::Instant::now();
        let cells = cells::Cells::build(rows, |id| ready.count(id), |id| inputs.asking(id));
        tracing::info!(
            "interned {} cells and filled {} (shortcode, cell) bitmaps ({} bytes serialized) over {} \
             tickets in {:.1} ms",
            cells.keys().len(),
            cells.bitmaps(),
            cells.serialized_size(),
            order.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        let mut children = children;
        let started = std::time::Instant::now();
        let (tallies, holders) = Self::plant(&order, &at, &mut children, &cells, &ready, &inputs);
        tracing::info!(
            "kept the tree over {} tickets in {:.1} ms (FILTER-13): {tallies} tally entries over {} \
             parents, {holders} holder bitmaps",
            order.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            children.parents()
        );
        Self {
            order,
            at,
            paths,
            rollups,
            ids,
            children,
            ready,
            cells,
            // COMP-3: read beside the cache, never built from rows
            // ([`Store::with_connection`]).
            components: components::Registry::default(),
            // INPUT-3: so are the inputs — MENGER-12: but a build reads
            // them first, for the cells' `asking`.
            inputs,
            // COMMENT-2: and the comments.
            comments: comments::Comments::default(),
            // NOTE-2: and the notes.
            notes: notes::Notes::default(),
            // IDENTITY-21: and the members.
            members: members::Members::default(),
            // AUTH-7: and the edited roles.
            roles: roles::RoleEdits::new(),
            // MENGER-13: and the transition log.
            log: history::Log::began_at(None),
            themes: themes::Themes::default(),
            written: 0,
        }
    }

    /// MENGER-12: `t`'s cell as the cache stands — its row, its count of
    /// unsatisfied blockers and whether its inputs ask.
    fn key(&self, t: &Row) -> cells::Key {
        cells::Key::of(t, self.ready.count(&t.id), self.inputs.asking(&t.id))
    }

    /// FILTER-13: the kept tree's tallies and holders, built into `children`
    /// from the rows by a scan ([`tree::build`]) — at a build, after a seed,
    /// and for `--verify-derived`. Returns how many tally entries and holder
    /// bitmaps it made.
    fn plant(
        order: &BTreeMap<OrderKey, Row>,
        at: &HashMap<String, OrderKey>,
        children: &mut children::Children,
        cells: &cells::Cells,
        ready: &ready::Ready,
        inputs: &inputs::Inputs,
    ) -> (usize, usize) {
        let none = children::Children::filling();
        let rows = Rows { order, at, children: &none, patch: None };
        tree::build(rows, children, |t| {
            cells
                .id(cells::Key::of(t, ready.count(&t.id), inputs.asking(&t.id)))
                .expect("every ticket's cell is interned")
        });
        let tallies = children.every().map(|(_, k)| k.tally.len()).sum();
        let holders = children
            .every()
            .flat_map(|(_, k)| k.lists().filter_map(|(_, s)| s.many()).map(|m| m.holders.len()))
            .sum();
        (tallies, holders)
    }

    /// SCALE-28: the cache from the tickets' rows — each carrying its stored
    /// `child_count` — and the derived state as the file stores it: nothing
    /// is computed from the rows but their order.
    fn from_parts(
        loaded: HashMap<String, Row>,
        ids: ids::Ids,
        rollups: rollup::Rollups,
        children: children::Children,
        ready: ready::Ready,
        cells: cells::Cells,
    ) -> Self {
        let (order, at, paths) = ordered(loaded);
        Self {
            order,
            at,
            paths,
            rollups,
            ids,
            children,
            ready,
            cells,
            // COMP-3: read beside the cache, never built from rows
            // ([`Store::with_connection`]).
            components: components::Registry::default(),
            // INPUT-3: so are the inputs.
            inputs: inputs::Inputs::default(),
            // COMMENT-2: and the comments.
            comments: comments::Comments::default(),
            // NOTE-2: and the notes.
            notes: notes::Notes::default(),
            // IDENTITY-21: and the members.
            members: members::Members::default(),
            // AUTH-7: and the edited roles.
            roles: roles::RoleEdits::new(),
            // MENGER-13: and the transition log.
            log: history::Log::began_at(None),
            themes: themes::Themes::default(),
            written: 0,
        }
    }

    /// SCALE-28: the derived state as numbers ([`stored::Form`]) — what a
    /// build stores, and what is compared with what is stored.
    fn form(&self) -> stored::Form {
        let (runs, refs) = self.ids.stored();
        let (groups, cross, marks) = self.rollups.stored();
        let counts = self
            .order
            .values()
            .filter(|t| t.child_count > 0)
            .map(|t| (t.id.clone(), t.child_count))
            .collect();
        let (unsatisfied, statuses, ready) = self.ready.stored();
        stored::Form {
            runs,
            refs,
            groups,
            cross,
            marks,
            counts,
            unsatisfied,
            statuses,
            ready,
            cells: self.cells.stored(),
            tree: tree::stored(&self.children),
        }
    }

    fn rows(&self) -> Rows<'_> {
        Rows {
            order: &self.order,
            at: &self.at,
            children: &self.children,
            patch: None,
        }
    }

    /// WAVE-6: every row's placement from the rows alone
    /// ([`placements::place_all`]) — at open and after a seed. Logged with
    /// what it cost: the startup recompute WAVE-3 measures.
    fn place_all(&mut self) {
        if cfg!(feature = "wave3-no-replace") {
            return;
        }
        let started = std::time::Instant::now();
        let placed = placements::place_all(self.rows());
        let (mut waves, mut missing, mut done) = (0usize, 0usize, 0usize);
        for (t, p) in self.order.values_mut().zip(placed) {
            match p {
                Some(Placement::Wave { .. }) => waves += 1,
                Some(Placement::MissingBlocker) => missing += 1,
                Some(Placement::Done) => done += 1,
                None => {}
            }
            t.placement = p;
        }
        tracing::info!(
            target: "wave6",
            micros = started.elapsed().as_micros() as u64,
            "WAVE-6: placed {} tickets — {waves} in a wave, {missing} missing a blocker, {done} done — \
             from the rows in {:.1} ms",
            waves + missing + done,
            started.elapsed().as_secs_f64() * 1000.0
        );
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
    fn insert(&mut self, ticket: Row) {
        let key = OrderKey::of(&ticket);
        self.at.insert(ticket.id.clone(), key.clone());
        self.order.insert(key, ticket);
    }

    /// SCALE-28: what the birth of `newborn` does to the derived state,
    /// before it commits: its birth into the id indexes (SCALE-3), its
    /// parent's new `child_count` (SCALE-4 — the newborn is new, so never
    /// already one of its parent's children), and the roll-ups, worked out
    /// on a copy over the rows with the newborn among them (MNG-10).
    /// SCALE-32: and its own count of unsatisfied blockers and its status
    /// counted ([`ready::Ready::born`]). WAVE-6: and its placement, with the
    /// placements of the tickets a forward reference to it held missing,
    /// and theirs ([`placements::replace`]).
    ///
    /// MENGER-12: `asking` — whether the newborn's body asks (an input born
    /// open): its cell says so from birth.
    fn birth(&self, newborn: &Row, asking: bool) -> Derived {
        let rows = self.rows().with(newborn);
        let birth = self.ids.birth(rows, newborn);
        let mut rollups = self.rollups.clone();
        let groups = rollups.created(rows, newborn);
        let (ready, touched) = self.ready.born_touching(rows, newborn);
        let (cells, tree) = self.recell(rows, newborn, true, &touched, Some(asking));
        let mut derived = Derived::rolled(self, rollups, groups).readied(ready).celled(cells).treed(tree);
        birth.store(&mut derived.stored);
        if let Some(p) = &newborn.parent {
            derived.stored.counts.push((p.clone(), self.children.count(p) + 1));
        }
        derived.moved.birth = Some(birth);
        derived.moved.placed = Some(Self::replace(rows, newborn, true));
        derived
    }

    /// WAVE-6: the re-placement pass of a write ([`placements::replace`]),
    /// unless WAVE-3's measurement build leaves it out.
    fn replace(rows: Rows<'_>, written: &Row, born: bool) -> placements::Replaced {
        #[cfg(feature = "wave3-no-replace")]
        {
            let _ = (rows, born);
            return placements::Replaced {
                placement: written.placement,
                ..Default::default()
            };
        }
        #[cfg(not(feature = "wave3-no-replace"))]
        placements::replace(rows, written, born)
    }

    /// MNG-10: a create's rows — the newborn and every ticket it touched —
    /// go in, then the newborn is counted. Returns the groups whose row
    /// moved, innermost first. SCALE-3: and it is born into the id
    /// indexes — its shortcode's sequence, the forward references it
    /// resolved, the ones it makes. SCALE-4: and into its parent's
    /// children, whose count it moves; returns that count too. SCALE-28:
    /// the indexes and roll-ups as [`Cache::birth`] worked them out.
    fn created(
        &mut self,
        ticket: Row,
        touched: impl IntoIterator<Item = Row>,
        moved: Moved,
    ) -> (Vec<Group>, Option<u64>) {
        for updated in touched {
            self.insert(updated);
        }
        let key = OrderKey::of(&ticket);
        let parent = ticket.parent.clone();
        // WAVE-10: its address, if it has one — checked free under the
        // write-serial lock before the commit.
        if let Some(p) = &ticket.path {
            self.paths.insert((ticket.epic.clone(), p.clone()), ticket.id.clone());
        }
        self.insert(ticket);
        let count = parent.map(|p| {
            let count = self.children.add(&p, &key);
            self.set_child_count(&p, count);
            count
        });
        (self.take(moved), count)
    }

    /// SCALE-28: what `row`'s status change from `previous` does to the
    /// roll-ups, before it commits — nothing, copied or stored, unless it
    /// crosses `Complete`. SCALE-32: and to the status counts, the ready set
    /// and — across the satisfied line — the counts of the tickets it
    /// unlocks ([`ready::Ready::status_changed`]), whatever the roll-ups do.
    /// WAVE-6: and, across the satisfied line, its placement and every one
    /// downstream that moves with it ([`placements::replace`]) — on one side
    /// of the line, no placement moves.
    #[cfg(test)]
    fn status_change(&self, row: &Row, previous: Status) -> Derived {
        self.status_change_asking(row, previous, None)
    }

    /// MENGER-13: [`Cache::status_change`] with `asking` — whether the ticket
    /// asks as the write leaves it, when the write moves its inputs (a
    /// cancel withdraws them, leaving `Canceled` restores them).
    fn status_change_asking(&self, row: &Row, previous: Status, asking: Option<bool>) -> Derived {
        let rows = self.rows().with(row);
        let derived = if rollup::Rollups::crosses(previous, row.status) {
            let mut rollups = self.rollups.clone();
            let groups = rollups.status_changed(rows, row, previous);
            Derived::rolled(self, rollups, groups)
        } else {
            Derived::none()
        };
        let (ready, touched) = self.ready.status_changed_touching(rows, row, previous);
        let (cells, tree) = self.recell(rows, row, false, &touched, asking);
        let mut derived = derived.readied(ready).celled(cells).treed(tree);
        if satisfied(previous) != satisfied(row.status) {
            derived.moved.placed = Some(Self::replace(rows, row, false));
        }
        derived
    }

    /// FILTER-4: what a write does to the cells, before it commits: each
    /// ticket the write's ready move touched (`touched`, SCALE-32's
    /// fan-out, the written ticket among them: its row as the write leaves
    /// it, its count before and after), each from the cell it is in to the
    /// one its row and its count put it in now ([`cells::Cells::moved`]).
    /// The written ticket's cell before is its row before (`born`: none).
    ///
    /// FILTER-13: and what those moves do to the kept tree ([`tree::Plan`]):
    /// each moved ticket's count leaves its old cell and joins its new one
    /// in each of its ancestors' tallies; a newborn joins its parent's
    /// children — and, where a hand edit had named its id as a parent, its
    /// subtree's tally is counted in its ancestors too. `rows` are the rows
    /// as the write leaves them.
    ///
    /// MENGER-12: `asking` — whether the written ticket asks as the write
    /// leaves it, when the write moves its inputs (`None`: as the cache's
    /// inputs say, before and after). Every other ticket asks as it did.
    fn recell<'a>(
        &'a self,
        rows: Rows<'a>,
        written: &'a Row,
        born: bool,
        touched: &[ready::Touched<'a>],
        asking: Option<bool>,
    ) -> (cells::Move, tree::Move) {
        let mut moves: Vec<(&'a Row, Option<cells::Key>, cells::Key)> = Vec::with_capacity(touched.len());
        for (t, was, now) in touched {
            let asks = self.inputs.asking(&t.id);
            let (before, after) = if t.id != written.id {
                (Some(cells::Key::of(t, *was, asks)), cells::Key::of(t, *now, asks))
            } else if born {
                (None, cells::Key::of(t, *now, asking.unwrap_or(false)))
            } else {
                (
                    self.get(&t.id).map(|old| cells::Key::of(old, *was, asks)),
                    cells::Key::of(t, *now, asking.unwrap_or(asks)),
                )
            };
            moves.push((*t, before, after));
        }
        let cells = self.cells.moved(moves.iter().copied());
        let mut plan = tree::Plan::new(&self.children, rows);
        for (t, before, after) in &moves {
            if *before == Some(*after) {
                continue;
            }
            if let Some(b) = before {
                plan.count(t, cells.id_of(&self.cells, *b), -1);
            }
            plan.count(t, cells.id_of(&self.cells, *after), 1);
        }
        if born {
            if self.children.kids(&written.id).is_some() {
                let chain = tree::ancestors(rows, &written.id, written.parent.as_deref());
                plan.subtree(written, None, &chain, 1);
            }
            if let Some(p) = written.parent.as_deref() {
                plan.join(p, written);
            }
        }
        (cells, plan.finish())
    }

    /// FILTER-4 / MENGER-12: what a write that moves `row`'s cell and
    /// nothing else derived does, before it commits — a first commit or a
    /// last, a label, or (`asking`: whether it asks as the write leaves it)
    /// an input opened or settled. Nothing, unless the cell moves.
    fn cell_change(&self, row: &Row, asking: Option<bool>) -> Derived {
        let count = self.ready.count(&row.id);
        let (cells, tree) = self.recell(self.rows().with(row), row, false, &[(row, count, count)], asking);
        Derived::none().celled(cells).treed(tree)
    }

    /// FILTER-4: a `link_commits` or `unlink_commits`, once it has committed:
    /// the row, and the cell move [`Cache::cell_change`] worked out —
    /// MENGER-12: a `label`'s too.
    fn commits_changed(&mut self, row: Row, moved: Moved) {
        self.insert(row);
        let groups = self.take(moved);
        debug_assert!(groups.is_empty(), "a link moves no roll-up");
    }

    /// MNG-10: a status change; only a crossing of `Complete` moves a count.
    /// AGS-23: but a shortcode root's status is on its group's row
    /// ([`Group::root_row`]), so any change of it moves that group — first,
    /// as the innermost, when the crossing did not already report it.
    fn status_changed(&mut self, ticket: Row, previous: Status, moved: Moved) -> Vec<Group> {
        let changed = ticket.status != previous;
        let (epic, id) = (ticket.epic.clone(), ticket.id.clone());
        self.insert(ticket);
        let mut groups = self.take(moved);
        if changed && !groups.iter().any(|g| g.epic == epic) {
            if let Some(g) = self.root_group(&epic, &id) {
                groups.insert(0, g);
            }
        }
        groups
    }

    /// SDLC-13: what `row`'s change of kind (from `old`, its row before —
    /// and of status with it, when the write moves one) does to the derived
    /// state, before it commits. Everything kept by kind or by status moves
    /// here, from one worked-out change:
    ///
    /// - the roll-ups ([`rollup::Rollups::kind_changed`]) — the ticket's
    ///   tally moves between work and docs, a crossing of `Complete` moves
    ///   `done`, and a ticket that now roots its shortcode below its root
    ///   (or roots a rootless one) re-roots it, its shape settled again;
    /// - the status counts, the ready set and — across the satisfied line —
    ///   the counts of the tickets it unlocks, as a status change moves them
    ///   ([`ready::Ready::status_changed_touching`]; nothing, when the status
    ///   stays);
    /// - the cells and the kept tree ([`Cache::recell`]: the ticket's cell
    ///   moves, kind being in the cell, and so does every cell the fan-out
    ///   moved);
    /// - its placement — none for a doc, placed by the rule for anything
    ///   else ([`placements::replace`]) — and every one downstream that moves,
    ///   across the doc line or the satisfied line.
    fn kind_change(&self, row: &Row, old: &Row, asking: Option<bool>) -> Derived {
        let rows = self.rows().with(row);
        let mut rollups = self.rollups.clone();
        let derived = match rollups.kind_changed(rows, old, row) {
            Some(groups) => Derived::rolled(self, rollups, groups),
            None => Derived::none(),
        };
        let (ready, touched) = self.ready.status_changed_touching(rows, row, old.status);
        let (cells, tree) = self.recell(rows, row, false, &touched, asking);
        let mut derived = derived.readied(ready).celled(cells).treed(tree);
        if satisfied(old.status) != satisfied(row.status) || old.kind.has_lifecycle() != row.kind.has_lifecycle() {
            derived.moved.placed = Some(Self::replace(rows, row, false));
        }
        derived
    }

    /// SDLC-13: a `set_kind`, once it has committed — the row, then what
    /// [`Cache::kind_change`] worked out. AGS-23: a shortcode root's kind and
    /// status are on its group's row ([`Group::root_row`]), so a change of
    /// either moves that group — first, as the innermost, when the roll-ups
    /// did not already report it.
    fn kind_changed(&mut self, ticket: Row, moved: Moved) -> Vec<Group> {
        let (epic, id) = (ticket.epic.clone(), ticket.id.clone());
        self.insert(ticket);
        let mut groups = self.take(moved);
        if !groups.iter().any(|g| g.epic == epic) {
            if let Some(g) = self.root_group(&epic, &id) {
                groups.insert(0, g);
            }
        }
        groups
    }

    /// MNG-16: an amend — rows only: what it changes is counted by no
    /// roll-up and indexed by no id index. AGS-23: but a shortcode root's
    /// title is on its group's row, so a new one moves that group.
    fn amended(&mut self, row: Row, title_changed: bool) -> Vec<Group> {
        let (epic, id) = (row.epic.clone(), row.id.clone());
        self.insert(row);
        if !title_changed {
            return Vec::new();
        }
        self.root_group(&epic, &id).into_iter().collect()
    }

    /// AGS-23: `epic`'s group as it stands, dressed — when `id` is the
    /// shortcode's root and the nav draws it a row.
    fn root_group(&self, epic: &str, id: &str) -> Option<Group> {
        if self.rollups.root(epic) != Some(id) {
            return None;
        }
        self.rollups.group(epic).map(|g| self.dress(g))
    }

    /// AGS-23: a group as the wire carries it — the roll-ups' counts, root
    /// and host, and the root's own row ([`Group::root_row`]) from the rows
    /// as they stand. The roll-ups keep no ticket's fields; every group
    /// leaving the cache passes through here.
    fn dress(&self, mut g: Group) -> Group {
        g.root_row = g.root.as_deref().and_then(|r| self.get(r)).map(|t| RootRow {
            title: t.title.clone(),
            status: t.status,
            kind: t.kind,
        });
        g
    }

    /// SCALE-28: what moving `child` (as the move leaves it) off `previous`
    /// does to the derived state, before it commits. MNG-10: the moved
    /// tree's count is taken from where it is counted *before* the rows
    /// change (that is where the old parent still is) and given to where it
    /// will be; then the shape is settled against the rows as they will be.
    /// SCALE-4: each parent's count moves by one — the child leaves the old
    /// parent's children, where it was, and joins the new one's, where it
    /// was not (the parent it already has is no write).
    fn reparent(&self, child: &Row, previous: Option<&str>) -> Derived {
        let parent = child.parent.as_deref().expect("set_parent sets a parent");
        let rows = self.rows();
        let mut rollups = self.rollups.clone();
        let moving = rollups.reparenting(
            rows,
            rows.get(&child.id).expect("the child exists"),
            rows.get(parent).expect("the parent exists"),
        );
        let groups = rollups.reparented(rows.with(child), moving);
        let mut derived = Derived::rolled(self, rollups, groups);
        derived.stored.counts.push((parent.to_string(), self.children.count(parent) + 1));
        if let Some(p) = previous {
            derived.stored.counts.push((p.to_string(), self.children.count(p).saturating_sub(1)));
        }
        derived.treed(self.regraft(child, previous))
    }

    /// FILTER-13: what moving `child` (as the move leaves it) off `previous`
    /// does to the kept tree: its subtree's tally — its own cell and its
    /// tally — leaves each of its old ancestors and joins each of its new
    /// ones; it leaves the old parent's list and joins the new one's. Its
    /// descendants' tallies do not move, and none of them is looked at. A
    /// ticket on a hand-edited cycle has a subtree that holds its own
    /// ancestors: the move breaks the cycle, and the tree is built again.
    fn regraft(&self, child: &Row, previous: Option<&str>) -> tree::Move {
        let rows = self.rows();
        let moved = rows.with(child);
        let cell = self.cells.id(self.key(child)).expect("every ticket's cell is interned");
        if tree::on_cycle(rows, &child.id, previous) {
            tracing::warn!(
                "{} is on a parent cycle (a hand edit): moving it breaks the cycle, and the kept tree is built \
                 again from the rows",
                child.id
            );
            let none = children::Children::filling();
            let bare = Rows { children: &none, ..moved };
            let mut rebuilt = children::Children::build(bare.tickets());
            tree::build(bare, &mut rebuilt, |t| self.cells.id(self.key(t)).expect("every ticket's cell is interned"));
            return tree::Move::rebuilt(rebuilt);
        }
        let mut plan = tree::Plan::new(&self.children, moved);
        let before = tree::ancestors(rows, &child.id, previous);
        let after = tree::ancestors(moved, &child.id, child.parent.as_deref());
        plan.subtree(child, Some(cell), &before, -1);
        plan.subtree(child, Some(cell), &after, 1);
        if let Some(p) = previous {
            plan.leave(p, child);
        }
        if let Some(p) = child.parent.as_deref() {
            plan.join(p, child);
        }
        plan.finish()
    }

    /// MNG-10: a `set_parent`, once it has committed. SCALE-4: the child is
    /// the only row that changes. It leaves `previous`'s children and joins
    /// its new parent's, and each parent's count is set from the index;
    /// returns the groups, then the new parent's count and the old one's.
    fn reparented(&mut self, child: Row, previous: Option<&str>, moved: Moved) -> (Vec<Group>, u64, u64) {
        let parent = child.parent.clone().expect("set_parent sets a parent");
        let key = OrderKey::of(&child);
        self.insert(child);
        let left = previous.map_or(0, |p| {
            let count = self.children.remove(p, &key);
            self.set_child_count(p, count);
            count
        });
        let joined = self.children.add(&parent, &key);
        self.set_child_count(&parent, joined);
        (self.take(moved), joined, left)
    }

    /// SCALE-28: the cache's half of a committed write's [`Derived`].
    fn take(&mut self, moved: Moved) -> Vec<Group> {
        if let Some(birth) = moved.birth {
            self.ids.born(birth);
        }
        // WAVE-6: after the write's own rows went in, so a placement it
        // moved is never undone by a row it carried.
        if let Some(placed) = moved.placed {
            for shift in placed.shifts {
                if let Some(t) = self.order.get_mut(&shift.key) {
                    t.placement = Some(shift.placement);
                }
            }
        }
        if let Some(rollups) = moved.rollups {
            self.rollups = rollups;
        }
        if let Some(ready) = moved.ready {
            self.ready.moved(ready);
        }
        if let Some(cells) = moved.cells {
            self.cells.take(cells);
        }
        if let Some(tree) = moved.tree {
            tree::take(&mut self.children, tree);
        }
        moved.groups.into_iter().map(|g| self.dress(g)).collect()
    }

    fn get(&self, id: &str) -> Option<&Row> {
        self.order.get(self.at.get(id)?)
    }

    fn len(&self) -> usize {
        self.at.len()
    }
}

/// SCALE-14: how many read-only connections a store keeps open — and so
/// how many reads touch the database at once. A read holds one for a point
/// read (~10 µs warm, SCALE-12) or a step of bodies, so a few serve every
/// caller; a bound keeps a burst of readers from opening a connection (and
/// a page cache) each.
const READERS: usize = 4;

/// SCALE-14: how many bodies a full-ticket walk holds at once — the page
/// MNG-9's clients walk in. `list {}` and `ready_set {}` without `summary`
/// still answer every match with its body, as they always did, but a
/// [`Walk`] reads them [`BODIES_PER_STEP`] at a time, one step ahead of
/// the step it is handing out: never more than this many in hand.
pub const BODIES_AT_ONCE: u32 = 200;

/// SCALE-14: how many bodies one step of a [`Walk`] reads — half of
/// [`BODIES_AT_ONCE`], because the next step's are read while this one's go
/// out. Read in turn instead, the bodies cost a whole-store `list {}`
/// ~1.5 s at 10⁶ on top of sending it (measured: 7.7 s against 6.2 s).
pub const BODIES_PER_STEP: u32 = BODIES_AT_ONCE / 2;

/// SCALE-14: the database's read side. A body is read on a connection of
/// its own, not the writer's: SQLite in WAL mode lets a reader run beside
/// a writer, but one connection shared behind a mutex would make every
/// read wait out every write's fsync (SCALE-12, "Limits").
enum Readers {
    /// A file: [`READERS`] connections, opened read-only — they can never
    /// write — when the store opens, each kept for the next read once a
    /// read is done with it.
    File {
        path: PathBuf,
        idle: StdMutex<Vec<Connection>>,
        /// One permit per connection that may be open: a read waits here,
        /// for a free connection, not on a lock — and the permit goes into
        /// the blocking task with the connection, so a reader that is
        /// dropped mid-read cannot let a second read past the bound.
        permits: Arc<Semaphore>,
    },
    /// `:memory:` (the unit tests' store) has no file a second connection
    /// could open: its reads take the writer's connection in turn.
    Writer(Arc<StdMutex<Connection>>),
}

impl Readers {
    /// The read side of the database at `path`, every connection opened
    /// now and each through one read — which is when SQLite opens the WAL
    /// beside the file. So a database the reads cannot open fails the open,
    /// loudly, rather than the first `get`; and after it the read path
    /// opens nothing by name: its files are held, as the writer's are, and
    /// reads go on answering whatever becomes of the path (MNG-2's
    /// "the database location vanishes mid-run").
    fn open(path: &Path) -> anyhow::Result<Self> {
        let idle = (0..READERS)
            .map(|_| Self::connect(path))
            .collect::<rusqlite::Result<Vec<Connection>>>()?;
        Ok(Self::File {
            path: path.to_path_buf(),
            idle: StdMutex::new(idle),
            permits: Arc::new(Semaphore::new(READERS)),
        })
    }

    fn connect(path: &Path) -> rusqlite::Result<Connection> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )?;
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))?;
        Ok(conn)
    }

    /// Runs `f` on a read connection, off the async runtime. `f` begins and
    /// ends its own transaction: a connection goes back idle with none open,
    /// so the next read on it starts from the newest commit.
    async fn read<T, F>(self: &Arc<Self>, f: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    {
        let permit = match &**self {
            Self::File { permits, .. } => Some(
                Arc::clone(permits)
                    .acquire_owned()
                    .await
                    .expect("the read permits are never closed"),
            ),
            Self::Writer(_) => None,
        };
        let readers = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            match &*readers {
                Self::File { path, idle, .. } => {
                    let open = idle.lock().expect("readers lock poisoned").pop();
                    // A permit is a connection: none idle only if a read
                    // panicked with one, and then it is opened again.
                    let mut conn = match open {
                        Some(conn) => conn,
                        None => Self::connect(path)?,
                    };
                    let out = f(&mut conn);
                    idle.lock().expect("readers lock poisoned").push(conn);
                    out
                }
                Self::Writer(db) => f(&mut db.lock().expect("db lock poisoned")),
            }
        })
        .await
        .map_err(|e| StoreError::Db(format!("read task failed: {e}")))?
    }
}

pub struct Store {
    /// The cache. Every read in the process starts here; the lock is
    /// synchronous and never held across an await. SCALE-14: it holds
    /// rows, and a read that answers whole tickets takes their bodies from
    /// `readers` after it has let go of it. MNG-20: shared with the write in
    /// flight ([`Store::write`]), which applies itself whether or not its
    /// caller is still there to do it.
    tickets: Arc<RwLock<Cache>>,
    tx: Broadcast,
    /// The system of record. Private to this module — nothing else in the
    /// application holds a database handle. Only touched off-runtime, via
    /// `spawn_blocking`. SCALE-14: the writer; reads have their own.
    db: Arc<StdMutex<Connection>>,
    /// SCALE-14: the read-only connections bodies are read on.
    readers: Arc<Readers>,
    /// SCALE-14: how many writes have begun to commit — counted before the
    /// commit starts, so a reader that finds it equal to the count the
    /// cache gave it knows no commit can have reached its snapshot that
    /// the rows it holds do not show ([`Store::full`]).
    committing: Arc<AtomicU64>,
    /// FILTER-13: the open `watch { filter }`s whose filter asks `under` —
    /// what each write works out membership for, while any is open.
    unders: Arc<lineage::UnderWatches>,
    /// Serializes mutations end to end: id allocation, the durable write,
    /// the cache update, and the broadcast happen atomically with respect
    /// to other writers, so ids are unique under concurrent creates and
    /// `watch` subscribers see changes in acknowledgment order. Readers
    /// never take this lock. MNG-20: a write takes it owned and hands it to
    /// [`Store::write`], which lets it go only once the write has been
    /// broadcast — so the caller going away does not let the next write in
    /// before the cache holds this one.
    write_serial: Arc<AsyncMutex<()>>,
    /// MENGER-15: the longest body `create` and `amend` store, in bytes
    /// ([`BODY_MAX_DEFAULT`] unless [`Store::with_body_max`] said).
    body_max: usize,
}

/// MENGER-15: the default cap on a ticket body — 8 MiB, room for a whole
/// page with its media inlined. Bodies are never in the cache (SCALE-14):
/// the cap bounds what one `get` reads and one write commits, not what the
/// process holds. It sits under the WebSocket's own 10 MB request limit
/// (jsonrpsee's default) with room for JSON's escaping.
pub const BODY_MAX_DEFAULT: usize = 8 * 1024 * 1024;

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
        Self::with_connection(conn, None).expect("init in-memory schema")
    }

    /// Opens (creating if absent) the database at `path` and loads every
    /// ticket's row into the cache (SCALE-14: not its body). A fresh
    /// location is an empty store, not an error. Does blocking I/O — call
    /// from a blocking context (the builder wraps this in `spawn_blocking`).
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        // WAL + synchronous=FULL: an acknowledged commit survives SIGKILL.
        // SCALE-14: WAL is also what lets the read connections read beside
        // the writer instead of behind it.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA synchronous = FULL;")?;
        Self::with_connection(conn, Some(path))
    }

    /// The store over the writer `conn`. SCALE-14: the read connections are
    /// opened after every migration, on `path` — or, with none (`:memory:`),
    /// reads take the writer's turn.
    fn with_connection(mut conn: Connection, path: Option<&Path>) -> anyhow::Result<Self> {
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
        // A database written before SDLC-9 gains `commits` here, once,
        // durably — every ticket with none linked. Adding a column with a
        // constant default rewrites no row, so this is instant at any size.
        db::migrate_add_commits(&mut conn)?;
        // WAVE-10: a database written before WAVE-10 gains `path` here, once,
        // durably — every ticket without one; and the index that keeps a
        // path unique in its shortcode. No row is rewritten.
        db::migrate_add_path(&mut conn)?;
        // MENGER-12: a database written before MENGER-12 gains `labels` here,
        // once, durably — every ticket with none. No row is rewritten.
        db::migrate_add_labels(&mut conn)?;
        // COMP-3: a database written before COMP-3 — or a new one — gains the
        // `components` table here, once, durably, with the registry built
        // from every body; after it, every open reads the table.
        components::migrate(&mut conn)?;
        // INPUT-3: a database written before INPUT-3 — or a new one — gains
        // the `inputs` and `answers` tables here, once, durably, with every
        // body's inputs read in; after it, every open reads them.
        inputs::migrate(&mut conn)?;
        // SEARCH-2: a database written before SEARCH-2 — or a new one — gains
        // the search index here, once, durably, every ticket's title and text
        // indexed; after it, every write that changes either keeps it.
        search::migrate(&mut conn)?;
        // NOTE-2: a file without the notes table gains it, empty, here.
        notes::migrate(&mut conn)?;
        // MENGER-13: and one without the transition log gains it, empty,
        // with the moment it began — no ticket gets a history it never had.
        history::migrate(&mut conn)?;
        // THEME-8: and one without the themes tables gains them, empty.
        themes::migrate(&mut conn)?;
        let cache = Self::load_cache(&mut conn)?;
        let db = Arc::new(StdMutex::new(conn));
        let readers = match path {
            Some(path) => Readers::open(path)?,
            None => Readers::Writer(Arc::clone(&db)),
        };
        let tx = Broadcast::new();
        Ok(Self {
            tickets: Arc::new(RwLock::new(cache)),
            tx,
            db,
            readers: Arc::new(readers),
            committing: Arc::new(AtomicU64::new(0)),
            unders: Arc::default(),
            write_serial: Arc::new(AsyncMutex::new(())),
            body_max: BODY_MAX_DEFAULT,
        })
    }

    /// MENGER-15: the store with its body cap at `max` bytes (main's
    /// `--max-body-bytes`). Set before the store is shared.
    pub fn with_body_max(mut self, max: usize) -> Self {
        self.body_max = max;
        self
    }

    /// MENGER-15: the longest body `create` and `amend` store, in bytes.
    pub fn body_max(&self) -> usize {
        self.body_max
    }

    /// MENGER-15: refuse a body past the cap — before anything is read,
    /// locked or allocated.
    fn check_body(&self, id: Option<&str>, body: &str) -> Result<(), StoreError> {
        if body.len() > self.body_max {
            return Err(StoreError::BodyTooLarge { id: id.map(str::to_string), bytes: body.len(), max: self.body_max });
        }
        Ok(())
    }

    /// The cache from the file `conn` holds, every migration made — the
    /// derived state read (or built, once), the placements, and every
    /// record beside the tickets. MENGER-13: also what a batch write whose
    /// commit failed goes back to ([`Store::batch`]): the file is as it was.
    fn load_cache(conn: &mut Connection) -> anyhow::Result<Cache> {
        // SCALE-28: the derived state is read from the file — each
        // shortcode's run, the dangling forward references, the roll-ups,
        // the children index — beside the rows, each carrying its stored
        // `child_count`. A file that holds none (written before SCALE-28,
        // or new), or holds it behind its tickets (a seed that stopped),
        // has it built here, once, from the tickets, durably and in one
        // transaction, before anything is handed out; the cache that build
        // made is the one the store starts with.
        // COMMENT-2: a file in form 6 gains form 7 — the comments table,
        // empty — in place: nothing derived moved, so nothing is built again.
        let mut found = stored::state(conn)?;
        if let stored::State::Older(form) = found {
            if stored::upgrade(conn, form)? {
                found = stored::State::Current;
            }
        }
        let cache = match found {
            stored::State::Current => {
                let (rows, unsatisfied) = db::load_rows_counted(conn)?;
                stored::load(conn, rows, unsatisfied)?
            }
            found => stored::build(conn, found)?,
        };
        // WAVE-6: every placement, from the rows — nothing stores them, so
        // none can be stale after a restart.
        let mut cache = cache;
        cache.place_all();
        // COMP-3: the registry, as stored.
        cache.components = components::load(conn)?;
        // INPUT-3: the inputs and their histories, as stored.
        cache.inputs = inputs::load(conn)?;
        // COMMENT-2: the comments, as stored.
        cache.comments = comments::load(conn)?;
        // NOTE-2: the notes, as stored.
        cache.notes = notes::load(conn)?;
        // IDENTITY-21: the members, as stored.
        cache.members = members::load(conn)?;
        // AUTH-7: the edited roles, as stored.
        cache.roles = roles::load(conn)?;
        // MENGER-13: the transition log and its kept indexes.
        cache.log = history::load(conn, &cache.at)?;
        // THEME-8: the themes and settings, as stored.
        themes::load(conn, &mut cache.themes)?;
        Ok(cache)
    }

    /// SCALE-28: `menger --verify-derived` — the file at `path` opened
    /// read-only, its four derived structures built by a scan of its tickets
    /// and compared with what it stores ([`Verdict`]). No store is opened,
    /// nothing is migrated, nothing is written: the connection cannot write,
    /// and it is the store's own, opened and closed inside this module.
    pub fn verify_derived(path: &Path) -> anyhow::Result<Verdict> {
        stored::verify(path)
    }

    /// MNG-20: the write-serial lock, owned — what a write takes first and
    /// hands to [`Store::write`] with the rest of it.
    async fn serial(&self) -> OwnedMutexGuard<()> {
        Arc::clone(&self.write_serial).lock_owned().await
    }

    /// One write, from its commit to its broadcast, run to the end whether
    /// or not its caller is still waiting for it (MNG-20).
    ///
    /// A write is three steps, in this order: `commit` makes it durable in
    /// one transaction, off the async runtime — changing the rows and
    /// reading back what its events carry (SCALE-14: the bodies of the
    /// tickets they carry whole, a point read in the transaction that is
    /// about to fsync anyway); `apply` puts it into the cache, under the
    /// cache's lock, where it is counted ([`apply`]); `publish` broadcasts
    /// it, the lock let go, and makes the caller's answer. Each runs once,
    /// or — when the commit fails, and nothing has been acknowledged — only
    /// the first: the cache is untouched and nothing is emitted.
    ///
    /// Until MNG-20 only the commit ran off the caller: the caller awaited
    /// it, then applied and broadcast the write itself. A caller that went
    /// away during that await — a dropped socket, a cancelled request —
    /// dropped the write there: the commit landed, and the cache and
    /// `watch` never heard of it. The database was then ahead of the cache
    /// until a restart, and because ids are allocated from the cache, the
    /// next `create` in that shortcode was handed the same id and its
    /// upsert overwrote the stored row (`tests_detached` reproduced it).
    /// Now all three run in one blocking task, which nothing cancels once
    /// it is spawned, and `serial` — the write-serial guard the write was
    /// prepared under — goes with them, let go only once the write is
    /// broadcast. The caller waits for the answer and may stop waiting;
    /// what it loses is the answer. Everything a write does before it gets
    /// here — the lock, its reads, its checks — changes nothing, so a
    /// caller that goes away there leaves no trace either.
    ///
    /// SCALE-14: the write is counted in `committing` before its commit
    /// starts and in the cache's `written` when `apply` puts it there —
    /// what a reader compares to know the database it read is the cache's.
    /// Both now happen whatever becomes of the caller, so the two counts
    /// agree again after every write.
    async fn write<T, U, R>(
        &self,
        serial: OwnedMutexGuard<()>,
        commit: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, StoreError> + Send + 'static,
        apply: impl FnOnce(&mut Cache, &T) -> U + Send + 'static,
        publish: impl FnOnce(T, U, &Broadcast) -> R + Send + 'static,
    ) -> Result<R, StoreError>
    where
        T: Send + 'static,
        U: Send + 'static,
        R: Send + 'static,
    {
        let (db, tickets, tx) = (Arc::clone(&self.db), Arc::clone(&self.tickets), self.tx.clone());
        let committing = Arc::clone(&self.committing);
        // Counted and spawned in one poll: there is no await between them
        // for a caller to be dropped at.
        committing.fetch_add(1, Ordering::SeqCst);
        tokio::task::spawn_blocking(move || {
            let _serial = serial;
            let durable = {
                let mut conn = db.lock().expect("db lock poisoned");
                conn.transaction()
                    .map_err(StoreError::from)
                    .and_then(|txn| {
                        let out = commit(&txn)?;
                        txn.commit()?;
                        Ok(out)
                    })
            };
            let out = match durable {
                Ok(out) => out,
                Err(e) => {
                    // Nothing committed, so nothing is on its way to the
                    // cache: a reader may trust its rows again.
                    committing.fetch_sub(1, Ordering::SeqCst);
                    return Err(e);
                }
            };
            let applied = self::apply(&tickets, |cache| apply(cache, &out));
            Ok(publish(out, applied, &tx))
        })
        .await
        .map_err(|e| StoreError::Db(format!("write task failed: {e}")))?
    }

    /// SCALE-14: whole tickets for `rows`, which the cache answered while it
    /// held `written` writes — each row with its body from the database, in
    /// the rows' order. Every read that answers a full ticket ends here.
    ///
    /// The rows were read first, and the bodies after, on a read connection
    /// in one transaction — one snapshot. That order is what makes a read
    /// see every write acknowledged before it began: a write commits, then
    /// enters the cache, then is acknowledged, so a row the cache shows is
    /// already in any snapshot taken after it — never a body older than
    /// its row. And it takes no lock a writer holds: WAL lets the snapshot
    /// be read while a commit is fsyncing.
    ///
    /// The other way round — a write that commits between the rows and the
    /// snapshot — would put a newer body beside an older row (an `amend`'s
    /// new body under its old title). So the read counts: `committing` is
    /// raised before a commit starts, and if, once the snapshot is taken,
    /// it is still the `written` the rows came with, no commit can be in
    /// the snapshot that is not in the rows. If it has moved, every row is
    /// read from the snapshot with its body — one stored version (SCALE-28:
    /// its `child_count` too, which the file keeps now and moves in the
    /// same transaction as the child that moves it). Either
    /// way each ticket is one statement and the read waits for no writer.
    /// (A writer that never stops keeps the count moving, so its readers
    /// mostly take the second way: the price is parsing the row they read.)
    async fn full(&self, rows: Vec<Row>, written: u64) -> Result<Vec<Ticket>, StoreError> {
        self.full_owned(rows, written).await
    }

    /// [`Store::full`] as a future that borrows nothing from the store — so
    /// a [`Walk`] can set the next step's read going before it hands this
    /// one out.
    fn full_owned(
        &self,
        rows: Vec<Row>,
        written: u64,
    ) -> impl std::future::Future<Output = Result<Vec<Ticket>, StoreError>> + Send + 'static {
        let readers = Arc::clone(&self.readers);
        let committing = Arc::clone(&self.committing);
        async move {
            let Some(first) = rows.first().map(|row| row.id.clone()) else {
                return Ok(Vec::new());
            };
            readers
                .read(move |conn| {
                    let tx = conn.transaction()?;
                    // The first read takes the snapshot every other one reads
                    // from; only after it can the counts say what it holds.
                    let first = db::body(&tx, &first)?.ok_or_else(|| missing(&first))?;
                    let tickets = if committing.load(Ordering::SeqCst) == written {
                        let mut bodies = vec![first];
                        for row in &rows[1..] {
                            bodies.push(db::body(&tx, &row.id)?.ok_or_else(|| missing(&row.id))?);
                        }
                        rows.into_iter().zip(bodies).map(|(row, body)| whole(row, body)).collect()
                    } else {
                        #[cfg(test)]
                        tests_bodies::STORED_VERSION_READS.fetch_add(1, Ordering::Relaxed);
                        // WAVE-6: a placement is the cache's alone — nothing
                        // stores it — so the row's goes with the stored version.
                        rows.into_iter()
                            .map(|row| {
                                let mut t = db::ticket(&tx, &row.id)?.ok_or_else(|| missing(&row.id))?;
                                t.placement = row.placement;
                                Ok(t)
                            })
                            .collect::<Result<Vec<Ticket>, StoreError>>()?
                    };
                    tx.commit()?;
                    Ok(tickets)
                })
                .await
        }
    }

    /// [`Store::full`] for one row.
    async fn full_one(&self, row: Row, written: u64) -> Result<Ticket, StoreError> {
        Ok(self
            .full(vec![row], written)
            .await?
            .pop()
            .expect("one row, one ticket"))
    }

    /// SCALE-14: a row of the cache and the cache's count of writes, read
    /// together; [`StoreError::NotFound`] when `id` names no ticket.
    fn row(&self, id: &str) -> Result<(Row, u64), StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let row = cache
            .get(id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        Ok((row, cache.written))
    }

    /// SCALE-14: the commit of a write after birth that changes a ticket's
    /// row and not its body (`set_status`, `set_parent`, `link_commits`,
    /// `unlink_commits`): the row is written, the body left as stored and
    /// read back in the same transaction — the event carries the ticket
    /// whole.
    fn row_commit(
        row: Row,
    ) -> impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(Row, String), StoreError> + Send + 'static {
        move |tx| {
            db::update_row(tx, &row)?;
            let body = db::body(tx, &row.id)?.ok_or_else(|| missing(&row.id))?;
            Ok((row, body))
        }
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
    ///
    /// SDLC-11: a doc is born in `Doc` (every other kind in `Pending`), may
    /// be a shortcode's first ticket as a scope may, and stands outside the
    /// `blocked_by` graph at both ends — refused, before anything persists:
    /// a doc with a `blocked_by` ([`StoreError::DocBlocked`]), a `blocked_by`
    /// naming a doc ([`StoreError::BlockedByDoc`]), and a doc born as an id
    /// some `blocked_by` already names ([`StoreError::DocAwaited`]) — so no
    /// ticket ever waits on a doc, which never completes.
    pub async fn create(&self, new: NewTicket) -> Result<Ticket, StoreError> {
        self.create_by(new, Act::default()).await
    }

    /// MENGER-13: [`Store::create`] by `act` — the newborn's first status a
    /// transition (from nothing), recorded in the birth's write and said
    /// right after its `Created`.
    pub async fn create_by(&self, new: NewTicket, act: Act) -> Result<Ticket, StoreError> {
        self.create_from(new, None, act).await.map(|(ticket, _)| ticket)
    }

    /// NOTE-2: the note `id` made the ticket `new` — in one write: the
    /// ticket born exactly as [`Store::create`] makes it, and the note marked
    /// with its id and when, in the birth's own transaction, taken by the
    /// cache with it. A note that is none ([`StoreError::NoteNotFound`]) or
    /// already a ticket ([`StoreError::NoteUpgraded`]) is refused under the
    /// write-serial lock, before anything is allocated; a ticket `create`
    /// refuses leaves the note open, and spends no id. The ticket's events
    /// are `create`'s; the note is returned with it.
    pub async fn upgrade_note(&self, id: u64, new: NewTicket) -> Result<(Ticket, crate::notes::Note), StoreError> {
        self.upgrade_note_by(id, new, Act::default()).await
    }

    /// MENGER-13: [`Store::upgrade_note`] by `act`.
    pub async fn upgrade_note_by(&self, id: u64, new: NewTicket, act: Act) -> Result<(Ticket, crate::notes::Note), StoreError> {
        let (ticket, note) = self.create_from(new, Some(id), act).await?;
        Ok((ticket, note.expect("a note was named, so one was marked")))
    }

    /// [`Store::create`], and — NOTE-2 — the note `from` (if any) marked as
    /// the ticket it became, in the same write.
    async fn create_from(&self, new: NewTicket, from: Option<u64>, act: Act) -> Result<(Ticket, Option<crate::notes::Note>), StoreError> {
        let epic = new.epic.trim().to_ascii_uppercase();
        if epic.is_empty() || !epic.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(StoreError::InvalidEpic(new.epic));
        }
        self.check_body(None, &new.body)?;
        if !new.kind.has_lifecycle() && !new.blocked_by.is_empty() {
            return Err(StoreError::DocBlocked {
                epic,
                blocked_by: new.blocked_by,
            });
        }
        // WAVE-10: a path is an address menger keeps for its own documents —
        // `_MENGER/…`, on a doc; where it lives and whether it is free are
        // checked under the lock, with the rest.
        if let Some(path) = &new.path {
            if !is_menger_path(path) {
                return Err(StoreError::PathNotReserved { path: path.clone() });
            }
            if new.kind != Kind::Doc {
                return Err(StoreError::PathOnWork {
                    path: path.clone(),
                    kind: new.kind,
                });
            }
        }
        let requested_parent = new
            .parent
            .map(|p| p.trim().to_ascii_uppercase())
            .filter(|p| !p.is_empty());
        // COMP-3: what the body defines and references — read here, from the
        // body in hand, before the lock; a malformed ```component block is
        // refused now. Whether a name it defines is another ticket's is the
        // registry's to say, under the lock.
        let (uses, problems) = components::read(&new.body);
        if let Some(problem) = problems.into_iter().next() {
            return Err(StoreError::component(None, problem));
        }
        // INPUT-3: and the inputs it declares — refused now, naming the input,
        // when the markup is malformed or two share an id.
        let (declared, problems) = crate::inputs::read(&new.body);
        if let Some(problem) = problems.into_iter().next() {
            return Err(StoreError::InputMarkup { id: None, problem });
        }

        let serial = self.serial().await;
        // NOTE-2: the note a ticket is made from — open, read under the lock
        // so no other upgrade can take it first — or nothing happens.
        let note = match from {
            None => None,
            Some(id) => {
                let cache = self.tickets.read().expect("store lock poisoned");
                let n = cache.notes.get(id).cloned().ok_or_else(|| StoreError::NoteNotFound {
                    id,
                    newest: cache.notes.next_id().checked_sub(1).filter(|n| *n > 0),
                })?;
                if let Some(ticket) = n.ticket {
                    return Err(StoreError::NoteUpgraded { id, ticket });
                }
                Some(n)
            }
        };
        // Under the write-serial lock the cache mirrors the database
        // exactly, so the shortcode's highest seq + 1 is the next id —
        // monotonic across restarts (the cache was loaded from the
        // database) and race-free (writers are serialized). SCALE-3: that
        // highest seq is kept in the cache, beside the tickets and moved
        // by every birth, so allocating is a lookup rather than a walk of
        // every ticket in the store. This read phase computes everything
        // and mutates nothing: a rejection leaves no trace, in the
        // tickets or in what is kept about them.
        let (ticket, touched, unlocks_added, parent_epic, derived, registered, asked, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let tickets = &cache;
            let seq = tickets.ids.next_seq(&epic);
            let id = format!("{epic}-{seq}");

            // MNG-12: every shortcode starts with a scope. `seq == 1` is
            // exactly "this prefix has no tickets yet" (ids are never
            // reused), so the newborn would be the shortcode's root.
            // SDLC-11: or a doc — a doc collection's root.
            if seq == 1 && !new.kind.roots_a_shortcode() {
                return Err(StoreError::FirstMustBeScope {
                    epic,
                    kind: new.kind,
                });
            }

            // SDLC-11: a doc never completes, so nothing may wait on one —
            // neither by naming a doc that exists, nor by having named, before
            // it was born, the id a doc would now take.
            let docs = rules::docs_among(tickets, &new.blocked_by);
            if !docs.is_empty() {
                return Err(StoreError::BlockedByDoc { id, docs });
            }
            if !new.kind.has_lifecycle() {
                let by = tickets.ids.waiting_on(&id);
                if !by.is_empty() {
                    return Err(StoreError::DocAwaited { id, by });
                }
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

            // WAVE-10: a `_MENGER/` document lives directly under its
            // shortcode's scope root — which it cannot be itself (`seq == 1`)
            // — and its path is one no ticket of the shortcode holds yet.
            if let Some(path) = &new.path {
                let Some(root) = (seq > 1).then(|| shortcode_root(tickets, &epic)).flatten() else {
                    return Err(StoreError::PathNoRoot {
                        epic,
                        path: path.clone(),
                    });
                };
                if parent.as_deref() != Some(root.id.as_str()) {
                    return Err(StoreError::PathParent {
                        path: path.clone(),
                        parent: parent.unwrap_or_default(),
                        root: root.id.clone(),
                    });
                }
                if let Some(holder) = tickets.paths.get(&(epic.clone(), path.clone())) {
                    return Err(StoreError::PathTaken {
                        epic,
                        path: path.clone(),
                        holder: holder.clone(),
                    });
                }
            }

            // COMP-3: what the body does to the registry — refused, nothing
            // stored and no id spent, when it defines a name another ticket
            // owns. Last of the checks: a body without components is refused
            // for nothing it was not refused for before.
            let registered = tickets
                .components
                .plan(&id, &uses)
                .map_err(|o| StoreError::ComponentOwned {
                    id: None,
                    name: o.name,
                    owner: o.owner,
                })?;
            // INPUT-3: the newborn's inputs, born open.
            let asked = tickets.inputs.plan(&id, declared);

            // Dangling forward references to this id resolve now: their
            // owners are the newborn's `unlocks` from birth. SCALE-3: the
            // cache keeps them by the id they name, already in (epic, seq)
            // order — only ids that do not exist yet, so it holds the few
            // forward references there are, never every edge.
            let unlocks = tickets.ids.waiting_on(&id);

            let now = chrono::Utc::now().timestamp();
            let mut ticket = Ticket {
                id: id.clone(),
                epic,
                seq,
                title: new.title,
                // SDLC-11: `Doc` for a doc, which it never leaves.
                status: Status::at_birth(new.kind),
                kind: new.kind,
                severity: new.severity,
                blocked_by: new.blocked_by,
                unlocks,
                parent: parent.clone(),
                // SCALE-4: none, unless a hand-edited database already
                // names this id as some ticket's parent — the index has
                // kept those under the id, as `list { parent }` finds them.
                child_count: tickets.children.count(&id),
                // SDLC-9: linked after birth, by `link_commits`.
                commits: Vec::new(),
                // MENGER-12: labelled after birth, by `label`.
                labels: Vec::new(),
                // WAVE-10: checked above.
                path: new.path,
                // WAVE-6: placed below, with the derived state.
                placement: None,
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
            let mut touched: HashMap<String, Row> = HashMap::new();
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
            // SCALE-28: and what the birth does to the derived state — its
            // shortcode's run, the forward references, its parent's count,
            // the roll-ups — worked out now, while nothing has changed.
            let newborn = Row::from(&ticket);
            // MENGER-12: an input born open makes the newborn `asking`.
            let derived = tickets.birth(&newborn, asked.list.as_deref().is_some_and(inputs::asking_in));
            // WAVE-6: born placed — `created` carries it.
            ticket.placement = derived.moved.placement().flatten();
            // FILTER-13: what an open `watch { filter }` asking `under` sees
            // of it — the newborn, and (a hand edit's) the subtree that named
            // its id as a parent, and any shortcode it re-hosts.
            let under = self.under_before(tickets, || {
                let mut t = lineage::Touched::of_moves(derived.announced().as_deref());
                if tickets.children.kids(&newborn.id).is_some() {
                    t.subtree(tickets, &newborn.id, None);
                }
                if let Some(after) = &derived.moved.rollups {
                    t.hosts(tickets, after, tickets.rows().with(&newborn), Some(&newborn.epic));
                }
                t
            });
            (ticket, touched, unlocks_added, parent_epic, derived, registered, asked, under)
        };

        // MENGER-13: its first status, a transition from nothing — recorded
        // with the birth, appended with its row, said after its `Created`.
        let first = {
            let cache = self.tickets.read().expect("store lock poisoned");
            crate::types::Transition {
                id: cache.log.next_id(),
                ticket: ticket.id.clone(),
                field: crate::types::TransitionField::Status,
                from: None,
                to: ticket.status.as_str().to_string(),
                at_unix: ticket.created_unix,
                actor: act.actor,
                name: act.name,
                source: act.source,
                message: act.message,
                cause: None,
                cause_ticket: None,
            }
        };
        let (logged, appended, said) = (first.clone(), first.clone(), first);
        let born_key = OrderKey { epic: ticket.epic.clone(), seq: ticket.seq };

        // SCALE-14: the newborn is written whole — the one ticket a write
        // holds with its body, the caller's — and each blocker it touched
        // as its row, the body left as stored. The blockers ride out whole
        // in their `UnlocksChanged`, so their bodies are read back in the
        // same transaction. SCALE-28: and the derived state it moves, in
        // that transaction too.
        let born = ticket.clone();
        let newborn = Row::from(&ticket);
        let rows: Vec<Row> = touched.values().cloned().collect();
        // FILTER-4: the newborn's cell, said last of all.
        let announced = derived.announced();
        let Derived { stored, moved } = derived;
        // WAVE-6: the placements it moved, said last.
        let placed = moved.placed.as_ref().and_then(Change::placed);
        // COMP-3: and the registry it moved — stored in the transaction,
        // taken by the cache with the rows, said after everything else.
        let mut registered = registered;
        let registry_moves = std::mem::take(&mut registered.moves);
        let registry_rows = registry_moves.clone();
        // INPUT-3: and its inputs — stored, taken, said last of all.
        let mut asked = asked;
        let input_list = asked.list.take();
        let input_rows = input_list.clone();
        let born_id = ticket.id.clone();
        // NOTE-2: and the note it was made from, pointing at it.
        let upgraded = note.map(|n| crate::notes::Note {
            ticket: Some(ticket.id.clone()),
            upgraded_at: Some(ticket.created_unix),
            ..n
        });
        let (note_row, note_kept) = (upgraded.clone(), upgraded.clone());
        let cache = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                db::upsert(tx, &born)?;
                history::record(tx, &logged)?;
                if let Some(n) = &note_row {
                    notes::store(tx, n)?;
                }
                // SEARCH-2: and its row in the search index.
                search::put(tx, &born.id, &born.title, &born.body)?;
                if let Some(list) = &input_rows {
                    inputs::store(tx, &born.id, list)?;
                }
                let blockers = rows
                    .into_iter()
                    .map(|row| {
                        db::update_row(tx, &row)?;
                        let body = db::body(tx, &row.id)?.ok_or_else(|| missing(&row.id))?;
                        Ok((row.id.clone(), whole(row, body)))
                    })
                    .collect::<Result<HashMap<String, Ticket>, StoreError>>()?;
                stored.write(tx)?;
                components::store(tx, &registry_rows)?;
                Ok(blockers)
            },
            move |cache, _| {
                let out = cache.created(newborn, touched.into_values(), moved);
                cache.log.append(&appended, &born_key);
                if let Some(n) = note_kept {
                    cache.notes.put(n);
                }
                cache.components.take(registry_moves);
                if let Some(list) = input_list {
                    cache.inputs.take(&born_id, list);
                }
                out
            },
            move |blockers, (groups, child_count), tx| {
                let _ = tx.send(Change::Created {
                    ticket: ticket.clone(),
                });
                let _ = tx.send(Change::Transition { epic: ticket.epic.clone(), transition: said });
                if let (Some(parent_id), Some(epic), Some(child_count)) =
                    (&ticket.parent, parent_epic, child_count)
                {
                    let _ = tx.send(Change::ChildrenChanged {
                        id: parent_id.clone(),
                        epic,
                        added: ticket.id.clone(),
                        child_count,
                    });
                }
                for blocker_id in unlocks_added {
                    let _ = tx.send(Change::UnlocksChanged {
                        ticket: blockers[&blocker_id].clone(),
                        added: ticket.id.clone(),
                    });
                }
                broadcast_groups(tx, groups);
                if let Some(placed) = placed {
                    let _ = tx.send(placed);
                }
                for change in Change::components(registered) {
                    let _ = tx.send(change);
                }
                for change in Change::inputs(&ticket.epic, asked) {
                    let _ = tx.send(change);
                }
                if let Some(moves) = announced {
                    let _ = tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    let _ = tx.send(change);
                }
                (ticket, upgraded)
            },
        )
        .await
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
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.rollups.groups().into_iter().map(|g| cache.dress(g)).collect()
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

    /// WAVE-4: every ticket of the shortcodes `epics` (case-insensitive;
    /// one with no tickets contributes nothing) whose kind is in `kinds` —
    /// never a doc — placed in the order of work, with what it waits on,
    /// and the tally ([`waves::Waves`]). Blockers are followed into every
    /// shortcode, asked for or not, so a ticket's wave is the same whatever
    /// board shows it. Rows only, under the read lock and nothing else —
    /// no await, no body, no write — memoized for the call, so the cost is
    /// the yielded tickets and their blocker closure.
    pub fn waves(&self, epics: &[String], kinds: &[Kind]) -> waves::Waves {
        let epics: std::collections::BTreeSet<String> = epics.iter().map(|e| e.to_ascii_uppercase()).collect();
        let cache = self.tickets.read().expect("store lock poisoned");
        waves::waves(cache.rows(), &epics, kinds)
    }

    /// MENGER-31: the wave board of a ticket's subtree — `id` and every
    /// ticket under it, at any depth and in any shortcode hosted there (the
    /// kept tree, FILTER-13's `under`) — placed as [`Store::waves`] places a
    /// shortcode's, of the kinds `kinds`, never a doc; only tickets whose
    /// shortcode `visible` holds (all with `None`). Rows only, under the
    /// read lock. [`StoreError::NotFound`] when `id` names no ticket.
    pub fn waves_under(
        &self,
        id: &str,
        kinds: &[Kind],
        visible: Option<&std::collections::HashSet<String>>,
    ) -> Result<waves::Waves, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let row = cache.get(id).ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        let sub = lineage::subtree(&cache, &row.id, Some(row));
        let tickets = sub
            .iter()
            .flat_map(|(epic, seqs)| seqs.iter().filter_map(|seq| cache.order.get(&OrderKey { epic: epic.clone(), seq })));
        let admit = |t: &Row| visible.is_none_or(|v| v.contains(&t.epic));
        Ok(waves::of(cache.rows(), tickets, kinds, &admit))
    }

    /// MENGER-31: the wave board whose final state is `id` ([`waves::to`]):
    /// what must happen first — its blockers, theirs, transitively, and a
    /// scope's or an execution's open descendants (the kept tree) and what
    /// they wait on — each where the store keeps it, the ticket alone in the
    /// last wave, done blockers `Done`, and the cycles among them named.
    /// Rows only, under the read lock. [`StoreError::NotFound`] when `id`
    /// names no ticket.
    pub fn waves_to(
        &self,
        id: &str,
        kinds: &[Kind],
        visible: Option<&std::collections::HashSet<String>>,
    ) -> Result<waves::WavesTo, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let row = cache.get(id).ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        if row.kind == Kind::Doc {
            return Err(StoreError::DocHasNoLifecycle { id: row.id.clone() });
        }
        let sub = lineage::subtree(&cache, &row.id, None);
        let descendants = sub
            .iter()
            .flat_map(|(epic, seqs)| seqs.iter().filter_map(|seq| cache.order.get(&OrderKey { epic: epic.clone(), seq })))
            .filter(|t| t.id != row.id);
        let admit = |t: &Row| visible.is_none_or(|v| v.contains(&t.epic));
        Ok(waves::to(cache.rows(), row, descendants, kinds, &admit))
    }

    /// One ticket, whole. SCALE-14: its row from the cache and its body from
    /// the database ([`Store::full`]); [`StoreError::NotFound`] when `id`
    /// names no ticket — answered by the cache, with no read.
    ///
    /// WAVE-10: `id` may be a path address, `<SHORTCODE>/<path>` (the
    /// shortcode case-insensitive, the path exact) — the one ticket of that
    /// shortcode that holds the path; `NotFound` names the address as given.
    pub async fn get(&self, id: &str) -> Result<Ticket, StoreError> {
        let (row, written) = match path_address(id) {
            Some((epic, path)) => self.addressed(&epic, path, id)?,
            None => self.row(id)?,
        };
        self.full_one(row, written).await
    }

    /// WAVE-10: the row a path address names, and the cache's count of
    /// writes — [`Store::row`] by address.
    fn addressed(&self, epic: &str, path: &str, address: &str) -> Result<(Row, u64), StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let row = cache
            .paths
            .get(&(epic.to_string(), path.to_string()))
            .and_then(|id| cache.get(id))
            .cloned()
            .ok_or_else(|| StoreError::NotFound(address.to_string()))?;
        Ok((row, cache.written))
    }

    /// Tickets ordered by (epic, seq), optionally filtered. The `parent`
    /// filter (AGS-19) selects exactly that ticket's children (SCALE-4:
    /// from the children index, still in the store's order). The unpaged
    /// shape, kept: [`Store::page`] is the same walk with a cursor and a
    /// limit (MNG-9).
    ///
    /// SCALE-14: whole tickets, every one with its body — so the answer
    /// holds every match's body at once, by its own signature. The wire
    /// never asks for it: `list {}` walks the same selection a page of
    /// bodies at a time ([`Store::walk`]); this is that walk, collected.
    pub async fn list(
        &self,
        epic: Option<&str>,
        status: Option<Status>,
        parent: Option<&str>,
    ) -> Result<Vec<Ticket>, StoreError> {
        let select = Select::List {
            epic: epic.map(String::from),
            status,
            parent: parent.map(String::from),
        };
        self.collect(self.walk(select, None, None)).await
    }

    /// SCALE-14: a walk of whole tickets — `select`, after the `after`
    /// cursor, at most `limit` of them — never more than [`BODIES_AT_ONCE`]
    /// bodies held. Nothing is read here: [`Walk::next`] reads each step.
    pub fn walk(&self, select: Select, after: Option<&str>, limit: Option<u32>) -> Walk {
        let up = |s: Option<String>| s.map(|s| s.to_ascii_uppercase());
        let select = match select {
            Select::List {
                epic,
                status,
                parent,
            } => Select::List {
                epic: up(epic),
                status,
                parent: up(parent),
            },
            Select::Ready { epic } => Select::Ready { epic: up(epic) },
            Select::Filtered(q) => Select::Filtered(Filtered {
                epic: up(q.epic),
                parent: up(q.parent),
                ..q
            }),
        };
        Walk {
            select,
            after: after.map(str::to_ascii_uppercase),
            at: None,
            first: None,
            left: limit,
            over: None,
            ahead: None,
        }
    }

    /// Every page of `walk`, gathered.
    async fn collect(&self, mut walk: Walk) -> Result<Vec<Ticket>, StoreError> {
        let mut out = Vec::new();
        loop {
            match walk.next(self).await? {
                Step::Tickets(page) => out.extend(page),
                Step::End { .. } => return Ok(out),
            }
        }
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
    ///
    /// SCALE-14: a page of rows, from the cache alone — the `summary` page;
    /// a page of whole tickets is a [`Walk`].
    ///
    /// SCALE-32: with a `status` (and no `parent`), `remaining` is whole
    /// shortcodes' kept counts of it and a walk of the rest of the one the
    /// cursor cuts into ([`Span::len_of`]); the page itself still tests each
    /// row from the cursor until it has `limit` — the whole span, for a
    /// status none of it holds. An index on `(status, epic, seq)` would make
    /// it a range; nothing keeps one yet.
    pub fn page(&self, q: PageQuery<'_>) -> Result<Page, StoreError> {
        let select = Select::List {
            epic: q.epic.map(str::to_ascii_uppercase),
            status: q.status,
            parent: q.parent.map(str::to_ascii_uppercase),
        };
        let after = q.after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        Self::cut(&cache, &select, after.as_deref(), q.limit, Rest::Count)
    }

    /// MNG-9: one page of the ready set — the [`Store::ready_set`]
    /// membership rule, cut with the same cursor and limit as
    /// [`Store::page`]. SCALE-14: of rows, as `page`. SCALE-32: from the
    /// set the cache keeps ([`Store::walk_ready`]): the page costs the page.
    pub fn ready_page(
        &self,
        epic: Option<&str>,
        after: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Page, StoreError> {
        let select = Select::Ready {
            epic: epic.map(str::to_ascii_uppercase),
        };
        let after = after.map(str::to_ascii_uppercase);
        let cache = self.tickets.read().expect("store lock poisoned");
        Self::cut(&cache, &select, after.as_deref(), limit, Rest::Count)
    }

    /// What `page` and `ready_page` cut, and what a [`Walk`] cuts each step
    /// from: `select`'s rows after `after` (already upper case), at most
    /// `limit`, under the read lock the caller holds.
    fn cut(
        cache: &Cache,
        select: &Select,
        after: Option<&str>,
        limit: Option<u32>,
        rest: Rest,
    ) -> Result<Page, StoreError> {
        match select {
            Select::List {
                epic,
                status,
                parent,
            } => {
                let span = Span::of(cache, after, epic.as_deref())?;
                let keep = |t: &Row| status.is_none_or(|s| t.status == s);
                // The epic is already spent — it bounded the span rather than
                // filtering inside it — so with no status asked for, there is
                // nothing left to test on the way through.
                Ok(match (parent.as_deref(), *status) {
                    (Some(p), None) => Self::walk_children(cache, p, span, limit, Filter::All, rest),
                    (Some(p), Some(_)) => Self::walk_children(cache, p, span, limit, Filter::By(&keep), rest),
                    (None, None) => Self::walk_cache(cache, span, limit, Filter::All, rest),
                    // SCALE-32: a status is tested on the way through the
                    // page, and what is left is counted from what the cache
                    // keeps per shortcode.
                    (None, Some(s)) => {
                        let rows = span.range(cache).map(|range| range.map(|(_, t)| t));
                        let cursor = span.cursor.clone();
                        Self::walk_rows(rows, || span.len_of(cache, s), cursor, limit, Filter::Kept(&keep), rest)
                    }
                })
            }
            Select::Ready { epic } => {
                let span = Span::of(cache, after, epic.as_deref())?;
                Ok(Self::walk_ready(cache, span, limit, rest))
            }
            // FILTER-4: from the bitmaps the filter admits.
            Select::Filtered(q) => Self::cut_filtered(cache, q, after, None, limit, rest),
        }
    }

    /// SCALE-32: a page of the ready set — the kept set itself
    /// ([`ready::Ready`]), cut by the span (the epic and the cursor are
    /// positions in it as they are in the store), each member looked up by
    /// its key. Membership is already decided, so nothing is tested: the
    /// page costs the page, and what is left is the set's arithmetic — whole
    /// shortcodes by their share, the one the cursor cuts into by its
    /// members past the cursor. Before, every ticket in the span had each
    /// of its blockers looked up, for the page and then for the count: at
    /// 10⁶, 60 ms for the first page.
    fn walk_ready(cache: &Cache, span: Span, limit: Option<u32>, rest: Rest) -> Page {
        let (start, end) = (span.start.as_ref(), span.end.as_ref());
        let rows = (!span.empty).then(|| cache.ready.range(start, end).filter_map(|key| cache.order.get(&key)));
        let cursor = span.cursor.clone();
        Self::walk_rows(rows, || cache.ready.len(start, end), cursor, limit, Filter::All, rest)
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
            return Ok(Self::walk_cache(&cache, span, limit, Filter::All, Rest::Count));
        };
        let keep = |t: &Row| needle.matches(t);
        Ok(Self::walk_cache(&cache, span, limit, Filter::By(&keep), Rest::Count))
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
    fn walk_cache(
        cache: &Cache,
        span: Span,
        limit: Option<u32>,
        filter: Filter<'_>,
        rest: Rest,
    ) -> Page {
        let rows = span.range(cache).map(|range| range.map(|(_, t)| t));
        let cursor = span.cursor.clone();
        Self::walk_rows(rows, || span.len(cache), cursor, limit, filter, rest)
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
        rest: Rest,
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
            rest,
        )
    }

    /// What every walk shares: `limit` matches cloned into the page, then
    /// `remaining` — `len` less the page when nothing is filtered (the
    /// page is a prefix of what `rows` holds), or the rest of the matches
    /// counted when something is. SCALE-14: unless the walk's own step
    /// asks for the rows alone ([`Rest::Skip`]). SCALE-32: a filter the
    /// cache keeps counts of ([`Filter::Kept`]) tests the page, and `len` —
    /// then the span's matches — less the page is what is left.
    fn walk_rows<'c>(
        rows: Option<impl Iterator<Item = &'c Row>>,
        len: impl FnOnce() -> u64,
        cursor: Option<String>,
        limit: Option<u32>,
        filter: Filter<'_>,
        rest: Rest,
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
                match rest {
                    // The page is a prefix of the span: it cannot be longer.
                    Rest::Count => len().saturating_sub(tickets.len() as u64),
                    Rest::Skip => 0,
                }
            }
            (Some(rows), Filter::Kept(keep)) => {
                tickets.extend(rows.filter(|t| keep(t)).take(take).cloned());
                match rest {
                    // The page is a prefix of the span's matches.
                    Rest::Count => len().saturating_sub(tickets.len() as u64),
                    Rest::Skip => 0,
                }
            }
            (Some(rows), Filter::By(keep)) => {
                let mut it = rows.filter(|t| keep(t));
                tickets.extend(it.by_ref().take(take).cloned());
                match rest {
                    Rest::Count => it.count() as u64,
                    Rest::Skip => 0,
                }
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
            first: None,
        }
    }

    /// SDLC-13: changes a ticket's kind — the one write that does; a kind
    /// was fixed at birth until it — write-through, as `set_status` is, and
    /// everything kept by kind moves with it in the write's own [`Derived`]
    /// and transaction ([`Cache::kind_change`]).
    ///
    /// The status goes with the kind across the doc line: to a doc, `Doc`
    /// (a `status` other than `Doc` is refused); from a doc, `status` when
    /// given (anything but `Doc`), else `Epic` for a scope and `Pending` for
    /// any other kind; between two kinds with a lifecycle it stays, unless
    /// `status` is given. Refused, before anything is written, and each
    /// naming what it refuses: a doc in the `blocked_by` graph — a ticket
    /// that waits on a blocker, or that a ticket waits on, cannot become a
    /// doc ([`StoreError::DocInGraph`]); a shortcode's root becoming a kind
    /// that roots nothing ([`StoreError::RootKind`] — doc ↔ scope on a root
    /// is a kind change like any other); a ticket holding a path address
    /// (WAVE-10, only a doc's) becoming work ([`StoreError::PathKind`]).
    /// A lower-seq ticket that becomes a scope or a doc becomes its
    /// shortcode's root.
    ///
    /// The kind and status it already has — the same kind, and no `status`
    /// or the one it holds — is no write and no broadcast: the returned
    /// `KindChanged` then has `previous` equal to the ticket's kind, as
    /// `set_parent` answers a no-op. Otherwise the row commits, then the
    /// cache, then `KindChanged`, then what it moved as `set_status` sends
    /// it: one `GroupChanged` per row that moved, innermost first (AGS-23:
    /// a root's group whenever its kind or status changed), the
    /// `PlacementsChanged`, the cell moves and what an `under` watch saw.
    pub async fn set_kind(&self, id: &str, kind: Kind, status: Option<Status>) -> Result<Change, StoreError> {
        self.set_kind_by(id, kind, status, Act::default()).await
    }

    /// MENGER-13: [`Store::set_kind`] by `act` — the kind's change a
    /// transition, and the status's (when it moves with it) another, caused
    /// by the first; both recorded in the write and said after its
    /// `KindChanged`. A status crossing into `Canceled` withdraws the
    /// ticket's open inputs, and out of it restores them, as `set_status`
    /// does; `set_kind` cancels no descendant (only `set_status` cascades).
    pub async fn set_kind_by(&self, id: &str, kind: Kind, status: Option<Status>, act: Act) -> Result<Change, StoreError> {
        let serial = self.serial().await;
        let (mut row, written) = self.row(id)?;
        let old = row.clone();
        let previous = row.kind;
        let status = rules::kind_status(&row.id, previous, row.status, kind, status)?;
        if kind == previous && status == row.status {
            return Ok(Change::KindChanged {
                ticket: self.full_one(row, written).await?,
                previous,
            });
        }
        let (derived, under, made, moved_inputs) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            rules::kind_refusal(&cache, &row, &row.blocked_by, kind)?;
            row.kind = kind;
            row.status = status;
            row.updated_unix = chrono::Utc::now().timestamp();
            // MENGER-13: the kind's transition, then the status's, caused by
            // it — and the inputs a crossing of `Canceled` moves.
            let (made, moved_inputs) = update::kind_moves(&cache, &old, &row, &act);
            // Everything kept by kind, and by status — worked out now, while
            // nothing has changed. FILTER-13: and what an open `watch { filter
            // }` asking `under` sees of it: the cells it moves, and — when it
            // re-roots a shortcode — every shortcode whose hosts that moves.
            let derived = cache.kind_change(&row, &old, cache.inputs.asking_after(&row.id, &moved_inputs));
            let under = self.under_before(&cache, || {
                let mut t = lineage::Touched::of_moves(derived.announced().as_deref());
                if let Some(after) = &derived.moved.rollups {
                    t.hosts(&cache, after, cache.rows().with(&row), None);
                }
                t
            });
            (derived, under, made, moved_inputs)
        };
        let announced = derived.announced();
        let Derived { stored, moved } = derived;
        // WAVE-6: a doc is placed nowhere; work is placed by the rule — its
        // `kind_changed` carries the placement, and the ones downstream that
        // move are said after the groups.
        if let Some(placement) = moved.placement() {
            row.placement = placement;
        }
        let placed = moved.placed.as_ref().and_then(Change::placed);
        let key = OrderKey::of(&row);
        let epic = row.epic.clone();
        let commit = Self::row_commit(row);
        let cache = Arc::clone(&self.tickets);
        let (logged, appended) = (made.clone(), made);
        let kept_inputs = moved_inputs.clone();
        self.write(
            serial,
            move |tx| {
                let out = commit(tx)?;
                stored.write(tx)?;
                for t in &logged {
                    history::record(tx, t)?;
                }
                for (input, entry) in &moved_inputs {
                    inputs::record(tx, input, entry)?;
                }
                Ok(out)
            },
            move |cache, (row, _)| {
                let groups = cache.kind_changed(row.clone(), moved);
                for t in &appended {
                    cache.log.append(t, &key);
                }
                for (input, _) in &kept_inputs {
                    cache.inputs.put(input.clone());
                }
                (groups, appended, kept_inputs)
            },
            move |(row, body), (groups, made, kept_inputs), tx| {
                let change = Change::KindChanged {
                    ticket: whole(row, body),
                    previous,
                };
                let _ = tx.send(change.clone());
                for transition in made {
                    let _ = tx.send(Change::Transition { epic: epic.clone(), transition });
                }
                broadcast_groups(tx, groups);
                if let Some(placed) = placed {
                    let _ = tx.send(placed);
                }
                for (input, _) in kept_inputs {
                    let _ = tx.send(Change::InputChanged { epic: epic.clone(), input });
                }
                if let Some(moves) = announced {
                    let _ = tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    let _ = tx.send(change);
                }
                change
            },
        )
        .await
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
        let serial = self.serial().await;
        // The parent it already has: no write. SCALE-14: the answer still
        // carries the child whole, so its body is read — after the lock on
        // the cache is let go.
        let unchanged = {
            let tickets = self.tickets.read().expect("store lock poisoned");
            let child = tickets
                .get(&id)
                .ok_or_else(|| StoreError::NotFound(id.clone()))?;
            if tickets.get(&parent).is_none() {
                return Err(StoreError::ParentNotFound { id, parent });
            }
            (child.parent.as_deref() == Some(parent.as_str()))
                .then(|| (child.clone(), tickets.written))
        };
        if let Some((child, written)) = unchanged {
            return Ok(Change::ParentChanged {
                ticket: self.full_one(child, written).await?,
                previous: Some(parent),
            });
        }
        let (child, new_epic, old_epic, previous) = {
            let tickets = self.tickets.read().expect("store lock poisoned");
            // Both checked above, under the write-serial lock: nothing has
            // changed them since.
            let mut child = tickets.get(&id).expect("the child exists").clone();
            let parent_ticket = tickets.get(&parent).expect("the parent exists");
            // MNG-14: only the shortcode's scope root may hang under
            // another shortcode; everything else stays inside its own.
            rules::parent_refusal(&tickets, &child, &parent)?;
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

        // SCALE-28: both parents' counts and the roll-ups it moves, stored
        // with the child's row. FILTER-13: and what an open `watch { filter
        // }` asking `under` sees of it — the moved subtree, and any
        // shortcode it re-hosts.
        let (Derived { stored, moved }, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let derived = cache.reparent(&child, previous.as_deref());
            let under = self.under_before(&cache, || {
                let mut t = lineage::Touched::default();
                t.subtree(&cache, &child.id, cache.get(&child.id));
                if let Some(after) = &derived.moved.rollups {
                    t.hosts(&cache, after, cache.rows().with(&child), None);
                }
                t
            });
            (derived, under)
        };
        let left_behind = previous.clone();
        let commit = Self::row_commit(child);
        let cache = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                let out = commit(tx)?;
                stored.write(tx)?;
                Ok(out)
            },
            move |cache, (child, _)| cache.reparented(child.clone(), left_behind.as_deref(), moved),
            move |(child, body), (groups, joined, left), tx| {
                let change = Change::ParentChanged {
                    ticket: whole(child, body),
                    previous: previous.clone(),
                };
                let _ = tx.send(change.clone());
                let _ = tx.send(Change::ChildrenChanged {
                    id: parent,
                    epic: new_epic,
                    added: id.clone(),
                    child_count: joined,
                });
                if let (Some(old), Some(epic)) = (previous, old_epic) {
                    let _ = tx.send(Change::ChildRemoved {
                        id: old,
                        epic,
                        removed: id,
                        child_count: left,
                    });
                }
                broadcast_groups(tx, groups);
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    let _ = tx.send(change);
                }
                change
            },
        )
        .await
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
    /// to now, the row commits, then the cache, then one `Amended` —
    /// AGS-23: and, when the title of a shortcode's root changed, one
    /// `GroupChanged` for that shortcode ([`Group::root_row`]). A title
    /// that is empty after trimming is refused before anything is read; a
    /// title or body that is given is stored exactly as given, byte for
    /// byte (like `create`, which does not trim either).
    ///
    /// SCALE-14: the body it compares with is the stored one, read under
    /// the write-serial lock (so it is the newest), and only a body that
    /// changed is written.
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
        if let Some(body) = &body {
            self.check_body(Some(&id), body)?;
        }
        // COMP-3: what a new body defines and references, read from the body
        // in hand before the lock — and refused, if malformed, only once it
        // is known to change the body (the same body again is still a no-op).
        let uses = body.as_deref().map(components::read);
        // INPUT-3: and the inputs it declares, the same way.
        let declares = body.as_deref().map(crate::inputs::read);
        let serial = self.serial().await;
        let mut ticket = self.get(&id).await?;
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
        // COMP-3: a new body moves the registry — the whole amend refused,
        // title too, when a block is malformed or defines a name another
        // ticket owns.
        let mut registered = match uses.filter(|_| changed.iter().any(|c| c == "body")) {
            None => components::Plan::default(),
            Some((uses, problems)) => {
                if let Some(problem) = problems.into_iter().next() {
                    return Err(StoreError::component(Some(&id), problem));
                }
                let cache = self.tickets.read().expect("store lock poisoned");
                cache
                    .components
                    .plan(&ticket.id, &uses)
                    .map_err(|o| StoreError::ComponentOwned {
                        id: Some(id.clone()),
                        name: o.name,
                        owner: o.owner,
                    })?
            }
        };
        // INPUT-3: and its inputs — the whole amend refused, title too, when
        // the markup is malformed.
        let mut asked = match declares.filter(|_| changed.iter().any(|c| c == "body")) {
            None => inputs::Plan::default(),
            Some((declared, problems)) => {
                if let Some(problem) = problems.into_iter().next() {
                    return Err(StoreError::InputMarkup {
                        id: Some(id.clone()),
                        problem,
                    });
                }
                self.tickets.read().expect("store lock poisoned").inputs.plan(&ticket.id, declared)
            }
        };
        ticket.updated_unix = chrono::Utc::now().timestamp();
        // MENGER-13: a canceled ticket asks no one — an input its new body
        // would leave open is withdrawn as it is declared.
        let withdrawn = if ticket.status == Status::Canceled {
            let reason = format!("{} is canceled", ticket.id);
            asked.withdraw_open(&reason, ticket.updated_unix)
        } else {
            Vec::new()
        };

        let row = Row::from(&ticket);
        let body = changed.iter().any(|c| c == "body").then(|| ticket.body.clone());
        let title_changed = changed.iter().any(|c| c == "title");
        // SEARCH-2: the title and body the search index holds from now on.
        let indexed = (ticket.title.clone(), ticket.body.clone());
        let registry_moves = std::mem::take(&mut registered.moves);
        let registry_rows = registry_moves.clone();
        let input_list = asked.list.take();
        let input_rows = input_list.clone();
        // MENGER-12: a body that opens its first input, or settles or drops
        // its last open one, moves the ticket in or out of `asking` — its
        // cell, in the same write.
        let asks = input_list.as_deref().map(inputs::asking_in);
        let was = self.tickets.read().expect("store lock poisoned").inputs.asking(&row.id);
        let (derived, under) = match asks.filter(|a| *a != was) {
            Some(a) => {
                let (d, u) = self.cell_derived(&row, Some(a));
                (Some(d), u)
            }
            None => (None, None),
        };
        let announced = derived.as_ref().and_then(Derived::announced);
        let (cells_stored, moved) = match derived {
            Some(Derived { stored, moved }) => (Some(stored), Some(moved)),
            None => (None, None),
        };
        let cache = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                db::update_row(tx, &row)?;
                if let Some(body) = body {
                    db::set_body(tx, &row.id, &body)?;
                }
                search::put(tx, &row.id, &indexed.0, &indexed.1)?;
                components::store(tx, &registry_rows)?;
                if let Some(list) = &input_rows {
                    inputs::store(tx, &row.id, list)?;
                }
                for (input, entry) in &withdrawn {
                    inputs::record(tx, input, entry)?;
                }
                if let Some(cells) = &cells_stored {
                    cells.write(tx)?;
                }
                Ok(row)
            },
            // Rows only: what an amend changes is counted by no roll-up and
            // indexed by no id index (see above). AGS-23: a root's new title
            // is its group's, though — one `GroupChanged` after it. COMP-3:
            // and a new body's registry, beside the row. MENGER-12: and the
            // cell an input opened or settled moves.
            move |cache, row| {
                let groups = cache.amended(row.clone(), title_changed);
                cache.components.take(registry_moves);
                if let Some(list) = input_list {
                    cache.inputs.take(&row.id, list);
                }
                if let Some(moved) = moved {
                    let more = cache.take(moved);
                    debug_assert!(more.is_empty(), "an input moves no roll-up");
                }
                groups
            },
            move |_, groups, tx| {
                let asked = Change::inputs(&ticket.epic, asked);
                let change = Change::Amended { ticket, changed };
                let _ = tx.send(change.clone());
                broadcast_groups(tx, groups);
                for change in Change::components(registered) {
                    let _ = tx.send(change);
                }
                for change in asked {
                    let _ = tx.send(change);
                }
                if let Some(moves) = announced {
                    tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    tx.send(change);
                }
                change
            },
        )
        .await
    }

    /// COMP-3: every component in the registry, in name order — orphans
    /// included (`owner: None`), waiting names not. From the cache alone.
    pub fn components(&self) -> Vec<Component> {
        self.tickets.read().expect("store lock poisoned").components.components()
    }

    /// INPUT-3: every input of `ticket` (every ticket's, without one) whose
    /// ticket is in the shortcode `epic` and which is in `state` — each with
    /// its kind, its declaration, its answer and its history — in the store's
    /// order of tickets and each ticket's order (the body's, then the
    /// removed). A `ticket` naming no ticket is [`StoreError::NotFound`]; one
    /// declaring none answers nothing. Ids and shortcodes fold to upper case.
    /// From the cache alone.
    pub fn inputs(
        &self,
        ticket: Option<&str>,
        epic: Option<&str>,
        state: Option<InputState>,
    ) -> Result<Vec<Input>, StoreError> {
        let ticket = ticket.map(|t| t.trim().to_ascii_uppercase());
        let epic = epic.map(|e| e.trim().to_ascii_uppercase());
        let cache = self.tickets.read().expect("store lock poisoned");
        if let Some(t) = ticket.as_ref().filter(|t| cache.get(t).is_none()) {
            return Err(StoreError::NotFound(t.clone()));
        }
        Ok(cache.inputs.query(ticket.as_deref(), epic.as_deref(), state))
    }

    /// INPUT-3: records `value` as the answer to `ticket`'s input `input` —
    /// checked against its kind and options ([`crate::inputs::check`]; a
    /// `multi`'s set kept in the declared order), with `rationale` (a blank
    /// one is none), `source` and the time; one history entry, every call,
    /// the same value again included. Refused when `ticket` names no ticket
    /// ([`StoreError::NotFound`]), the input is not declared, removed or
    /// closed, or the value does not answer it — nothing written. The body
    /// is never touched, nor the ticket's row.
    pub async fn answer(
        &self,
        ticket: &str,
        input: &str,
        value: InputValue,
        rationale: Option<String>,
        source: AnswerSource,
    ) -> Result<InputWrite, StoreError> {
        self.input_write(ticket, input, inputs::Op::Answer { value, rationale, source }).await
    }

    /// INPUT-3: clears the input's answer — `open` again, the clear in its
    /// history, with `source` (INPUT-9: `app` when the host clears it for a
    /// frame). One holding no answer is [`InputWrite::Unchanged`]: nothing
    /// written. Refused as `answer` is, but for the value: closed, removed.
    pub async fn clear_answer(&self, ticket: &str, input: &str, source: AnswerSource) -> Result<InputWrite, StoreError> {
        self.input_write(ticket, input, inputs::Op::Clear { source }).await
    }

    /// INPUT-3: closes the input — frozen, with its answer or without one;
    /// an answer or a clear is refused until it is reopened. A closed one is
    /// [`InputWrite::Unchanged`]. Refused when removed.
    pub async fn close_input(&self, ticket: &str, input: &str) -> Result<InputWrite, StoreError> {
        self.input_write(ticket, input, inputs::Op::Close).await
    }

    /// INPUT-3: reopens a closed input — `answered` again when it holds an
    /// answer, else `open`. One not closed is [`InputWrite::Unchanged`].
    /// Refused when removed.
    pub async fn reopen_input(&self, ticket: &str, input: &str) -> Result<InputWrite, StoreError> {
        self.input_write(ticket, input, inputs::Op::Reopen).await
    }

    /// INPUT-3: one change to one input, through the write path every write
    /// takes (MNG-20): worked out under the write-serial lock from the
    /// cache's record of the input, one history row and the input's state
    /// committed, the cache moved, `InputChanged` broadcast. A change that
    /// changes nothing writes and says nothing.
    async fn input_write(&self, ticket: &str, input: &str, op: inputs::Op) -> Result<InputWrite, StoreError> {
        let ticket = ticket.trim().to_ascii_uppercase();
        let input = input.trim().to_string();
        let serial = self.serial().await;
        let (epic, current, row) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&ticket).ok_or_else(|| StoreError::NotFound(ticket.clone()))?;
            let current = cache.inputs.get(&ticket, &input).cloned().ok_or_else(|| StoreError::InputUnknown {
                ticket: ticket.clone(),
                input: input.clone(),
                known: cache.inputs.of(&ticket).iter().map(|i| i.id.clone()).collect(),
            })?;
            (row.epic.clone(), current, row.clone())
        };
        let moved = inputs::apply(&current, op, chrono::Utc::now().timestamp()).map_err(|refused| match refused {
            inputs::Refused::Removed => StoreError::InputRemoved { ticket, input },
            inputs::Refused::Withdrawn => StoreError::InputWithdrawn { ticket, input },
            inputs::Refused::Closed => StoreError::InputClosed { ticket, input },
            inputs::Refused::Invalid(reason) => StoreError::InputInvalid { ticket, input, reason },
        })?;
        let Some((next, entry)) = moved else {
            return Ok(InputWrite::Unchanged(current));
        };
        // MENGER-12: an input opened or settled may move the ticket in or out
        // of `asking` — its cell, in the same write.
        let (asked, asks) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            (cache.inputs.asking(&row.id), cache.inputs.asking_with(&row.id, &next))
        };
        let (derived, under) = match asked != asks {
            true => {
                let (d, u) = self.cell_derived(&row, Some(asks));
                (Some(d), u)
            }
            false => (None, None),
        };
        let announced = derived.as_ref().and_then(Derived::announced);
        let (cells_stored, moved) = match derived {
            Some(Derived { stored, moved }) => (Some(stored), Some(moved)),
            None => (None, None),
        };
        let (stored, kept) = (next.clone(), next.clone());
        let cache = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                inputs::record(tx, &stored, &entry)?;
                if let Some(cells) = &cells_stored {
                    cells.write(tx)?;
                }
                Ok(())
            },
            move |cache, _| {
                cache.inputs.put(kept);
                if let Some(moved) = moved {
                    let groups = cache.take(moved);
                    debug_assert!(groups.is_empty(), "an input moves no roll-up");
                }
            },
            move |(), (), tx| {
                let _ = tx.send(Change::InputChanged {
                    epic,
                    input: next.clone(),
                });
                if let Some(moves) = announced {
                    tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    tx.send(change);
                }
                InputWrite::Changed(next)
            },
        )
        .await
    }

    /// COMMENT-2: keeps a comment — `anchor` and `body` checked
    /// ([`crate::comments::check`]: an empty body, a blank author and a
    /// malformed anchor are each refused, naming the problem), `author`
    /// `owner` when none is given; then, under the write-serial lock, the
    /// anchor's `ticket` must name a ticket ([`StoreError::CommentTicket`])
    /// and is kept as its id, with its `updated_unix` as `version`; the
    /// comment takes the next id, state `open` and the time. One row
    /// committed, the cache moved, `Added` broadcast; the comment as kept is
    /// the answer. A refusal keeps nothing.
    pub async fn add_comment(&self, anchor: crate::comments::Anchor, body: String, author: Option<String>) -> Result<crate::comments::Comment, StoreError> {
        self.add_comment_as(anchor, body, author, None).await
    }

    /// IDENTITY-21: [`Store::add_comment`], signed by `principal`.
    pub async fn add_comment_as(
        &self,
        anchor: crate::comments::Anchor,
        body: String,
        author: Option<String>,
        principal: Option<String>,
    ) -> Result<crate::comments::Comment, StoreError> {
        let (mut anchor, author) = crate::comments::check(anchor, &body, author.as_deref()).map_err(StoreError::CommentRefused)?;
        let serial = self.serial().await;
        let comment = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if let Some(id) = anchor.ticket.take() {
                let row = cache.get(&id).ok_or_else(|| StoreError::CommentTicket { ticket: id.clone() })?;
                anchor.ticket = Some(row.id.clone());
                anchor.version = Some(row.updated_unix);
            }
            crate::comments::Comment {
                id: cache.comments.next_id(),
                state: crate::comments::CommentState::Open,
                author,
                at: chrono::Utc::now().timestamp(),
                anchor,
                body,
                principal,
            }
        };
        let (stored, kept) = (comment.clone(), comment.clone());
        self.write(
            serial,
            move |tx| Ok(comments::store(tx, &stored)?),
            move |cache, _| cache.comments.put(kept),
            move |(), (), tx| {
                tx.comment(CommentChange::Added(comment.clone()));
                comment
            },
        )
        .await
    }

    /// NOTE-2: keeps a note — its body as given, `author` (default
    /// [`crate::notes::DEFAULT_AUTHOR`]), the signed-in `principal` when
    /// login is on, the next id and the time — through the write path every
    /// write takes. A blank or oversized body or author is
    /// [`StoreError::NoteRefused`], with nothing kept.
    pub async fn add_note_as(
        &self,
        body: String,
        author: Option<String>,
        principal: Option<String>,
    ) -> Result<crate::notes::Note, StoreError> {
        let author = crate::notes::check(&body, author.as_deref()).map_err(StoreError::NoteRefused)?;
        let serial = self.serial().await;
        let note = crate::notes::Note {
            id: self.tickets.read().expect("store lock poisoned").notes.next_id(),
            at: chrono::Utc::now().timestamp(),
            author,
            body,
            ticket: None,
            upgraded_at: None,
            principal,
        };
        let (stored, kept) = (note.clone(), note.clone());
        self.write(
            serial,
            move |tx| Ok(notes::store(tx, &stored)?),
            move |cache, _| cache.notes.put(kept),
            move |(), (), _| note,
        )
        .await
    }

    /// NOTE-2: one page of the notes `q` admits, newest first, from the
    /// cache ([`notes::page`]).
    pub fn notes(&self, q: &NoteQuery, after: Option<u64>, limit: Option<u32>) -> Result<NotePage, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        notes::page(&cache, q, after, limit)
    }

    /// NOTE-2: the note `id`, from the cache; [`StoreError::NoteNotFound`]
    /// when it names none.
    pub fn note(&self, id: u64) -> Result<crate::notes::Note, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.notes.get(id).cloned().ok_or_else(|| StoreError::NoteNotFound {
            id,
            newest: cache.notes.next_id().checked_sub(1).filter(|n| *n > 0),
        })
    }

    /// NOTE-2: the title and body a note gives the ticket it becomes —
    /// its first line with words (else when it was taken), and its text
    /// with a line saying when it was noted and which note it was.
    pub fn note_ticket_text(note: &crate::notes::Note) -> (String, String) {
        (notes::ticket_title(note), notes::ticket_body(note))
    }

    /// IDENTITY-21: a principal's grants, from the cache.
    pub fn grants_of(&self, principal: &str) -> Vec<crate::members::Grant> {
        self.tickets.read().expect("store lock poisoned").members.of(principal)
    }

    /// IDENTITY-21: every grant, by principal then scope, from the cache.
    pub fn grants(&self) -> Vec<crate::members::Grant> {
        self.tickets.read().expect("store lock poisoned").members.all()
    }

    /// IDENTITY-21: keeps a grant (replacing the principal's role on that
    /// scope): one row committed, then the cache moved. The grant as kept.
    pub async fn grant(&self, g: crate::members::Grant) -> Result<crate::members::Grant, StoreError> {
        let serial = self.serial().await;
        let (stored, kept) = (g.clone(), g.clone());
        self.write(
            serial,
            move |tx| Ok(members::store(tx, &stored)?),
            move |cache, _| cache.members.put(kept),
            move |(), (), _| g,
        )
        .await
    }

    /// IDENTITY-21: removes a grant; whether there was one.
    pub async fn revoke(&self, principal: String, scope: String) -> Result<bool, StoreError> {
        let serial = self.serial().await;
        let (p, sc) = (principal.clone(), scope.clone());
        self.write(
            serial,
            move |tx| Ok(members::delete(tx, &principal, &scope)? > 0),
            move |cache, _| cache.members.remove(&p, &sc).is_some(),
            move |existed, _, _| existed,
        )
        .await
    }

    /// AUTH-7: the roles an admin edited (`auth.role_set`), each with its
    /// grants as set — from the cache.
    pub fn role_edits(&self) -> RoleEdits {
        self.tickets.read().expect("store lock poisoned").roles.clone()
    }

    /// AUTH-7: keeps an admin's edit of `role` — its grants, replacing any
    /// edit before: one row committed, then the cache moved.
    pub async fn set_role(&self, role: String, grants: Vec<String>) -> Result<(), StoreError> {
        let serial = self.serial().await;
        let at = chrono::Utc::now().timestamp();
        let (r, g) = (role.clone(), grants.clone());
        self.write(
            serial,
            move |tx| Ok(roles::store(tx, &role, &grants, at)?),
            move |cache, _| {
                cache.roles.insert(r, g);
            },
            move |(), (), _| (),
        )
        .await
    }

    /// AUTH-7: the scopes `target` lives in, innermost first, as the gate
    /// asks them (`Resolve::chain`) — the filter language's `under` read
    /// backwards: for a ticket id, its shortcode, itself, its ancestors and
    /// the shortcodes hosting its shortcode; for a path address
    /// (`SHORTCODE/path`) or a shortcode name, the shortcode and the
    /// shortcodes hosting it. Upper case. A ticket the store does not hold
    /// is placed by the shortcode its id names, so a hidden ticket and a
    /// missing one resolve alike (IDENTITY-37). `None`: `target` names
    /// nothing a scope could hold (blank, or not a shortcode's spelling).
    /// `*` is the caller's to add.
    pub fn scope_chain(&self, target: &str) -> Option<Vec<String>> {
        let t = target.trim();
        let code_ok = |c: &str| !c.is_empty() && c.len() <= 32 && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        let cache = self.tickets.read().expect("store lock poisoned");
        let hosts = |epic: &str| lineage::host_chain(&cache, epic);
        if let Some((code, _path)) = t.split_once('/') {
            if !code_ok(code) {
                return None;
            }
            let code = code.to_ascii_uppercase();
            let mut out = vec![code.clone()];
            out.extend(hosts(&code));
            return Some(out);
        }
        let ticket = t.rsplit_once('-').filter(|(c, n)| code_ok(c) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        match ticket {
            Some((code, n)) => {
                let code = code.to_ascii_uppercase();
                let id = format!("{code}-{n}");
                let mut out = vec![code.clone()];
                if let Some(row) = cache.get(&id) {
                    out.push(row.id.clone());
                    for a in tree::ancestors(cache.rows(), &row.id, row.parent.as_deref()) {
                        if !out.iter().any(|o| o == a) {
                            out.push(a.to_string());
                        }
                    }
                }
                for h in hosts(&code) {
                    if !out.contains(&h) {
                        out.push(h);
                    }
                }
                Some(out)
            }
            None if code_ok(t) => {
                let code = t.to_ascii_uppercase();
                let mut out = vec![code.clone()];
                out.extend(hosts(&code));
                Some(out)
            }
            None => None,
        }
    }

    /// COMMENT-2: one page of the comments `q` admits, newest first — after
    /// the comment `after` (exclusive), at most `limit` — with the cursor
    /// for the next ([`comments::page`]). From the cache alone.
    pub fn comments(&self, q: &CommentQuery, after: Option<u64>, limit: Option<u32>) -> Result<CommentPage, StoreError> {
        let cache = self.tickets.read().expect("store lock poisoned");
        comments::page(&cache, q, after, limit)
    }

    /// COMMENT-2: one comment as the cache holds it.
    pub fn comment(&self, id: u64) -> Option<crate::comments::Comment> {
        self.tickets.read().expect("store lock poisoned").comments.get(id).cloned()
    }

    /// COMMENT-2: resolves the comment `id` — `resolved`, the row's state
    /// committed, the cache moved, `Resolved` broadcast. One already
    /// resolved is the answer as it is: nothing written, nothing said. An
    /// id that names no comment is [`StoreError::CommentNotFound`].
    pub async fn resolve_comment(&self, id: u64) -> Result<crate::comments::Comment, StoreError> {
        self.comment_state(id, crate::comments::CommentState::Resolved).await
    }

    /// COMMENT-2: reopens the comment `id` — `open` again; as
    /// [`Store::resolve_comment`], the other way.
    pub async fn reopen_comment(&self, id: u64) -> Result<crate::comments::Comment, StoreError> {
        self.comment_state(id, crate::comments::CommentState::Open).await
    }

    /// COMMENT-2: the comment `id` moved to `to`, through the write path
    /// every write takes; a move to where it is writes and says nothing.
    async fn comment_state(&self, id: u64, to: crate::comments::CommentState) -> Result<crate::comments::Comment, StoreError> {
        let serial = self.serial().await;
        let current = {
            let cache = self.tickets.read().expect("store lock poisoned");
            cache.comments.get(id).cloned().ok_or(StoreError::CommentNotFound { id, newest: cache.comments.newest() })?
        };
        if current.state == to {
            return Ok(current);
        }
        let next = crate::comments::Comment { state: to, ..current };
        let (stored, kept) = (next.clone(), next.clone());
        self.write(
            serial,
            move |tx| Ok(comments::store(tx, &stored)?),
            move |cache, _| cache.comments.put(kept),
            move |(), (), tx| {
                tx.comment(match to {
                    crate::comments::CommentState::Resolved => CommentChange::Resolved(next.clone()),
                    crate::comments::CommentState::Open => CommentChange::Reopened(next.clone()),
                });
                next
            },
        )
        .await
    }

    /// COMMENT-2: `comments.watch`'s channel — every comment added, resolved
    /// or reopened, in the order the writes were acknowledged.
    pub fn subscribe_comments(&self) -> broadcast::Receiver<CommentChange> {
        self.tx.comments.subscribe()
    }

    /// SDLC-9: links `entries` to the ticket `id` — the record of which
    /// commits finished it. Every entry must be a commit reference
    /// ([`commits::reference`]); any that is not refuses the whole call,
    /// naming it, before anything is read: a call is all or nothing. Of the
    /// references, in the order given, each the ticket does not already
    /// carry is appended — compared as stored text, so an entry named twice
    /// is linked once — and `added` names them in that order.
    ///
    /// Nothing new — `entries` empty, or every one already linked — is no
    /// write and no broadcast, and the returned `CommitsLinked` says
    /// `added: []`, as `amend` answers a no-op. A link that would put more
    /// than [`commits::MAX_PER_TICKET`] on the ticket is refused whole.
    /// Otherwise the row commits — its commits and `updated_unix`, nothing
    /// else: not the status (whether `done` needs a commit is SDLC-4's) —
    /// then the cache, then one `CommitsLinked`. Like `amend`, it moves no
    /// roll-up and no index, so the row goes into the cache and that is all.
    pub async fn link_commits(&self, id: &str, entries: Vec<String>) -> Result<Change, StoreError> {
        let id = id.trim().to_ascii_uppercase();
        let refs = Self::references(&id, &entries)?;
        let serial = self.serial().await;
        let (mut row, written) = self.row(&id)?;
        let mut known: HashSet<String> = row.commits.iter().cloned().collect();
        let added: Vec<String> = refs.into_iter().filter(|r| known.insert(r.clone())).collect();
        if added.is_empty() {
            return Ok(Change::CommitsLinked {
                ticket: self.full_one(row, written).await?,
                added,
            });
        }
        if row.commits.len() + added.len() > commits::MAX_PER_TICKET {
            return Err(StoreError::TooManyCommits {
                id,
                linked: row.commits.len(),
                adding: added.len(),
            });
        }
        row.commits.extend(added.iter().cloned());
        row.updated_unix = chrono::Utc::now().timestamp();
        // FILTER-4: a first commit moves the ticket's cell.
        let (derived, under) = self.commits_derived(&row);
        let announced = derived.announced();
        let Derived { stored, moved } = derived;
        let commit = Self::row_commit(row);
        let cache = Arc::clone(&self.tickets);

        self.write(
            serial,
            move |tx| {
                let out = commit(tx)?;
                stored.write(tx)?;
                Ok(out)
            },
            move |cache, (row, _)| cache.commits_changed(row.clone(), moved),
            move |(row, body), (), tx| {
                let change = Change::CommitsLinked {
                    ticket: whole(row, body),
                    added,
                };
                let _ = tx.send(change.clone());
                if let Some(moves) = announced {
                    let _ = tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    let _ = tx.send(change);
                }
                change
            },
        )
        .await
    }

    /// FILTER-4 / FILTER-13: what a link or an unlink does to the derived
    /// state, and what an open `watch { filter }` asking `under` sees of it.
    fn commits_derived(&self, row: &Row) -> (Derived, Option<lineage::UnderBefore>) {
        self.cell_derived(row, None)
    }

    /// FILTER-4 / MENGER-12: what a write that moves only `row`'s cell does
    /// to the derived state ([`Cache::cell_change`]; `asking`, when the
    /// write moves its inputs), and what an open `watch { filter }` asking
    /// `under` sees of it.
    fn cell_derived(&self, row: &Row, asking: Option<bool>) -> (Derived, Option<lineage::UnderBefore>) {
        let cache = self.tickets.read().expect("store lock poisoned");
        let derived = cache.cell_change(row, asking);
        let under = self.under_before(&cache, || lineage::Touched::of_moves(derived.announced().as_deref()));
        (derived, under)
    }

    /// MENGER-12: moves the labels of the ticket `id` — `add` added and
    /// `remove` taken off ([`crate::labels::apply`]: each entry a label,
    /// lower-cased; a label in both is added; at most
    /// [`crate::labels::MAX_PER_TICKET`] after), all or nothing: a bad entry
    /// or too many refuses the whole call, naming them, before anything is
    /// written. Nothing moved — every label to add already held, every one
    /// to remove not — is no write and no broadcast, answered
    /// `LabelsChanged` with `added: []` and `removed: []`. Otherwise the row
    /// commits (its labels and `updated_unix`) with the cell move its new
    /// set makes (`label <name>`), then the cache, then one
    /// `LabelsChanged`, then the cell moves.
    pub async fn label(&self, id: &str, add: Vec<String>, remove: Vec<String>) -> Result<Change, StoreError> {
        let id = id.trim().to_ascii_uppercase();
        let serial = self.serial().await;
        let (mut row, written) = self.row(&id)?;
        let (after, added, removed) =
            crate::labels::apply(&row.labels, &add, &remove).map_err(|why| StoreError::Labels { id: id.clone(), why })?;
        if added.is_empty() && removed.is_empty() {
            return Ok(Change::LabelsChanged {
                ticket: self.full_one(row, written).await?,
                added,
                removed,
            });
        }
        row.labels = after;
        row.updated_unix = chrono::Utc::now().timestamp();
        let (derived, under) = self.cell_derived(&row, None);
        let announced = derived.announced();
        let Derived { stored, moved } = derived;
        let commit = Self::row_commit(row);
        let cache = Arc::clone(&self.tickets);

        self.write(
            serial,
            move |tx| {
                let out = commit(tx)?;
                stored.write(tx)?;
                Ok(out)
            },
            move |cache, (row, _)| cache.commits_changed(row.clone(), moved),
            move |(row, body), (), tx| {
                let change = Change::LabelsChanged {
                    ticket: whole(row, body),
                    added,
                    removed,
                };
                tx.send(change.clone());
                if let Some(moves) = announced {
                    tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    tx.send(change);
                }
                change
            },
        )
        .await
    }

    /// SDLC-9: unlinks `entries` from the ticket `id` — every entry the
    /// ticket carries comes off, and `removed` names them in the order
    /// given. The same grammar and the same all-or-nothing refusal as
    /// [`Store::link_commits`]; an entry that is a reference but is not
    /// linked is simply not removed. None of them linked is no write and no
    /// broadcast, answered `removed: []`; otherwise the row commits (its
    /// commits and `updated_unix`), then the cache, then one
    /// `CommitsUnlinked`.
    pub async fn unlink_commits(&self, id: &str, entries: Vec<String>) -> Result<Change, StoreError> {
        let id = id.trim().to_ascii_uppercase();
        let refs = Self::references(&id, &entries)?;
        let serial = self.serial().await;
        let (mut row, written) = self.row(&id)?;
        let mut removed: Vec<String> = Vec::new();
        for r in refs {
            let before = row.commits.len();
            row.commits.retain(|c| *c != r);
            if row.commits.len() < before {
                removed.push(r);
            }
        }
        if removed.is_empty() {
            return Ok(Change::CommitsUnlinked {
                ticket: self.full_one(row, written).await?,
                removed,
            });
        }
        row.updated_unix = chrono::Utc::now().timestamp();
        // FILTER-4: taking the last commit moves the ticket's cell.
        let (derived, under) = self.commits_derived(&row);
        let announced = derived.announced();
        let Derived { stored, moved } = derived;
        let commit = Self::row_commit(row);
        let cache = Arc::clone(&self.tickets);

        self.write(
            serial,
            move |tx| {
                let out = commit(tx)?;
                stored.write(tx)?;
                Ok(out)
            },
            move |cache, (row, _)| cache.commits_changed(row.clone(), moved),
            move |(row, body), (), tx| {
                let change = Change::CommitsUnlinked {
                    ticket: whole(row, body),
                    removed,
                };
                let _ = tx.send(change.clone());
                if let Some(moves) = announced {
                    let _ = tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    let _ = tx.send(change);
                }
                change
            },
        )
        .await
    }

    /// SDLC-9: every entry of a link or an unlink as the reference it is
    /// stored as, in order — or the refusal naming every entry that is not
    /// one. Before the write-serial lock and before any read, so a call
    /// with one bad entry does nothing at all.
    fn references(id: &str, entries: &[String]) -> Result<Vec<String>, StoreError> {
        let mut refs = Vec::with_capacity(entries.len());
        let mut not: Vec<String> = Vec::new();
        for entry in entries {
            match commits::reference(entry) {
                Some(r) => refs.push(r),
                None => not.push(entry.clone()),
            }
        }
        if !not.is_empty() {
            return Err(StoreError::NotACommit {
                id: id.to_string(),
                entries: not,
            });
        }
        Ok(refs)
    }

    /// Tickets that can start now: status `Ready` and every `blocked_by`
    /// entry resolves to a ticket whose status is `Complete` or
    /// `Superseded`. A dangling blocker (id naming no existing ticket)
    /// keeps its ticket out. The epic filter applies to membership;
    /// blockers resolve globally. Ordered by (epic, seq).
    ///
    /// SCALE-14: whole tickets — the unpaged shape, as [`Store::list`]: a
    /// [`Walk`] of the ready set, collected.
    pub async fn ready_set(&self, epic: Option<&str>) -> Result<Vec<Ticket>, StoreError> {
        let select = Select::Ready {
            epic: epic.map(String::from),
        };
        self.collect(self.walk(select, None, None)).await
    }

    /// One ticket's `blocked_by`, each entry resolved to the current
    /// ticket or marked dangling, in `blocked_by` order. SCALE-14: each
    /// blocker whole — its row from the cache, its body from the database,
    /// all of them in one read ([`Store::full`]).
    pub async fn blockers(&self, id: &str) -> Result<Vec<BlockerRef>, StoreError> {
        // Per entry: `None` takes the next resolved blocker, `Some` dangles.
        let (slots, rows, written) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let ticket = cache
                .get(id)
                .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
            let mut rows = Vec::new();
            let slots: Vec<Option<String>> = ticket
                .blocked_by
                .iter()
                .map(|b| match cache.get(b) {
                    Some(row) => {
                        rows.push(row.clone());
                        None
                    }
                    None => Some(b.clone()),
                })
                .collect();
            (slots, rows, cache.written)
        };
        let mut resolved = self.full(rows, written).await?.into_iter();
        Ok(slots
            .into_iter()
            .map(|slot| match slot {
                None => BlockerRef::Resolved(resolved.next().expect("a ticket per resolved row")),
                Some(id) => BlockerRef::Dangling(id),
            })
            .collect())
    }

    pub fn count(&self) -> usize {
        self.tickets.read().expect("store lock poisoned").len()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.tx.plain.subscribe()
    }

    /// FILTER-4: `watch { filter }`'s channel — every change [`Store::subscribe`]
    /// gives, in the same order, and after each write's last one the cells it
    /// moved ([`Change::CellsMoved`]).
    pub fn subscribe_filtered(&self) -> broadcast::Receiver<Change> {
        self.tx.filtered.subscribe()
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
        self.seed_synthetic_shaped(spec, SeedShape::Flat)
    }

    /// FILTER-13: [`Store::seed_synthetic`] in a given shape — PERF-2's flat
    /// one, or FILTER-3 §6.5's nested one ([`SeedShape`]).
    pub fn seed_synthetic_shaped(&self, spec: SeedSpec, shape: SeedShape) -> Result<SeedReport, StoreError> {
        // Mutations stay serialized even here: a concurrent writer means
        // this was called while serving, which it must not be.
        let _serial = self
            .write_serial
            .try_lock()
            .map_err(|_| StoreError::Db("seed requested while another write is in flight".into()))?;
        let started = std::time::Instant::now();

        let (next_seq, existing): (HashMap<String, u32>, usize) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if cache.len() > 0 && !spec.into_existing {
                return Err(StoreError::SeedRefused {
                    existing: cache.len(),
                });
            }
            // SCALE-3: the per-shortcode sequence `create` allocates from,
            // so seeded ids continue exactly where a create's would.
            let next_seq = cache
                .ids
                .last_seqs()
                .map(|(epic, seq)| (epic.to_string(), seq))
                .collect();
            (next_seq, cache.len())
        };

        let generated = synthetic::generate(spec, &next_seq, shape);
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
        // SCALE-14: one write as a reader counts them ([`Store::full`]),
        // however many transactions carry it. A seed that fails part way
        // leaves it counted: some of it may be stored, none of it is in the
        // cache, and a reader that sees the two counts disagree reads rows
        // from the database instead of trusting its own.
        //
        // SCALE-28: the derived state cannot ride in each chunk's
        // transaction — it is built once the whole seed is in the cache, as
        // below — so the first chunk's transaction also says the stored
        // state is behind the tickets, and the transaction that stores it
        // says it no longer is. Killed in between, the file says so, and
        // the next open builds it from the tickets (as for a file from
        // before SCALE-28): at no instant does it claim a state it does not
        // hold. The children index is part of that state, kept by SQLite a
        // row at a time in parent order — not the order a seed writes in:
        // at 10⁶ it took the persist from ~10 s to ~15 s. So a seed at least
        // the size of the store it lands in drops it with its first chunk
        // and builds it again, sorted, with the rest (0.95 s at 10⁶).
        // SCALE-32: the ready set's index too — and it is built after the
        // counts it is over are written, not moved by each one.
        let rebuild_index = generated.len() >= existing;
        // SCALE-32: each ticket's count of unsatisfied blockers, worked out
        // before its row is written so the row is written with it. Counted
        // after the seed is in, the counts were written a second time, a row
        // each — 177,000 rows at 10⁶, most pages of the file rewritten: the
        // seed went 14.6 s → 21 s. A generated ticket's blockers are tickets
        // generated before it; a store's own are looked up, should one be
        // named. What the seed does to the store's own counts (a newborn that
        // resolves a forward reference, born done) the stored state after it
        // still settles.
        let open: Vec<u32> = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let born: HashMap<&str, Status> = generated.iter().map(|t| (t.id.as_str(), t.status)).collect();
            generated
                .iter()
                .map(|t| {
                    ready::open_blockers(&t.blocked_by, |b| {
                        born.get(b).copied().or_else(|| cache.get(b).map(|r| r.status))
                    })
                })
                .collect()
        };
        self.committing.fetch_add(1, Ordering::SeqCst);
        {
            let mut conn = self.db.lock().expect("db lock poisoned");
            let chunks = generated
                .chunks(synthetic::ROWS_PER_TRANSACTION)
                .zip(open.chunks(synthetic::ROWS_PER_TRANSACTION));
            for (i, (chunk, open)) in chunks.enumerate() {
                let tx = conn.transaction()?;
                if i == 0 {
                    stored::mark_stale(&tx, true)?;
                    if rebuild_index {
                        stored::drop_children_index(&tx)?;
                        stored::drop_ready_index(&tx)?;
                    }
                }
                db::insert_many(&tx, chunk, open)?;
                // SEARCH-2: indexed in the transaction that writes them.
                search::put_many(&tx, chunk)?;
                tx.commit()?;
            }
        }
        let persist = persist_started.elapsed();

        let form = apply(&self.tickets, |tickets| {
            tickets.at.reserve(generated.len());
            // SCALE-14: into the cache as rows — each body is dropped as its
            // ticket goes in, as a load never reads one.
            for t in generated {
                tickets.insert(Row::from(t));
            }
            // SCALE-4: the children index, and every parent's count from
            // it, built over the seeded store as a build does — first, because
            // the roll-ups walk it. A seed only ever adds children, so a
            // count set before it is either set again here or still right.
            let children = children::Children::build(tickets.order.values());
            count_children(&mut tickets.order, &tickets.at, &children);
            tickets.children = children;
            // MNG-10: built once over the seeded store, as a build does — not
            // kept per row: a seed into an existing store can add tickets
            // to its prefixes in any shape, and nothing is broadcast here.
            let rollups = rollup::Rollups::build(tickets.rows());
            tickets.rollups = rollups;
            // SCALE-3: the id indexes too, for the same reason — the seed
            // went into the cache without passing through `create`, and
            // one of its tickets may be an id the store already named.
            let ids = ids::Ids::build(tickets.rows());
            tickets.ids = ids;
            // SCALE-32: and the counts, the ready set and the status counts —
            // a seed's tickets arrive with their statuses and blockers set.
            let ready = ready::Ready::build(tickets.rows());
            tickets.ready = ready;
            // FILTER-4: and the cells, from the rows and those counts.
            let cells =
                cells::Cells::build(tickets.rows(), |id| tickets.ready.count(id), |id| tickets.inputs.asking(id));
            tickets.cells = cells;
            // FILTER-13: and the kept tree, from the rows, the children and
            // the cells.
            Cache::plant(
                &tickets.order,
                &tickets.at,
                &mut tickets.children,
                &tickets.cells,
                &tickets.ready,
                &tickets.inputs,
            );
            // WAVE-6: and every placement, over the seeded store.
            tickets.place_all();
            tickets.form()
        });
        // SCALE-28: and all of it stored, in one transaction, which also
        // says the stored state is the tickets' again.
        {
            let mut conn = self.db.lock().expect("db lock poisoned");
            let tx = conn.transaction()?;
            stored::create_children_index(&tx)?;
            stored::write_all(&tx, &form)?;
            stored::create_ready_index(&tx)?;
            stored::mark_stale(&tx, false)?;
            tx.commit()?;
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
///
/// SCALE-28: stored, a row per shortcode, per cross top and per hand-edit
/// mark, every field as kept — so a start reads the shape instead of
/// settling it. A write works its change out on a copy before it commits
/// (the two rebuilds included: they rebuild from the rows as the write
/// will leave them), stores the rows that differ in its transaction, and
/// the cache takes the copy.
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
/// Built in one pass after a seed, and kept by every birth through
/// `create`. `set_parent` and `set_status` touch neither a sequence nor a
/// `blocked_by`, so they leave it as it is. SCALE-28: and stored — a row
/// per shortcode, a row per reference — by the birth's own transaction, so
/// a start reads it instead of building it in a pass over every ticket.
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
/// answered. Built in one pass after a seed, and moved by `create` and
/// `set_parent` in the same critical section as the row whose parent it
/// is; each move sets the parent's `child_count` from it.
///
/// SCALE-28: the file holds the same thing as an index on the tickets,
/// `(parent, epic, seq)` — the store's order under each parent, kept by
/// SQLite with the rows — and each `child_count` as a column, written by the
/// transaction of the birth or move that changes it. A start reads this
/// index from that one rather than from a pass over the tickets; B4 can
/// page children from it directly.
mod children;

/// SCALE-32 (B3 of SCALE-13): what `ready_set` and `list { status }` answer
/// from instead of testing every ticket in their span.
///
/// MNG-4's rule — `Ready`, and every id in `blocked_by` a ticket that is
/// `Complete` or `Superseded` — as a number kept per ticket: how many of
/// the ids it names are not (an id not born yet counts). The ready set is
/// the tickets `Ready` with that number 0, kept per shortcode in the
/// store's order; beside it, how many tickets of each shortcode hold each
/// status.
///
/// Three writes move them, and only three: a ticket's birth (its own count,
/// from its blockers as they stand; its status counted), a change of its
/// own status (its status counts, its membership), and a change of a
/// blocker's status across the satisfied line — which moves the count of
/// every ticket that names it, found from the blocker's `unlocks` (the
/// mirror MNG-4 keeps; never a scan). A move (`set_parent`), an amend, a
/// link touch neither a status nor a `blocked_by`, and leave them as they
/// are. A seed and a build count them in one pass, as the other structures
/// are built. Stored by SCALE-28's form: the count a column on the ticket's
/// row, the ready set a partial index over it and the status, the status
/// counts and each shortcode's share of the ready set two small tables.
///
/// SDLC-11: a doc is never in the ready set, by the rule as it stands — its
/// status is `Doc` from birth and `set_status` never moves it, so it is never
/// `Ready` — and no count is kept for one or hangs on one: `create` refuses a
/// doc a `blocked_by` and a `blocked_by` a doc. Its status is counted in the
/// `Doc` bucket of its shortcode, as every status is counted in its own.
mod ready;

/// WAVE-4: the order of work, read off the rows on demand — each ticket's
/// wave (MNG-4's rule over `blocked_by`, the whole graph), `Done`, or the
/// missing-blocker lane, and what it waits on. Nothing kept: a call places
/// what it yields and the blockers behind it, once each, and forgets them.
pub mod waves;

/// WAVE-6: the placements kept — the pass a birth or a status change across
/// the satisfied line runs before it commits, re-placing the written ticket
/// and what moves downstream of it, and the pass that places every ticket at
/// open.
mod placements;

/// SDLC-9: what a commit reference is — the grammar `link_commits` and
/// `unlink_commits` hold every entry to, and the text a reference is stored
/// and compared as — and how many one ticket may carry.
mod commits;

/// SQL for the system of record. Private: the rest of the application
/// only ever sees the cache above.
mod db;

/// COMP-3: the component registry — what a body defines and references, the
/// registry the cache keeps, the plan a write makes of it, its table and the
/// first open that builds it.
mod components;
/// INPUT-3: the inputs store — each ticket's inputs as its body declares
/// them, their answers and state changes, the tables and the migration.
mod inputs;
/// COMMENT-2: the comments store — a reader's comments on the app, kept
/// beside the tickets, behind the cache.
mod comments;
pub use comments::{CommentChange, CommentPage, CommentQuery};
/// NOTE-2: the notes store — every note, kept beside the tickets; a note
/// made a ticket in the ticket's own write.
mod notes;
pub use notes::{NotePage, NoteQuery};
/// IDENTITY-21: the members store — every grant of a role on a scope, kept
/// beside the tickets, behind the cache.
mod members;
pub use members::Members;
// AUTH-7: an admin's edits of a role's grants.
mod roles;
pub use roles::RoleEdits;
/// MENGER-13: the transition log — every status and kind change, who made
/// it, from where, why and caused by what — and the kept indexes a history
/// filter is answered from.
mod history;
pub use history::{zone, Act, Clock};
/// MENGER-13: cancellation cascades and their undoing — one batch write.
mod cascade;
/// MENGER-30: the rules a write after birth is refused by, one place each.
mod rules;
/// MENGER-30: `update` — any part of a ticket in one write.
mod update;
pub use update::{Update, Updated};
/// THEME-8: the themes and what the site and each shortcode say.
mod themes;
pub use themes::ThemeChange;

/// FILTER-4: the cells — every ticket's `(status, kind, severity, blocked,
/// has commits)` interned as one cell; per shortcode and cell, a Roaring
/// bitmap of the sequences in it. What a filter is compiled against, and
/// what `counts`, `facets`, a filtered page and `watch { filter }` answer
/// from: sums and merges over the bitmaps a filter admits, never a test of
/// a ticket. Moved by every write that moves a cell — a birth, a status
/// change and its fan-out, a first commit and a last — in the write,
/// before it commits; stored as form 4; read at start; verified.
mod cells;
pub use cells::{CellMove, Moves, RECOUNT_OVER};

/// FILTER-13: the kept tree — for each internal ticket a tally of its
/// descendants by (shortcode, cell), and for each long list of children the
/// holders of each cell — worked out per write, stored as form 5, read at
/// start, verified.
mod tree;

/// FILTER-4: what a filter answers — `counts`, a filtered page by cursor or
/// position, and `watch { filter }`'s membership — compiled against the
/// cells and answered from their bitmaps.
mod filtered;
pub use filtered::{Filtered, Lens, Membership, Watcher};

/// FILTER-13: a filter compiled with its lineage (`under`), and `tree`.
mod lineage;
pub use lineage::{Node, TreePage, TreeQuery, UnderMoves, UNDERS_MAX};

/// FILTER-15: every read takes the lens — `find { query, filter }`,
/// `inputs { filter }`, `ready_set { filter }` and `member { ids, filter }`:
/// membership one lookup in the admitted bitmaps, `find` testing only the
/// candidates the cells admit, the ready set a filtered page of `ready and
/// (filter)`. Never a walk of the store.
mod lensed;
pub use lensed::{narrowing, narrowing_lens, ready_and, MEMBER_MAX};

/// SCALE-28: the derived state, stored — the tables beside the tickets,
/// what a write's transaction writes to them ([`stored::Changes`]), a start
/// reads from them, a first open builds into them, and `--verify-derived`
/// compares with a scan.
mod stored;

pub use stored::Verdict;

/// SEARCH-2: search inside tickets — the FTS5 index of every ticket's title
/// and text beside them in the file, kept by every write that changes
/// either, and `search`: prefix, typo-tolerant, ranked, marked, filtered.
mod search;
pub use search::{SearchHit, SearchPage, SEARCH_LIMIT, SEARCH_MAX};
pub use waves::{Placed, Waves};

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

/// SEARCH-2: search inside tickets — the text a body is indexed as, how a
/// query is read and corrected, the order, the filter, the pages; the index
/// built by a first open, kept by every write, held to the tickets.
#[cfg(test)]
mod tests_search;

/// NOTE-2: the notes store — kept, listed, filtered and paged; an upgrade's
/// one write and its refusals; across a reopen.
#[cfg(test)]
mod tests_notes;
/// THEME-8: define, set, theme_for (every row), refusals, a reopen.
#[cfg(test)]
mod tests_themes;

/// SDLC-7: all nine statuses through the system of record, and what the
/// two worker states are not (ready, done, a satisfied blocker).
#[cfg(test)]
mod tests_worker_states;

/// MNG-16: `amend` — every row of its table, byte-exact bodies through the
/// file, and nothing else about the ticket moving.
#[cfg(test)]
mod tests_amend;

/// SCALE-14: bodies out of the cache — whole tickets from rows and bodies,
/// byte-exact on every path that answers one; a walk that holds a page of
/// bodies and answers what one page would; the cache-only answers that
/// read no database; a read beside an uncommitted write; and read-your-
/// writes, deterministically and under a hammer of amends.
#[cfg(test)]
mod tests_bodies;

/// SDLC-9: `link_commits` and `unlink_commits` — the reference grammar,
/// every row of the ticket's table, nothing else about the ticket moving,
/// the bound, and a database from before the column.
#[cfg(test)]
mod tests_commits;

/// MNG-20: every write dropped by its caller mid-commit — reproduced first,
/// then held to the fix: the write reaches the cache and `watch` anyway,
/// the store mirrors its database, and the next id is the next one.
#[cfg(test)]
mod tests_detached;

/// SCALE-28: the stored derived state, read back through a connection of
/// its own, against the scan oracles after every kind of write — on stores
/// built by create, by seed, from the fixtures written before it and from
/// hand-edited shapes — and the build on first open, the reads at start,
/// the seed's marker and `--verify-derived`.
#[cfg(test)]
mod tests_stored;

/// SDLC-11: docs — each row of the ticket's table (born in `Doc`, rooting
/// a shortcode, no `blocked_by` either way, `set_status` refused both ways,
/// never ready, roll-ups that count them apart, everything else as any
/// ticket), a file with docs verified and reopened, and form 2's first open.
#[cfg(test)]
mod tests_docs;

/// SCALE-32: the counts of unsatisfied blockers, the ready set and the
/// status counts against a scan after every write, in the cache and read
/// back from the file; every `ready_set` and `list { status }` page against
/// the page the code before it gives; the fan-out; the first open of a file
/// from before it; and `--verify-derived` naming what differs.
#[cfg(test)]
mod tests_ready;

/// WAVE-4: `waves` — chains, diamonds, completion and supersession moving
/// waves left, the missing-blocker lane, blockers in shortcodes not asked
/// for, docs never placed, `kinds` filtering the yield and not the walk,
/// the yield order and the tally, and a hand-edited cycle that terminates.
#[cfg(test)]
mod tests_waves;

/// MENGER-31: `waves_to` — a ticket as the final state: chains, diamonds,
/// done blockers, a scope's descendants, a cycle named — and `waves_under`.
#[cfg(test)]
mod tests_waves_to;

/// WAVE-10: path addresses — `create { path }` on each row of the ticket's
/// table, `get` by `SHORTCODE/path`, uniqueness in the cache and the file,
/// and a file from before the column.
#[cfg(test)]
mod tests_paths;

/// WAVE-6: the placements kept — the property test against WAVE-4's rule
/// and `watch`, the two-blockers-back case, the pass stopping where nothing
/// moves, forward references, a hand-edited cycle, a reopen, `watch { epic }`.
#[cfg(test)]
mod tests_placements;

/// COMP-3: the component registry — a test per row of the ticket's table,
/// the code examples that are not definitions, the code span that is not a
/// reference, durability, events, and a database from before the table.
#[cfg(test)]
mod tests_components;
/// INPUT-3: the inputs store — every row of the ticket's table: parsed on
/// write (and refused), answered (and refused), cleared, closed, reopened,
/// removed and restored; the query; the events and their place; durability;
/// the tables added to an old file.
#[cfg(test)]
mod tests_inputs;

/// FILTER-4: the cells — every ticket's cell and every bitmap held to a scan
/// after every write, in the cache and the file; each write's announced
/// moves exactly the scan's; form 3's first open; `--verify-derived` naming
/// an altered container and count; the container encoding.
#[cfg(test)]
mod tests_cells;

/// FILTER-4: what a filter answers — `counts`, every filtered page by
/// cursor and by position, and `watch { filter }`'s membership — held to
/// the same filter tested on every row, after hundreds of random writes.
#[cfg(test)]
mod tests_filtered;

/// FILTER-13: the kept tree — tallies and holders held to an oracle from
/// the rows after every write, in the cache and the file.
#[cfg(test)]
mod tests_tree;

/// FILTER-4: random filters from the language's own AST, sent as text,
/// against its reference evaluator on every row — every read and `facets`;
/// the refusals, the empty filter, `ready` as the ready set, FILTER-3's
/// examples.
#[cfg(test)]
mod tests_language;

/// FILTER-15: `find`, `inputs`, `ready_set` and `member` under random
/// filters, held to the language's evaluator on every row after hundreds of
/// random writes; each without a filter exactly today's answer.
#[cfg(test)]
mod tests_lensed;

/// SDLC-13: `set_kind` — each row of the ticket's table (the status across
/// the doc line, each refusal naming what it refuses, a root between the
/// kinds that root, a lower-seq ticket made the root, the no-op, the events
/// in order), `--verify-derived` after each kind of change; and the random
/// `set_kind` every random walk takes into its mix, held to the rules.
#[cfg(test)]
mod tests_kinds;

/// COMMENT-2: the comments store — `add` (kept, returned, every refusal
/// keeping nothing), `resolve` / `reopen`, `list` newest first and paged,
/// every filter and conjunction held to a reference (`under` to the filter
/// language's own answer) over random comments on a moving tree, a comment's
/// anchor kept through its ticket's writes, and the form 6 → 7 upgrade.
#[cfg(test)]
mod tests_comments;

/// MENGER-13: the transition log (every write's transition, who, where
/// from, why, caused by what; durable; `history`), cancellation cascades and
/// their undoing (the rules, the inputs withdrawn and restored, one write's
/// events, the derived state a scan), the windows (`Clock::bounds`, zones),
/// the log's kept indexes against a scan, and `--verify-derived` on the log.
#[cfg(test)]
mod tests_history;

/// MENGER-30: `update` — every field alone and all at once, the no-op,
/// every refusal writing nothing, blockers moving everything derived, and a
/// random walk of updates leaving `--verify-derived` clean.
#[cfg(test)]
mod tests_update;
