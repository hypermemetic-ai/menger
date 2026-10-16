import type { RpcClient } from '../../rpc';
import type { ShortcodeRef, TicketRef } from '../../notes/types';
import type { Cursor, FilterText, TicketsEvent, Zone } from '../../tickets/types';
/** Typed client interface for tickets.nav plugin */
export interface TicketsNavClient {
    /**
     * Each shortcode's number of tickets matching a filter, in shortcode order — the filtered extents (every ticket without one).
  
  FILTER-4: the filtered `extents`. Yields one `Count { epic, held }`
  per shortcode that holds a match of `filter`, in shortcode order,
  and nothing after them — the stream's own `done` ends it, as for
  `extents`. Without a filter, each shortcode's tickets. A sum over
  the kept counts of the cells the filter admits: no ticket is tested,
  and the cost is the shortcodes and the cells, never the tickets.
  With the counts, a client turns a position among the matches into a
  (shortcode, rank) by prefix sums, and asks `list { filter, at }`. A
  filter not taken, or one that asks `under` or `text`, is one in-band
  `error` and nothing else. FILTER-24: `keep` (a ticket id inside the
  caller's reach) is counted as a match whatever the filter says of it —
  once — as `list { filter, keep }` pages it.
     * @requires `nav.counts` anywhere it is held
     */
    counts(filter?: FilterText | null, keep?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
    /**
     * Each shortcode's last sequence (a deleted ticket's too: numbers are never reused) and the holes below it, in shortcode order — where each shortcode ends in the store's order.
  
  SCALE-16: where each shortcode ends in the store's order. Yields
  one `Extent` per shortcode that holds a ticket, in shortcode order
  — its highest sequence, read from the per-shortcode sequence
  `create` allocates from (SCALE-3), so the cost is the number of
  shortcodes. ACT-5: past every deleted number, which a shortcode's
  `holes` name — sequences are dense but for them, so with these a
  client turns a position in the store's order into a cursor without
  walking to it:
  prefix sums find the shortcode E and the row k, and `list { after:
  "E-k" }` starts at row k + 1. Nothing trails the extents — there is
  no cursor to hand back — so the stream's own `done` is the whole
  answer, as for `groups` and `blockers`. No watch event keeps them:
  `created` already carries the newborn's `epic` and `seq`.
     * @requires `nav.extents` anywhere it is held
     */
    extents(): AsyncGenerator<TicketsEvent>;
    /**
     * Menu counts: per field (status, category, kind, severity, blocked, ready, has commits, menger doc, in), the matches of base and filter without filter's own groups for that field, by value.
  
  FILTER-4: the counts behind every menu (Linear's facets). For each
  field — `status`, `category`, `kind`, `severity`, `blocked`, `ready`,
  `has commits`, `menger doc` (FILTER-21), `in`, in that order — the
  tickets matching `base` and `filter` with `filter`'s own groups for
  that field set aside (a group is a field's own when every predicate
  in it names that field; `base`'s are never set aside), counted by
  value: one `Facet { field, value, held }` per value with `held` above
  0 — `field` as the language spells it, `value` canonical (`Coding`,
  `build`, `none`, `yes`/`no` for the four flags, the shortcode for
  `in`), statuses in the store's order of statuses, shortcodes in
  shortcode order — then the stream's done. A sum over kept cell
  counts, never a ticket. A filter not taken, or one that asks `under`
  or `text`, is one in-band `error` and nothing else.
     * @requires `nav.facets` anywhere it is held
     */
    facets(base?: FilterText | null, filter?: FilterText | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
    /**
     * One roll-up per shortcode: done/total over its subtree's work, its docs, its root (with the root's title, status and kind), and the host it nests under.
  
  MNG-10: the nav's tree without its rows. Yields one `Group` per
  shortcode row the nav draws, in shortcode order — `done/total` over
  the row's whole subtree (SDLC-11: its work; `docs` counts its docs,
  which have no completion), its scope (or doc) `root`, and the `host` ticket it
  nests under when it is a sub-shortcode — exactly as AGS-20's
  `plan()` computes them from every row. AGS-23: and the root's own
  row — its title, status and kind (`root_row`) — which the nav draws
  as the shortcode's top row, so its top level needs nothing else.
  Nothing trails the groups:
  unlike `list` there is no cursor to hand back, so the stream's own
  `done` is the whole answer, as for `blockers`. `watch` keeps a
  client's copy current with `GroupChanged`. ACCESS-10: the shortcodes
  the caller sees; a root or host out of reach is none (the shortcode
  at the top for this caller).
     * @requires `nav.groups` anywhere it is held
     */
    groups(): AsyncGenerator<TicketsEvent>;
    /**
     * Which of these tickets a filter admits: one member { id, matched } per id, in the order given (an unknown id: false); at most 1000 ids.
  
  FILTER-15: which of some tickets a filter admits — what a page that
  shows tickets it did not list (a ticket page, its blockers, its
  unlocks) asks of the global filter. Yields one `Member { id, matched
  }` per id, in the order given — `id` as given, `matched` whether the
  filter admits the ticket it names; an id (or `SHORTCODE/path`
  address) that names no ticket is `matched: false` — and nothing
  after them: the stream's own `done` ends it. Each id is one lookup in
  the filter's admitted cells, never a test of the ticket. At most
  1,000 ids: more, a filter not taken, or one that asks `under` or
  `text`, is one in-band `error` and nothing else.
     * @requires `nav.member` anywhere it is held
     */
    member(filter: FilterText, ids: TicketRef[], tz?: Zone | null): AsyncGenerator<TicketsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `nav.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
    /**
     * The nav under a filter: a parent's children (or a shortcode's top level) that match or hold a match, each with matched and below (matches strictly under it), then a page; matches_only: the matches nearest below instead, then the hosted shortcodes drawn there.
  
  FILTER-13: the nav under a filter — the rows the nav would show at an
  address unfiltered (`parent`: that ticket's children, as `list {
  parent }` gives them; `epic`: the shortcode's top level — its root,
  or, for a shortcode with none, its tickets as `list { epic }` gives
  them), only those that match `filter` or hold a match below them, in
  the store's order. One `Node { summary, matched, below }` per row —
  `matched` whether the row matches, `below` how many matches are
  strictly under it — then one `Page { next, remaining }`, paged as
  `list` pages (`after` a ticket id, at most `limit`). Answered from the
  kept tree: a parent's children under a filter are (C(P) ∩ M) ∪ the
  holders of every admitted cell, and `below` a sum over the row's
  tally — no subtree is walked. Exactly one of `parent` and `epic`; a
  filter not taken, or one that asks `text`, is one in-band `error` and
  nothing else.
  
  FILTER-23: `matches_only` — the nav that draws only matches, no
  ticket that only holds them. At `parent`, the matches nearest below
  it: its children that match and, in place of a child that does not,
  that child's own nearest matches, at any depth, in the store's order
  — then the hosted shortcodes whose row is drawn there (hosted at the
  parent, or at a ticket between that is not drawn), in shortcode
  order, each as its root's node, whether or not it holds a match. At
  `epic`, the root when it matches, else the top of the shortcode: what
  the root's `parent` address answers (a shortcode with no root: its
  tickets that match). Paged the same way. Off, `tree` is FILTER-13's,
  byte for byte.
  
  FILTER-24: `keep`, with `matches_only` — a ticket id (the nav's open
  ticket) answered as if it matched: a row where a match would be, its
  own nearest matches answered under it rather than above, a hosted
  shortcode placed at it; its node still says `matched: false`. A
  match, a shortcode's root, an id naming no ticket, or one outside the
  caller's reach changes nothing.
     * @requires `nav.tree` anywhere it is held
     */
    tree(filter: FilterText, after?: Cursor | null, epic?: ShortcodeRef | null, keep?: TicketRef | null, limit?: number | null, matchesOnly?: boolean | null, parent?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
}
/** Create a typed tickets.nav client from an RPC client */
export declare function createTicketsNavClient(rpc: RpcClient): TicketsNavClient;
/** What each tickets.nav method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "nav.counts": {
        readonly grant: "nav.counts";
        readonly target: null;
    };
    readonly "nav.extents": {
        readonly grant: "nav.extents";
        readonly target: null;
    };
    readonly "nav.facets": {
        readonly grant: "nav.facets";
        readonly target: null;
    };
    readonly "nav.groups": {
        readonly grant: "nav.groups";
        readonly target: null;
    };
    readonly "nav.member": {
        readonly grant: "nav.member";
        readonly target: null;
    };
    readonly "nav.schema": {
        readonly grant: "nav.schema";
        readonly target: null;
    };
    readonly "nav.tree": {
        readonly grant: "nav.tree";
        readonly target: null;
    };
};
/** The tickets.nav activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export declare const defaultRoles: {
    readonly editor: readonly ["nav.groups", "nav.extents", "nav.tree", "nav.counts", "nav.facets", "nav.member"];
    readonly viewer: readonly ["nav.groups", "nav.extents", "nav.tree", "nav.counts", "nav.facets", "nav.member"];
};
