//! ORG-3: the shortcodes — each one's namespace, stored, and the one
//! resolver from what a reader writes to what the store keys on.
//!
//! A shortcode is unique only within its namespace: its organization and
//! the shortcode its root is hosted under (none for a root of the org) —
//! [`Namespace`]. The namespace is **stored** on the shortcode, in the
//! `shortcodes` table, whose unique index on `(org, ifnull(host, ''), code)`
//! is the file's own guard (a plain `UNIQUE (org, host, code)` would admit two
//! roots of one name: SQLite holds NULLs distinct — ORG-6's trap). The
//! roll-ups derive a host from where each root hangs ([`rollup`]'s settled
//! host); the two must agree, every write that moves a root keeps them so
//! ([`Cache::namespaces`]), and `--verify-derived` holds the stored to the
//! derived ([`check`]).
//!
//! The store keys on [`ShortcodeId`]s and ticket keys; text becomes one
//! here and nowhere else ([`Cache::resolve`], [`Cache::resolve_shortcode`]).
//! A short form resolves to exactly one in the reader's reach, else
//! `ambiguous` with every candidate's full address, else not found — never
//! forbidden. Today's ids are **pinned** to today's tickets (ORG-2's input
//! `pinned`, on its recommended answer): `THEME-3` keeps naming today's
//! THEME-3 however many THEMEs come, through the `pins` table — every id
//! and tombstone the file held when it gained this table — and so does a
//! bare code name today's shortcode of that code (`pinned`). ACT-6's
//! aliases are rows of the same table.

use super::rollup::Rollups;
use super::{Cache, Rows, StoreError};
use crate::address::{shortcode_address_in, Address, At, Code, EpicName, Namespace, OrgId, Shortcode, ShortcodeId, TicketName};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, HashMap};

/// The tables: the organizations (ORG-4 makes more than the one), the
/// shortcodes with their namespaces, and the pins. Additive — an older
/// build never reads them.
pub(super) const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS orgs (
            id   INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );
        CREATE TABLE IF NOT EXISTS shortcodes (
            id     TEXT PRIMARY KEY,
            org    INTEGER NOT NULL REFERENCES orgs (id),
            host   TEXT,
            code   TEXT NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0,
            era    INTEGER
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS pins (
            text   TEXT PRIMARY KEY,
            ticket TEXT NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS org_aliases (
            name TEXT PRIMARY KEY,
            org  INTEGER NOT NULL REFERENCES orgs (id)
        ) WITHOUT ROWID;
    ";

/// ORG-13: the epics' tables — each organization's epics (its first holds
/// everything made before epics), and each principal's current one per
/// organization. Additive: an older build never reads them. The namespace
/// index gains the epic ([`ensure_eras`]), so one code may be a root in two
/// epics of one organization.
pub(super) const ERA_TABLES: &str = "
        CREATE TABLE IF NOT EXISTS eras (
            id           INTEGER PRIMARY KEY,
            org          INTEGER NOT NULL REFERENCES orgs (id),
            name         TEXT NOT NULL,
            created_unix INTEGER NOT NULL DEFAULT 0
        );
        CREATE UNIQUE INDEX IF NOT EXISTS eras_name ON eras (org, name);
        CREATE TABLE IF NOT EXISTS era_choices (
            principal TEXT NOT NULL,
            org       INTEGER NOT NULL,
            era       INTEGER NOT NULL,
            PRIMARY KEY (principal, org)
        ) WITHOUT ROWID;
    ";

/// ORG-13: the epics made to cover the file — idempotent, in the caller's
/// transaction, after the shortcodes' tables and the organizations are
/// there: the epics' tables; the shortcodes' `era` column (a file from
/// before ORG-13 gains it, every row NULL); every organization without an
/// epic given its first, `genesis`; every shortcode without an epic of its
/// organization put in that organization's first; and the namespace index
/// made `(org, era, host, code)` — ORG-3's `(org, host, code)` index
/// dropped. Writes nothing else: no ticket, no pin, no address. Returns how
/// many shortcodes it placed.
pub(super) fn ensure_eras(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute_batch(ERA_TABLES)?;
    // ORG-23: the archive's columns and tables, additive.
    super::era_archive::ensure(conn)?;
    let has_era: i64 = conn.query_row("SELECT count(*) FROM pragma_table_info('shortcodes') WHERE name = 'era'", [], |r| r.get(0))?;
    if has_era == 0 {
        conn.execute_batch("ALTER TABLE shortcodes ADD COLUMN era INTEGER")?;
    }
    conn.execute(
        "INSERT INTO eras (org, name, created_unix) SELECT id, ?1, CAST(strftime('%s', 'now') AS INTEGER) FROM orgs
             WHERE id NOT IN (SELECT org FROM eras) ORDER BY id",
        [EpicName::GENESIS],
    )?;
    let placed = conn.execute(
        "UPDATE shortcodes SET era = (SELECT min(e.id) FROM eras e WHERE e.org = shortcodes.org)
             WHERE era IS NULL OR era NOT IN (SELECT e.id FROM eras e WHERE e.org = shortcodes.org)",
        [],
    )?;
    conn.execute_batch(
        "DROP INDEX IF EXISTS shortcodes_ns;
         CREATE UNIQUE INDEX IF NOT EXISTS shortcodes_ns_era ON shortcodes (org, ifnull(era, 0), ifnull(host, ''), code);",
    )?;
    Ok(placed)
}

/// ORG-13: one epic as the registry keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Era {
    /// Its organization's row.
    pub(super) org: i64,
    pub(super) name: EpicName,
    pub(super) created: i64,
    /// ORG-23: its organization's default (`eras.is_default`).
    pub(super) default: bool,
    /// ORG-23: archived — when, by whom (`eras.archived_at`, `archived_by`).
    pub(super) archived: Option<(i64, String)>,
}

impl Era {
    /// A live epic, not the default.
    pub(super) fn new(org: i64, name: EpicName, created: i64) -> Self {
        Self { org, name, created, default: false, archived: None }
    }
}

/// Which organizations a read may resolve in — ORG-4: a caller's, the
/// organizations it holds a role in (`All` for `*`, and with login off).
#[derive(Debug, Clone, Copy, Default)]
pub enum OrgReach<'a> {
    #[default]
    All,
    Only(&'a [OrgId]),
}

impl OrgReach<'_> {
    pub(super) fn holds(&self, org: &OrgId) -> bool {
        match self {
            Self::All => true,
            Self::Only(orgs) => orgs.contains(org),
        }
    }
}

/// One shortcode as the registry keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) sc: Shortcode,
    /// The org's row id.
    pub(super) org: i64,
    /// Its bare code names it, whatever namesakes come (a shortcode the file
    /// held when it gained the table).
    pinned: bool,
    /// Its full address, `org:ROOT/…/SC`, kept with it.
    pub(super) address: String,
}

/// What a resolve found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Found<T> {
    One(T),
    /// Every candidate's full address, in the store's order.
    Many(Vec<String>),
    None,
}

/// The shortcodes, their namespaces and the pins — read from the file at
/// open, moved by the writes that mint a shortcode or move a root, and the
/// only place text becomes a [`ShortcodeId`].
#[derive(Debug, Clone, Default)]
pub(super) struct Registry {
    pub(super) orgs: BTreeMap<i64, OrgId>,
    /// ORG-4: every name an organization had before a rename, to its row —
    /// an address written with it keeps resolving ([`super::orgs`]).
    pub(super) aliases: BTreeMap<String, i64>,
    /// V2-41: each person's own organization, by row, to its owner's
    /// principal ([`super::personal`]).
    pub(super) owners: BTreeMap<i64, String>,
    pub(super) entries: BTreeMap<ShortcodeId, Entry>,
    /// The resolver's index from the letters to the shortcodes that have
    /// them — the one map keyed by a code's text, private to the resolver:
    /// it answers candidates, never an identity on its own.
    by_letters: HashMap<String, Vec<ShortcodeId>>,
    /// Text → the key it is pinned to (today's ids, tombstones; ACT-6's
    /// aliases).
    pins: HashMap<String, String>,
    /// ACT-6: each moved ticket's key → the ids it had, sorted — the
    /// pins whose text is another ticket's id ([`Registry::index_aliases`]).
    moved_ids: HashMap<String, Vec<String>>,
    /// ACT-6: each shortcode's highest number a move took out of it — the
    /// next ticket of it is born past it, as past a deleted one's: an old id
    /// keeps naming the ticket that moved.
    moved_last: HashMap<ShortcodeId, u32>,
    /// ORG-13: every epic, by its row (the `eras` table).
    pub(super) eras: BTreeMap<i64, Era>,
    /// ORG-13: each principal's current epic in each organization (the
    /// `era_choices` table) — `(principal, org row) → era row`; one with
    /// none is in its organization's default (ORG-23).
    pub(super) choices: HashMap<(String, i64), i64>,
    /// ORG-23: each move an archive made of a principal's current epic, not
    /// yet dismissed — `(principal, org row) → the move` (`era_moves`).
    pub(super) moves: HashMap<(String, i64), super::era_archive::MoveRec>,
    /// ORG-23: each old name a rename left, as an alias — `(org row, name)
    /// → era row` (`era_aliases`).
    pub(super) era_aliases: HashMap<(i64, String), i64>,
}

impl Registry {
    pub(super) fn get(&self, id: &ShortcodeId) -> Option<&Shortcode> {
        self.entries.get(id).map(|e| &e.sc)
    }

    /// Every shortcode, in the store's order.
    #[cfg(test)]
    pub(super) fn all(&self) -> impl Iterator<Item = &Shortcode> {
        self.entries.values().map(|e| &e.sc)
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// `id`'s letters — its own id's text for one the registry does not
    /// hold yet (a row being read before the registry is).
    pub(super) fn code(&self, id: &ShortcodeId) -> Code {
        self.entries.get(id).map_or_else(|| Code::stored(id.as_str()), |e| e.sc.code.clone())
    }

    /// `id`'s full address, `org:ROOT/…/SC` — ORG-13: `org:epic:ROOT/…/SC`
    /// past its organization's first epic.
    pub(super) fn address(&self, id: &ShortcodeId) -> String {
        self.entries.get(id).map_or_else(|| format!("{}:{}", self.first_org(), id.as_str()), |e| e.address.clone())
    }

    /// ORG-13: the first epic of the organization `org` (its row) — the one
    /// everything before epics is in, whose addresses name no epic.
    pub(super) fn first_era(&self, org: i64) -> Option<i64> {
        self.eras.iter().find(|(_, e)| e.org == org).map(|(id, _)| *id)
    }

    /// ORG-23: `org`'s default epic — where a principal who chose none is:
    /// the one marked default and live, else its first live one (never the
    /// newest: making an epic moves nobody).
    pub(super) fn default_era(&self, org: i64) -> Option<i64> {
        let live = || self.eras.iter().filter(|(_, e)| e.org == org && e.archived.is_none());
        live().find(|(_, e)| e.default).or_else(|| live().next()).map(|(id, _)| *id).or_else(|| self.first_era(org))
    }

    /// ORG-23: whether the epic `era` is archived.
    pub(super) fn era_archived(&self, era: i64) -> bool {
        self.eras.get(&era).is_some_and(|e| e.archived.is_some())
    }

    /// ORG-13: `org`'s epic called `name` — ORG-23: or an old name a rename
    /// left as its alias.
    pub(super) fn era_named(&self, org: i64, name: &str) -> Option<i64> {
        let n = name.trim();
        self.eras
            .iter()
            .find(|(_, e)| e.org == org && e.name.as_str() == n)
            .map(|(id, _)| *id)
            .or_else(|| self.era_aliases.get(&(org, n.to_string())).copied().filter(|e| self.eras.contains_key(e)))
    }

    /// ORG-13: the epic `id` is in (0 when the registry places it nowhere).
    pub(super) fn era_of(&self, id: &ShortcodeId) -> i64 {
        self.entries.get(id).map_or(0, |e| e.sc.ns.era)
    }

    /// ORG-13: an epic's name as an address writes it — `None` for its
    /// organization's first (and for none).
    fn era_label(&self, era: i64) -> Option<&EpicName> {
        let e = self.eras.get(&era)?;
        (self.first_era(e.org) != Some(era)).then_some(&e.name)
    }

    /// ORG-13: whether some organization holds more than one epic — until
    /// one does, nothing reads differently.
    pub(super) fn several_eras(&self) -> bool {
        let mut orgs = std::collections::HashSet::new();
        self.eras.values().any(|e| !orgs.insert(e.org))
    }

    /// ORG-13: `principal`'s current epic in `org` — its choice, else
    /// (ORG-23) the organization's default. An archived epic is no one's.
    pub(super) fn current_era(&self, principal: &str, org: i64) -> Option<i64> {
        self.choices
            .get(&(principal.to_string(), org))
            .copied()
            .filter(|e| self.eras.get(e).is_some_and(|x| x.archived.is_none()))
            .or_else(|| self.default_era(org))
    }

    /// ORG-13: the epics a call is read in first — its caller's current one
    /// in each organization (the call's act names the caller); none outside
    /// a call, or while no organization holds more than one.
    pub(super) fn preferred_now(&self) -> Vec<i64> {
        if !self.several_eras() {
            return Vec::new();
        }
        let Some(me) = super::eras::reader_now() else { return Vec::new() };
        self.orgs.keys().filter_map(|o| self.current_era(&me, *o)).collect()
    }

    /// ORG-13: `ids` narrowed to the epics `pref` names — `None` when none
    /// of them is there.
    fn in_eras<'i>(&self, ids: impl IntoIterator<Item = &'i ShortcodeId>, pref: &[i64]) -> Vec<ShortcodeId> {
        ids.into_iter().filter(|id| pref.contains(&self.era_of(id))).cloned().collect()
    }

    /// The full address of `id`'s ticket numbered `seq`.
    pub(super) fn ticket_address(&self, id: &ShortcodeId, seq: u32, key: &str) -> String {
        format!("{}-{}", self.address(id), crate::address::number_of(key, seq))
    }

    /// The org a shortcode is in.
    pub(super) fn org(&self, id: &ShortcodeId) -> Option<&OrgId> {
        self.entries.get(id).map(|e| &e.sc.ns.org)
    }

    /// The org of that name, if there is one — ORG-4: or of that name
    /// before a rename.
    pub(super) fn org_row(&self, org: &OrgId) -> Option<i64> {
        self.org_named(org.as_str()).map(|(id, _)| id)
    }

    /// The shortcode with `code` in `ns`, if one is there.
    pub(super) fn in_namespace(&self, ns: &Namespace, code: &Code) -> Option<&ShortcodeId> {
        self.by_letters
            .get(code.as_str())
            .into_iter()
            .flatten()
            .find(|id| self.entries.get(*id).is_some_and(|e| e.sc.ns == *ns))
    }

    /// `id` and the shortcodes hosting it, outermost first.
    fn chain(&self, id: &ShortcodeId) -> Vec<&ShortcodeId> {
        let mut out: Vec<&ShortcodeId> = Vec::new();
        let mut cur = self.entries.get(id).map(|e| &e.sc.id);
        while let Some(c) = cur {
            if out.contains(&c) {
                break; // a host loop the roll-ups would not draw: no address goes round it
            }
            out.push(c);
            cur = self.entries.get(c).and_then(|e| e.sc.ns.host.as_ref());
        }
        out.reverse();
        out
    }

    /// Every entry's full address, from the chain of hosts, again — after
    /// the registry is read or a write moved a namespace. Returns the
    /// shortcodes whose address it changed.
    pub(super) fn readdress(&mut self) -> Vec<ShortcodeId> {
        let fresh: Vec<(ShortcodeId, String)> = self
            .entries
            .keys()
            .map(|id| {
                let chain: Vec<&str> = self.chain(id).into_iter().map(|c| self.entries[c].sc.code.as_str()).collect();
                let ns = &self.entries[id].sc.ns;
                (id.clone(), shortcode_address_in(&ns.org, self.era_label(ns.era), &chain))
            })
            .collect();
        let mut moved = Vec::new();
        for (id, address) in fresh {
            let e = self.entries.get_mut(&id).expect("listed");
            if e.address != address {
                e.address = address;
                moved.push(id);
            }
        }
        moved
    }

    fn insert(&mut self, e: Entry) {
        let letters = self.by_letters.entry(e.sc.code.as_str().to_string()).or_default();
        if !letters.contains(&e.sc.id) {
            letters.push(e.sc.id.clone());
            letters.sort();
        }
        self.entries.insert(e.sc.id.clone(), e);
    }

    /// ORG-3: a new shortcode `code` in `ns` — refused, naming the one there,
    /// when `ns` already holds the code. Its id is the code itself when no
    /// shortcode has that id yet (anywhere: an id is unique app-wide), else
    /// `CODE.N`, `N` the first number free. Not inserted: the write that
    /// mints it does that, with its first ticket.
    pub(super) fn mint(&self, ns: Namespace, code: Code) -> Result<Shortcode, StoreError> {
        if let Some(there) = self.in_namespace(&ns, &code) {
            return Err(self.taken(there, &ns, &code, None));
        }
        let plain = ShortcodeId::new(code.as_str());
        let id = match self.entries.contains_key(&plain) {
            false => plain,
            true => (self.entries.len() as u32 + 1..)
                .map(|n| ShortcodeId::new(&format!("{}.{n}", code.as_str())))
                .find(|id| !self.entries.contains_key(id))
                .expect("a free number"),
        };
        Ok(Shortcode { id, ns, code })
    }

    /// The refusal that names the shortcode already in `ns`.
    fn taken(&self, there: &ShortcodeId, ns: &Namespace, code: &Code, id: Option<&str>) -> StoreError {
        StoreError::ShortcodeTaken {
            id: id.map(str::to_string),
            code: code.as_str().to_string(),
            namespace: self.namespace_name(ns),
            there: self.address(there),
        }
    }

    /// A namespace as a reader writes it: `org:` for an org's roots, else
    /// its host's full address.
    pub(super) fn namespace_name(&self, ns: &Namespace) -> String {
        match &ns.host {
            None => format!("{}:", ns.org),
            Some(h) => self.address(h),
        }
    }

    /// ORG-3: the shortcodes an address's shortcode part matches in `reach` —
    /// its letters, its org when it names one, and the hosts it names among
    /// the shortcode's own, in order (a short form may skip some). In the
    /// store's order.
    pub(super) fn matching(&self, a: &Address, reach: OrgReach<'_>) -> Vec<ShortcodeId> {
        let Some(ids) = self.by_letters.get(a.code.as_str()) else { return Vec::new() };
        ids.iter()
            .filter(|id| {
                let e = &self.entries[*id];
                if !reach.holds(&e.sc.ns.org) || a.org.as_ref().is_some_and(|o| self.canonical(o) != Some(&e.sc.ns.org)) {
                    return false;
                }
                // ORG-13: an address that names its org names an epic — the
                // one it names, else the org's first.
                if a.org.is_some() {
                    let ok = match &a.era {
                        Some(name) => self.era_named(e.org, name) == Some(e.sc.ns.era),
                        None => e.sc.ns.era == 0 || self.first_era(e.org).is_none_or(|f| f == e.sc.ns.era),
                    };
                    if !ok {
                        return false;
                    }
                }
                // The hosts named, in order, among the chain above it.
                let chain = self.chain(id);
                let above = &chain[..chain.len().saturating_sub(1)];
                let mut at = 0;
                a.hosts.iter().all(|h| match above[at..].iter().position(|c| self.entries[*c].sc.code == *h) {
                    Some(i) => {
                        at += i + 1;
                        true
                    }
                    None => false,
                })
            })
            .cloned()
            .collect()
    }

    /// ORG-3: the shortcode `text` names in `reach` — a bare code that names
    /// a pinned shortcode is that one; else an id the store minted
    /// (`THEME.62`); else every shortcode the address matches.
    pub(super) fn find_shortcode(&self, text: &str, reach: OrgReach<'_>) -> Found<ShortcodeId> {
        let t = text.trim();
        let exact = ShortcodeId::new(t);
        if t.contains('.') && self.entries.get(&exact).is_some_and(|e| reach.holds(&e.sc.ns.org)) {
            return Found::One(exact);
        }
        let Ok(a) = Address::parse(t) else { return Found::None };
        if a.names_ticket() {
            return Found::None;
        }
        // ORG-13: the caller's current epic first — a pin there, then a
        // match there — then the pin, then every match.
        let pref = self.preferred_now();
        let pinned = Some(ShortcodeId::new(a.code.as_str()))
            .filter(|plain| a.is_bare() && self.entries.get(plain).is_some_and(|e| e.pinned && reach.holds(&e.sc.ns.org)));
        if let Some(p) = pinned.as_ref().filter(|p| pref.is_empty() || pref.contains(&self.era_of(p))) {
            return Found::One(p.clone());
        }
        let mut ids = self.matching(&a, reach);
        if a.org.is_none() && !pref.is_empty() {
            let mine = self.in_eras(&ids, &pref);
            match (mine.is_empty(), pinned) {
                (false, _) => ids = mine,
                (true, Some(p)) => return Found::One(p),
                (true, None) => {}
            }
        }
        self.found(ids)
    }

    /// Every shortcode's id.
    pub(super) fn ids(&self) -> impl Iterator<Item = &ShortcodeId> {
        self.entries.keys()
    }

    fn found(&self, ids: Vec<ShortcodeId>) -> Found<ShortcodeId> {
        match ids.len() {
            0 => Found::None,
            1 => Found::One(ids.into_iter().next().expect("one")),
            _ => Found::Many(ids.iter().map(|id| self.address(id)).collect()),
        }
    }

    /// ORG-3: the shortcodes an `in` names — every one whose letters are the
    /// code (the owner: match on letters, so `in THEME` is every THEME in
    /// reach), or, written with hosts or an org, every one the address
    /// matches. An id the store minted names itself.
    pub(super) fn in_set(&self, text: &str) -> Vec<ShortcodeId> {
        let exact = ShortcodeId::new(text.trim());
        if text.contains('.') && self.entries.contains_key(&exact) {
            return vec![exact];
        }
        // The language reads a shortcode whatever its case (FILTER-4): a
        // hand-edited `low` is `in LOW`, as before ORG-3.
        match Address::parse(text) {
            Ok(a) if !a.names_ticket() => {
                let mut all: Vec<ShortcodeId> = self
                    .by_letters
                    .keys()
                    .filter(|k| k.eq_ignore_ascii_case(a.code.as_str()))
                    .flat_map(|k| self.matching(&Address { code: Code::stored(k), ..a.clone() }, OrgReach::All))
                    .collect();
                all.sort();
                all
            }
            _ => Vec::new(),
        }
    }

    /// ACT-6: the ids `key` had before a move named it anew, sorted.
    pub(super) fn moved_from(&self, key: &str) -> Vec<String> {
        self.moved_ids.get(key).cloned().unwrap_or_default()
    }

    /// ACT-6: the highest number a move took out of `sc` — 0 for none.
    pub(super) fn moved_after(&self, sc: &ShortcodeId) -> u32 {
        self.moved_last.get(sc).copied().unwrap_or(0)
    }

    /// ACT-6: the aliases and each shortcode's moved-out numbers, from the
    /// pins — a pin whose text is a ticket's id (`{shortcode}-{n}`, no path)
    /// and whose ticket is another is an alias. Built once the pins are read.
    fn index_aliases(&mut self) {
        let (mut aliases, mut last): (HashMap<String, Vec<(u32, String)>>, HashMap<ShortcodeId, u32>) = Default::default();
        for (text, key) in &self.pins {
            if text == key || text.contains('/') {
                continue;
            }
            let Some((sc, n)) = text.rsplit_once('-').and_then(|(sc, n)| Some((ShortcodeId::new(sc), n.parse::<u32>().ok()?))) else {
                continue;
            };
            let l = last.entry(sc).or_insert(0);
            *l = (*l).max(n);
            aliases.entry(key.clone()).or_default().push((n, text.clone()));
        }
        self.moved_last = last;
        // Sorted: the pins keep no time, so a ticket moved twice lists both by
        // their text.
        self.moved_ids = aliases
            .into_iter()
            .map(|(k, v)| {
                let mut v: Vec<String> = v.into_iter().map(|(_, t)| t).collect();
                v.sort();
                (k, v)
            })
            .collect();
    }

    /// The key `text` is pinned to — today's id folded as a reader may write
    /// it (`theme-3`), or an alias as written.
    pub(super) fn pin(&self, text: &str) -> Option<&str> {
        let t = text.trim();
        if let Some(key) = self.pins.get(t) {
            return Some(key);
        }
        let a = Address::parse(t).ok().filter(|a| a.is_bare())?;
        let folded = match &a.at {
            At::Seq(n) => format!("{}-{n}", a.code.as_str()),
            At::Sub(..) => format!("{}-{}", a.code.as_str(), a.at_number()),
            At::Path(p) => format!("{}/{p}", a.code.as_str()),
            At::Shortcode => return None,
        };
        self.pins.get(&folded).map(String::as_str)
    }
}

/// The registry as the file holds it — every table empty, or absent (a file
/// `--verify-derived` reads before its first open by this build).
pub(super) fn load(conn: &Connection) -> rusqlite::Result<Registry> {
    let mut r = Registry::default();
    let present: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('orgs', 'shortcodes', 'pins')",
        [],
        |row| row.get(0),
    )?;
    if present < 3 {
        return Ok(r);
    }
    let mut stmt = conn.prepare("SELECT id, name FROM orgs")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        let org = OrgId::new(&name)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, e.into()))?;
        r.orgs.insert(row.get(0)?, org);
    }
    // ORG-13: the epics, and each shortcode's (a file from before ORG-13
    // has neither: every shortcode is then in no epic until the next open).
    let has_era: i64 = conn.query_row("SELECT count(*) FROM pragma_table_info('shortcodes') WHERE name = 'era'", [], |r| r.get(0))?;
    let eras: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('eras', 'era_choices')", [], |r| r.get(0))?;
    if eras == 2 {
        let mut stmt = conn.prepare("SELECT id, org, name, created_unix FROM eras ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(2)?;
            let name = EpicName::new(&name).map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into()))?;
            r.eras.insert(row.get(0)?, Era::new(row.get(1)?, name, row.get(3)?));
        }
        let mut stmt = conn.prepare("SELECT principal, org, era FROM era_choices")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            r.choices.insert((row.get(0)?, row.get(1)?), row.get(2)?);
        }
        // ORG-23: the archive's marks, the default, the moves and the aliases.
        super::era_archive::load(conn, &mut r)?;
    }
    let sql = match has_era {
        0 => "SELECT id, org, host, code, pinned, 0 FROM shortcodes",
        _ => "SELECT id, org, host, code, pinned, ifnull(era, 0) FROM shortcodes",
    };
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let org: i64 = row.get(1)?;
        let Some(org_name) = r.orgs.get(&org).cloned() else {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Integer,
                format!("shortcodes: org {org} is no organization").into(),
            ));
        };
        let id: String = row.get(0)?;
        let host: Option<String> = row.get(2)?;
        let code: String = row.get(3)?;
        r.insert(Entry {
            sc: Shortcode {
                id: ShortcodeId::new(&id),
                ns: Namespace { org: org_name, era: row.get(5)?, host: host.as_deref().map(ShortcodeId::new) },
                code: Code::stored(&code),
            },
            org,
            pinned: row.get(4)?,
            address: String::new(),
        });
    }
    let mut stmt = conn.prepare("SELECT text, ticket FROM pins")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        r.pins.insert(row.get(0)?, row.get(1)?);
    }
    // ORG-4: an organization's old names (a file from before ORG-4 has none).
    let aliased: i64 =
        conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'org_aliases'", [], |row| row.get(0))?;
    if aliased > 0 {
        let mut stmt = conn.prepare("SELECT name, org FROM org_aliases")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            r.aliases.insert(row.get(0)?, row.get(1)?);
        }
    }
    r.index_aliases();
    r.readdress();
    Ok(r)
}

/// One shortcode's row, written as it now stands (in the caller's
/// transaction).
pub(super) fn put(conn: &Connection, r: &Registry, id: &ShortcodeId) -> rusqlite::Result<()> {
    let e = &r.entries[id];
    conn.prepare_cached(
        "INSERT INTO shortcodes (id, org, host, code, pinned, era) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET org = excluded.org, host = excluded.host, code = excluded.code,
                 pinned = excluded.pinned, era = excluded.era",
    )?
    .execute(params![
        e.sc.id.as_str(),
        e.org,
        e.sc.ns.host.as_ref().map(ShortcodeId::as_str),
        e.sc.code.as_str(),
        e.pinned,
        (e.sc.ns.era != 0).then_some(e.sc.ns.era)
    ])?;
    Ok(())
}

/// The namespace each shortcode's root gives it as the rows and roll-ups
/// stand — the roll-ups' settled host ([`Rollups::host`]), a ticket, read as
/// that ticket's shortcode; `None` for a root of the org.
fn derived_host(rows: Rows<'_>, rollups: &Rollups, id: &ShortcodeId) -> Option<ShortcodeId> {
    rollups.host(id).and_then(|h| rows.get(h)).map(|t| t.shortcode.clone()).filter(|h| h != id)
}

/// ORG-3: a write's move of namespaces — the registry as the write leaves
/// it, and the shortcodes whose row it rewrites.
#[derive(Debug, Clone)]
pub(super) struct Moves {
    pub(super) after: Registry,
    pub(super) written: Vec<ShortcodeId>,
    /// The shortcodes whose full address moved — every row of each takes its
    /// new address once the write commits.
    pub(super) readdressed: Vec<ShortcodeId>,
}

impl Moves {
    /// Writes the rows it moves, inside the write's transaction.
    pub(super) fn write(&self, conn: &Connection) -> rusqlite::Result<()> {
        for id in &self.written {
            put(conn, &self.after, id)?;
        }
        Ok(())
    }
}

impl Cache {
    /// ORG-3: what a write that moves roots does to the namespaces, worked
    /// out before it commits — every shortcode whose host the roll-ups as
    /// the write leaves them (`after`, over `rows`) give differently from its
    /// stored namespace moves to the new one, and `minted` (a shortcode the
    /// write makes) joins. Refused, naming the shortcode already there, when
    /// any lands in a namespace holding its code — a rehoming into a clash
    /// (`set_parent` of a root, `update --parent`, a `set_kind` that changes
    /// which ticket roots it, a delete that reparents a root, an undelete).
    /// `None`: nothing moves. `id`: the ticket whose write it is, for the
    /// refusal.
    pub(super) fn namespaces(
        &self,
        rows: Rows<'_>,
        after: &Rollups,
        minted: Option<&Shortcode>,
        id: Option<&str>,
    ) -> Result<Option<Moves>, StoreError> {
        let mut next = self.shortcodes.clone();
        let mut written: Vec<ShortcodeId> = Vec::new();
        if let Some(sc) = minted {
            let org = next.org_row(&sc.ns.org).ok_or_else(|| StoreError::Invalid(format!("no organization {}", sc.ns.org)))?;
            next.insert(Entry { sc: sc.clone(), org, pinned: false, address: String::new() });
            written.push(sc.id.clone());
        }
        let ids: Vec<ShortcodeId> = next.entries.keys().cloned().collect();
        for sc in &ids {
            if after.root(sc).is_none() {
                continue; // a shortcode with no root (all deleted, or legacy) keeps where it was
            }
            let host = derived_host(rows, after, sc);
            let e = next.entries.get_mut(sc).expect("listed");
            if e.sc.ns.host != host {
                e.sc.ns.host = host;
                if !written.contains(sc) {
                    written.push(sc.clone());
                }
            }
        }
        // ORG-13: a hosted shortcode is in its host's epic — settled down
        // every chain of hosts.
        for _ in 0..ids.len().max(1) {
            let mut moved = false;
            for sc in &ids {
                let Some(h) = next.entries[sc].sc.ns.host.clone() else { continue };
                let era = next.era_of(&h);
                let e = next.entries.get_mut(sc).expect("listed");
                if era != 0 && e.sc.ns.era != era {
                    e.sc.ns.era = era;
                    moved = true;
                    if !written.contains(sc) {
                        written.push(sc.clone());
                    }
                }
            }
            if !moved {
                break;
            }
        }
        if written.is_empty() {
            return Ok(None);
        }
        // Every moved one in a namespace free of its code — the others there
        // as the write leaves them.
        for sc in &written {
            let e = &next.entries[sc];
            let clash = next.by_letters[e.sc.code.as_str()]
                .iter()
                .find(|other| *other != sc && next.entries[*other].sc.ns == e.sc.ns);
            if let Some(there) = clash {
                return Err(next.taken(there, &e.sc.ns, &e.sc.code, id));
            }
        }
        let readdressed = next.readdress();
        Ok(Some(Moves { after: next, written, readdressed }))
    }

    /// ORG-3: a committed write's [`Moves`], taken — the registry, and every
    /// row of a shortcode whose address moved given its new one.
    pub(super) fn take_namespaces(&mut self, moves: Moves) {
        self.shortcodes = moves.after;
        for sc in moves.readdressed {
            self.readdress_rows(&sc);
        }
    }

    /// Every row of `sc` with its code and address as the registry says.
    pub(super) fn readdress_rows(&mut self, sc: &ShortcodeId) {
        use std::ops::Bound::Included;
        let (from, to) = (super::OrderKey { shortcode: sc.clone(), seq: 0 }, super::OrderKey { shortcode: sc.clone(), seq: u32::MAX });
        let code = self.shortcodes.code(sc);
        let address = self.shortcodes.address(sc);
        for (_, t) in self.order.range_mut((Included(from), Included(to))) {
            t.epic = code.clone();
            t.address = format!("{address}-{}", crate::address::number_of(&t.id, t.seq));
        }
    }

    /// Every row's code and address, as the registry says — at open.
    pub(super) fn readdress_all(&mut self) {
        let ids: Vec<ShortcodeId> = self.shortcodes.entries.keys().cloned().collect();
        for sc in &ids {
            self.readdress_rows(sc);
        }
    }

    /// ORG-3: a row as it is about to be written, given its shortcode's code
    /// and address from `r` (a write's registry, before or after).
    pub(super) fn dressed(r: &Registry, mut row: super::Row) -> super::Row {
        row.epic = r.code(&row.shortcode);
        row.address = r.ticket_address(&row.shortcode, row.seq, &row.id);
        row
    }

    /// ORG-3: the ticket `text` names in the whole store — its key. An id
    /// pinned (today's, folded as a reader may write it: `theme-3`) is the
    /// ticket it was; an address — any short form — is the one ticket it
    /// matches; a key the store minted for a namesake (`THEME.62-3`) is
    /// itself. Several: [`StoreError::Ambiguous`], each by its full address;
    /// none: [`StoreError::NotFound`] with the text as given. WAVE-10's path
    /// addresses resolve here too.
    pub(super) fn resolve(&self, text: &str) -> Result<String, StoreError> {
        self.resolve_in(text, OrgReach::All)
    }

    /// [`Cache::resolve`] in `reach` (ORG-4's per-caller orgs).
    pub(super) fn resolve_in(&self, text: &str, reach: OrgReach<'_>) -> Result<String, StoreError> {
        match self.find_ticket(text, reach) {
            Found::One(key) => Ok(key),
            Found::Many(candidates) => Err(StoreError::Ambiguous { text: text.trim().to_string(), candidates }),
            // An id as the store spells it (`theme-3` → THEME-3); an address
            // as given.
            Found::None if text.contains('/') || text.contains(':') => Err(StoreError::NotFound(text.trim().to_string())),
            Found::None => Err(StoreError::NotFound(crate::address::folded(text))),
        }
    }

    /// What [`Cache::resolve`] found, unrefused.
    pub(super) fn find_ticket(&self, text: &str, reach: OrgReach<'_>) -> Found<String> {
        let t = text.trim();
        let held = |key: &str| self.at.contains_key(key) || self.tombstones.get(key).is_some();
        // ORG-4: a pin names its ticket to a reader who can reach its
        // organization; to another, the text is any short form.
        let reachable = |key: &str| match reach {
            OrgReach::All => true,
            OrgReach::Only(_) => self.get(key).and_then(|r| self.shortcodes.org(&r.shortcode)).is_some_and(|o| reach.holds(o)),
        };
        // ORG-13: the caller's current epic first — a pin there, then a
        // match there — then the pin, then every match.
        let pref = self.shortcodes.preferred_now();
        let in_pref = |key: &str| self.get(key).is_some_and(|r| pref.contains(&self.shortcodes.era_of(&r.shortcode)));
        let pinned = self.shortcodes.pin(t).filter(|k| held(k) && reachable(k)).map(str::to_string);
        if let Some(key) = pinned.as_ref().filter(|k| pref.is_empty() || in_pref(k)) {
            return Found::One(key.clone());
        }
        let a = match Address::parse(t) {
            Ok(a) => a,
            // Not an address: a key as the store minted it, or nothing.
            Err(_) if pinned.is_some() => return Found::One(pinned.unwrap_or_default()),
            Err(_) => return if self.at.contains_key(t) { Found::One(t.to_string()) } else { Found::None },
        };
        let mut keys: Vec<String> = Vec::new();
        for sc in self.shortcodes.matching(&a, reach) {
            let key = match &a.at {
                At::Shortcode => continue,
                At::Seq(n) => {
                    let k = super::OrderKey { shortcode: sc.clone(), seq: *n };
                    match self.order.get(&k) {
                        Some(row) => row.id.to_string(),
                        None => continue,
                    }
                }
                // TICKETS-26: the subtask of the ticket numbered so — its key is
                // that ticket's with the tail, in the same shortcode.
                At::Sub(n, tail) => {
                    let base = super::OrderKey { shortcode: sc.clone(), seq: *n };
                    let Some(base) = self.order.get(&base) else { continue };
                    let key = tail.iter().fold(base.id.to_string(), |k, t| format!("{k}.{t}"));
                    match self.get(&key) {
                        Some(row) if row.shortcode == sc => row.id.to_string(),
                        _ => continue,
                    }
                }
                At::Path(p) => match self.paths.get(&(sc.clone(), p.clone())) {
                    Some(id) => id.clone(),
                    None => continue,
                },
            };
            keys.push(key);
        }
        if a.org.is_none() && !pref.is_empty() {
            let mine: Vec<String> = keys.iter().filter(|k| in_pref(k)).cloned().collect();
            match (mine.is_empty(), &pinned) {
                (false, _) => keys = mine,
                (true, Some(p)) => return Found::One(p.clone()),
                (true, None) => {}
            }
        } else if let Some(p) = pinned {
            return Found::One(p);
        }
        // A shortcode the registry does not hold yet (a hand edit's): its
        // bare id is still its key, as before ORG-3.
        if keys.is_empty() && a.is_bare() && self.at.contains_key(&a.to_string()) {
            return Found::One(a.to_string());
        }
        match keys.len() {
            0 => Found::None,
            1 => Found::One(keys.pop().expect("one")),
            _ => Found::Many(keys.iter().filter_map(|k| self.get(k)).map(|r| r.address.clone()).collect()),
        }
    }

    /// ORG-3: a ticket named where an id may name one not born yet (a
    /// forward reference in `blocked_by`): the key it resolves to — or, when
    /// it names none, the key it will be born as, `{shortcode}-{n}` in the
    /// one shortcode it names; text that names no shortcode (`RT-0`) stays as
    /// written, as before. Ambiguous is refused: a forward reference has to
    /// say which namespace's ticket it waits for.
    pub(super) fn name_ticket(&self, text: &str) -> Result<TicketName, StoreError> {
        let t = text.trim();
        match self.find_ticket(t, OrgReach::All) {
            Found::One(key) => return Ok(TicketName::new(key)),
            Found::Many(candidates) => return Err(StoreError::Ambiguous { text: t.to_string(), candidates }),
            Found::None => {}
        }
        let Ok(a) = Address::parse(t) else { return Ok(TicketName::new(t)) };
        let At::Seq(n) = a.at else { return Ok(TicketName::new(t)) };
        if a.is_bare() {
            let plain = ShortcodeId::new(a.code.as_str());
            let pref = self.shortcodes.preferred_now();
            if self.shortcodes.entries.get(&plain).is_some_and(|e| e.pinned && (pref.is_empty() || pref.contains(&e.sc.ns.era))) {
                return Ok(TicketName::new(plain.key(n)));
            }
        }
        match self.shortcodes.find_shortcode(&Address { at: At::Shortcode, ..a.clone() }.to_string(), OrgReach::All) {
            Found::One(sc) => Ok(TicketName::new(sc.key(n))),
            Found::Many(candidates) => Err(StoreError::Ambiguous { text: t.to_string(), candidates }),
            Found::None => Ok(TicketName::new(a.to_string())),
        }
    }

    /// ORG-3: the shortcode `text` names — pinned, minted id, or address —
    /// or the refusal: ambiguous with every candidate's full address, or
    /// [`StoreError::NoShortcode`].
    pub(super) fn resolve_shortcode(&self, text: &str) -> Result<ShortcodeId, StoreError> {
        match self.shortcodes.find_shortcode(text, OrgReach::All) {
            Found::One(sc) => Ok(sc),
            Found::Many(candidates) => Err(StoreError::Ambiguous { text: text.trim().to_string(), candidates }),
            Found::None => Err(StoreError::NoShortcode(text.trim().to_string())),
        }
    }

    /// [`Cache::resolve_shortcode`], `None` for one that names none.
    pub(super) fn shortcode_named(&self, text: &str) -> Result<Option<ShortcodeId>, StoreError> {
        match self.resolve_shortcode(text) {
            Ok(sc) => Ok(Some(sc)),
            Err(StoreError::NoShortcode(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// ORG-3: the file's shortcodes made to cover every shortcode its tickets
/// and tombstones name — at every open, after the cache is loaded or built
/// and before it is handed out. A file from before ORG-3 gains the tables
/// here, once, durably, in one transaction: the organization `hypermemetic`,
/// a row per shortcode with the namespace its root's host gives it (the
/// roll-ups' settled host, read as that ticket's shortcode — today's 59 are
/// unique app-wide, so none is renamed), and a pin for every id and
/// tombstone it holds and every shortcode it names. After that, a shortcode
/// a hand edit introduced is registered the same way, unpinned. Every row
/// then takes its code and address. Returns how many shortcodes it added.
pub(super) fn settle(conn: &mut Connection, cache: &mut Cache) -> anyhow::Result<usize> {
    let tx = conn.transaction()?;
    tx.execute_batch(TABLES)?;
    let first = tx.query_row("SELECT count(*) FROM orgs", [], |r| r.get::<_, i64>(0))? == 0;
    if first {
        tx.execute("INSERT INTO orgs (id, name) VALUES (1, ?1)", [OrgId::HYPERMEMETIC])?;
    }
    // ORG-13: every organization its first epic, every shortcode in one.
    let placed = ensure_eras(&tx)?;
    if placed > 0 && !first {
        tracing::warn!("ORG-13: put {placed} shortcode(s) into their organization's first epic ({}), durably; nothing else moved", EpicName::GENESIS);
    }
    let mut r = load(&tx)?;
    let adopted = adopt(&mut r, cache, first)?;
    for sc in &adopted {
        put(&tx, &r, sc).map_err(|e| anyhow::anyhow!("shortcode {}: {e}", sc.as_str()))?;
    }
    let added = adopted.len();
    let org = r.first_org();
    if first {
        let mut keys: Vec<String> = cache.at.keys().cloned().collect();
        keys.extend(cache.tombstones.ids());
        let mut pinned: Vec<(String, String)> = keys.into_iter().map(|k| (k.clone(), k)).collect();
        // WAVE-10's path addresses too, as a reader writes them today.
        pinned.extend(cache.paths.iter().map(|((sc, path), id)| (format!("{}/{path}", sc.as_str()), id.clone())));
        for (text, key) in &pinned {
            tx.prepare_cached("INSERT OR IGNORE INTO pins (text, ticket) VALUES (?1, ?2)")?.execute([text, key])?;
            r.pins.insert(text.clone(), key.clone());
        }
    }
    tx.commit()?;
    r.readdress();
    if first {
        tracing::warn!(
            "ORG-3: the file gained its shortcodes — {added} in the organization {org}, each in the namespace its root's \
             host gives it, none renamed — and {} pins (every id and tombstone it held), durably; every later open reads them",
            r.pins.len()
        );
    } else if added > 0 {
        tracing::warn!("ORG-3: registered {added} shortcode(s) the file's tickets name and its table did not (a hand edit), durably");
    }
    cache.shortcodes = r;
    cache.readdress_all();
    Ok(added)
}

/// The shortcodes the cache's tickets and tombstones name that `r` does not
/// hold, registered in it — in the one organization, each in the namespace
/// its root's host gives it, `pinned` when they are today's (a file's first
/// open by ORG-3). Their code is their id's (an id the store did not mint is
/// its code). Returns them, in the store's order.
fn adopt(r: &mut Registry, cache: &Cache, pinned: bool) -> anyhow::Result<Vec<ShortcodeId>> {
    // ORG-4: the first organization, whatever it is called now.
    let org = r.first_org();
    let org_row = r.org_row(&org).ok_or_else(|| anyhow::anyhow!("no organization {org}"))?;
    // ORG-13: into the organization's first epic.
    let era = r.first_era(org_row).unwrap_or(0);
    let mut named: Vec<ShortcodeId> = cache.ids.last_seqs().map(|(sc, _)| sc.clone()).collect();
    named.extend(cache.tombstones.shortcodes());
    named.sort();
    named.dedup();
    let rows = cache.rows();
    let mut adopted = Vec::new();
    let fresh: Vec<ShortcodeId> = named.into_iter().filter(|sc| !r.entries.contains_key(sc)).collect();
    for sc in fresh {
        let host = derived_host(rows, &cache.rollups, &sc);
        let code = Code::stored(sc.as_str().split('.').next().unwrap_or(sc.as_str()));
        r.insert(Entry { sc: Shortcode { id: sc.clone(), ns: Namespace { org: org.clone(), era, host }, code }, org: org_row, pinned, address: String::new() });
        adopted.push(sc);
    }
    r.readdress();
    Ok(adopted)
}

impl Cache {
    /// ORG-3: a seed's shortcodes registered in the cache's registry
    /// ([`adopt`], unpinned) and every row given its address — the
    /// shortcodes to store, in the seed's last transaction.
    pub(super) fn adopt_shortcodes(&mut self) -> Vec<ShortcodeId> {
        let mut r = std::mem::take(&mut self.shortcodes);
        if r.orgs.is_empty() {
            r.orgs.insert(1, OrgId::hypermemetic());
        }
        // ORG-13: and its first epic, as [`ensure_eras`] will store it.
        if r.first_era(1).is_none() && r.eras.is_empty() {
            r.eras.insert(1, Era::new(1, EpicName::genesis(), 0));
        }
        let adopted = adopt(&mut r, self, false).unwrap_or_default();
        self.shortcodes = r;
        self.readdress_all();
        adopted
    }
}

/// ORG-3: `--verify-derived`'s five checks of the shortcodes — `None` when
/// the file has no table yet (the next open makes it):
///
/// 1. every ticket's shortcode is a row of the table;
/// 2. no two rows share `(org, ifnull(host, ''), code)` — the index, as a query;
/// 3. every shortcode's stored namespace is the one its root's host gives
///    it (the stored against the derived);
/// 4. every full address is distinct;
/// 5. every pin names a ticket or a tombstone.
pub(super) fn check(conn: &Connection, scanned: &Cache) -> rusqlite::Result<Option<(usize, Vec<String>)>> {
    let present: i64 =
        conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'shortcodes'", [], |r| r.get(0))?;
    if present == 0 {
        return Ok(None);
    }
    let r = load(conn)?;
    let mut out = Vec::new();
    for (sc, _) in scanned.ids.last_seqs() {
        if !r.entries.contains_key(sc) {
            out.push(format!("shortcode {}: its tickets name it; the shortcodes table holds no row of it", sc.as_str()));
        }
    }
    let mut stmt = conn.prepare(
        "SELECT org, ifnull(host, ''), code, count(*) FROM shortcodes GROUP BY org, ifnull(era, 0), ifnull(host, ''), code HAVING count(*) > 1",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let (org, host, code, n): (i64, String, String, i64) = (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
        out.push(format!("shortcodes: {n} rows hold {code} in org {org}, one epic, host '{host}' — a code is unique in its namespace"));
    }
    let rows = scanned.rows();
    for e in r.entries.values() {
        if scanned.rollups.root(&e.sc.id).is_none() {
            continue;
        }
        let want = derived_host(rows, &scanned.rollups, &e.sc.id);
        if e.sc.ns.host != want {
            let show = |h: &Option<ShortcodeId>| h.as_ref().map_or("none (a root)".to_string(), |h| h.as_str().to_string());
            out.push(format!(
                "shortcode {}: its stored host is {}; where its root hangs gives {}",
                e.sc.id.as_str(),
                show(&e.sc.ns.host),
                show(&want)
            ));
        }
    }
    let mut seen: HashMap<&str, &ShortcodeId> = HashMap::new();
    for e in r.entries.values() {
        if let Some(other) = seen.insert(e.address.as_str(), &e.sc.id) {
            out.push(format!("address {}: both {} and {} have it", e.address, other.as_str(), e.sc.id.as_str()));
        }
    }
    let tombstoned: Option<i64> = conn
        .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'tombstones'", [], |r| r.get(0))
        .optional()?;
    let mut lost = 0usize;
    for (text, key) in &r.pins {
        let dead = match tombstoned {
            Some(n) if n > 0 => conn
                .prepare_cached("SELECT count(*) FROM tombstones WHERE id = ?1")?
                .query_row([key], |row| row.get::<_, i64>(0))?
                > 0,
            _ => false,
        };
        if !scanned.at.contains_key(key) && !dead {
            lost += 1;
            if lost <= 20 {
                out.push(format!("pin {text}: names {key}, which is no ticket and no tombstone"));
            }
        }
    }
    if lost > 20 {
        out.push(format!("… and {} more pins naming nothing", lost - 20));
    }
    Ok(Some((r.entries.len(), out)))
}

/// ORG-3: a shortcode's id for each text a column holds, made once — a
/// start reads the same few ids on a row each.
#[derive(Default)]
pub(super) struct Ids(HashMap<String, ShortcodeId>);

impl Ids {
    pub(super) fn of(&mut self, text: &str) -> ShortcodeId {
        if let Some(id) = self.0.get(text) {
            return id.clone();
        }
        let id = ShortcodeId::new(text);
        self.0.insert(text.to_string(), id.clone());
        id
    }
}

/// A shortcode's id as a column holds it — its text, as before ORG-3.
impl rusqlite::ToSql for ShortcodeId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Borrowed(rusqlite::types::ValueRef::Text(self.as_str().as_bytes())))
    }
}

/// …and read back.
impl rusqlite::types::FromSql for ShortcodeId {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value.as_str().map(ShortcodeId::new)
    }
}

impl Cache {
    /// ORG-13: the epic a new root shortcode of `org` is born in — the one
    /// `asked` names (refused when `org` has none of that name), else the
    /// caller's current one, else the organization's first.
    pub(super) fn era_to_mint(&self, org: &OrgId, asked: &Address) -> Result<i64, StoreError> {
        let r = &self.shortcodes;
        let Some(row) = r.org_row(org) else { return Ok(0) };
        if let Some(name) = &asked.era {
            let era = r.era_named(row, name).ok_or_else(|| StoreError::Invalid(format!("no epic `{name}` in {org} (nothing was written)")))?;
            r.live_era(era, "making a ticket in it")?;
            return Ok(era);
        }
        let me = super::eras::reader_now();
        Ok(me.and_then(|p| r.current_era(&p, row)).or_else(|| r.first_era(row)).unwrap_or(0))
    }
}

/// ORG-3: where a `create` (or a note's upgrade) files its ticket — a
/// shortcode there is, or one the write mints with it.
#[derive(Debug, Clone)]
pub(super) enum Target {
    In(ShortcodeId),
    Mint(Shortcode),
}

impl Cache {
    /// ORG-3: the shortcode `asked` (the `epic` a create names, as an
    /// address) means for a ticket filed under `parent` — the namespace
    /// comes first:
    ///
    /// - under a parent, the parent's own shortcode when `asked` names it
    ///   (its code, and any hosts it names among the parent shortcode's);
    ///   else the shortcode of that code in the parent's shortcode's
    ///   namespace — refused, naming it, when it holds tickets already (a
    ///   ticket's parent is inside its own shortcode, and a second root of
    ///   that code there would be a clash) — else a new one minted there:
    ///   two THEMEs under two hosts are two shortcodes;
    /// - with no parent, the one shortcode `asked` resolves to in reach
    ///   (refused, with every candidate, when several), else a new root of
    ///   its org — refused when `asked` names hosts (a hosted shortcode hangs
    ///   under one of its host's tickets: name it as the parent).
    ///
    /// `roots`: whether the ticket is of a kind that roots a shortcode (a
    /// scope or a doc) — only such a ticket can be a second root there; any
    /// other files into the shortcode it names, and the parent rule says why
    /// its parent is outside it.
    ///
    /// ORG-23: refused when the shortcode is in an archived epic.
    pub(super) fn target(&self, asked: &Address, parent: Option<&super::Row>, roots: bool) -> Result<Target, StoreError> {
        let t = self.target_any(asked, parent, roots)?;
        let era = match &t {
            Target::In(sc) => self.shortcodes.era_of(sc),
            Target::Mint(sc) => sc.ns.era,
        };
        self.shortcodes.live_era(era, "making a ticket in it")?;
        Ok(t)
    }

    fn target_any(&self, asked: &Address, parent: Option<&super::Row>, roots: bool) -> Result<Target, StoreError> {
        let r = &self.shortcodes;
        // ORG-4: an org named by a name it had before a rename is that org.
        let asked_org = match &asked.org {
            Some(org) => match r.canonical(org) {
                Some(o) => Some(o.clone()),
                None => return Err(StoreError::Invalid(format!("no organization `{org}` (nothing was created)"))),
            },
            None => None,
        };
        let Some(p) = parent else {
            return match r.find_shortcode(&asked.to_string(), OrgReach::All) {
                Found::One(sc) => Ok(Target::In(sc)),
                Found::Many(candidates) => Err(StoreError::Ambiguous { text: asked.to_string(), candidates }),
                Found::None if !asked.hosts.is_empty() => Err(StoreError::Invalid(format!(
                    "no shortcode {asked}: a shortcode hosted under {} hangs under one of its tickets — name it as the parent \
                     (nothing was created)",
                    asked.hosts.last().expect("not empty")
                ))),
                Found::None => {
                    let org = asked_org.unwrap_or_else(|| r.first_org());
                    let era = self.era_to_mint(&org, asked)?;
                    Ok(Target::Mint(r.mint(Namespace { org, era, host: None }, asked.code.clone())?))
                }
            };
        };
        let psc = &p.shortcode;
        let org = r.org(psc).cloned().unwrap_or_else(|| r.first_org());
        if asked_org.as_ref().is_some_and(|o| *o != org) {
            return Err(StoreError::Invalid(format!(
                "{} is in the organization {org}, and {asked} names another (nothing was created)",
                p.id
            )));
        }
        if r.code(psc) == asked.code && (asked.hosts.is_empty() || r.matching(asked, OrgReach::All).contains(psc)) {
            return Ok(Target::In(psc.clone()));
        }
        // ORG-13: under a parent, in its epic — an address naming another is
        // refused.
        let era = r.era_of(psc);
        if let Some(name) = &asked.era {
            if r.org_row(&org).and_then(|o| r.era_named(o, name)) != Some(era) {
                return Err(StoreError::Invalid(format!("{} is not in the epic {name}: a ticket is filed in its parent's epic (nothing was created)", p.id)));
            }
        }
        let ns = Namespace { org, era, host: Some(psc.clone()) };
        if let Some(there) = r.in_namespace(&ns, &asked.code) {
            if roots && self.ids.held(there) > 0 {
                return Err(r.taken(there, &ns, &asked.code, None));
            }
            return Ok(Target::In(there.clone()));
        }
        // A ticket that roots nothing never starts a shortcode: it files into
        // the one its letters name (and the parent rule says why its parent
        // is outside it), as before ORG-3 — or, naming none, is refused as
        // every shortcode's first ticket of its kind is.
        if !roots {
            match r.find_shortcode(&asked.to_string(), OrgReach::All) {
                Found::One(sc) => return Ok(Target::In(sc)),
                Found::Many(candidates) => return Err(StoreError::Ambiguous { text: asked.to_string(), candidates }),
                Found::None => {}
            }
        }
        // The hosts the address names must be the parent shortcode's own.
        let chain: Vec<&Code> = r.chain(psc).into_iter().filter_map(|c| r.entries.get(c)).map(|e| &e.sc.code).collect();
        let mut at = 0;
        let fits = asked.hosts.iter().all(|h| match chain[at..].iter().position(|c| *c == h) {
            Some(i) => {
                at += i + 1;
                true
            }
            None => false,
        });
        if !fits {
            return Err(StoreError::Invalid(format!(
                "{asked} is not under {}: a new shortcode hangs where its parent {} is (nothing was created)",
                r.address(psc),
                p.id
            )));
        }
        Ok(Target::Mint(r.mint(ns, asked.code.clone())?))
    }
}
