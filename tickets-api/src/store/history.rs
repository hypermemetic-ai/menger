//! MENGER-13: the **transition log** — every change of a ticket's status
//! and of its kind, kept by the store, write-through, append-only: who made
//! it (a principal, `system` for the store's own, `anonymous` with login
//! off), from where (`app`, `api`, `pipeline`, `cascade`), why (a message,
//! always given for `system`) and what caused it (the transition a cascade
//! carried down or undid).
//!
//! **Behind the cache, as everything stored is.** The log is read whole at
//! open ([`load`]) into a [`Log`]: each transition as a compact mark — its
//! ticket's place in the store's order, when, who, the field, from, to, the
//! source and the cause — beside the few strings that are not compact (a
//! message, a display name). Every read answers from it: `history { id }` is
//! the ticket's marks and the marks they caused; a history filter is a range
//! of a **kept index** — each status's entries and exits, every transition
//! and each actor's, each sorted by `(at, id)` — so a window is two binary
//! searches and the transitions inside it, never a scan of the tickets or of
//! the log ([`Log::holds`]). A write that makes a transition records it in
//! its own transaction ([`record`]) and appends it to the cache's log in
//! the same critical section as its rows ([`Log::append`]).
//!
//! **Stored** in one table, `transitions` — a row per transition, its `id`
//! the log's order. A file without it gains it, empty, on open ([`migrate`]),
//! with the moment it began: the log starts then, and no ticket is given a
//! history it did not have ("history begins …" says so). `--verify-derived`
//! holds the log to the tickets ([`check`]): each ticket's latest status and
//! kind transitions are its status and kind now, each transition starts
//! where the one before it ended, each `cause` names an earlier transition,
//! a cascade's names its cause.

use crate::address::ShortcodeId;
use super::{Cache, OrderKey};
use super::times::{self, Times};
use crate::filter::{Event, History, Moment, When, Who, Window};
use crate::types::{Kind, Status, TicketSummary, Transition, TransitionField, TransitionSource, ANONYMOUS_ACTOR, SYSTEM_ACTOR};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, HashMap};

/// The table, its index by ticket, and the one row saying when it began.
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS transitions (
            id         INTEGER PRIMARY KEY,
            ticket     TEXT NOT NULL,
            field      TEXT NOT NULL,
            from_value TEXT,
            to_value   TEXT NOT NULL,
            at_unix    INTEGER NOT NULL,
            actor      TEXT NOT NULL,
            name       TEXT,
            source     TEXT NOT NULL,
            message    TEXT,
            cause      INTEGER
        );
        CREATE INDEX IF NOT EXISTS transitions_by_ticket ON transitions (ticket, id);
        CREATE TABLE IF NOT EXISTS transitions_began (
            one     INTEGER PRIMARY KEY CHECK (one = 1),
            at_unix INTEGER NOT NULL
        );
    ";

/// A file without the log gains it, empty, here — and the moment it began —
/// on every open a no-op once it is there. Returns whether it was made.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<bool> {
    let had: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'transitions'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )?;
    if had {
        return Ok(false);
    }
    let tx = conn.transaction()?;
    tx.execute_batch(TABLES)?;
    tx.execute(
        "INSERT OR IGNORE INTO transitions_began (one, at_unix) VALUES (1, ?1)",
        [chrono::Utc::now().timestamp()],
    )?;
    tx.commit()?;
    tracing::warn!(
        "MENGER-13: the file gained the transition log, empty, durably — every status and kind change from now on \
         is kept; nothing before it has a history"
    );
    Ok(true)
}

/// Who made a transition, where from, and why — what a write hands the
/// store beside the change itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Act {
    /// The caller's principal; [`ANONYMOUS_ACTOR`] with login off.
    pub actor: String,
    /// The caller's display name, when it has one.
    pub name: Option<String>,
    pub source: TransitionSource,
    pub message: Option<String>,
    /// ACT-2: the transition every transition of this act's write that has
    /// no cause of its own is caused by — its id and its ticket: an
    /// `update_many`'s first, shared by the rest. `None`: none.
    pub cause: Option<(u64, String)>,
}

impl Default for Act {
    /// Nobody signed in, through the API, saying nothing.
    fn default() -> Self {
        Self { actor: ANONYMOUS_ACTOR.into(), name: None, source: TransitionSource::Api, message: None, cause: None }
    }
}

impl Act {
    /// How a message names the actor: its name, else its principal.
    pub(super) fn who(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.actor)
    }

    /// ACT-2: the shared cause, as a transition takes it.
    pub(super) fn shared_cause(&self) -> Option<(u64, &str)> {
        self.cause.as_ref().map(|(c, t)| (*c, t.as_str()))
    }
}

/// A status or a kind, as a mark holds it: its place in [`Status::ALL`] or
/// [`Kind::ALL`].
fn status_ix(s: Status) -> u8 {
    Status::ALL.iter().position(|x| *x == s).expect("a status") as u8
}

fn kind_ix(k: Kind) -> u8 {
    Kind::ALL.iter().position(|x| *x == k).expect("a kind") as u8
}

/// One transition as the cache keeps it — compact: where its ticket sits,
/// when, who (interned), what it moved, and what caused it. The strings that
/// are not compact live beside the marks ([`Log`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Mark {
    pub(super) id: u64,
    pub(super) at: i64,
    epic: u32,
    pub(super) seq: u32,
    actor: u32,
    pub(super) field: TransitionField,
    pub(super) from: Option<u8>,
    pub(super) to: u8,
    pub(super) source: TransitionSource,
    /// 0: none (ids start at 1).
    pub(super) cause: u64,
}

impl Mark {
    /// Its status before, when it moved a status.
    pub(super) fn status_before(self) -> Option<Status> {
        (self.field == TransitionField::Status).then(|| self.from.map(|i| Status::ALL[i as usize])).flatten()
    }

    /// Its status after, when it moved a status.
    pub(super) fn status_after(self) -> Option<Status> {
        (self.field == TransitionField::Status).then(|| Status::ALL[self.to as usize])
    }
}

/// The log, as the cache holds it — the marks in the order written, and
/// the kept indexes a history filter is answered from.
#[derive(Debug, Clone, Default)]
pub(crate) struct Log {
    marks: Vec<Mark>,
    epics: Vec<ShortcodeId>,
    epic_ix: HashMap<ShortcodeId, u32>,
    actors: Vec<String>,
    actor_ix: HashMap<String, u32>,
    names: HashMap<u64, String>,
    messages: HashMap<u64, String>,
    /// Each ticket's marks, oldest first — by `(epic, seq)`.
    tickets: HashMap<(u32, u32), Vec<u32>>,
    /// The marks each transition caused — by the cause's id.
    caused: HashMap<u64, Vec<u32>>,
    /// Every mark, by `(at, id)`.
    by_time: Vec<u32>,
    /// Per status (in [`Status::ALL`]'s order): the marks into it, by
    /// `(at, id)`.
    entered: Vec<Vec<u32>>,
    /// …and out of it.
    left: Vec<Vec<u32>>,
    /// Per actor: its marks, by `(at, id)`.
    by_actor: HashMap<u32, Vec<u32>>,
    /// When the log began — `None` for a log nobody began (a test's).
    began: Option<i64>,
    /// FILTER-27: each ticket's birth and status-since, and their indexes
    /// ([`Times`]) — moved by [`Log::retime`] (a row written) and by a
    /// status mark's append.
    pub(super) times: Times,
}

/// Inserts `pos` into `list`, kept by `(at, id)` — at the end, unless a
/// clock stepped back.
fn insert_sorted(list: &mut Vec<u32>, marks: &[Mark], pos: u32) {
    let m = marks[pos as usize];
    let key = (m.at, m.id);
    let at = list.partition_point(|&p| {
        let x = marks[p as usize];
        (x.at, x.id) < key
    });
    list.insert(at, pos);
}

/// The marks of `list` with `lo <= at < hi` (each bound optional).
fn range<'a>(list: &'a [u32], marks: &[Mark], lo: Option<i64>, hi: Option<i64>) -> &'a [u32] {
    let start = lo.map_or(0, |lo| list.partition_point(|&p| marks[p as usize].at < lo));
    let end = hi.map_or(list.len(), |hi| list.partition_point(|&p| marks[p as usize].at < hi));
    &list[start..end.max(start)]
}

impl Log {
    /// An empty log that began at `began`.
    pub(super) fn began_at(began: Option<i64>) -> Self {
        Self {
            entered: vec![Vec::new(); Status::ALL.len()],
            left: vec![Vec::new(); Status::ALL.len()],
            began,
            ..Self::default()
        }
    }

    /// When it began.
    pub(crate) fn began(&self) -> Option<i64> {
        self.began
    }

    /// How many transitions it holds.
    pub(crate) fn len(&self) -> usize {
        self.marks.len()
    }

    /// The id the next transition takes.
    pub(crate) fn next_id(&self) -> u64 {
        self.marks.last().map_or(1, |m| m.id + 1)
    }

    fn intern(ix: &mut HashMap<String, u32>, all: &mut Vec<String>, s: &str) -> u32 {
        if let Some(i) = ix.get(s) {
            return *i;
        }
        let i = all.len() as u32;
        all.push(s.to_string());
        ix.insert(s.to_string(), i);
        i
    }

    /// ORG-3: a shortcode's index in the log, interning it on first sight.
    fn intern_shortcode(&mut self, sc: &ShortcodeId) -> u32 {
        if let Some(i) = self.epic_ix.get(sc) {
            return *i;
        }
        let i = self.epics.len() as u32;
        self.epics.push(sc.clone());
        self.epic_ix.insert(sc.clone(), i);
        i
    }

    /// The cache's half of a committed transition of the ticket at `key`.
    pub(crate) fn append(&mut self, t: &Transition, key: &OrderKey) {
        if self.entered.is_empty() {
            self.entered = vec![Vec::new(); Status::ALL.len()];
            self.left = vec![Vec::new(); Status::ALL.len()];
        }
        let epic = self.intern_shortcode(&key.shortcode);
        let actor = Self::intern(&mut self.actor_ix, &mut self.actors, &t.actor);
        if t.field == TransitionField::Status {
            if let Ok(to) = t.to.parse::<Status>() {
                self.times.moved(epic, key.seq, status_ix(to), t.at_unix);
            }
        }
        let (from, to) = match t.field {
            TransitionField::Status => (
                t.from.as_deref().and_then(|s| s.parse::<Status>().ok()).map(status_ix),
                t.to.parse::<Status>().map(status_ix).unwrap_or(0),
            ),
            TransitionField::Kind => (
                t.from.as_deref().and_then(|s| s.parse::<Kind>().ok()).map(kind_ix),
                t.to.parse::<Kind>().map(kind_ix).unwrap_or(0),
            ),
        };
        let mark = Mark {
            id: t.id,
            at: t.at_unix,
            epic,
            seq: key.seq,
            actor,
            field: t.field,
            from,
            to,
            source: t.source,
            cause: t.cause.unwrap_or(0),
        };
        debug_assert!(self.marks.last().is_none_or(|m| m.id < mark.id), "the log is appended in id order");
        let pos = self.marks.len() as u32;
        self.marks.push(mark);
        if let Some(n) = &t.name {
            self.names.insert(t.id, n.clone());
        }
        if let Some(m) = &t.message {
            self.messages.insert(t.id, m.clone());
        }
        self.tickets.entry((epic, key.seq)).or_default().push(pos);
        if let Some(c) = t.cause {
            self.caused.entry(c).or_default().push(pos);
        }
        insert_sorted(&mut self.by_time, &self.marks, pos);
        insert_sorted(self.by_actor.entry(actor).or_default(), &self.marks, pos);
        if t.field == TransitionField::Status {
            insert_sorted(&mut self.entered[to as usize], &self.marks, pos);
            if let Some(f) = from {
                insert_sorted(&mut self.left[f as usize], &self.marks, pos);
            }
        }
    }

    /// FILTER-27: `row` written — its times as its row and the log say
    /// ([`times::timed`]).
    pub(super) fn retime(&mut self, row: &TicketSummary) {
        let epic = self.intern_shortcode(&row.shortcode);
        let latest = self.latest(&row.shortcode, row.seq, TransitionField::Status).map(|p| self.mark(p).at);
        self.times.put(epic, row.seq, times::timed(row, latest, self.began));
    }

    /// FILTER-27: every row's times, afresh — at open, after the log is read.
    /// FILTER-30: the activity kept stays ([`Times::touch`]).
    pub(super) fn time_all<'r>(&mut self, rows: impl IntoIterator<Item = &'r TicketSummary>) {
        self.times = Times::with_active(self.times.take_active());
        for row in rows {
            self.retime(row);
        }
    }

    /// FILTER-30: activity on the ticket `(epic, seq)` at `at` — a comment
    /// left on it, an answer to or a clear of one of its inputs.
    pub(super) fn touch(&mut self, epic: &ShortcodeId, seq: u32, at: i64) {
        let e = self.intern_shortcode(epic);
        self.times.touch(e, seq, at);
    }

    /// FILTER-30: every ticket's activity, as the comments and the inputs
    /// hold it ([`activity`]) — at open, and by `--verify-derived`.
    pub(super) fn touch_all(&mut self, activity: impl IntoIterator<Item = (OrderKey, i64)>) {
        for (key, at) in activity {
            self.touch(&key.shortcode, key.seq, at);
        }
    }

    /// FILTER-27: `(epic, seq)`'s times.
    pub(crate) fn timed(&self, epic: &ShortcodeId, seq: u32) -> Option<times::Timed> {
        self.epic_ix.get(epic).and_then(|e| self.times.get(*e, seq))
    }

    /// FILTER-27: the shortcode the log interned as `e`.
    pub(super) fn epic_name(&self, e: u32) -> ShortcodeId {
        self.epics.get(e as usize).cloned().unwrap_or_default()
    }

    /// The mark at `pos`.
    pub(super) fn mark(&self, pos: u32) -> &Mark {
        &self.marks[pos as usize]
    }

    /// The ticket `(epic, seq)`'s marks, oldest first.
    pub(super) fn of(&self, epic: &ShortcodeId, seq: u32) -> &[u32] {
        self.epic_ix
            .get(epic)
            .and_then(|e| self.tickets.get(&(*e, seq)))
            .map_or(&[][..], Vec::as_slice)
    }

    /// The marks the transition `id` caused, in the order written.
    pub(super) fn caused_by(&self, id: u64) -> &[u32] {
        self.caused.get(&id).map_or(&[][..], Vec::as_slice)
    }

    /// The ticket `(epic, seq)` the mark at `pos` is of.
    pub(super) fn key_of(&self, pos: u32) -> OrderKey {
        let m = self.mark(pos);
        OrderKey { shortcode: self.epics[m.epic as usize].clone(), seq: m.seq }
    }

    /// The transition at `pos`, whole — its ticket's id `ticket`, its cause's
    /// `cause_ticket`.
    pub(super) fn transition(&self, pos: u32, ticket: String, cause_ticket: Option<String>) -> Transition {
        let m = *self.mark(pos);
        let spell = |i: u8| match m.field {
            TransitionField::Status => Status::ALL[i as usize].as_str().to_string(),
            TransitionField::Kind => Kind::ALL[i as usize].as_str().to_string(),
        };
        Transition {
            id: m.id,
            ticket: crate::address::TicketId::new(ticket),
            field: m.field,
            from: m.from.map(spell),
            to: spell(m.to),
            at_unix: m.at,
            actor: self.actors[m.actor as usize].clone(),
            name: self.names.get(&m.id).cloned(),
            source: m.source,
            message: self.messages.get(&m.id).cloned(),
            cause: (m.cause != 0).then_some(m.cause),
            cause_ticket,
        }
    }

    /// Where the mark with id `id` is.
    pub(super) fn pos_of(&self, id: u64) -> Option<u32> {
        self.marks.binary_search_by_key(&id, |m| m.id).ok().map(|p| p as u32)
    }

    /// The latest mark of `(epic, seq)` that moved `field`.
    pub(super) fn latest(&self, epic: &ShortcodeId, seq: u32, field: TransitionField) -> Option<u32> {
        self.of(epic, seq).iter().rev().copied().find(|p| self.mark(*p).field == field)
    }

    /// MENGER-13: the tickets `pred` holds for — some transition of each is
    /// its event, inside its window as `clock` reads it, by its actor — per
    /// shortcode. A range of one kept index, never a scan: the status's
    /// entries or exits for `entered` / `left`, the actor's marks (or every
    /// mark) for `changed`; the actor tested on each mark in the range only
    /// when the range is not the actor's own.
    pub(crate) fn holds(&self, pred: &History, clock: &Clock) -> BTreeMap<ShortcodeId, RoaringBitmap> {
        // FILTER-27: births and status-since, from the times. FILTER-30: and
        // last touches.
        let timed = match &pred.event {
            Event::Created => Some(self.times.created(pred.window.as_ref(), clock)),
            Event::Touched => Some(self.times.touched(pred.window.as_ref(), clock)),
            Event::Held(h) => Some(self.times.held(h, clock)),
            _ => None,
        };
        if let Some(by) = timed {
            return by.into_iter().map(|(e, b)| (self.epic_name(e), b)).collect();
        }
        let mut out: BTreeMap<ShortcodeId, RoaringBitmap> = BTreeMap::new();
        let Some((lo, hi)) = clock.bounds(pred.window.as_ref()) else { return out };
        let actor: Option<Option<u32>> = pred.by.as_ref().map(|w| {
            let name = match w {
                Who::Me => clock.me.as_str(),
                Who::System => SYSTEM_ACTOR,
                Who::Principal(p) => p.as_str(),
            };
            self.actor_ix.get(name).copied()
        });
        // Asked of an actor the log has never seen: nothing.
        if actor == Some(None) {
            return out;
        }
        let actor = actor.flatten();
        let none: Vec<u32> = Vec::new();
        let (list, test_actor): (&[u32], bool) = match &pred.event {
            Event::Entered(s) => (self.entered.get(status_ix(*s) as usize).unwrap_or(&none), actor.is_some()),
            Event::Left(s) => (self.left.get(status_ix(*s) as usize).unwrap_or(&none), actor.is_some()),
            Event::Changed => match actor {
                Some(a) => (self.by_actor.get(&a).map_or(&none[..], Vec::as_slice), false),
                None => (&self.by_time, false),
            },
            // Answered from the times, above. DEPLOY-11: `deployed` from the
            // deployments' cells ([`Cache::holds`]), never here.
            Event::Created | Event::Touched | Event::Held(_) | Event::Deployed(_) | Event::Queued(_) => return out,
        };
        let mut by_epic: HashMap<u32, RoaringBitmap> = HashMap::new();
        for &p in range(list, &self.marks, lo, hi) {
            let m = self.mark(p);
            if test_actor && Some(m.actor) != actor {
                continue;
            }
            by_epic.entry(m.epic).or_default().insert(m.seq);
        }
        for (e, bm) in by_epic {
            out.insert(self.epics[e as usize].clone(), bm);
        }
        out
    }
}

impl Log {
    /// FILTER-27: the next moment after the clock's now at which `pred`'s
    /// answer moves with no write — a ticket crossing a `for`'s length, a
    /// transition or a birth leaving (or entering) a relative window, a new
    /// day or week; `None` when it never will (fixed moments, or nothing to
    /// cross).
    pub(crate) fn next_boundary(&self, pred: &History, clock: &Clock) -> Option<i64> {
        let now = clock.now();
        if let Event::Held(h) = &pred.event {
            return self.times.next_held(h, now);
        }
        let w = pred.window.as_ref()?;
        let none: Vec<u32> = Vec::new();
        // The earliest moment at or after `from` the predicate's list holds.
        let first_from = |from: i64| -> Option<i64> {
            let list: &[u32] = match &pred.event {
                Event::Created => return self.times.first_born_from(from),
                Event::Touched => return self.times.first_touched_from(from),
                Event::Held(_) | Event::Deployed(_) | Event::Queued(_) => return None,
                Event::Entered(s) => self.entered.get(status_ix(*s) as usize).unwrap_or(&none),
                Event::Left(s) => self.left.get(status_ix(*s) as usize).unwrap_or(&none),
                Event::Changed => &self.by_time,
            };
            range(list, &self.marks, Some(from), None).first().map(|p| self.mark(*p).at)
        };
        // A length back from now: what is inside it now leaves (or enters
        // `before …`) when it is that length old.
        let edge = |len: i64| first_from(now - len).map(|at| at + len + 1);
        let ago = |x: &When| match x {
            When::Ago(n, u) => edge(i64::from(*n) * u.seconds()),
            When::At(_) => None,
        };
        match w {
            Window::Within(n, u) => edge(i64::from(*n) * u.seconds()),
            Window::Today | Window::Yesterday | Window::ThisWeek => clock.next_day(matches!(w, Window::ThisWeek)),
            Window::Since(x) | Window::Before(x) => ago(x),
            Window::Between(a, b) => match (ago(a), ago(b)) {
                (Some(x), Some(y)) => Some(x.min(y)),
                (x, y) => x.or(y),
            },
        }
    }
}

/// MENGER-13: how a filter reads time — the zone its calendar windows are
/// days of, the moment it is read at (the clock's, unless a test fixes it),
/// and who `by me` is.
#[derive(Clone, Debug)]
pub struct Clock {
    pub tz: jiff::tz::TimeZone,
    /// A fixed "now" (unix seconds) — tests; `None`: the system clock.
    pub fixed: Option<i64>,
    /// Who `by me` names: the caller's principal (`anonymous` with login
    /// off).
    pub me: String,
}

impl Default for Clock {
    /// UTC, now, nobody signed in.
    fn default() -> Self {
        Self { tz: jiff::tz::TimeZone::UTC, fixed: None, me: ANONYMOUS_ACTOR.into() }
    }
}

impl Clock {
    /// Now, in unix seconds.
    pub fn now(&self) -> i64 {
        self.fixed.unwrap_or_else(|| chrono::Utc::now().timestamp())
    }

    /// The clock in zone `name`: `UTC` (or `Z`), a fixed offset (`+02:00`,
    /// `-0530`, `+2`), or an IANA name read from the system's tzdb
    /// (`Europe/Berlin`). Blank: UTC.
    pub fn zoned(mut self, name: Option<&str>) -> Result<Self, String> {
        self.tz = zone(name)?;
        Ok(self)
    }

    /// The window `w` as `[lo, hi)` in unix seconds — `None` when it holds
    /// no moment at all (a `between` that ends before it starts). No window:
    /// every moment.
    pub fn bounds(&self, w: Option<&Window>) -> Option<(Option<i64>, Option<i64>)> {
        let Some(w) = w else { return Some((None, None)) };
        let now = self.now();
        let day_start = |d: jiff::civil::Date| -> i64 {
            d.to_zoned(self.tz.clone()).map(|z| z.timestamp().as_second()).unwrap_or(now)
        };
        let today = jiff::Timestamp::from_second(now)
            .map(|t| t.to_zoned(self.tz.clone()).date())
            .unwrap_or_else(|_| jiff::civil::date(1970, 1, 1));
        let on = |m: &Moment| -> i64 {
            let d = jiff::civil::date(m.year, m.month as i8, m.day as i8);
            match m.time {
                None => day_start(d),
                Some((h, mi)) => d
                    .at(h as i8, mi as i8, 0, 0)
                    .to_zoned(self.tz.clone())
                    .map(|z| z.timestamp().as_second())
                    .unwrap_or(now),
            }
        };
        // FILTER-27: an `ago` is a length back from now.
        let at = |w: &When| -> i64 {
            match w {
                When::At(m) => on(m),
                When::Ago(n, u) => now - i64::from(*n) * u.seconds(),
            }
        };
        let end_of = |w: &When| -> i64 {
            match w {
                When::At(m) if m.time.is_none() => {
                    let d = jiff::civil::date(m.year, m.month as i8, m.day as i8);
                    d.tomorrow().map(day_start).unwrap_or(i64::MAX)
                }
                w => at(w),
            }
        };
        let (lo, hi) = match w {
            Window::Within(n, u) => (Some(now - i64::from(*n) * u.seconds()), None),
            Window::Today => (Some(day_start(today)), None),
            Window::Yesterday => {
                let y = today.yesterday().unwrap_or(today);
                (Some(day_start(y)), Some(day_start(today)))
            }
            Window::ThisWeek => {
                let back = i64::from(today.weekday().to_monday_zero_offset());
                let monday = today.checked_sub(jiff::Span::new().days(back)).unwrap_or(today);
                (Some(day_start(monday)), None)
            }
            Window::Since(m) => (Some(at(m)), None),
            Window::Before(m) => (None, Some(at(m))),
            Window::Between(a, b) => (Some(at(a)), Some(end_of(b))),
        };
        match (lo, hi) {
            (Some(l), Some(h)) if h <= l => None,
            other => Some(other),
        }
    }
}

impl Clock {
    /// FILTER-27: the start of tomorrow in the clock's zone — of next
    /// Monday, for `week` — as unix seconds.
    pub fn next_day(&self, week: bool) -> Option<i64> {
        let today = jiff::Timestamp::from_second(self.now()).ok()?.to_zoned(self.tz.clone()).date();
        let ahead = if week { 7 - i64::from(today.weekday().to_monday_zero_offset()) } else { 1 };
        let next = today.checked_add(jiff::Span::new().days(ahead)).ok()?;
        next.to_zoned(self.tz.clone()).ok().map(|z| z.timestamp().as_second())
    }
}

/// A zone by name — see [`Clock::zoned`].
pub fn zone(name: Option<&str>) -> Result<jiff::tz::TimeZone, String> {
    let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else { return Ok(jiff::tz::TimeZone::UTC) };
    if name.eq_ignore_ascii_case("utc") || name.eq_ignore_ascii_case("z") || name.eq_ignore_ascii_case("gmt") {
        return Ok(jiff::tz::TimeZone::UTC);
    }
    if let Some(sign) = name.chars().next().filter(|c| *c == '+' || *c == '-') {
        let rest = &name[1..];
        let (h, m) = match rest.split_once(':') {
            Some((h, m)) => (h, m),
            None if rest.len() == 4 => (&rest[..2], &rest[2..]),
            None => (rest, "0"),
        };
        let parsed = h.parse::<i32>().ok().zip(m.parse::<i32>().ok()).filter(|(h, m)| *h <= 18 && *m < 60);
        let Some((h, m)) = parsed else {
            return Err(format!("`{name}` is not a zone — an offset is +02:00 or -0530; a name, Europe/Berlin"));
        };
        let secs = (h * 3600 + m * 60) * if sign == '-' { -1 } else { 1 };
        let off = jiff::tz::Offset::from_seconds(secs).map_err(|e| e.to_string())?;
        return Ok(jiff::tz::TimeZone::fixed(off));
    }
    jiff::tz::TimeZone::get(name).map_err(|_| format!("unknown time zone `{name}` — a name like Europe/Berlin, UTC, or an offset like +02:00"))
}

/// FILTER-30: each ticket's activity — the moment of every comment left on
/// it and of every answer to or clear of one of its inputs, placed by `at`
/// (a ticket it does not place — a deleted one's — left out).
pub(super) fn activity(
    at: &HashMap<String, OrderKey>,
    comments: &super::comments::Comments,
    inputs: &super::inputs::Inputs,
) -> Vec<(OrderKey, i64)> {
    let place = |id: &str| at.get(id).cloned();
    let said = comments.activity().filter_map(|(id, t)| Some((place(id)?, t)));
    let answered = inputs.activity().filter_map(|(id, t)| Some((place(id)?, t)));
    said.chain(answered).collect()
}

/// Records `t` inside the caller's transaction.
pub(crate) fn record(conn: &Connection, t: &Transition) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO transitions (id, ticket, field, from_value, to_value, at_unix, actor, name, source, message, cause)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?
    .execute(params![
        t.id as i64,
        t.ticket,
        t.field.as_str(),
        t.from,
        t.to,
        t.at_unix,
        t.actor,
        t.name,
        t.source.as_str(),
        t.message,
        t.cause.map(|c| c as i64),
    ])?;
    Ok(())
}

/// Every transition as stored, in id order.
fn read_all(conn: &Connection) -> rusqlite::Result<Vec<Transition>> {
    let mut stmt = conn.prepare(
        "SELECT id, ticket, field, from_value, to_value, at_unix, actor, name, source, message, cause
             FROM transitions ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        let bad = |at: usize, e: String| rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into());
        Ok(Transition {
            id: r.get::<_, i64>(0)? as u64,
            ticket: r.get(1)?,
            field: r.get::<_, String>(2)?.parse().map_err(|e| bad(2, e))?,
            from: r.get(3)?,
            to: r.get(4)?,
            at_unix: r.get(5)?,
            actor: r.get(6)?,
            name: r.get(7)?,
            source: r.get::<_, String>(8)?.parse().map_err(|e| bad(8, e))?,
            message: r.get(9)?,
            cause: r.get::<_, Option<i64>>(10)?.map(|c| c as u64),
            cause_ticket: None,
        })
    })?;
    rows.collect()
}

/// When the log began, as stored.
fn began(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT at_unix FROM transitions_began WHERE one = 1", [], |r| r.get(0)).optional()
}

/// The log as stored, its marks placed by `at` (the cache's map from an id
/// to its place in the store's order). A transition of an id the store does
/// not hold — only a hand edit makes one — is logged and left out.
pub(crate) fn load<'r>(
    conn: &Connection,
    at: &HashMap<String, OrderKey>,
    rows: impl IntoIterator<Item = &'r TicketSummary>,
) -> anyhow::Result<Log> {
    let started = std::time::Instant::now();
    let mut log = Log::began_at(began(conn)?);
    let mut stray = 0usize;
    // ACT-5: a deleted ticket's transitions are kept, placed where its
    // tombstone says it was — ORG-3: read from the tombstone, never parsed
    // from the id (a key is opaque).
    let deleted = super::delete::deleted_places(conn)?;
    let place = |id: &str| -> Option<OrderKey> { deleted.get(id).cloned() };
    for t in read_all(conn)? {
        match at.get(t.ticket.as_str()).cloned().or_else(|| place(&t.ticket)) {
            Some(key) => log.append(&t, &key),
            None => stray += 1,
        }
    }
    if stray > 0 {
        tracing::warn!("MENGER-13: {stray} transition(s) name no ticket the store holds — not loaded");
    }
    // FILTER-27: the times, from the rows and the log just read.
    log.time_all(rows);
    tracing::info!(
        "MENGER-13: read {} transition(s) and kept their indexes in {:.1} ms",
        log.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(log)
}

/// MENGER-13: `--verify-derived`'s look at the log, against `cache` (built
/// by a scan of the same snapshot's tickets): `None` when the file has no
/// log yet; else how many transitions it holds and each place it disagrees
/// with the tickets or with itself, one line each.
pub(super) fn check(conn: &Connection, cache: &Cache) -> anyhow::Result<Option<(usize, Vec<String>)>> {
    let had: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'transitions'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )?;
    if !had {
        return Ok(None);
    }
    let all = read_all(conn)?;
    let deleted = super::delete::deleted_ids(conn)?;
    let mut out = Vec::new();
    let mut ids: HashMap<u64, &Transition> = HashMap::new();
    let mut last: HashMap<(&str, TransitionField), &Transition> = HashMap::new();
    let mut previous_id = 0u64;
    for t in &all {
        if t.id <= previous_id {
            out.push(format!("transition {}: ids out of order (after {previous_id})", t.id));
        }
        previous_id = t.id;
        // ACT-5: a deleted ticket's transitions stay, with its tombstone.
        if cache.get(&t.ticket).is_none() && !deleted.contains(t.ticket.as_str()) {
            out.push(format!("transition {}: names {}, which is no ticket", t.id, t.ticket));
            continue;
        }
        let parses = match t.field {
            TransitionField::Status => {
                t.to.parse::<Status>().is_ok() && t.from.as_deref().is_none_or(|f| f.parse::<Status>().is_ok())
            }
            TransitionField::Kind => t.to.parse::<Kind>().is_ok() && t.from.as_deref().is_none_or(|f| f.parse::<Kind>().is_ok()),
        };
        if !parses {
            out.push(format!("transition {}: {:?} → {:?} is not a {}", t.id, t.from, t.to, t.field.as_str()));
        }
        match t.cause {
            Some(c) if !ids.contains_key(&c) => {
                out.push(format!("transition {}: its cause {c} is no earlier transition", t.id));
            }
            None if t.source == TransitionSource::Cascade => {
                out.push(format!("transition {}: a cascade's, with no cause", t.id));
            }
            _ => {}
        }
        if t.actor == SYSTEM_ACTOR && t.message.as_deref().is_none_or(|m| m.trim().is_empty()) {
            out.push(format!("transition {}: the system's, with no message", t.id));
        }
        if let Some(before) = last.get(&(t.ticket.as_str(), t.field)) {
            if t.from.as_deref() != Some(before.to.as_str()) {
                out.push(format!(
                    "transition {} of {}: from {:?}, but the transition before it ({}) ended at {}",
                    t.id, t.ticket, t.from, before.id, before.to
                ));
            }
        }
        ids.insert(t.id, t);
        last.insert((t.ticket.as_str(), t.field), t);
    }
    for ((ticket, field), t) in &last {
        let Some(row) = cache.get(ticket) else { continue };
        let now = match field {
            TransitionField::Status => row.status.as_str(),
            TransitionField::Kind => row.kind.as_str(),
        };
        if t.to != now {
            out.push(format!(
                "{} of {ticket}: the log's latest transition ({}) ends at {}; the ticket is {now}",
                field.as_str(),
                t.id,
                t.to
            ));
        }
    }
    // FILTER-27: the times, as an open reads them, held to a fresh reading
    // of every row and to themselves. FILTER-30: with the comments' and the
    // answers' activity.
    let mut log = load(conn, &cache.at, cache.order.values())?;
    let has_comments: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'comments'",
        [],
        |r| r.get::<_, i64>(0).map(|n| n > 0),
    )?;
    let comments = if has_comments { super::comments::load(conn)? } else { Default::default() };
    let inputs = super::inputs::load_if_any(conn)?;
    log.touch_all(activity(&cache.at, &comments, &inputs));
    out.extend(log.check_times(cache.order.values()));
    out.sort();
    Ok(Some((all.len(), out)))
}

impl Log {
    /// FILTER-27: where the kept times differ from the times `rows` and the
    /// log give afresh, or from themselves ([`Times::check`]) — and each
    /// exact entry's status from its latest transition's.
    pub(super) fn check_times<'r>(&self, rows: impl IntoIterator<Item = &'r TicketSummary>) -> Vec<String> {
        let mut fresh = Log { times: Times::with_active(self.times.clone().take_active()), ..self.clone() };
        fresh.time_all(rows);
        let mut out = self.times.check(&fresh.times, |e| self.epic_name(e).to_string());
        for (key, ps) in &self.tickets {
            let Some(t) = self.times.get(key.0, key.1) else { continue };
            let last = ps.iter().rev().map(|p| self.marks[*p as usize]).find(|m| m.field == TransitionField::Status);
            if let Some(m) = last {
                if (m.at, m.to) != (t.since, t.status) || !t.exact {
                    out.push(format!(
                        "times of {}-{}: status {} since {} ({}); its latest transition: {} at {}",
                        self.epic_name(key.0),
                        key.1,
                        t.status,
                        t.since,
                        if t.exact { "exact" } else { "a bound" },
                        m.to,
                        m.at
                    ));
                }
            }
        }
        out
    }
}
