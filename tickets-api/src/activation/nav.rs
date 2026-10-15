//! ACCESS-16: `tickets nav` — what the nav and the menus ask (MNG-10,
//! SCALE-16, FILTER-4, FILTER-13, FILTER-15): `groups`, `extents`, `tree`,
//! `counts`, `facets`, `member`, at `tickets.nav.<method>`.
//!
//! A child of `tickets` (`#[plexus_macros::child] fn nav` in
//! `activation/mod.rs`), declared on the frame as its parent is (README
//! "The frame"): `auth = "required"`, `request = MengerRequest`, every
//! caller a `VerifiedUser` resolved before the body, typed parameters, every
//! description a `///` doc, every body a line through the helpers beside
//! `activation/mod.rs` (`reads.rs`' `listed`, `member_events`,
//! `tree_events`; `overview.rs`' `counts_stream`, `facets_stream`) — each
//! answer through the caller's reach (ACCESS-10). The parent's client
//! reaches it as `client.nav()` (MACRO-16); its tests are
//! `activation/tests/nav.rs`.
//!
//! Its roles are declared in its own `roles(...)` (`nav.<method>`); the gate
//! asks a child's call by its whole path, `tickets.nav.<method>`, which is
//! how `Tickets::child_roles` hands them to the hub's book (MACRO-20: the
//! hub indexes no child).
#![allow(clippy::too_many_arguments)]

use crate::types::TicketsEvent;
use crate::vocab::{Cursor, FilterText, ShortcodeRef, TicketRef, Zone};
#[cfg(feature = "server")]
use super::{counts_stream, facets_stream, listed, member_events, tree_events, TreeAsk};
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

/// What the nav and the menus ask: the roll-ups, the extents, the tree under a filter, its counts and facets, and membership.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Nav {
    store: Arc<Store>,
    access: Access,
}

#[cfg(feature = "server")]
impl Nav {
    /// Over the parent's store and access rule — what `tickets`' `nav`
    /// accessor hands the router.
    pub fn new(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

/// What the nav and the menus ask — `tickets.nav.<method>`: the roll-ups, the extents, the tree under a filter, its counts and facets, and membership.
#[plexus_macros::activation(
    namespace = "nav",
    auth = "required",
    request = MengerRequest,
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    roles(
        viewer = ["groups", "extents", "tree", "counts", "facets", "member"],
        editor = [viewer],
    )
)]
impl Nav {
    /// One roll-up per shortcode: done/total over its subtree's work, its docs, its root (with the root's title, status and kind), and the host it nests under.
    ///
    /// MNG-10: the nav's tree without its rows. Yields one `Group` per
    /// shortcode row the nav draws, in shortcode order — `done/total` over
    /// the row's whole subtree (SDLC-11: its work; `docs` counts its docs,
    /// which have no completion), its scope (or doc) `root`, and the `host` ticket it
    /// nests under when it is a sub-shortcode — exactly as AGS-20's
    /// `plan()` computes them from every row. AGS-23: and the root's own
    /// row — its title, status and kind (`root_row`) — which the nav draws
    /// as the shortcode's top row, so its top level needs nothing else.
    /// Nothing trails the groups:
    /// unlike `list` there is no cursor to hand back, so the stream's own
    /// `done` is the whole answer, as for `blockers`. `watch` keeps a
    /// client's copy current with `GroupChanged`. ACCESS-10: the shortcodes
    /// the caller sees; a root or host out of reach is none (the shortcode
    /// at the top for this caller).
    #[plexus_macros::method(group = "overview")]
    pub async fn groups(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        listed(&self.store, &user, |store, user| store.groups(user).into_iter().map(|group| TicketsEvent::Group { group }).collect())
    }

    /// Each shortcode's last sequence (a deleted ticket's too: numbers are never reused) and the holes below it, in shortcode order — where each shortcode ends in the store's order.
    ///
    /// SCALE-16: where each shortcode ends in the store's order. Yields
    /// one `Extent` per shortcode that holds a ticket, in shortcode order
    /// — its highest sequence, read from the per-shortcode sequence
    /// `create` allocates from (SCALE-3), so the cost is the number of
    /// shortcodes. ACT-5: past every deleted number, which a shortcode's
    /// `holes` name — sequences are dense but for them, so with these a
    /// client turns a position in the store's order into a cursor without
    /// walking to it:
    /// prefix sums find the shortcode E and the row k, and `list { after:
    /// "E-k" }` starts at row k + 1. Nothing trails the extents — there is
    /// no cursor to hand back — so the stream's own `done` is the whole
    /// answer, as for `groups` and `blockers`. No watch event keeps them:
    /// `created` already carries the newborn's `epic` and `seq`.
    #[plexus_macros::method(group = "overview")]
    pub async fn extents(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        listed(&self.store, &user, |store, user| {
            store.extents_holed(user).into_iter().map(|(shortcode, epic, last_seq, holes)| TicketsEvent::Extent { epic, shortcode, last_seq, holes }).collect()
        })
    }

    /// Each shortcode's number of tickets matching a filter, in shortcode order — the filtered extents (every ticket without one).
    ///
    /// FILTER-4: the filtered `extents`. Yields one `Count { epic, held }`
    /// per shortcode that holds a match of `filter`, in shortcode order,
    /// and nothing after them — the stream's own `done` ends it, as for
    /// `extents`. Without a filter, each shortcode's tickets. A sum over
    /// the kept counts of the cells the filter admits: no ticket is tested,
    /// and the cost is the shortcodes and the cells, never the tickets.
    /// With the counts, a client turns a position among the matches into a
    /// (shortcode, rank) by prefix sums, and asks `list { filter, at }`. A
    /// filter not taken, or one that asks `under` or `text`, is one in-band
    /// `error` and nothing else. FILTER-24: `keep` (a ticket id inside the
    /// caller's reach) is counted as a match whatever the filter says of it —
    /// once — as `list { filter, keep }` pages it.
    #[plexus_macros::method(group = "overview")]
    pub async fn counts(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// A filter, e.g. 'ready and kind build' (FILTER-3's language; optional: every ticket).
        filter: Option<FilterText>,
        /// This ticket id is counted as a match whatever the filter says of it, e.g. the open ticket (optional).
        keep: Option<TicketRef>,
        /// The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC).
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        counts_stream(self.store.clone(), user, filter, keep, tz)
    }

    /// Menu counts: per field (status, category, kind, severity, blocked, ready, has commits, menger doc, in), the matches of base and filter without filter's own groups for that field, by value.
    ///
    /// FILTER-4: the counts behind every menu (Linear's facets). For each
    /// field — `status`, `category`, `kind`, `severity`, `blocked`, `ready`,
    /// `has commits`, `menger doc` (FILTER-21), `in`, in that order — the
    /// tickets matching `base` and `filter` with `filter`'s own groups for
    /// that field set aside (a group is a field's own when every predicate
    /// in it names that field; `base`'s are never set aside), counted by
    /// value: one `Facet { field, value, held }` per value with `held` above
    /// 0 — `field` as the language spells it, `value` canonical (`Coding`,
    /// `build`, `none`, `yes`/`no` for the four flags, the shortcode for
    /// `in`), statuses in the store's order of statuses, shortcodes in
    /// shortcode order — then the stream's done. A sum over kept cell
    /// counts, never a ticket. A filter not taken, or one that asks `under`
    /// or `text`, is one in-band `error` and nothing else.
    #[plexus_macros::method(group = "overview")]
    pub async fn facets(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The filter whose menus are counted (optional): its own groups for a field are set aside for that field's counts.
        filter: Option<FilterText>,
        /// A filter that always applies, e.g. the global one (optional): never set aside.
        base: Option<FilterText>,
        /// The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC).
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        facets_stream(self.store.clone(), user, filter, base, tz)
    }

    /// The nav under a filter: a parent's children (or a shortcode's top level) that match or hold a match, each with matched and below (matches strictly under it), then a page; matches_only: the matches nearest below instead, then the hosted shortcodes drawn there.
    ///
    /// FILTER-13: the nav under a filter — the rows the nav would show at an
    /// address unfiltered (`parent`: that ticket's children, as `list {
    /// parent }` gives them; `epic`: the shortcode's top level — its root,
    /// or, for a shortcode with none, its tickets as `list { epic }` gives
    /// them), only those that match `filter` or hold a match below them, in
    /// the store's order. One `Node { summary, matched, below }` per row —
    /// `matched` whether the row matches, `below` how many matches are
    /// strictly under it — then one `Page { next, remaining }`, paged as
    /// `list` pages (`after` a ticket id, at most `limit`). Answered from the
    /// kept tree: a parent's children under a filter are (C(P) ∩ M) ∪ the
    /// holders of every admitted cell, and `below` a sum over the row's
    /// tally — no subtree is walked. Exactly one of `parent` and `epic`; a
    /// filter not taken, or one that asks `text`, is one in-band `error` and
    /// nothing else.
    ///
    /// FILTER-23: `matches_only` — the nav that draws only matches, no
    /// ticket that only holds them. At `parent`, the matches nearest below
    /// it: its children that match and, in place of a child that does not,
    /// that child's own nearest matches, at any depth, in the store's order
    /// — then the hosted shortcodes whose row is drawn there (hosted at the
    /// parent, or at a ticket between that is not drawn), in shortcode
    /// order, each as its root's node, whether or not it holds a match. At
    /// `epic`, the root when it matches, else the top of the shortcode: what
    /// the root's `parent` address answers (a shortcode with no root: its
    /// tickets that match). Paged the same way. Off, `tree` is FILTER-13's,
    /// byte for byte.
    ///
    /// FILTER-24: `keep`, with `matches_only` — a ticket id (the nav's open
    /// ticket) answered as if it matched: a row where a match would be, its
    /// own nearest matches answered under it rather than above, a hosted
    /// shortcode placed at it; its node still says `matched: false`. A
    /// match, a shortcode's root, an id naming no ticket, or one outside the
    /// caller's reach changes nothing.
    #[plexus_macros::method(
        group = "read",
        example(note = "FILTER's top level, under a filter", params = r#"{"epic": "FILTER", "filter": "status Ready"}"#)
    )]
    pub async fn tree(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Whose children, e.g. FILTER-9 (or give epic).
        parent: Option<TicketRef>,
        /// A shortcode's top level: its root, or its tickets when it has none (or give parent).
        epic: Option<ShortcodeRef>,
        /// A filter, e.g. 'status Ready and under SCALE' (FILTER-3's language; empty: every ticket).
        filter: FilterText,
        /// Cursor: start strictly after this ticket id (optional).
        after: Option<Cursor>,
        /// At most this many rows; the page event carries the next cursor (optional).
        limit: Option<u32>,
        /// Only matches (optional, default false): a ticket that does not match is not a row, and the matches below it are answered in its place, at the nearest ancestor that matches or at the top of the shortcode; then the hosted shortcodes drawn there.
        matches_only: Option<bool>,
        /// With matches_only: this ticket id is drawn as if it matched — where a match would be, its nearest matches under it — e.g. the open ticket (optional; its node still says matched false).
        keep: Option<TicketRef>,
        /// The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC).
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(tree_events(&self.store, &user, TreeAsk { parent, epic, filter, after, limit, matches_only, keep, tz }))
    }

    /// Which of these tickets a filter admits: one member { id, matched } per id, in the order given (an unknown id: false); at most 1000 ids.
    ///
    /// FILTER-15: which of some tickets a filter admits — what a page that
    /// shows tickets it did not list (a ticket page, its blockers, its
    /// unlocks) asks of the global filter. Yields one `Member { id, matched
    /// }` per id, in the order given — `id` as given, `matched` whether the
    /// filter admits the ticket it names; an id (or `SHORTCODE/path`
    /// address) that names no ticket is `matched: false` — and nothing
    /// after them: the stream's own `done` ends it. Each id is one lookup in
    /// the filter's admitted cells, never a test of the ticket. At most
    /// 1,000 ids: more, a filter not taken, or one that asks `under` or
    /// `text`, is one in-band `error` and nothing else.
    #[plexus_macros::method(group = "read")]
    pub async fn member(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket ids (or SHORTCODE/path addresses), e.g. ["FILTER-4", "FILTER-5"]; at most 1000.
        ids: Vec<TicketRef>,
        /// A filter, e.g. 'status Ready' (FILTER-3's language; empty: every ticket).
        filter: FilterText,
        /// The time zone a history filter's days are read in: an IANA name, UTC, or an offset (optional; default UTC).
        tz: Option<Zone>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(member_events(&self.store, &user, ids, filter, tz))
    }
}
