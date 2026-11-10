//! COMP-16: **what a body's frame may read** — the reader's reach ∩ the
//! reach of whoever wrote the code that runs there ∩ what the body asks.
//! The owner, 2026-10-13: *"we should be able to give components wider
//! access obviously bounded by reader access"*; the decision, taken on
//! COMP-16: reader ∩ every author ∩ what the body asks, so no one can use a
//! body to read what they could not read already (FRAMES-5's gap: a body's
//! script runs with its reader's eyes, and can carry what it reads out).
//!
//! - **The author** ([`FrameOf::author`]) is who last wrote the body — the
//!   actor of its latest revision that created the ticket or changed its
//!   body (`store/revisions.rs` keeps one for every create, amend, update
//!   and restore). Nothing else is kept: the revisions are the record. A
//!   body whose latest such revision names nobody — `system`, `anonymous`,
//!   a `baseline` or a `backup` — has no author known, and its frame reads
//!   **the ticket alone** ([`FrameOf::alone`] says why).
//! - **What it asks** ([`ReachAsked`]) is its frontmatter's `reach-read:` in
//!   FILTER-3's language (`under PLEXUS-1 or under IDENTITY`); `all`, as
//!   wide as allowed; none, its own shortcode and those hosted under it
//!   (IDENTITY-36's bound); a text the language does not take (or one that
//!   asks `text`), the ticket alone.
//! - **The bound** ([`Framed`]) is a [`Reader`] whose reach is the reader's
//!   ANDed with the author's (by the author's grants alone, as
//!   `tickets.get` is held: [`Store::frame`]'s `author_reach`) and with what
//!   the body asks — so every store read made through it answers only what
//!   all three admit, and a ticket outside is not found, as everywhere. It
//!   is never wider than the reader: the reader's reach is one of the
//!   parts. An author who holds nothing reads nothing.
//!
//! It is read afresh at each call: an amend by another author moves the
//! bound with its revision, and nothing is cached.

use super::*;
use crate::filter::{Field, Filter, Group, Predicate};
use crate::revisions::RevisionSource;
use crate::types::{ANONYMOUS_ACTOR, SYSTEM_ACTOR};
use rusqlite::OptionalExtension;

/// The frontmatter key a body asks its frame's reach with.
pub const REACH_READ: &str = "reach-read";

/// What a body asks its frame to read ([`REACH_READ`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachAsked {
    /// Nothing asked: its own shortcode, with those hosted under it.
    Own,
    /// `all`: as wide as the reader and the author allow.
    All,
    /// A filter, as written and as read.
    Filter(String, Filter),
    /// A text the language does not take, as written, and why: the ticket
    /// alone.
    Unreadable(String, String),
}

/// A body's frame, as the store finds it for a reader: the ticket (its key
/// and shortcode), who last wrote its body, and what it asks.
#[derive(Debug, Clone)]
pub struct FrameOf {
    pub ticket: TicketId,
    pub shortcode: ShortcodeId,
    /// The principal who last wrote the body; `None`: nobody known.
    pub author: Option<String>,
    pub asked: ReachAsked,
}

impl FrameOf {
    /// What its `reach-read:` says, or the default it reads under, spelled
    /// in the language.
    pub fn asked_text(&self) -> String {
        match &self.asked {
            ReachAsked::Own => format!("under {}", self.shortcode.as_str()),
            ReachAsked::All => "all".into(),
            ReachAsked::Filter(text, _) | ReachAsked::Unreadable(text, _) => text.clone(),
        }
    }

    /// Why its frame reads the ticket alone — no author known, or an ask
    /// the language does not take — or `None`.
    pub fn alone(&self) -> Option<String> {
        if self.author.is_none() {
            return Some(format!("nobody known last wrote {}'s body, so its frame reads the ticket alone", self.ticket));
        }
        match &self.asked {
            ReachAsked::Unreadable(text, why) => Some(format!("{REACH_READ}: `{text}` is not read ({why}), so its frame reads the ticket alone")),
            _ => None,
        }
    }

    /// The parts the bound ANDs onto the reader's reach — what the body
    /// asks, and the ticket alone when [`FrameOf::alone`] says so. The
    /// author's reach is [`Store::frame`]'s.
    fn narrowing(&self) -> Vec<Filter> {
        let one = |field: Field| Filter { groups: vec![Group::any(vec![Predicate::is(field)])], display: Default::default() };
        let mut out = Vec::new();
        if self.alone().is_some() {
            out.push(one(Field::Ticket(crate::address::folded(self.ticket.as_str()))));
        }
        match &self.asked {
            ReachAsked::Own => out.push(one(Field::Under(self.shortcode.as_str().to_string()))),
            ReachAsked::Filter(_, f) => out.push(f.clone()),
            ReachAsked::All | ReachAsked::Unreadable(..) => {}
        }
        out
    }
}

/// A reader narrowed to a frame's bound: `reach` is the reader's ANDed with
/// every part ([`Framed::over`]). Owned — what a read's stream holds. A
/// [`Reader`] (sealed): built only here, and only ever narrower than the
/// reader it was built over.
#[derive(Debug, Clone)]
pub struct Framed {
    principal: Option<String>,
    reach: Result<Option<Filter>, String>,
}

impl Framed {
    /// `reader` narrowed by every one of `parts` — a part that reaches
    /// nothing (`Err`: an author holding no read anywhere) makes the bound
    /// read nothing; the reader's own refusal stays its refusal.
    pub fn over(reader: &(impl Reader + ?Sized), parts: Vec<Result<Option<Filter>, String>>) -> Self {
        let principal = reader.principal_of();
        let mut reach = match reader.reach() {
            Ok(r) => r,
            Err(e) => return Self { principal, reach: Err(e.to_string()) },
        };
        for part in parts {
            let part = match part {
                Ok(None) => continue,
                Ok(Some(f)) => f,
                Err(_) => nothing(),
            };
            reach = Some(match reach {
                None => part,
                Some(r) => r.and(&part),
            });
        }
        Self { principal, reach: Ok(reach) }
    }

    /// The reader unnarrowed — a read that names no frame.
    pub fn plain(reader: &(impl Reader + ?Sized)) -> Self {
        Self::over(reader, Vec::new())
    }
}

/// A filter no ticket passes: the ticket named by no key (`ticket ~`).
fn nothing() -> Filter {
    Filter { groups: vec![Group::any(vec![Predicate::is(Field::Ticket("~".into()))])], display: Default::default() }
}

impl Reader for Framed {
    fn reach(&self) -> Result<Option<Filter>, StoreError> {
        self.reach.clone().map_err(StoreError::Filter)
    }

    fn principal_of(&self) -> Option<String> {
        self.principal.clone()
    }
}

/// `reach-read:`'s value as written: a trailing ` # comment` dropped, and
/// one pair of quotes around it.
fn asked_value(body: &str) -> Option<String> {
    let (_, v) = crate::diff::frontmatter(body).into_iter().find(|(k, _)| k == REACH_READ)?;
    let v = match v.find(" #") {
        Some(at) if v[..at].matches('"').count() % 2 == 0 => &v[..at],
        _ => v.as_str(),
    };
    let v = v.trim();
    let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\''))).unwrap_or(v);
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

impl Store {
    /// What `body` asks its frame to read: its saved names resolved, held to
    /// what a reach can be ([`Lens::reach`]: no `text`).
    fn asked_of(&self, body: &str) -> ReachAsked {
        let Some(text) = asked_value(body) else { return ReachAsked::Own };
        if text.eq_ignore_ascii_case("all") {
            return ReachAsked::All;
        }
        let read = Filter::parse(&text).map_err(|e| e.to_string()).and_then(|f| self.resolve_saved(&f).map_err(|e| e.to_string())).and_then(|f| match Lens::reach(&f) {
            Ok(_) => Ok(f),
            Err(e) => Err(e.to_string()),
        });
        match read {
            Ok(f) => ReachAsked::Filter(text, f),
            Err(why) => ReachAsked::Unreadable(text, why),
        }
    }

    /// COMP-16: who last wrote `key`'s body — its latest revision that
    /// created it or changed its body; `None` when that one names nobody (a
    /// `baseline`, a `backup`, `system`, `anonymous`) or there is none.
    async fn body_author(&self, key: String) -> Result<Option<String>, StoreError> {
        let row: Option<(String, String)> = self
            .readers
            .read(move |conn| {
                Ok(conn
                    .prepare_cached(
                        "SELECT actor, source FROM revisions WHERE ticket = ?1 AND (fields LIKE '%\"body\"%' OR fields LIKE '%\"created\"%' OR source = 'baseline')
                          ORDER BY rev DESC LIMIT 1",
                    )?
                    .query_row([&key], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?)
            })
            .await?;
        Ok(row.and_then(|(actor, source)| {
            let nobody = matches!(RevisionSource::parse(&source), RevisionSource::Baseline | RevisionSource::Backup) || actor == SYSTEM_ACTOR || actor == ANONYMOUS_ACTOR || actor.trim().is_empty();
            (!nobody).then_some(actor)
        }))
    }

    /// COMP-16: the frame of `id`'s body as `who` would draw it — its key,
    /// who last wrote it, what it asks ([`FrameOf`]). [`StoreError::NotFound`]
    /// for a ticket out of `who`'s reach, as `get` answers it.
    pub async fn frame_of(&self, who: &(impl Reader + ?Sized), id: &str) -> Result<FrameOf, StoreError> {
        let ticket = self.get(who, id).await?;
        let author = self.body_author(ticket.id.to_string()).await?;
        let asked = self.asked_of(&ticket.body);
        Ok(FrameOf { ticket: ticket.id, shortcode: ticket.shortcode, author, asked })
    }

    /// COMP-16: the bound a frame of `id`'s body reads under, for `who` —
    /// [`Store::frame_of`], then a [`Framed`] reader: `who`'s reach ∩ the
    /// author's (`author_reach` of the author's principal, a reader of that
    /// principal's grants; skipped when no author is known, the ticket alone
    /// standing for it) ∩ what the body asks.
    pub async fn frame<A: Reader>(&self, who: &(impl Reader + ?Sized), id: &str, author_reach: impl FnOnce(&str) -> A) -> Result<(FrameOf, Framed), StoreError> {
        let frame = self.frame_of(who, id).await?;
        let mut parts: Vec<Result<Option<Filter>, String>> = frame.narrowing().into_iter().map(|f| Ok(Some(f))).collect();
        if let Some(author) = &frame.author {
            parts.push(author_reach(author).reach().map_err(|e| e.to_string()));
        }
        let framed = Framed::over(who, parts);
        Ok((frame, framed))
    }

    /// COMP-16: what `who` reads, by shortcode — each shortcode it reads a
    /// live ticket of and how many, in the store's order. A reach that
    /// reads nothing answers none.
    pub fn extent(&self, who: &(impl Reader + ?Sized)) -> Result<Vec<(ShortcodeId, u64)>, StoreError> {
        let seen = Seen::of(who)?.viewing(&self.tickets.read().expect("store lock poisoned"));
        let lens = seen.view_lens()?;
        Ok(self.counts_unscoped(&lens).into_iter().filter(|(_, n)| *n > 0).collect())
    }
}
