//! COMMENT-2: the **comments store** — every comment a reader has left on
//! the app (a page, an element on it, a ticket's body), kept beside the
//! tickets as records of their own (COMMENT-1, decided 2026-10-08), never in
//! a body and never as a ticket.
//!
//! **Behind the cache, as everything stored is** (AGENTS.md: the service-wide
//! cache is the only interface the application has to stored data): every
//! comment is read into the cache when the store opens ([`load`]) and every
//! read answers from it — `list` is a walk of the cache's comments, newest
//! first, under the read lock, with no I/O. A write — `add`, `resolve`,
//! `reopen` — takes the write path every write takes ([`super::Store::write`],
//! MNG-20): worked out under the write-serial lock from the cache's record,
//! committed to the `comments` table in one transaction, then put into the
//! cache, then broadcast on the comments' own channel (`watch`). A change
//! that changes nothing — resolving what is resolved — writes and says
//! nothing.
//!
//! **A comment keeps its anchor.** Nothing a ticket's write does reaches
//! this table: an amend, a move (`set_parent`), a kind or a status change
//! leaves every comment as it was — its anchor, `version` included, is what
//! the reader saw when they left it. What follows the ticket is `list
//! { under }`: the ticket's lineage is read as the tree stands at the read
//! ([`under`]).
//!
//! **Stored** in one table, `comments` — a row per comment: its id, state,
//! author, time, the anchor's six fields and the body. A file in stored form
//! 6 gains it, empty, once, on its first open (form 7, [`super::stored`]); a
//! file built from older forms, or new, gains it with the rest.

use super::{lineage, shortcodes, tree, Cache, Needle, StoreError};
use crate::comments::{Anchor, Comment, CommentIntent, CommentState};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap};

/// The table. `INTEGER PRIMARY KEY`: a comment's id is its rowid, allocated
/// by the store (the newest id + 1) and never reused — nothing deletes one.
pub(super) const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS comments (
            id      INTEGER PRIMARY KEY,
            state   TEXT NOT NULL,
            author  TEXT NOT NULL,
            at      INTEGER NOT NULL,
            route   TEXT NOT NULL,
            path    TEXT NOT NULL,
            label   TEXT NOT NULL,
            excerpt TEXT NOT NULL,
            ticket  TEXT,
            version INTEGER,
            body    TEXT NOT NULL,
            principal TEXT,
            intent  TEXT
        );
    ";

/// APP-23: a file whose comments table has no `intent` gains it here, in
/// place — NULL is a comment, `changes` a change request; on every open a
/// no-op once it is there. Nothing derived moves and the form stays: an
/// older build names the columns it reads and writes, and never sees this
/// one (a change request reads there as a comment).
pub(crate) fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let table: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'comments'", [], |r| r.get(0))?;
    if table > 0 && !super::db::has_column(conn, "comments", "intent")? {
        conn.execute("ALTER TABLE comments ADD COLUMN intent TEXT", [])?;
        tracing::warn!("APP-23: the comments table gained its intent column, durably");
    }
    Ok(())
}

/// A change to the comments, as `watch` hears it — the comment as the
/// change left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentChange {
    Added(Comment),
    Resolved(Comment),
    Reopened(Comment),
}

/// What `list` asks: every filter given must hold. Blank is not given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentQuery {
    /// The comment's ticket is this one (whatever the case).
    pub ticket: Option<String>,
    /// The comment's ticket is in this one's lineage — a ticket or a
    /// shortcode, as the filter language's `under` ([`under`]).
    pub under: Option<String>,
    /// The anchor's route is exactly this.
    pub route: Option<String>,
    /// The anchor's path begins with this.
    pub path: Option<String>,
    pub state: Option<CommentState>,
    /// The author is exactly this.
    pub author: Option<String>,
    /// In the body, the label or the excerpt — `find`'s match: a substring,
    /// whatever its ASCII case.
    pub text: Option<String>,
    /// APP-23: a comment or a change request.
    pub intent: Option<CommentIntent>,
}

/// One page of `list`: the comments, newest first, and the cursor to pass
/// as the next page's `after` — `None` when nothing more matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentPage {
    pub comments: Vec<Comment>,
    pub next: Option<u64>,
}

/// The store's comments, as the cache holds them: by id, so newest first is
/// the map backwards and a cursor is a range.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Comments {
    by_id: BTreeMap<u64, Comment>,
}

impl Comments {
    pub(crate) fn get(&self, id: u64) -> Option<&Comment> {
        self.by_id.get(&id)
    }

    /// The newest comment's id, if there is one.
    pub(crate) fn newest(&self) -> Option<u64> {
        self.by_id.keys().next_back().copied()
    }

    /// The id the next comment is born with.
    pub(crate) fn next_id(&self) -> u64 {
        self.newest().map_or(1, |n| n + 1)
    }

    /// FILTER-30: each comment's ticket and when it was left — a touch of
    /// that ticket (a resolve or a reopen is not one: it keeps no time).
    pub(crate) fn activity(&self) -> impl Iterator<Item = (&str, i64)> + '_ {
        self.by_id.values().filter_map(|c| Some((c.anchor.ticket.as_deref()?, c.at)))
    }

    /// APP-24: the comments left at or after `at`, oldest first.
    pub(crate) fn since(&self, at: i64) -> impl Iterator<Item = &Comment> + '_ {
        self.by_id.values().filter(move |c| c.at >= at)
    }

    /// The cache's half of a committed write: `comment` as it is now.
    pub(crate) fn put(&mut self, comment: Comment) {
        self.by_id.insert(comment.id.get(), comment);
    }
}

/// Text that is blank is no filter.
fn given(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Whether `name` is in the lineage of the ticket `id` — its own id, its
/// ancestors' ids, its shortcode and the shortcodes hosting it, compared
/// whatever the case: what the filter language's `under` reads
/// ([`crate::filter::Facts::lineage`]), from the same two walks its
/// reference judgment takes (`lineage::Compiled::judge_row`). A ticket
/// the cache does not hold has no lineage but its id.
///
/// ORG-3: `name` an identity — a ticket's key or a shortcode's id, the
/// reader's text resolved first ([`under_identity`]).
pub(super) fn under(cache: &Cache, id: &str, name: &str) -> bool {
    if id.eq_ignore_ascii_case(name) {
        return true;
    }
    let Some(row) = cache.get(id) else { return false };
    tree::ancestors(cache.rows(), &row.id, row.parent.as_deref())
        .into_iter()
        .any(|a| a.eq_ignore_ascii_case(name))
        || row.shortcode.as_str() == name
        || lineage::host_chain(cache, &row.shortcode).iter().any(|h| h.as_str() == name)
}

/// ORG-3: what a `comments list { under }` names, as an identity — a
/// ticket by any short form (a number or a path), else a shortcode by its
/// letters or address; one naming several is refused, naming each; one
/// naming nothing stays as written (a ticket a hand edit names, or none).
fn under_identity(cache: &Cache, name: &str) -> Result<String, StoreError> {
    let names_ticket = crate::address::Address::parse(name).map_or(true, |a| a.names_ticket());
    let found = match names_ticket {
        true => cache.find_ticket(name, shortcodes::OrgReach::All),
        false => match cache.shortcodes.find_shortcode(name, shortcodes::OrgReach::All) {
            shortcodes::Found::One(sc) => shortcodes::Found::One(sc.as_str().to_string()),
            shortcodes::Found::Many(c) => shortcodes::Found::Many(c),
            shortcodes::Found::None => shortcodes::Found::None,
        },
    };
    match found {
        shortcodes::Found::One(id) => Ok(id),
        shortcodes::Found::Many(candidates) => Err(StoreError::Ambiguous { text: name.trim().to_string(), candidates }),
        shortcodes::Found::None => Ok(name.trim().to_ascii_uppercase()),
    }
}

/// One page of the comments `q` admits, newest first: strictly older than
/// `after` (a comment id) when given, at most `limit` of them. An `after`
/// naming no comment is [`StoreError::CommentCursor`]: there is no position
/// to resume from. `next` is the page's last id when more match past it —
/// for `limit: 0`, the cursor it was cut with — else `None`. Under the
/// read lock, from the cache alone.
pub(super) fn page(cache: &Cache, q: &CommentQuery, after: Option<u64>, limit: Option<u32>) -> Result<CommentPage, StoreError> {
    let comments = &cache.comments;
    if let Some(a) = after.filter(|a| comments.get(*a).is_none()) {
        return Err(StoreError::CommentCursor(a));
    }
    let ticket = given(&q.ticket);
    let under_id = given(&q.under).map(|u| under_identity(cache, u)).transpose()?;
    let under_name = under_id.as_deref();
    let (route, path, author) = (given(&q.route), given(&q.path), given(&q.author));
    let needle = q.text.as_deref().and_then(Needle::of);
    // `under`, judged once per ticket a comment names.
    let mut lineage_holds: HashMap<String, bool> = HashMap::new();
    let mut admits = |c: &Comment| -> bool {
        if q.state.is_some_and(|s| c.state != s)
            || q.intent.is_some_and(|i| c.intent != i)
            || route.is_some_and(|r| c.anchor.route != r)
            || path.is_some_and(|p| !c.anchor.path.starts_with(p))
            || author.is_some_and(|a| c.author != a)
        {
            return false;
        }
        if let Some(t) = ticket {
            if !c.anchor.ticket.as_deref().is_some_and(|ct| ct.eq_ignore_ascii_case(t)) {
                return false;
            }
        }
        if let Some(n) = &needle {
            if !(n.within(&c.body) || n.within(&c.anchor.label) || n.within(&c.anchor.excerpt)) {
                return false;
            }
        }
        if let Some(u) = under_name {
            let Some(ct) = c.anchor.ticket.as_deref() else { return false };
            let holds = match lineage_holds.get(ct) {
                Some(&holds) => holds,
                None => *lineage_holds.entry(ct.to_string()).or_insert(under(cache, ct, u)),
            };
            if !holds {
                return false;
            }
        }
        true
    };
    let newest_first: Box<dyn Iterator<Item = &Comment>> = match after {
        Some(a) => Box::new(comments.by_id.range(..a).rev().map(|(_, c)| c)),
        None => Box::new(comments.by_id.values().rev()),
    };
    let take = limit.map_or(usize::MAX, |n| n as usize);
    let mut matches = newest_first.filter(|c| admits(c));
    let page: Vec<Comment> = matches.by_ref().take(take).cloned().collect();
    let more = limit.is_some() && matches.next().is_some();
    let next = more.then(|| page.last().map(|c| c.id.get()).or(after)).flatten();
    Ok(CommentPage { comments: page, next })
}

/// Writes `comment` whole — a new one, or one whose state moved — inside the
/// caller's transaction.
pub(super) fn store(conn: &Connection, c: &Comment) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO comments (id, state, author, at, route, path, label, excerpt, ticket, version, body, principal, intent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT (id) DO UPDATE SET state = excluded.state",
    )?
    .execute(params![
        c.id.get() as i64,
        c.state.as_str(),
        c.author,
        c.at,
        c.anchor.route,
        c.anchor.path,
        c.anchor.label,
        c.anchor.excerpt,
        c.anchor.ticket,
        c.anchor.version,
        c.body,
        c.principal,
        (!c.intent.is_comment()).then_some(c.intent.as_str()),
    ])?;
    Ok(())
}

/// Every comment as stored — what a start reads.
pub(super) fn load(conn: &Connection) -> rusqlite::Result<Comments> {
    let mut stmt = conn.prepare(
        "SELECT id, state, author, at, route, path, label, excerpt, ticket, version, body, principal, intent FROM comments ORDER BY id",
    )?;
    let mut rows = stmt.query([])?;
    let mut comments = Comments::default();
    while let Some(r) = rows.next()? {
        let state = r.get_ref(1)?.as_str()?.parse::<CommentState>().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, e.into())
        })?;
        comments.put(Comment {
            id: crate::vocab::CommentId::new(r.get::<_, i64>(0)? as u64),
            state,
            author: r.get(2)?,
            at: r.get(3)?,
            anchor: Anchor {
                route: r.get(4)?,
                path: r.get(5)?,
                label: r.get(6)?,
                excerpt: r.get(7)?,
                ticket: r.get(8)?,
                version: r.get(9)?,
            },
            body: r.get(10)?,
            principal: r.get(11)?,
            intent: match r.get_ref(12)?.as_str_or_null()? {
                None => CommentIntent::Comment,
                Some(i) => i.parse().map_err(|e: String| rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, e.into()))?,
            },
        });
    }
    Ok(comments)
}

/// How many comments the file holds — for a log line.
#[cfg(test)]
pub(super) fn count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT count(*) FROM comments", [], |r| r.get(0))
}
