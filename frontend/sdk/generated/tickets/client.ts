// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { CallerSource, CommitRef, Cursor, DeleteChildren, FilterText, InputId, InputSource, InputState, InputValue, Label, MengerPath, Message, Reason, Severity, SeverityChange, Status, TicketsEvent, Urgency, Zone } from './types';
import type { Body, Kind, Query, ShortcodeRef, TicketRef, Title } from '../notes/types';
import type { OrgId } from '../orgs/types';

/** Typed client interface for tickets plugin */
export interface TicketsClient {
  /** Replace a ticket's title and/or body (whole values); yields amended { ticket, changed }.

MNG-16: replaces a ticket's title and/or body — the correction a
DB-first store otherwise has no way to make (a worker's branch, a
divergence note, a contract patched with its finding). Whole values,
last writer wins. Yields exactly one event: `Amended { ticket,
changed }` — `changed` names what moved, and is empty when nothing
did (neither given, or the values already there), which writes and
broadcasts nothing — `NotFound` if `id` names no ticket, or an
in-band `Error` for a title that is empty after trimming. `watch`
carries every `Amended` that changed something. COMP-3: a new body
moves the component registry — its ` ```component ` blocks and `<tag`
references — and `watch` says so after the `Amended`; a malformed
block, or a name another ticket defines, is an in-band `Error` (the
latter naming that ticket) and nothing is written, title included. */
  amend(id: TicketRef, body?: Body | null, title?: Title | null): AsyncGenerator<TicketsEvent>;
  /** Answer an input: record a value (checked against its kind and options) with a rationale, a source and the time; yields input_changed.

INPUT-3: answers an input — records `value` against `ticket`'s input
`input`, with its `rationale`, its `source` (`app`: a viewer's click,
which the host says; `api`, the default: anything else) and the
time. The value is typed by the input's kind and checked against the
options its body declares: `{"type": "choice", "option": …}` names
one, `{"type": "multi", "options": […]}` a set (kept in the body's
order, each once; empty: none ticked), `{"type": "text", "text": …}`
a reply, not blank. Every answer is a history entry — a changed one,
and the same one again. Yields exactly one event: `InputChanged` (the
input as it is now, its answer and history included), `NotFound` for
a ticket that is none, or an in-band `Error` — the ticket declares no
such input, the value does not answer it, it is closed, or it was
removed from its body — and nothing is written. Never touches the
body. `watch` carries the `InputChanged`. */
  answer(input: InputId, ticket: TicketRef, value: InputValue, rationale?: Message | null, source?: InputSource | null): AsyncGenerator<TicketsEvent>;
  /** Wait for the owner's response on a ticket: waiting { ticket, queued } at once, then one replied { ticket, reply } — an input answered, changes requested, a comment or an unqueue by another, the ticket canceled or deleted — and the end; timed_out after timeout_minutes.

APP-24: after queueing, an agent waits here — `waiting`, then one `replied` (or `timed_out`), then the end. */
  awaitReply(id: TicketRef, input?: InputId | null, since?: number | null, timeoutMinutes?: number | null): AsyncGenerator<TicketsEvent>;
  /** One ticket's blockers resolved to current statuses; dangling ids marked.

"Why can't this start?" in one call. Yields, in `blocked_by`
order, one `Blocker` event per blocker that exists (the full
ticket, at its current status) and one `DanglingBlocker` event per
id that names no ticket yet; `NotFound` if the queried ticket
itself doesn't exist. An empty `blocked_by` yields nothing.
IDENTITY-37: a blocker the caller cannot view is a `DanglingBlocker`
too — its id, which the ticket's own `blocked_by` already shows, and
nothing of it: exactly what an id naming no ticket gets. */
  blockers(id: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Clear an input's answer (open again; kept in its history, with a source); yields input_changed.

INPUT-3: clears an input's answer — `open` again, the clear in its
history, with its `source` (INPUT-9: `app`, a viewer's click, which
the host says for a frame's `menger.input.clear`; `api`, the default:
anything else). Yields exactly one event: `InputChanged`; `Input`
when it held no answer (nothing written); `NotFound`; or an in-band
`Error` — no such input, closed, removed, an unknown source. */
  clearAnswer(input: InputId, ticket: TicketRef, source?: InputSource | null): AsyncGenerator<TicketsEvent>;
  /** Close an input: freeze its answer (or its lack of one); yields input_changed.

INPUT-3: closes an input — frozen (a decision's DECIDED), with its
answer or without one; an answer or a clear is refused until it is
reopened. Yields exactly one event: `InputChanged`; `Input` when it
was closed already; `NotFound`; or an in-band `Error` — no such
input, removed. */
  closeInput(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Every component in the registry, in name order: its source, its owner (null: an orphan), what references it, its docs (summary, typed attributes, slots, examples, help; null without <docs>) and its preview attributes.

COMP-3: the component registry. Yields one `Component` per component
a ticket defines or once defined, in name order — its `source` (the
` ```component <name> ` block's content), its `owner` (`null` for an
orphan: no ticket defines it any more, and something still uses it)
and its `references` (the tickets whose bodies hold `<name` as an
HTML tag, in the store's order, then the components whose sources
do). COMP-8: and its help — `docs`, the source's `<docs>` parsed
(summary, typed attributes, slots, examples, markdown help; `null`
without one) — and `preview`, the attributes on the block's opening
line. Nothing trails them — the stream's own `done` is the whole
answer, as for `groups`. From the cache alone; `watch` keeps a
client's copy current with `ComponentChanged` and `ComponentRemoved`.
IDENTITY-37: a caller who cannot view every shortcode gets the
components whose owner it can view — no orphan — each with only the
references it can view. */
  components(): AsyncGenerator<TicketsEvent>;
  /** Each shortcode's number of tickets matching a filter, in shortcode order — the filtered extents (every ticket without one).

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
once — as `list { filter, keep }` pages it. */
  counts(filter?: FilterText | null, keep?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Create a ticket; allocates the next EPIC-N id, status starts Pending (a doc's: Doc, for good); the first ticket of a prefix must be a scope or a doc.

Creates a ticket in an epic. The store allocates the next `EPIC-N`
id; status starts at `Pending` (only a human promotes to `Ready`);
`kind` defaults to `build`, and one outside the six yields the
in-band error naming them. SDLC-11: a `doc` starts — and stays — in
`Doc`; it may be a shortcode's first ticket, as a scope may; a doc
with a `blocked_by`, a `blocked_by` naming a doc, and a doc born as
an id some `blocked_by` already names are in-band errors (a doc never
completes, so nothing may wait on one). `parent` (AGS-19) files the ticket as an
output of an existing ticket in any epic; one that names no ticket,
or that would make the newborn its own ancestor, is an in-band error
naming the reason. MNG-14: omitted, `parent` defaults to the
shortcode's scope root; given, it must be inside the same shortcode
— except for the scope root itself, whose parent is the shortcode
it hangs under. Every shortcode starts with a scope (MNG-12) or a
doc (SDLC-11): the first ticket of a prefix must be `kind: scope` or
`kind: doc`, and anything else is an in-band error naming the rule,
with nothing allocated. WAVE-10: `path` gives the ticket a path
address in its shortcode (`<SHORTCODE>/<path>`, which `get` resolves)
— accepted only when it starts with `_MENGER/` and names something
after it, `kind` is `doc`, and `parent` is the shortcode's scope root
(or omitted, which defaults to it); a path another ticket of the
shortcode already holds is an in-band error naming that ticket.
COMP-3: a body's ` ```component <tag> ` blocks register components
(`watch` says so after the rest of the birth); a block whose tag is
not a component name, a name defined twice, and a name another
ticket defines are in-band errors — the last naming that ticket —
with nothing stored or allocated.
Yields exactly one event: `Created` or `Error`. */
  create(epic: ShortcodeRef, title: Title, blockedBy?: TicketRef[] | null, body?: Body | null, kind?: Kind | null, org?: OrgId | null, parent?: TicketRef | null, path?: MengerPath | null, severity?: Severity | null): AsyncGenerator<TicketsEvent>;
  /** Delete tickets for good (admin): refused while children, blockers, a shortcode, a component, a theme or a live input point at one, unless children reparent|delete and unblock say what moves; a tombstone kept (get answers deleted), the body 30 days for undelete; yields deleted { ids, tombstones }.

ACT-5: tickets gone for good — refused while something points at
them unless told what moves; a tombstone kept, the body 30 days. The
rules, the write and the events are [`crate::store::Deleted`]'s
module's. Admin only. Yields `Deleted { ids, tombstones }`, or one
`Error` naming every refused ticket and why. */
  delete(children?: DeleteChildren | null, filter?: FilterText | null, ids?: TicketRef[] | null, max?: number | null, message?: Message | null, tz?: Zone | null, unblock?: boolean | null): AsyncGenerator<TicketsEvent>;
  /** Each shortcode's last sequence (a deleted ticket's too: numbers are never reused) and the holes below it, in shortcode order — where each shortcode ends in the store's order.

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
`created` already carries the newborn's `epic` and `seq`. */
  extents(): AsyncGenerator<TicketsEvent>;
  /** Menu counts: per field (status, category, kind, severity, blocked, ready, has commits, menger doc, in), the matches of base and filter without filter's own groups for that field, by value.

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
or `text`, is one in-band `error` and nothing else. */
  facets(base?: FilterText | null, filter?: FilterText | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Find tickets whose id or title contains the query (case-insensitive), optionally under a filter; summary rows, paged with after + limit.

SCALE-6: the jump palette's question, answered where the rows are
— so a client that does not hold every row (SCALE-7) can still
find one. Yields one `summary` row per ticket whose id or title
contains `query`, case-insensitively with ASCII folding (AGS-18's
rule, the palette's own), in (epic, seq) order — at most `limit`,
default 50, what the palette shows — then exactly one `page`
event: `remaining` plus the rows is the palette's total. `query`
is trimmed; empty or blank matches every ticket, so the palette
opens on the head of the store. `after` pages it exactly as
`list` (MNG-9) — a position, not a match — and one naming no
ticket is one in-band `error`. Ids and titles only: a jump aid,
not search over bodies.

FILTER-15: `filter` (the language's text, FILTER-10) keeps the text
matches the filter admits, in the same order, paged and counted as
without it — the needle tested only on the candidates the filter's
cells admit, never on the rest of the store. A filter not taken, or
one that asks `under` or `text`, is one in-band `error`; one that
narrows nothing (empty, a display part alone) is no filter. */
  find(query: Query, after?: Cursor | null, filter?: FilterText | null, limit?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Get one ticket by id, or by path address SHORTCODE/path (e.g. PLEXUS/_MENGER/board).

Yields exactly one event: `Ticket` if found, `NotFound` otherwise.
WAVE-10: `id` may be a path address, `<SHORTCODE>/<path>` — the
shortcode case-insensitive, the path exact — naming the one ticket of
that shortcode that holds the path; `NotFound` carries the address. */
  get(id: TicketRef): AsyncGenerator<TicketsEvent>;
  /** One roll-up per shortcode: done/total over its subtree's work, its docs, its root (with the root's title, status and kind), and the host it nests under.

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
client's copy current with `GroupChanged`. */
  groups(): AsyncGenerator<TicketsEvent>;
  /** Heartbeat — version, unix time, ticket count.

Returns the binary version, the server's unix time, and
how many tickets the store holds. */
  health(): AsyncGenerator<TicketsEvent>;
  /** A ticket's history: each status and kind transition (from, to, when, who, source, message, cause), oldest first, then those it caused in other tickets, then its queue records (MENGER-39), then history_end (when the log began; what a restore would put back; since when it has held its status).

MENGER-13: a ticket's history — every change of its status and its
kind since the log began, as the store keeps them. Yields one
`Transition` per transition of the ticket's, oldest first — `from` →
`to`, when, who (`actor`: a principal, `system`, or `anonymous` with
login off; `name`), from where (`source`: app, api, pipeline,
cascade), why (`message`) and what caused it (`cause`, a transition
id) — then one per transition those caused in other tickets (a
cancel's cascade, a restore's), in the order written, then exactly one
`HistoryEnd { began_unix, restorable }`: when the log began (nothing
before it has a history), and for a canceled ticket the descendants
`set_status { restore_children: true }` would put back. `NotFound` for
an id that names no ticket (or one out of the caller's reach). From
the cache; `watch` carries each new `Transition`. */
  history(id: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Every input (a body's <m-decision id> / <m-input id kind>) with its kind, question, options, recommendation, state, answer and history; filtered by ticket, shortcode, state and a filter on its ticket.

INPUT-3: the inputs store. Yields one `Input` per input of `ticket`
(every ticket's, without one) whose ticket is in the shortcode `epic`
and which is in `state` — its ticket, id, kind (`choice`, `multi`,
`text`), question, options (`{ value, label, recommended }`, the
body's order), recommendation, state (`open`, `answered`, `closed`,
`removed`), current answer (`{ value, rationale, source, at_unix }`,
`null` with none) and whole history, oldest first — in the store's
order of tickets, each ticket's inputs in its body's order and the
removed after. Nothing trails them; the stream's own `done` is the
whole answer. `NotFound` when `ticket` names no ticket; an unknown
`state` is an in-band `Error`. From the cache; `watch` keeps a
client's copy current with `InputChanged`.

FILTER-15: `filter` (the language's text, FILTER-10) keeps the inputs
whose ticket the filter admits — each such ticket one lookup in the
filter's admitted cells; every other parameter as without it. A
filter not taken, or one that asks `under` or `text`, is one in-band
`error`; one that narrows nothing (empty, a display part alone) is no
filter. */
  inputs(epic?: ShortcodeRef | null, filter?: FilterText | null, state?: InputState | null, ticket?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Add or remove a ticket's labels (e.g. needs-you), or many tickets' (ids; one write, all or none); yields labels_changed { ticket, added, removed } per ticket.

MENGER-12: moves a ticket's labels — free-form tags such as
`needs-you`, which puts the ticket on Home's "Needs you" panel.
`add` and `remove` are label names (letters, digits and `-`, at most
32 characters, folded to lower case); a label in both is added. The
ticket keeps its labels sorted, each once, at most 16. Yields exactly
one event: `LabelsChanged { ticket, added, removed }` — both empty
when nothing moved (every added label already held, every removed one
not), which writes and broadcasts nothing — `NotFound` if `id` names
no ticket, or an in-band `Error` naming every entry that is not a
label, or a result past 16; an error writes nothing. Moves
`updated_unix` and the ticket's filter cell (`label <name>`), nothing
else. `watch` carries every `LabelsChanged` that moved something.
ACT-2: `ids` instead of `id` moves each one's, in one write, all or
none, one `LabelsChanged` per ticket (`cells_stream`). */
  label(add?: Label[] | null, id?: TicketRef | null, ids?: TicketRef[] | null, remove?: Label[] | null): AsyncGenerator<TicketsEvent>;
  /** Link commits to a ticket, or to many (ids; one write, all or none) — the commits that finished it; yields commits_linked { ticket, added } per ticket.

SDLC-9: records which commits finished a ticket. Each entry is a
commit reference — a hexadecimal object name of 7 to 64 characters
(stored lowercase), optionally prefixed by a repository and `@`
(`77df9a0`, `plexus-macros@4f7114a`) — compared as text: the server
has no repository, so link the form you want kept. Every entry not
already on the ticket is appended, in the order given. Yields
exactly one event: `CommitsLinked { ticket, added }` — `added` is
what was newly linked, empty when nothing was (every entry already
linked, or none given), which writes and broadcasts nothing —
`NotFound` if `id` names no ticket, or an in-band `Error` naming
every entry that is not a reference, or for a link past 256 commits
on the ticket; an error writes nothing. Moves `updated_unix` and
nothing else — not the status. `watch` carries every
`CommitsLinked` that added something. ACT-2: `ids` instead of `id`
links them to each, in one write, all or none, one `CommitsLinked`
per ticket (`cells_stream`). */
  linkCommits(commits: CommitRef[], id?: TicketRef | null, ids?: TicketRef[] | null): AsyncGenerator<TicketsEvent>;
  /** List tickets, optionally filtered by epic, status, parent and/or a filter; paged with after + limit, or at a position with at.

Yields one row event per match, ordered by (epic, seq), then
exactly one `page` event saying where the walk stands. The
`parent` filter (AGS-19) yields exactly that ticket's children —
since SCALE-4 the one way to read them (a row carries only their
`child_count`), and a page of them costs the page.

MNG-9 adds the page: `after` is the exclusive cursor (a ticket id
— the order is total and unique, so an id is a position), `limit`
caps the page, and `summary` swaps the `ticket` rows for `summary`
rows that carry every field but the body. ACCESS-29: a page is 1,000
rows unless `limit` says, and never past 10,000 ([`crate::limits`]).
An `after` naming no ticket
is one in-band `error` and nothing else. SCALE-14: the rows come
from the cache and the bodies from the database, read a page at a
time as the rows go out — the same events, never every body at
once.

FILTER-4: `filter` (the language's text, FILTER-10) narrows the
matches further — ANDed with `epic`, `status` and `parent`, which are
sugar beside it — answered from the cells, never by testing a
ticket; and `at` starts the page at a position among the matches
(0-based) instead of after a cursor. Given either, the page event
carries `first`, the page's first row's position among the matches;
`at` past the end is an empty page with `first = at` and `next:
null`; `at` with `after` is an in-band `error`, and so is a filter
the language does not take (every bad chunk named) or one that
asks `under` or `text` (each named, with where it lives). Given
neither, every event is what it was.

FILTER-24: with a filter, `keep` (a ticket id: the nav's open ticket)
is listed and counted as a match whatever the filter says of it — at
its place, in `first` and `remaining` — when it is inside `epic`,
`status` and `parent` and inside the caller's reach; otherwise, and
without a filter, it changes nothing. */
  list(after?: Cursor | null, at?: number | null, epic?: ShortcodeRef | null, filter?: FilterText | null, keep?: TicketRef | null, limit?: number | null, parent?: TicketRef | null, status?: Status | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Which of these tickets a filter admits: one member { id, matched } per id, in the order given (an unknown id: false); at most 1000 ids.

FILTER-15: which of some tickets a filter admits — what a page that
shows tickets it did not list (a ticket page, its blockers, its
unlocks) asks of the global filter. Yields one `Member { id, matched
}` per id, in the order given — `id` as given, `matched` whether the
filter admits the ticket it names; an id (or `SHORTCODE/path`
address) that names no ticket is `matched: false` — and nothing
after them: the stream's own `done` ends it. Each id is one lookup in
the filter's admitted cells, never a test of the ticket. At most
1,000 ids: more, a filter not taken, or one that asks `under` or
`text`, is one in-band `error` and nothing else. */
  member(filter: FilterText, ids: TicketRef[], tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Queue a ticket for the owner — urgency whenever | today | now | blocking (blocking names the input that answers it), and why; queued again it is raised, never lowered; yields queued { entry, ticket, action }.

MENGER-39: puts a ticket in front of the owner — at an urgency, for a
reason; `blocking` names the input that answers it. Queued again: the
urgency raised, never lowered. Yields one `queued`, or an `error`. */
  queue(id: TicketRef, reason: Reason, urgency: Urgency, input?: InputId | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** The owner's queue, most urgent first, then oldest: a queue_row { entry, ticket } each, then queue_end { count, snoozed, now_unix }.

MENGER-39: the owner's queue, most urgent first, then the oldest. */
  queued(snoozed?: boolean | null): AsyncGenerator<TicketsEvent>;
  /** Tickets that can start now: Ready, with every blocker Complete, Superseded or Canceled; optionally under a filter.

The DAG query: which tickets can start now? Yields one row event
per member of the ready set — status `Ready` and every
`blocked_by` entry `Complete` or `Superseded`; never a doc (SDLC-11),
whose status is `Doc` for good — ordered by
(epic, seq), then one `page` event. A dangling blocker keeps its
ticket out (use `blockers` to see which ids dangle). The epic
filter applies to membership; blockers resolve globally.
`after` / `limit` / `summary` page it exactly as `list` (MNG-9).

FILTER-15: `filter` (the language's text, FILTER-10) narrows it to
the ready set ∧ the filter — a filtered page of `ready and (filter)`
from the cells (`ready` is the ready set's rule), paged and counted
as without it, the page event without `first`. A filter not taken,
or one that asks `under` or `text`, is one in-band `error`; one that
narrows nothing (empty, a display part alone) is no filter. */
  readySet(after?: Cursor | null, epic?: ShortcodeRef | null, filter?: FilterText | null, limit?: number | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Reopen a closed input; yields input_changed.

INPUT-3: reopens a closed input — `answered` again when it holds an
answer, else `open`. Yields exactly one event: `InputChanged`;
`Input` when it was not closed; `NotFound`; or an in-band `Error` —
no such input, removed. */
  reopenInput(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Request changes on a queued ticket: the body kept as a comment on its conversation (intent changes) and the ticket unqueued (action changes_requested), in one write; its input stays open; yields changes_requested { id, comment, entry, by, at_unix }.

APP-23: hands a queued ticket back — a comment (intent `changes`) on its conversation and the entry unqueued, one write. */
  requestChanges(body: Body, id: TicketRef, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** Move tickets into another shortcode, renumbered there in the order given (a scope moved into a new code becomes its root; --new-scope makes the shortcode first; a root moves with --with-children as a rename); old ids keep resolving (get says moved_from); all or none; yields reshortcoded { moves, tickets }.

ACT-6: tickets moved into another shortcode ([`crate::store::Reshortcoded`]'s module). */
  reshortcode(ids: TicketRef[], to: ShortcodeRef, message?: Message | null, newScope?: Title | null, parent?: TicketRef | null, withChildren?: boolean | null): AsyncGenerator<TicketsEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /** Search inside tickets: the tickets whose title or body holds every word of the query (prefixes and typos too), best first, each with the passage that matched; optionally under a filter, paged with offset + limit.

SEARCH-2: search inside tickets — the tickets whose title or body's
text holds every word of `query`, best first, from the search index
in the database (the store's `search`): a word matches itself and as
a prefix; one of four letters or more that the index does not know,
or holds in only a few tickets, also matches the index's words one
edit away (two from eight letters); a word joined by `-` `_` `.` `/`, or a `"quoted phrase"`,
is a phrase. Tickets matched without a correction come first, then
the rest; BM25 within each, a title weighing 4 to the text's 1.

Yields one `hit { summary, title, snippet, corrected }` per ticket of
the page, then one `searched { matched, next, words }`. `filter` (the
language's text, FILTER-10) keeps the matches it admits (`under`
too); one not taken, or one that asks `text`, is one in-band `error`
and nothing else. `limit` is [`SEARCH_LIMIT`] unless given, at most
[`SEARCH_MAX`]; `offset` skips that many matches. */
  search(query: Query, filter?: FilterText | null, limit?: number | null, offset?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's kind; everything kept by kind moves with it (to a doc: status Doc; from a doc: the status given, else Epic for a scope and Pending otherwise).

SDLC-13: changes a ticket's kind — the one write that does — and
everything kept by kind with it, in the one write. Yields exactly one
event: `KindChanged { ticket, previous }`, `NotFound` if `id` names no
ticket, or an in-band `Error` naming what it refuses. The status goes
with the kind across the doc line: to a doc it becomes `Doc` (a
`status` other than `Doc` is an `Error`); from a doc it becomes
`status` when given (never `Doc`), else `Epic` for a scope and
`Pending` for any other kind; between two other kinds it stays unless
`status` is given (`Doc` an `Error`). Refused: a ticket that waits on
a blocker, or that a ticket waits on, becoming a doc (both named); a
shortcode's root becoming a kind that roots nothing (the shortcode
named — doc ↔ scope on a root is allowed); a ticket holding a path
address becoming work; an unknown kind or status. A lower-seq ticket
that becomes a scope or a doc becomes its shortcode's root. The kind
(and status) it already has writes and sends nothing: `KindChanged`
with `previous` equal to its kind. `watch` carries `KindChanged`
first, then the `GroupChanged`, `PlacementsChanged` and (filtered)
`Entered` / `Left` it caused, as for a status change. */
  setKind(id: TicketRef, kind: Kind, message?: Message | null, source?: CallerSource | null, status?: Status | null): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's parent — the ticket it is an output of; adopts a parentless ticket or moves one.

MNG-12: files an existing ticket as an output of `parent` — the
verb AGS-19 left out. Adopts a ticket born parentless, or moves
one. Yields exactly one event: `ParentChanged` (the child, with the
parent it had before), `NotFound` if `id` names no ticket, or an
in-band `Error` when the parent does not exist or the chain would
make the ticket its own ancestor. `watch` also carries the parents'
side: `ChildrenChanged` for the new parent and `ChildRemoved` for
the old one — each the parent's id and new `child_count` (SCALE-4). */
  setParent(id: TicketRef, parent: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's status (not a doc's: a doc has no lifecycle); Canceled cancels its open descendants in the same write, and leaving Canceled may restore them (restore_children).

Yields exactly one event: `StatusChanged`, `NotFound`, or `Error`.
SDLC-7: the vocabulary is nine — the worker states `Coding` and
`In Review` included — and the write is free: any status to any
other, nothing gated (SDLC-4 owns transitions). SDLC-11: a doc has no
lifecycle — `set_status` on one is an in-band `Error`, whatever the
status — and `Doc` is a doc's own: setting it on any other kind is an
in-band `Error` too. Neither writes anything.

MENGER-13: the change is a transition — the caller's (`anonymous`
with login off), from `source` (`api` unless the call says `app` or
`pipeline`), with its `message` — kept in the log and said on `watch`
right after the `StatusChanged`. Into `Canceled`, the ticket's open
inputs are withdrawn and every descendant still open work is
canceled in the same write, each a transition of `system`'s naming
the cause. Out of `Canceled`, its withdrawn inputs come back; with
`restore_children`, so do the descendants its cancel canceled (and
nothing has moved since), to the statuses they had. */
  setStatus(id: TicketRef, status: Status, message?: Message | null, restoreChildren?: boolean | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** Snooze a queued ticket for some minutes (1 to a week): off the alerting surfaces, still queued; yields snoozed { entry, ticket }.

MENGER-39: a queued ticket off the alerting surfaces for a while; it
stays queued. Yields one `snoozed`, or an `error`. */
  snooze(id: TicketRef, minutes: number, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** The nav under a filter: a parent's children (or a shortcode's top level) that match or hold a match, each with matched and below (matches strictly under it), then a page; matches_only: the matches nearest below instead, then the hosted shortcodes drawn there.

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
caller's reach changes nothing. */
  tree(filter: FilterText, after?: Cursor | null, epic?: ShortcodeRef | null, keep?: TicketRef | null, limit?: number | null, matchesOnly?: boolean | null, parent?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Bring deleted tickets back (admin) within 30 days of the delete, whole, children and blocks put back; yields undeleted { ids, tickets }.

ACT-5: deleted tickets brought back while their tombstones keep the
body — each child its delete moved and each block it dropped put back
where nothing has moved them since. Admin only. Yields `Undeleted {
ids, tickets }`, or one `Error` naming every refused ticket and why. */
  undelete(ids: TicketRef[]): AsyncGenerator<TicketsEvent>;
  /** Unlink commits from a ticket; yields commits_unlinked { ticket, removed }.

SDLC-9: the inverse of `link_commits` — every entry the ticket
carries comes off. The same references, compared the same way, all
or nothing. Yields exactly one event: `CommitsUnlinked { ticket,
removed }` — `removed` empty when none of them was linked, which
writes and broadcasts nothing — `NotFound`, or an in-band `Error`
naming every entry that is not a reference. `watch` carries every
`CommitsUnlinked` that removed something. */
  unlinkCommits(commits: CommitRef[], id: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Take a ticket out of the owner's queue (done with it, or withdrawn); yields unqueued { id, entry, action, by, at_unix }, or not_queued.

MENGER-39: takes a ticket out of the queue. Yields one `unqueued`,
`not_queued` (nothing written), or an `error`. */
  unqueue(id: TicketRef, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** Change any part of a ticket in one write — title, body, kind, status, parent, severity, blockers (block / unblock); all lands or none does; yields updated { ticket, changed }.

MENGER-30: any part of a ticket in one write, all of it or none — the
rules, the steps and the events are [`crate::store::Update`]'s
module's. Yields exactly one event: `Updated { ticket, changed }`
(`changed` empty: nothing written or sent), `NotFound`,
`BodyTooLarge`, or one `Error` naming every refusal. */
  update(id: TicketRef, block?: TicketRef[] | null, body?: Body | null, kind?: Kind | null, message?: Message | null, parent?: TicketRef | null, severity?: SeverityChange | null, source?: CallerSource | null, status?: Status | null, title?: Title | null, unblock?: TicketRef[] | null): AsyncGenerator<TicketsEvent>;
  /** The same update to many tickets in one write — kind, status, parent, severity, blockers, labels; by ids or a filter; all lands or none does; yields updated_many { ids, changed, tickets, unchanged }.

ACT-2: `update`'s change to many tickets in one write, all or none —
the selection, the rules, the steps and the events are
[`crate::store::UpdatedMany`]'s module's. Yields exactly one event:
`UpdatedMany { ids, changed, tickets, unchanged }`, or one `Error`
naming every refused ticket and why. */
  updateMany(block?: TicketRef[] | null, filter?: FilterText | null, ids?: TicketRef[] | null, kind?: Kind | null, label?: Label[] | null, max?: number | null, message?: Message | null, parent?: TicketRef | null, severity?: SeverityChange | null, source?: CallerSource | null, status?: Status | null, tz?: Zone | null, unblock?: TicketRef[] | null, unlabel?: Label[] | null): AsyncGenerator<TicketsEvent>;
  /** Subscribe to ticket changes (Created / StatusChanged / KindChanged / Amended / CommitsLinked / CommitsUnlinked / LabelsChanged / UnlocksChanged / UnlocksRemoved / BlockersChanged / SeverityChanged / ChildrenChanged / ParentChanged / ChildRemoved / GroupChanged / PlacementsChanged / ComponentChanged / ComponentRemoved / InputChanged / UpdatedMany / Deleted / Undeleted / Reshortcoded), until disconnect; Lagged { missed } where a slow subscriber lost changes; with a filter, Entered / Left / Recount as each write's last events; Entered / Left / Recount when the caller's grants move what it sees; WatchClosed { reason } when they end it.

The reactive primitive: an open-ended stream of every mutation, as
`Created` / `StatusChanged` / `UnlocksChanged` / `ChildrenChanged`
events. Runs until the client disconnects. Every event carries the
full affected ticket, so a client that listed once and then only
watches can compute the ready set — and the parent tree — itself.

SCALE-5: the stream is fed from the store's bounded broadcast, and
a consumer that falls more than it holds behind loses the oldest
changes. That used to be silent — the stream went on as if whole and
the client's copy was stale with nothing to say so. Now the gap is
one `Lagged { missed }` where the lost changes were, then the oldest
change still held, in order, no change twice. Delivered past the
`epic` filter: whether a lost change was in the epic is exactly what
cannot be known. Raising the capacity instead would move the cliff
and keep the silence.

MNG-10: a mutation that moves a roll-up is followed, after its
ticket events, by one `GroupChanged` per group whose row moved,
innermost first — the whole group, so applying it is one upsert.
The `epic` filter applies to a group by the group's own shortcode.
AGS-23: a group's row includes its root's (`root_row`), so a status
change of a shortcode's root moves its group whether or not it
crosses `Complete`.

MNG-16: an `amend` that changed something is one `Amended`, and
moves no group — AGS-23: but a new title on a shortcode's root, which
is followed by its group's `GroupChanged`.

SDLC-9: so is a `link_commits` or `unlink_commits` that changed
something — one `CommitsLinked` or `CommitsUnlinked`, the whole
ticket, no group.

SDLC-13: a `set_kind` that changed something is one `KindChanged`
— the whole ticket — then what it moved, as a status change's: a
`GroupChanged` per row (a root's whenever its kind or status
changed; every row a re-rooted shortcode moved), the
`PlacementsChanged`, and with a filter the `Entered` / `Left`.

SCALE-4: a parent's side of a birth or a move is the exception to
"the full ticket": `ChildrenChanged` / `ChildRemoved` carry the
parent's id, the child, and the parent's new `child_count` — the
child's own `Created` / `ParentChanged` already says where it
hangs. The `epic` filter applies to them by the parent's shortcode.

COMP-3: a `create` or `amend` that moved the component registry ends
with one `ComponentChanged` per component it registered or moved
(source, owner, references — the whole component), in name order,
then one `ComponentRemoved` per component it took out — after every
other event of the write. A component is in no shortcode, so both
pass any `epic` filter.

INPUT-3: an input a write moved is one `InputChanged` — the whole
input — after every other event of the write, the component events
included: an `answer`, `clear_answer`, `close_input` or
`reopen_input`; or a `create` or `amend` whose body declared,
re-declared or removed inputs (in the body's order, the removed
after). The `epic` filter applies to it by its ticket's shortcode.

FILTER-4: with a `filter`, the same stream plus, as each write's
last events, one `Entered { id, epic, seq }` or `Left { id, epic,
seq }` per ticket the write moved into or out of the filter — a
birth, a status change, every ticket its blocker crossing moved, a
first commit or a last — in the store's order; a write that moves
more than 256 sends instead one `Recount { epic, held }` per
shortcode it moved, the filter's count there as the write left it.
`epic` filters them by their own shortcode. `membership` sends only
those three and `Lagged` — for a client whose rows already ride an
unfiltered `watch`. Without a filter or `membership`, the stream is
exactly what it was. A filter not taken, or one that asks `under` or
`text`, is one in-band `error` and the stream ends.

ACCESS-12: it hears only what the caller's reach admits, each row
redacted as a read's is, and follows the caller's grants: when they
move it says what entered and left (`entered` / `left`, past 256
`recount`) and hears through the new reach; refused — signed out,
expired, held nowhere — it ends with `watch_closed { reason, message }`. */
  watch(epic?: ShortcodeRef | null, filter?: FilterText | null, membership?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** The order of work: each ticket of the shortcodes placed in its wave over the whole blocked_by graph (or missing_blocker, or done) with what it waits on, then the wave counts.

WAVE-4: the order of work, for a board. Yields one `Placed` per
ticket of the shortcodes `epics` (case-insensitive; one with no
tickets contributes nothing) whose kind is in `kinds` — default
`build`, `spike`, `gap`; never a doc — then exactly one
`WaveCounts`. Each ticket's placement is computed over the whole
`blocked_by` graph by MNG-4's rule, following blockers into any
shortcode: `done` when it is `Complete` or `Superseded`;
`missing_blocker` when it, or anything it waits on, names a blocker
that is no ticket; otherwise `wave { n }` — 1 with every blocker
satisfied, else 1 + the highest wave among its unsatisfied ones.
`waiting_on` is its unsatisfied direct blockers, in `blocked_by`
order, distinct. Order: waves ascending, inside a wave Coding, In
Review, Ready, Pending, Blocked, Idea, Epic and then (epic, seq);
then the missing lane; then the done — each in (epic, seq). `epics`
empty, or a kind outside the six, is one in-band `Error` and
nothing else. WAVE-6: each placement is the one the store keeps on the
row (the same rule, kept current by every write that moves one); rows
only. */
  waves(epics: ShortcodeRef[], kinds?: Kind[] | null): AsyncGenerator<TicketsEvent>;
  /** The wave board whose final state is one ticket: its blockers transitively (and a scope's open descendants) placed in waves, done blockers done, the ticket alone in the last wave; cycles named; then the wave counts.

MENGER-31: the wave board whose final state is `id` (activation/waves.rs). */
  wavesTo(id: TicketRef, kinds?: Kind[] | null): AsyncGenerator<TicketsEvent>;
  /** The order of work under one ticket: each ticket of its subtree placed in its wave (or missing_blocker, or done) with what it waits on, then the wave counts.

MENGER-31: the wave board of `id`'s subtree (activation/waves.rs). */
  wavesUnder(id: TicketRef, kinds?: Kind[] | null): AsyncGenerator<TicketsEvent>;
}

/** Typed client implementation for tickets plugin */
class TicketsClientImpl implements TicketsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *amend(id: TicketRef, body?: Body | null, title?: Title | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.amend', { body, id, title });
    yield* extractData<TicketsEvent>(stream);
  }

  async *answer(input: InputId, ticket: TicketRef, value: InputValue, rationale?: Message | null, source?: InputSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.answer', { input, rationale, source, ticket, value });
    yield* extractData<TicketsEvent>(stream);
  }

  async *awaitReply(id: TicketRef, input?: InputId | null, since?: number | null, timeoutMinutes?: number | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.await_reply', { id, input, since, timeout_minutes: timeoutMinutes });
    yield* extractData<TicketsEvent>(stream);
  }

  async *blockers(id: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.blockers', { id });
    yield* extractData<TicketsEvent>(stream);
  }

  async *clearAnswer(input: InputId, ticket: TicketRef, source?: InputSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.clear_answer', { input, source, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async *closeInput(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.close_input', { input, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async *components(): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.components', {});
    yield* extractData<TicketsEvent>(stream);
  }

  async *counts(filter?: FilterText | null, keep?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.counts', { filter, keep, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *create(epic: ShortcodeRef, title: Title, blockedBy?: TicketRef[] | null, body?: Body | null, kind?: Kind | null, org?: OrgId | null, parent?: TicketRef | null, path?: MengerPath | null, severity?: Severity | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.create', { blocked_by: blockedBy, body, epic, kind, org, parent, path, severity, title });
    yield* extractData<TicketsEvent>(stream);
  }

  async *delete(children?: DeleteChildren | null, filter?: FilterText | null, ids?: TicketRef[] | null, max?: number | null, message?: Message | null, tz?: Zone | null, unblock?: boolean | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.delete', { children, filter, ids, max, message, tz, unblock });
    yield* extractData<TicketsEvent>(stream);
  }

  async *extents(): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.extents', {});
    yield* extractData<TicketsEvent>(stream);
  }

  async *facets(base?: FilterText | null, filter?: FilterText | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.facets', { base, filter, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *find(query: Query, after?: Cursor | null, filter?: FilterText | null, limit?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.find', { after, filter, limit, query, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *get(id: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.get', { id });
    yield* extractData<TicketsEvent>(stream);
  }

  async *groups(): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.groups', {});
    yield* extractData<TicketsEvent>(stream);
  }

  async *health(): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.health', {});
    yield* extractData<TicketsEvent>(stream);
  }

  async *history(id: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.history', { id });
    yield* extractData<TicketsEvent>(stream);
  }

  async *inputs(epic?: ShortcodeRef | null, filter?: FilterText | null, state?: InputState | null, ticket?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.inputs', { epic, filter, state, ticket, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *label(add?: Label[] | null, id?: TicketRef | null, ids?: TicketRef[] | null, remove?: Label[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.label', { add, id, ids, remove });
    yield* extractData<TicketsEvent>(stream);
  }

  async *linkCommits(commits: CommitRef[], id?: TicketRef | null, ids?: TicketRef[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.link_commits', { commits, id, ids });
    yield* extractData<TicketsEvent>(stream);
  }

  async *list(after?: Cursor | null, at?: number | null, epic?: ShortcodeRef | null, filter?: FilterText | null, keep?: TicketRef | null, limit?: number | null, parent?: TicketRef | null, status?: Status | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.list', { after, at, epic, filter, keep, limit, parent, status, summary, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *member(filter: FilterText, ids: TicketRef[], tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.member', { filter, ids, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *queue(id: TicketRef, reason: Reason, urgency: Urgency, input?: InputId | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.queue', { id, input, reason, source, urgency });
    yield* extractData<TicketsEvent>(stream);
  }

  async *queued(snoozed?: boolean | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.queued', { snoozed });
    yield* extractData<TicketsEvent>(stream);
  }

  async *readySet(after?: Cursor | null, epic?: ShortcodeRef | null, filter?: FilterText | null, limit?: number | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.ready_set', { after, epic, filter, limit, summary, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *reopenInput(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.reopen_input', { input, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async *requestChanges(body: Body, id: TicketRef, source?: CallerSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.request_changes', { body, id, source });
    yield* extractData<TicketsEvent>(stream);
  }

  async *reshortcode(ids: TicketRef[], to: ShortcodeRef, message?: Message | null, newScope?: Title | null, parent?: TicketRef | null, withChildren?: boolean | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.reshortcode', { ids, message, new_scope: newScope, parent, to, with_children: withChildren });
    yield* extractData<TicketsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('tickets.schema', {});
    return collectOne<unknown>(stream);
  }

  async *search(query: Query, filter?: FilterText | null, limit?: number | null, offset?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.search', { filter, limit, offset, query, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *setKind(id: TicketRef, kind: Kind, message?: Message | null, source?: CallerSource | null, status?: Status | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.set_kind', { id, kind, message, source, status });
    yield* extractData<TicketsEvent>(stream);
  }

  async *setParent(id: TicketRef, parent: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.set_parent', { id, parent });
    yield* extractData<TicketsEvent>(stream);
  }

  async *setStatus(id: TicketRef, status: Status, message?: Message | null, restoreChildren?: boolean | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.set_status', { id, message, restore_children: restoreChildren, source, status });
    yield* extractData<TicketsEvent>(stream);
  }

  async *snooze(id: TicketRef, minutes: number, source?: CallerSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.snooze', { id, minutes, source });
    yield* extractData<TicketsEvent>(stream);
  }

  async *tree(filter: FilterText, after?: Cursor | null, epic?: ShortcodeRef | null, keep?: TicketRef | null, limit?: number | null, matchesOnly?: boolean | null, parent?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.tree', { after, epic, filter, keep, limit, matches_only: matchesOnly, parent, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *undelete(ids: TicketRef[]): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.undelete', { ids });
    yield* extractData<TicketsEvent>(stream);
  }

  async *unlinkCommits(commits: CommitRef[], id: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.unlink_commits', { commits, id });
    yield* extractData<TicketsEvent>(stream);
  }

  async *unqueue(id: TicketRef, source?: CallerSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.unqueue', { id, source });
    yield* extractData<TicketsEvent>(stream);
  }

  async *update(id: TicketRef, block?: TicketRef[] | null, body?: Body | null, kind?: Kind | null, message?: Message | null, parent?: TicketRef | null, severity?: SeverityChange | null, source?: CallerSource | null, status?: Status | null, title?: Title | null, unblock?: TicketRef[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.update', { block, body, id, kind, message, parent, severity, source, status, title, unblock });
    yield* extractData<TicketsEvent>(stream);
  }

  async *updateMany(block?: TicketRef[] | null, filter?: FilterText | null, ids?: TicketRef[] | null, kind?: Kind | null, label?: Label[] | null, max?: number | null, message?: Message | null, parent?: TicketRef | null, severity?: SeverityChange | null, source?: CallerSource | null, status?: Status | null, tz?: Zone | null, unblock?: TicketRef[] | null, unlabel?: Label[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.update_many', { block, filter, ids, kind, label, max, message, parent, severity, source, status, tz, unblock, unlabel });
    yield* extractData<TicketsEvent>(stream);
  }

  async *watch(epic?: ShortcodeRef | null, filter?: FilterText | null, membership?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.watch', { epic, filter, membership, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *waves(epics: ShortcodeRef[], kinds?: Kind[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.waves', { epics, kinds });
    yield* extractData<TicketsEvent>(stream);
  }

  async *wavesTo(id: TicketRef, kinds?: Kind[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.waves_to', { id, kinds });
    yield* extractData<TicketsEvent>(stream);
  }

  async *wavesUnder(id: TicketRef, kinds?: Kind[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.waves_under', { id, kinds });
    yield* extractData<TicketsEvent>(stream);
  }
}

/** Create a typed tickets client from an RPC client */
export function createTicketsClient(rpc: RpcClient): TicketsClient {
  return new TicketsClientImpl(rpc);
}