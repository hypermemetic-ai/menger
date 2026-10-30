//! TICKETS-35: **the uids** — every ticket's one name that never changes,
//! and what the store does with them. Owner, 2026-10-30: *"update prod
//! menger to properly handle mulitple shortcodes of the same name. that
//! means rewiring the urls too. urls should be uuid based not shortcode
//! based"*; the form is TICKETS-11's `uid-slug` ([`Uid`]).
//!
//! - **Kept**: a column of the tickets (`uid`) and of the tombstones, each
//!   under a unique index — not derived state: the stored form stays 15. A
//!   write carries its row's uid wherever the row goes: `reshortcode` and
//!   `rename` rewrite the key, never the uid; a delete keeps it on the
//!   tombstone, an undelete gives it back ([`super::delete`]).
//! - **Minted** at birth ([`Cache::mint_uid`]): 40 random bits, drawn again
//!   until they are no ticket's or tombstone's uid and no shortcode's code.
//! - **The open** ([`migrate`]): a file whose rows have no uid — every file
//!   before TICKETS-35, and a row a server before it wrote since — gains one
//!   for every ticket and every tombstone, in one transaction, the count in
//!   the log; every later open mints none. A SIGKILL before the commit
//!   leaves the file as it was, and the next open mints them all.
//! - **The index** ([`Uids`]): each uid to its ticket's key, a live one's
//!   or a deleted one's — what the one resolver reads a uid by
//!   ([`super::shortcodes`]'s `find_ticket`).
//! - **Going out**: `uid_of` — the uid of each ticket a ticket or a summary
//!   names, of the tickets its reader sees ([`Store::dressed`]); and
//!   `choices` — what `ambiguous` says of each ticket a name fits
//!   ([`Store::choices`]).
//! - **`--verify-derived`** ([`check`]): a ticket or a tombstone without a
//!   uid, and two with one, are differences.

use super::seen::{Reader, Seen};
use super::*;
use crate::address::TicketName;
use crate::types::{Choice, ChoiceParent, TicketsEvent};
use crate::vocab::Uid;
use std::collections::HashSet;

/// The column, on the tickets and on the tombstones.
pub(super) const COLUMN: &str = "uid";

/// The file's own guard under the minting: no two tickets, and no two
/// tombstones, share a uid (a ticket and a tombstone never do either: an
/// undelete moves the uid in one transaction). Partial — a row a server
/// before TICKETS-35 wrote has none until the next open.
const INDEXES: &str = "
        CREATE UNIQUE INDEX IF NOT EXISTS tickets_uid ON tickets (uid) WHERE uid IS NOT NULL;
        CREATE UNIQUE INDEX IF NOT EXISTS tombstones_uid ON tombstones (uid) WHERE uid IS NOT NULL;
    ";

/// Each uid the store holds → its ticket's key: every live ticket's and
/// every tombstone's. Built at the open, a birth adds one, and a move that
/// rewrites keys reads it back from the file with the rest.
#[derive(Debug, Clone, Default)]
pub(super) struct Uids {
    of: HashMap<Uid, String>,
}

impl Uids {
    /// The index of `rows` and `tombstones`.
    pub(super) fn of<'r>(rows: impl IntoIterator<Item = &'r Row>, tombstones: &delete::Tombstones) -> Self {
        let mut of: HashMap<Uid, String> = rows.into_iter().filter(|r| r.uid.is_some()).map(|r| (r.uid, r.id.to_string())).collect();
        for key in tombstones.ids() {
            if let Some(t) = tombstones.get(&key).filter(|t| t.uid.is_some()) {
                of.entry(t.uid).or_insert(key);
            }
        }
        Self { of }
    }

    /// The key `uid` is the uid of — a live ticket's or a deleted one's.
    pub(super) fn key(&self, uid: &Uid) -> Option<&str> {
        self.of.get(uid).map(String::as_str)
    }

    /// `uid` is `key`'s now (a birth).
    pub(super) fn put(&mut self, uid: Uid, key: &str) {
        if uid.is_some() {
            self.of.insert(uid, key.to_string());
        }
    }

    /// Whether some ticket, live or deleted, has `uid`.
    pub(super) fn holds(&self, uid: &Uid) -> bool {
        self.of.contains_key(uid)
    }
}

/// 40 bits from the system's randomness — a v4 uuid's first five bytes,
/// which no version or variant bit touches.
pub(super) fn draw() -> u64 {
    let b = uuid::Uuid::new_v4().into_bytes();
    u64::from_be_bytes([0, 0, 0, b[0], b[1], b[2], b[3], b[4]])
}

/// Many draws at once — a migration's, a seed's: SplitMix64 over 64 bits of
/// the system's randomness, so a million uids cost no million system calls.
/// A uid is a name, not a secret: what may be read is the grants' to say.
pub(super) fn drawer() -> impl FnMut() -> u64 {
    let a = uuid::Uuid::new_v4().into_bytes();
    let b = uuid::Uuid::new_v4().into_bytes();
    splitmix(u64::from_be_bytes([a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]]))
}

/// A seed's draws (PERF-2): from its seed value — so the uids a synthetic
/// seed lands with are a function of it, as its tickets are.
pub(super) fn seeded(seed: u64) -> impl FnMut() -> u64 {
    splitmix(seed ^ 0x7123_5ED5_EED0_0035)
}

/// SplitMix64 from `state`.
fn splitmix(mut state: u64) -> impl FnMut() -> u64 {
    move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// A uid `taken` does not refuse, from `draw`'s bits — drawn again on a
/// clash (at prod's size, one in about 600 million).
pub(super) fn mint(taken: impl Fn(&Uid) -> bool, mut draw: impl FnMut() -> u64) -> Uid {
    loop {
        let uid = Uid::from_bits(draw());
        if !taken(&uid) {
            return uid;
        }
    }
}

impl Cache {
    /// TICKETS-35 (TICKETS-32): the ticket a write names by its permit — the
    /// key itself, read as a key and never again as a name: the bare form of
    /// a key may fit several tickets (`PERF-19` is MENGER/STORE's key and the
    /// bare id of PLEXUS's namesake too). Text that is no key — the system's
    /// and a test's permits name tickets by any name — is resolved as a
    /// name ([`Cache::resolve`]).
    pub(super) fn keyed(&self, text: &str) -> Result<String, StoreError> {
        let t = text.trim();
        if self.at.contains_key(t) {
            return Ok(t.to_string());
        }
        let folded = crate::address::folded(t);
        if self.at.contains_key(&folded) {
            return Ok(folded);
        }
        self.resolve(t)
    }

    /// TICKETS-35: a uid for a ticket being born — no ticket's or
    /// tombstone's, no shortcode's code (case-blind), and none of `also`
    /// (the write's own, when it mints more than one).
    pub(super) fn mint_uid(&self, also: &[Uid]) -> Uid {
        mint(|u| self.uids.holds(u) || also.contains(u) || self.shortcodes.holds_letters(u.as_str()), draw)
    }

    /// TICKETS-35: the key `text` names by its uid — bare or in a link — a
    /// live ticket's or a deleted one's. `None`: no uid, or one no ticket has.
    pub(super) fn uid_key(&self, text: &str) -> Option<&str> {
        self.uids.key(&crate::vocab::uid_named(text)?)
    }

    /// TICKETS-35: the uids of the tickets `names` names that are there and
    /// that `sees` admits, keyed as named — a forward reference (no ticket
    /// yet) has none.
    pub(super) fn uid_of<'n>(&self, names: impl IntoIterator<Item = &'n str>, sees: impl Fn(&str) -> bool) -> BTreeMap<TicketName, Uid> {
        names
            .into_iter()
            .filter_map(|n| {
                let row = self.get(n).filter(|r| r.uid.is_some())?;
                sees(n).then(|| (TicketName::new(n), row.uid))
            })
            .collect()
    }

    /// TICKETS-35: what `ambiguous` says of the candidate at `address` (a
    /// full address) to a reader who `sees` some tickets: the ticket it
    /// names, when the reader reads it — its parent too, when it reads that
    /// — else its address alone.
    fn choice(&self, address: &str, sees: &dyn Fn(&str) -> bool) -> Choice {
        let alone = || Choice { address: address.to_string(), ..Choice::default() };
        let shortcodes::Found::One(key) = self.find_ticket(address, shortcodes::OrgReach::All) else { return alone() };
        let Some(row) = self.get(&key).filter(|_| sees(&key)) else { return alone() };
        let r = &self.shortcodes;
        let parent = row.parent.as_deref().filter(|p| sees(p)).and_then(|p| self.get(p)).map(|p| ChoiceParent {
            uid: p.uid,
            id: p.id.clone(),
            address: p.address.clone(),
            title: p.title.clone(),
        });
        Choice {
            uid: row.uid.is_some().then_some(row.uid),
            id: Some(row.id.clone()),
            address: row.address.clone(),
            org: r.org(&row.shortcode).cloned(),
            epic: r.eras.get(&r.era_of(&row.shortcode)).map(|e| e.name.clone()),
            kind: Some(row.kind),
            status: Some(row.status),
            title: Some(row.title.clone()),
            parent,
        }
    }
}

impl Store {
    /// TICKETS-35: what `ambiguous` says of each of `candidates` (full
    /// addresses, in the store's order) to `who` — a [`Choice`] each, in
    /// the same order: the ticket whole when `who` reads it, else its
    /// address alone (as a candidate is).
    pub fn choices(&self, who: &(impl Reader + ?Sized), candidates: &[String]) -> Vec<Choice> {
        let Ok(seen) = Seen::of(who) else {
            return candidates.iter().map(|c| Choice { address: c.clone(), ..Choice::default() }).collect();
        };
        let cache = self.tickets.read().expect("store lock poisoned");
        let sees = seen.sees(&cache);
        let sees = |k: &str| sees.id(k);
        candidates.iter().map(|c| cache.choice(c, &sees)).collect()
    }

    /// TICKETS-35: `ev` as it goes out — each ticket and summary it carries
    /// given `uid_of`: the uids of the tickets it names, from the store as
    /// it stands. What a row names is what its reader may see already (a
    /// read and a watch cut the rest away, ACCESS-10/12), so nothing more is
    /// asked of the reader here.
    pub fn dressed(&self, mut ev: TicketsEvent) -> TicketsEvent {
        let mut rows = ev.each_named();
        if !rows.is_empty() {
            let cache = self.tickets.read().expect("store lock poisoned");
            for named in rows.iter_mut() {
                *named.uid_of = cache.uid_of(named.names(), |_| true);
            }
        }
        drop(rows);
        ev
    }
}

/// TICKETS-35: the open's uids — a file without the column gains it (on the
/// tickets and the tombstones) and its indexes, and every ticket and
/// tombstone without a uid (or with one that is no uid, or another's: a
/// hand edit's — the first in the file keeps it) is given one: no other's,
/// no shortcode's code. One transaction, before the cache is loaded; the
/// count logged. A file whose every row has its own mints none and writes
/// nothing but the `IF NOT EXISTS` indexes. Returns how many it minted.
pub(super) fn migrate(conn: &mut Connection) -> anyhow::Result<Minted> {
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;
    let mut added = false;
    for table in ["tickets", "tombstones"] {
        if !db::has_column(&tx, table, COLUMN)? {
            tx.execute(&format!("ALTER TABLE {table} ADD COLUMN {COLUMN} TEXT"), [])?;
            added = true;
        }
    }
    // What is held, the first holder of each uid keeping it; what needs one.
    let mut taken: HashSet<Uid> = HashSet::new();
    let mut need: Vec<(&'static str, String)> = Vec::new();
    for table in ["tickets", "tombstones"] {
        let mut stmt = tx.prepare(&format!("SELECT id, {COLUMN} FROM {table} ORDER BY rowid"))?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
        for row in rows {
            let (id, uid) = row?;
            match uid.as_deref().and_then(|u| u.parse::<Uid>().ok().filter(|p| p.as_str() == u)) {
                Some(u) if taken.insert(u) => {}
                _ => need.push((table, id)),
            }
        }
    }
    let mut minted = Minted::default();
    if !need.is_empty() {
        tracing::info!("TICKETS-35: minting {} uid(s), in one transaction", need.len());
        let codes = codes(&tx)?;
        let mut draw = drawer();
        for (table, id) in &need {
            let uid = mint(|u| taken.contains(u) || codes.contains(u.as_str()), &mut draw);
            taken.insert(uid);
            tx.prepare_cached(&format!("UPDATE {table} SET {COLUMN} = ?2 WHERE id = ?1"))?.execute(rusqlite::params![id, uid])?;
            match *table {
                "tickets" => minted.tickets += 1,
                _ => minted.tombstones += 1,
            }
        }
    }
    tx.execute_batch(INDEXES)?;
    tx.commit()?;
    if minted.total() > 0 {
        tracing::warn!(
            "TICKETS-35: minted {} uid(s) — {} ticket(s) and {} tombstone(s){} — in {:.1} ms, durably; every later open mints none",
            minted.total(),
            minted.tickets,
            minted.tombstones,
            if added { ", the file's first" } else { " a server before TICKETS-35 left without one" },
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    Ok(minted)
}

/// What [`migrate`] minted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Minted {
    pub tickets: usize,
    pub tombstones: usize,
}

impl Minted {
    pub fn total(&self) -> usize {
        self.tickets + self.tombstones
    }
}

/// Every shortcode's code the file holds, lower case — what no uid may be:
/// the registry's codes, and the letters of every shortcode a ticket or a
/// tombstone names (a file from before ORG-3 has no registry).
fn codes(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<HashSet<String>> {
    let mut out = HashSet::new();
    let has = |table: &str| -> rusqlite::Result<bool> {
        tx.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get::<_, i64>(0).map(|n| n > 0))
    };
    let mut queries = vec!["SELECT DISTINCT epic FROM tickets".to_string(), "SELECT DISTINCT epic FROM tombstones".to_string()];
    if has("shortcodes")? {
        queries.push("SELECT code FROM shortcodes".into());
    }
    for q in queries {
        let mut stmt = tx.prepare(&q)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for code in rows {
            let code = code?;
            out.insert(code.split('.').next().unwrap_or(&code).to_ascii_lowercase());
        }
    }
    Ok(out)
}

/// TICKETS-35: `--verify-derived`'s look at the uids — `None` when the file
/// has no column yet (the next open mints them): every ticket and every
/// tombstone has one, and no two share one. How many uids it read, and a
/// line per difference.
pub(super) fn check(conn: &Connection) -> rusqlite::Result<Option<(usize, Vec<String>)>> {
    if !db::has_column(conn, "tickets", COLUMN)? {
        return Ok(None);
    }
    let mut out = Vec::new();
    let mut held: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let tombstones = db::has_column(conn, "tombstones", COLUMN)?;
    let mut tables = vec![("tickets", "ticket")];
    if tombstones {
        tables.push(("tombstones", "tombstone"));
    }
    for (table, what) in tables {
        let mut stmt = conn.prepare(&format!("SELECT id, {COLUMN} FROM {table} ORDER BY id"))?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
        for row in rows {
            let (id, uid) = row?;
            match uid.as_deref().and_then(|u| u.parse::<Uid>().ok().filter(|p| p.as_str() == u)) {
                Some(u) => held.entry(u.into_string()).or_default().push(format!("{what} {id}")),
                None => out.push(match uid {
                    None => format!("uid of {what} {id}: none (the next open mints one)"),
                    Some(u) => format!("uid of {what} {id}: `{u}` is no uid"),
                }),
            }
        }
    }
    let n = held.values().map(Vec::len).sum();
    for (uid, by) in held.into_iter().filter(|(_, by)| by.len() > 1) {
        out.push(format!("uid {uid}: held by {}", by.join(" and ")));
    }
    Ok(Some((n, out)))
}
