//! ACCESS-17: `tickets work` — the order of work (WAVE-4, MENGER-31):
//! `ready` (was `tickets ready_set`), `waves`, `waves_under`, `waves_to`
//! and `blockers`, a `#[plexus_macros::child]` of `tickets` on the frame
//! (README "The frame"), mounted in `activation/mod.rs` as `fn work`.
//!
//! The methods are declarations: `///` docs on each method and parameter,
//! the caller a `VerifiedUser` the dispatch resolves, typed parameters, a
//! body that is one call to the helpers beside `activation/mod.rs` —
//! `reads.rs` (`ready_stream`, `blockers_stream`) and `waves.rs` (the wave
//! answers). Every read answers through the caller's reach (ACCESS-10): a
//! blocker out of it is counted (`hidden_blockers`), never resolved or
//! named; a wave's `waiting_on` names only tickets in reach.
//!
//! Its grants are `work.<method>` (its own namespace, listed in its own
//! `roles(...)`); the hub's gate decides a call to `tickets.work.<method>`
//! by that dotted path, and seeds each role with the child's grants under
//! it (`tickets.work.ready`; MACRO-20).
//! Its tests are `activation/tests/work.rs`.

// The attribute runs in the client-only build too (it emits `WorkClient`);
// the crate's `server` feature gates the server half — this impl included —
// as `activation/mod.rs` says.
use super::*;

/// The order of work: what can start now, the waves, and what a ticket waits on.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct WorkActivation {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

/// The order of work: what can start now, the waves, and what a ticket waits on.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["ready", "waves", "waves_under", "waves_to", "blockers"],
        editor = [viewer],
    )
)]
impl WorkActivation {
    /// Tickets that can start now: Ready, with every blocker Complete, Superseded or Canceled; optionally under a filter.
    ///
    /// The DAG query: which tickets can start now? Yields one row event
    /// per member of the ready set — status `Ready` and every
    /// `blocked_by` entry `Complete` or `Superseded`; never a doc (SDLC-11),
    /// whose status is `Doc` for good — ordered by
    /// (epic, seq), then one `page` event. A dangling blocker keeps its
    /// ticket out (use `blockers` to see which ids dangle). The epic
    /// filter applies to membership; blockers resolve globally.
    /// `after` / `limit` / `summary` page it exactly as `list` (MNG-9).
    ///
    /// FILTER-15: `filter` (the language's text, FILTER-10) narrows it to
    /// the ready set ∧ the filter — a filtered page of `ready and (filter)`
    /// from the cells (`ready` is the ready set's rule), paged and counted
    /// as without it, the page event without `first`. A filter not taken,
    /// or one that asks `under` or `text`, is one in-band `error`; one that
    /// narrows nothing (empty, a display part alone) is no filter.
    /// ACCESS-17: was `tickets ready_set`.
    #[plexus_macros::method(
        group = "read",
        example(note = "what can start now in SCALE", params = r#"{"filter": "kind build and in SCALE", "summary": true}"#)
    )]
    pub async fn ready(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only tickets in this epic (optional; blockers still resolve globally).
        epic: Option<ShortcodeRef>,
        /// Cursor: start strictly after this ticket id, e.g. MNG-9 (optional).
        after: Option<Cursor>,
        /// At most this many tickets; the page event carries the next cursor (optional).
        limit: Option<u32>,
        /// Yield summary rows (every field but the body) instead of full tickets (optional).
        summary: Option<bool>,
        /// A filter, e.g. 'kind build and in SCALE' (FILTER-3's language; optional): the ready set and the filter.
        filter: Option<FilterText>,
        /// The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC).
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        ready_stream(self.store.clone(), user, ReadyAsk { epic, after, limit, summary, filter, tz })
    }

    /// The order of work: each ticket of the shortcodes placed in its wave over the whole blocked_by graph (or missing_blocker, or done) with what it waits on, then the wave counts.
    ///
    /// WAVE-4: the order of work, for a board. Yields one `Placed` per
    /// ticket of the shortcodes `epics` (case-insensitive; one with no
    /// tickets contributes nothing) whose kind is in `kinds` — default
    /// `build`, `spike`, `gap`; never a doc — then exactly one
    /// `WaveCounts`. Each ticket's placement is computed over the whole
    /// `blocked_by` graph by MNG-4's rule, following blockers into any
    /// shortcode: `done` when it is `Complete` or `Superseded`;
    /// `missing_blocker` when it, or anything it waits on, names a blocker
    /// that is no ticket; otherwise `wave { n }` — 1 with every blocker
    /// satisfied, else 1 + the highest wave among its unsatisfied ones.
    /// `waiting_on` is its unsatisfied direct blockers in the caller's
    /// reach, in `blocked_by` order, distinct (one out of reach holds it
    /// back, never named). Order: waves ascending, inside a wave Coding, In
    /// Review, Ready, Pending, Blocked, Idea, Epic and then (epic, seq);
    /// then the missing lane; then the done — each in (epic, seq). `epics`
    /// empty, or a kind outside the six, is one in-band `Error` and
    /// nothing else. WAVE-6: each placement is the one the store keeps on the
    /// row (the same rule, kept current by every write that moves one); rows
    /// only.
    #[plexus_macros::method(group = "read", values(kinds = ["scope", "execution", "build", "gap", "spike"]))]
    pub async fn waves(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Shortcodes whose tickets are placed, e.g. ["PLEXUS", "MACRO"] (case-insensitive; at least one); blockers are followed into every shortcode.
        epics: Vec<ShortcodeRef>,
        /// Kinds to place: scope | execution | build | gap | spike (optional; default build, spike, gap — a doc is never placed).
        kinds: Option<Vec<Kind>>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(waves_answer(&self.store, user, texts(epics), kinds))
    }

    /// The order of work under one ticket: each ticket of its subtree placed in its wave (or missing_blocker, or done) with what it waits on, then the wave counts.
    ///
    /// MENGER-31: the wave board of `id`'s subtree (activation/waves.rs).
    #[plexus_macros::method(group = "read", values(kinds = ["scope", "execution", "build", "gap", "spike"]))]
    pub async fn waves_under(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket whose subtree is placed, e.g. MENGER-2 (it and every ticket under it, any depth, hosted shortcodes included).
        id: TicketRef,
        /// Kinds to place: scope | execution | build | gap | spike (optional; default build, spike, gap — a doc is never placed).
        kinds: Option<Vec<Kind>>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(waves_under_answer(&self.store, user, id.into_string(), kinds))
    }

    /// The wave board whose final state is one ticket: its blockers transitively (and a scope's open descendants) placed in waves, done blockers done, the ticket alone in the last wave; cycles named; then the wave counts.
    ///
    /// MENGER-31: the wave board whose final state is `id` (activation/waves.rs).
    #[plexus_macros::method(group = "read", values(kinds = ["scope", "execution", "build", "gap", "spike"]))]
    pub async fn waves_to(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The final ticket, e.g. MENGER-31.
        id: TicketRef,
        /// Kinds to yield: scope | execution | build | gap | spike (optional; default all five — never a doc; the ticket itself always; every kind is followed).
        kinds: Option<Vec<Kind>>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(waves_to_answer(&self.store, user, id.into_string(), kinds))
    }

    /// One ticket's blockers resolved to current statuses; dangling ids marked; those out of your reach counted, never named.
    ///
    /// "Why can't this start?" in one call. Yields, in `blocked_by`
    /// order, one `Blocker` event per blocker that exists (the full
    /// ticket, at its current status) and one `DanglingBlocker` event per
    /// id that names no ticket yet; `NotFound` if the queried ticket
    /// itself doesn't exist (or is out of the caller's reach). An empty
    /// `blocked_by` yields nothing. ACCESS-17 (ACCESS-10): a blocker the
    /// caller may not see — and, to a reader restricted to part of the
    /// store, an id that names no ticket — is neither resolved nor named:
    /// the answer ends with one `HiddenBlockers { count }` saying how many,
    /// yielded only when there are any.
    #[plexus_macros::method(group = "read")]
    pub async fn blockers(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id whose blockers to resolve, e.g. MNG-2.
        id: TicketRef,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        blockers_stream(self.store.clone(), user, id.into_string())
    }
}
