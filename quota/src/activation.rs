//! The `quota` activation — **the one declaration**: the `Activation` impl,
//! dispatch, the schema and the methods' bodies from this `impl`, and
//! `QuotaClient<T>` for a client of it (the seam's [`crate::QuotaSeam`], a
//! test).
//!
//! **Who may.** The seam's four (`reserve`, `commit`, `release`, `credit`)
//! are in no role, and (KEEL-56) each refuses in its body a caller who does
//! not hold it on `*`: only an admin of `*` — the server's own caller, which
//! every service asks as. `usage` and (V2-70) `history` are
//! everyone's (their own; an admin of `*` names anyone, or everyone).
//! `table`, `set` and `recount` are an admin's of `*`.

use crate::caller::{Caller, Callers, QuotaRequest};
use crate::disk::DiskUsage;
use crate::ledger::Ledger;
use crate::seam::Seam;
use crate::vocab::{CapText, HistoryDays, HoldTicket, OrgRef, PrincipalName, QuotaEvent, SortKey, SourceName, TeamRef};
use futures::Stream;
use std::sync::Arc;

/// The activation: the ledger over `quota.db`, the disk it guards, and the
/// seam to the services it reads.
#[derive(Clone)]
pub struct QuotaActivation {
    pub(crate) ledger: Arc<Ledger>,
    pub(crate) disk: Arc<dyn DiskUsage>,
    pub(crate) seam: Seam,
    callers: Callers,
}

impl QuotaActivation {
    /// Over `ledger`, guarding `disk`.
    pub fn new(ledger: Arc<Ledger>, disk: Arc<dyn DiskUsage>) -> Self {
        Self { ledger, disk, seam: Seam::default(), callers: Callers }
    }

    /// The seam the builder attaches the running service to.
    pub fn seam(&self) -> &Seam {
        &self.seam
    }

    pub fn ledger(&self) -> &Arc<Ledger> {
        &self.ledger
    }
}

/// Allowances per principal and source — every write reserved against the cap; usage counted and tabulated.
#[plexus_rpc::macros::activation(
    auth = "required",
    request = QuotaRequest,
    typed_params,
    roles(
        viewer = ["usage", "history"],
        editor = [viewer],
    )
)]
impl QuotaActivation {
    /// Reserve an amount of a source for a principal before a write (a service's call); yields reserved { ticket, remaining, expires } or over_allowance { source, used, cap, asked }.
    ///
    /// The seam's first step: a service about to store or fetch for
    /// `principal` holds `amount` of `source` against their cap. One
    /// transaction: used + reserved + `amount` within the cap, else
    /// `over_allowance { source, used, cap, asked }`; `too_many_at_once`
    /// past the source's limit on reservations in flight (uploads: 2);
    /// `box_full` once the data disk is past the plan's full line — each
    /// with nothing held. Admins of `*` and the operator are never capped.
    /// The hold expires (the plan's hold time, an hour) unless committed or
    /// released, and is swept. Admin of `*` (the server's own caller).
    #[plexus_rpc::macros::method(group = "write", example(note = "an upload of 5 MB", params = r#"{"principal": "key:4b1c", "source": "media.bytes", "amount": 5000000}"#))]
    pub async fn reserve(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Whose allowance: the principal the write is for.
        principal: PrincipalName,
        /// The source, e.g. media.bytes.
        source: SourceName,
        /// How much, in the source's unit.
        amount: u64,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        futures::stream::once(crate::streams::reserve(self.clone(), user, principal.into_string(), source.into_string(), amount))
    }

    /// Count a reservation: what was actually written (at most what was reserved; none: all of it); yields committed { ticket, source, amount, used }.
    #[plexus_rpc::macros::method(group = "write")]
    pub async fn commit(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The reservation, as reserve answered it.
        ticket: HoldTicket,
        /// The actual amount, at most what was reserved (optional: all of it).
        amount: Option<u64>,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        futures::stream::once(crate::streams::commit(self.clone(), user, ticket.into_string(), amount))
    }

    /// Let a reservation go: the write did not land, nothing counts; yields released.
    #[plexus_rpc::macros::method(group = "write")]
    pub async fn release(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The reservation, as reserve answered it.
        ticket: HoldTicket,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        futures::stream::once(crate::streams::release(self.clone(), user, ticket.into_string()))
    }

    /// Give an amount back to a principal (a deletion); yields credited { principal, source, used }.
    #[plexus_rpc::macros::method(group = "write")]
    pub async fn credit(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Whose.
        principal: PrincipalName,
        /// The source, e.g. media.bytes.
        source: SourceName,
        /// How much comes back.
        amount: u64,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        futures::stream::once(crate::streams::credit(self.clone(), user, principal.into_string(), source.into_string(), amount))
    }

    /// Your usage per source — used, in flight, the cap and where it came from; an admin of * names anyone (principal).
    ///
    /// Yields one `usage { principal, exempt, bytes, sources }`: every
    /// source with its unit, window (a day's counter starts at nothing each
    /// UTC day), used, held (reserved, not yet committed), cap (none:
    /// unlimited) and `from` — own, team:<name>, default, or exempt.
    /// `bytes` is what the principal stores: its byte sources counted for
    /// good. Anyone signed in, for themselves.
    #[plexus_rpc::macros::method(group = "read", example(note = "mine", params = r#"{}"#))]
    pub async fn usage(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Whose (optional: yours; anyone else's is an admin of *'s).
        principal: Option<PrincipalName>,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        futures::stream::once(crate::streams::usage(self.clone(), user, principal.map(PrincipalName::into_string)))
    }

    /// Your consumption over time: a point per source per UTC day, oldest first, then end; an admin of * names anyone, or everyone (the system's sum per day).
    ///
    /// V2-70: one `point { day, source, used, estimate }` per source (or
    /// only `source`) per day of the last `days` (90 unless said; at most
    /// 730) to today — a total source's amount at the day's end (today's:
    /// now), carried over days nothing moved; a day's counter (fetches,
    /// served) its day's total. `estimate` marks a day before the ledger
    /// counted it, backfilled by the recount from the stores (media's files
    /// by when each was kept, documents' changes, tickets' revisions: what
    /// they keep now, laid back along when it was written). Then `end {
    /// principal, everyone, from, to, days }`. Anyone signed in, for
    /// themselves.
    #[plexus_rpc::macros::method(group = "read", example(note = "mine, the last 30 days", params = r#"{"days": 30}"#), example(note = "the system's stored bytes, a year (admin of *)", params = r#"{"everyone": true, "source": "media.bytes", "days": 365}"#))]
    pub async fn history(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Whose (optional: yours; anyone else's is an admin of *'s).
        principal: Option<PrincipalName>,
        /// One source, e.g. media.bytes (optional: every source).
        source: Option<SourceName>,
        /// How many days to today, 1 to 730 (optional: 90).
        days: Option<HistoryDays>,
        /// true: the system's — every principal's, summed per day (an admin of *'s).
        everyone: Option<bool>,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        let whose = (principal.map(PrincipalName::into_string), everyone.unwrap_or(false));
        crate::streams::history(self.clone(), user, whose, source.map(SourceName::into_string), days.map_or(90, HistoryDays::get))
    }

    /// Everyone's usage and caps, the tabulation (admin of *): box, then one usage per principal, sorted, then total.
    ///
    /// First `box { used_percent, warn_percent, full_percent, state }` —
    /// the data disk against the plan's lines (`warn` from 70 %, `full`
    /// from 85 %: writes by anyone capped are refused `box_full`). Then one
    /// `usage` per principal the ledger knows (or, with `org`, every
    /// principal granted a role in it), sorted by `sort`: bytes stored,
    /// most first (the default), principal, or a source's use, most first.
    /// Then `total { principals, bytes, sources }`.
    #[plexus_rpc::macros::method(group = "read", example(note = "everyone, the most stored first", params = r#"{"sort": "bytes"}"#))]
    pub async fn table(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Only principals granted a role in this organization (optional).
        org: Option<OrgRef>,
        /// bytes (default) | principal | a source's name.
        sort: Option<SortKey>,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        crate::streams::table(self.clone(), user, org.map(OrgRef::into_string), sort.map(SortKey::into_string))
    }

    /// Set an allowance (admin of *): a principal's, a team's, or the default, on one source; cap unlimited, a number, 2 GB, or clear.
    ///
    /// Exactly one of `principal`, `team`, `default: true`. A principal's
    /// own allowance wins; else their teams' largest; else the default.
    /// `clear` removes it (it falls back; a default cleared is unlimited).
    /// A cap below what is used blocks new writes of the source and
    /// deletes nothing. Yields `allowance { holder, source, cap, cleared }`.
    #[plexus_rpc::macros::method(
        group = "admin",
        example(note = "raise one person's media", params = r#"{"principal": "idp:c952", "source": "media.bytes", "cap": "20 GB"}"#),
        example(note = "the default for docs", params = r#"{"default": true, "source": "docs.bytes", "cap": "200 MB"}"#)
    )]
    pub async fn set(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// A principal's allowance (or team, or default).
        principal: Option<PrincipalName>,
        /// A team's allowance: its members' cap, unless their own is set.
        team: Option<TeamRef>,
        /// true: the default for the source.
        default: Option<bool>,
        /// The source, e.g. media.bytes.
        source: SourceName,
        /// unlimited | a number | a number with a unit (2 GB) | clear.
        cap: CapText,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        let who = (principal.map(PrincipalName::into_string), team.map(TeamRef::into_string), default.unwrap_or(false));
        futures::stream::once(crate::streams::set(self.clone(), user, who, source.into_string(), cap))
    }

    /// Rebuild usage from the stores (admin of *): each service's own usage method, by uploader or author; yields drift per change, then recounted.
    ///
    /// Asks, through the seam, `tickets.revisions.usage` (bodies and
    /// revisions by author: tickets.count, tickets.bytes), `media.usage`
    /// (blobs by uploader: media.bytes) and `docs.usage` (docs.count,
    /// docs.bytes), and sets each principal's total of each source the store
    /// answered to what it counts: one `drift { principal, source, was,
    /// counted }` per one that moved, then `recounted { principals,
    /// drifted, sources, not_counted }` — a store that has no such method
    /// yet is named in `not_counted`, with why. Daily sources are not
    /// recounted. The server runs one every night. V2-70: and backfills the
    /// history — each store asked by day (`daily: true`), the days before a
    /// principal's first counted point kept as estimates (`backfilled`: how many).
    #[plexus_rpc::macros::method(group = "admin")]
    pub async fn recount(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Only this principal (optional: everyone).
        principal: Option<PrincipalName>,
    ) -> impl Stream<Item = QuotaEvent> + Send + 'static {
        crate::streams::recount(self.clone(), Some(user), principal.map(PrincipalName::into_string))
    }
}
