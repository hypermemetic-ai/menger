// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { CallerSource, Cursor, DeleteChildren, FilterText, InputId, Label, MengerPath, Message, Reason, Severity, SeverityChange, Status, TicketsEvent, Urgency, Zone } from './types';
import type { Body, Kind, Query, ShortcodeRef, TicketRef, Title } from '../notes/types';
import type { OrgId } from '../orgs/types';

/** Typed client interface for tickets plugin */
export interface TicketsClient {
  /**
   * Replace a ticket's title and/or body (whole values); yields amended { ticket, changed }.

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
latter naming that ticket) and nothing is written, title included.
   * @requires `tickets.amend` on the scope of `id`
   */
  amend(id: TicketRef, body?: Body | null, title?: Title | null): AsyncGenerator<TicketsEvent>;
  /**
   * Archive tickets — each named (or a filter's) with its whole subtree, live work included and named (live); out of every view unless a filter says archived; status unchanged; yields archived { ids, live, unqueued, mark, dry_run }.

ARCHIVE-2 ([`crate::store::ArchiveAnswer`]'s module): a mark apart from status — status, kind, place and blockers stay; one already archived is a no-op; a queued one leaves the queue. dry_run: the answer, nothing written.
   * @requires `tickets.archive` on the scope of `ids`
   */
  archive(dryRun?: boolean | null, filter?: FilterText | null, ids?: TicketRef[] | null, max?: number | null, message?: Message | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /**
   * Wait for the owner's response on a ticket: waiting { ticket, queued } at once, then one replied { ticket, reply } — an input answered, changes requested, a comment or an unqueue by another, the ticket canceled or deleted — and the end; timed_out after timeout_minutes.

APP-24: after queueing, an agent waits here — `waiting`, then one `replied` (or `timed_out`), then the end.
   * @requires `tickets.await_reply` anywhere it is held
   */
  awaitReply(id: TicketRef, input?: InputId | null, since?: number | null, timeoutMinutes?: number | null): AsyncGenerator<TicketsEvent>;
  /**
   * Create a ticket; allocates the next EPIC-N id, status starts Pending (a doc's: Doc, for good); the first ticket of a prefix must be a scope or a doc.

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
Yields exactly one event: `Created` or `Error`.
   * @requires `tickets.create` anywhere it is held
   */
  create(epic: ShortcodeRef, title: Title, blockedBy?: TicketRef[] | null, body?: Body | null, kind?: Kind | null, org?: OrgId | null, parent?: TicketRef | null, path?: MengerPath | null, severity?: Severity | null): AsyncGenerator<TicketsEvent>;
  /**
   * Delete tickets for good (admin): refused while children, blockers, a shortcode, a component, a theme or a live input point at one, unless children reparent|delete and unblock say what moves; a tombstone kept (get answers deleted), the body 30 days for undelete; yields deleted { ids, tombstones }.

ACT-5: tickets gone for good — refused while something points at
them unless told what moves; a tombstone kept, the body 30 days. The
rules, the write and the events are [`crate::store::Deleted`]'s
module's. Admin only. Yields `Deleted { ids, tombstones }`, or one
`Error` naming every refused ticket and why.
   * @requires `tickets.delete` on the scope of `ids`
   */
  delete(children?: DeleteChildren | null, filter?: FilterText | null, ids?: TicketRef[] | null, max?: number | null, message?: Message | null, tz?: Zone | null, unblock?: boolean | null): AsyncGenerator<TicketsEvent>;
  /**
   * Find tickets whose id or title contains the query (case-insensitive), optionally under a filter; summary rows, paged with after + limit.

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
narrows nothing (empty, a display part alone) is no filter.
   * @requires `tickets.find` anywhere it is held
   */
  find(query: Query, after?: Cursor | null, filter?: FilterText | null, limit?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /**
   * Get one ticket by id, or by path address SHORTCODE/path (e.g. PLEXUS/_MENGER/board).

Yields exactly one event: `Ticket` if found, `NotFound` otherwise.
WAVE-10: `id` may be a path address, `<SHORTCODE>/<path>` — the
shortcode case-insensitive, the path exact — naming the one ticket of
that shortcode that holds the path; `NotFound` carries the address.
   * @requires `tickets.get` anywhere it is held
   */
  get(id: TicketRef): AsyncGenerator<TicketsEvent>;
  /**
   * Heartbeat — version, unix time, ticket count.

Returns the binary version, the server's unix time, and
how many tickets the store holds.
   * @requires `tickets.health` anywhere it is held
   */
  health(): AsyncGenerator<TicketsEvent>;
  /**
   * A ticket's history: each status and kind transition (from, to, when, who, source, message, cause), oldest first, then those it caused in other tickets, then its queue records (MENGER-39), then history_end (when the log began; what a restore would put back; since when it has held its status).

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
the cache; `watch` carries each new `Transition`.
   * @requires `tickets.history` anywhere it is held
   */
  history(id: TicketRef): AsyncGenerator<TicketsEvent>;
  /**
   * Add or remove a ticket's labels (e.g. needs-you), or many tickets' (ids; one write, all or none); yields labels_changed { ticket, added, removed } per ticket.

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
none, one `LabelsChanged` per ticket (`cells_stream`).
   * @requires `tickets.label` on the scope of `id`
   */
  label(add?: Label[] | null, id?: TicketRef | null, ids?: TicketRef[] | null, remove?: Label[] | null): AsyncGenerator<TicketsEvent>;
  /**
   * List tickets, optionally filtered by epic, status, parent and/or a filter; paged with after + limit, or at a position with at.

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
without a filter, it changes nothing.
   * @requires `tickets.list` anywhere it is held
   */
  list(after?: Cursor | null, at?: number | null, epic?: ShortcodeRef | null, filter?: FilterText | null, keep?: TicketRef | null, limit?: number | null, parent?: TicketRef | null, status?: Status | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /**
   * Queue a ticket for the owner — urgency whenever | today | now | blocking (blocking names the input that answers it), and why; queued again it is raised, never lowered; yields queued { entry, ticket, action }.

MENGER-39: puts a ticket in front of the owner — at an urgency, for a
reason; `blocking` names the input that answers it. Queued again: the
urgency raised, never lowered. Yields one `queued`, or an `error`.
   * @requires `tickets.queue` on the scope of `id`
   */
  queue(id: TicketRef, reason: Reason, urgency: Urgency, input?: InputId | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /**
   * The owner's queue, most urgent first, then oldest: a queue_row { entry, ticket } each, then queue_end { count, snoozed, now_unix }.

MENGER-39: the owner's queue, most urgent first, then the oldest.
   * @requires `tickets.queued` anywhere it is held
   */
  queued(snoozed?: boolean | null): AsyncGenerator<TicketsEvent>;
  /**
   * Request changes on a queued ticket: the body kept as a comment on its conversation (intent changes) and the ticket unqueued (action changes_requested), in one write; its input stays open; yields changes_requested { id, comment, entry, by, at_unix }.

APP-23: hands a queued ticket back — a comment (intent `changes`) on its conversation and the entry unqueued, one write.
   * @requires `tickets.request_changes` on the scope of `id`
   */
  requestChanges(body: Body, id: TicketRef, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /**
   * Move tickets into another shortcode, renumbered there in the order given (a scope moved into a new code becomes its root; --new-scope makes the shortcode first; a root moves with --with-children as a rename); old ids keep resolving (get says moved_from); all or none; yields reshortcoded { moves, tickets }.

ACT-6: tickets moved into another shortcode ([`crate::store::Reshortcoded`]'s module).
   * @requires `tickets.reshortcode` on the scope of `ids`
   */
  reshortcode(ids: TicketRef[], to: ShortcodeRef, message?: Message | null, newScope?: Title | null, parent?: TicketRef | null, withChildren?: boolean | null): AsyncGenerator<TicketsEvent>;
  /**
   * Restore archived tickets — each named (or a filter's archived ones) back in every view with its archived ancestors; with_children, the subtree archived with it; yields restored { ids, ancestors, still_archived, by, dry_run }.

ARCHIVE-2: the ancestor chain comes back, never its other children (ARCHIVE-1); what stays archived below is named (still_archived). dry_run: the answer, nothing written.
   * @requires `tickets.restore` on the scope of `ids`
   */
  restore(dryRun?: boolean | null, filter?: FilterText | null, ids?: TicketRef[] | null, max?: number | null, message?: Message | null, tz?: Zone | null, withChildren?: boolean | null): AsyncGenerator<TicketsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `tickets.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
  /**
   * Search inside tickets: the tickets whose title or body holds every word of the query (prefixes and typos too), best first, each with the passage that matched; optionally under a filter, paged with offset + limit.

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
[`SEARCH_MAX`]; `offset` skips that many matches.
   * @requires `tickets.search` anywhere it is held
   */
  search(query: Query, filter?: FilterText | null, limit?: number | null, offset?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /**
   * Set a ticket's kind; everything kept by kind moves with it (to a doc: status Doc; from a doc: the status given, else Epic for a scope and Pending otherwise).

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
`Entered` / `Left` it caused, as for a status change.
   * @requires `tickets.set_kind` on the scope of `id`
   */
  setKind(id: TicketRef, kind: Kind, message?: Message | null, source?: CallerSource | null, status?: Status | null): AsyncGenerator<TicketsEvent>;
  /**
   * Set a ticket's parent — the ticket it is an output of; adopts a parentless ticket or moves one.

MNG-12: files an existing ticket as an output of `parent` — the
verb AGS-19 left out. Adopts a ticket born parentless, or moves
one. Yields exactly one event: `ParentChanged` (the child, with the
parent it had before), `NotFound` if `id` names no ticket, or an
in-band `Error` when the parent does not exist or the chain would
make the ticket its own ancestor. `watch` also carries the parents'
side: `ChildrenChanged` for the new parent and `ChildRemoved` for
the old one — each the parent's id and new `child_count` (SCALE-4).
   * @requires `tickets.set_parent` on the scope of `id`
   */
  setParent(id: TicketRef, parent: TicketRef): AsyncGenerator<TicketsEvent>;
  /**
   * Set a ticket's status (not a doc's: a doc has no lifecycle); Canceled cancels its open descendants in the same write, and leaving Canceled may restore them (restore_children).

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
nothing has moved since), to the statuses they had.
   * @requires `tickets.set_status` on the scope of `id`
   */
  setStatus(id: TicketRef, status: Status, message?: Message | null, restoreChildren?: boolean | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /**
   * Snooze a queued ticket for some minutes (1 to a week): off the alerting surfaces, still queued; yields snoozed { entry, ticket }.

MENGER-39: a queued ticket off the alerting surfaces for a while; it
stays queued. Yields one `snoozed`, or an `error`.
   * @requires `tickets.snooze` on the scope of `id`
   */
  snooze(id: TicketRef, minutes: number, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /**
   * Bring deleted tickets back (admin) within 30 days of the delete, whole, children and blocks put back; yields undeleted { ids, tickets }.

ACT-5: deleted tickets brought back while their tombstones keep the
body — each child its delete moved and each block it dropped put back
where nothing has moved them since. Admin only. Yields `Undeleted {
ids, tickets }`, or one `Error` naming every refused ticket and why.
   * @requires `tickets.undelete` on the scope of `ids`
   */
  undelete(ids: TicketRef[]): AsyncGenerator<TicketsEvent>;
  /**
   * Take a ticket out of the owner's queue (done with it, or withdrawn); yields unqueued { id, entry, action, by, at_unix }, or not_queued.

MENGER-39: takes a ticket out of the queue. Yields one `unqueued`,
`not_queued` (nothing written), or an `error`.
   * @requires `tickets.unqueue` on the scope of `id`
   */
  unqueue(id: TicketRef, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /**
   * Change any part of a ticket in one write — title, body, kind, status, parent, severity, blockers (block / unblock); all lands or none does; yields updated { ticket, changed }.

MENGER-30: any part of a ticket in one write, all of it or none — the
rules, the steps and the events are [`crate::store::Update`]'s
module's. Yields exactly one event: `Updated { ticket, changed }`
(`changed` empty: nothing written or sent), `NotFound`,
`BodyTooLarge`, or one `Error` naming every refusal.
   * @requires `tickets.update` on the scope of `id`
   */
  update(id: TicketRef, block?: TicketRef[] | null, body?: Body | null, kind?: Kind | null, message?: Message | null, parent?: TicketRef | null, severity?: SeverityChange | null, source?: CallerSource | null, status?: Status | null, title?: Title | null, unblock?: TicketRef[] | null): AsyncGenerator<TicketsEvent>;
  /**
   * The same update to many tickets in one write — kind, status, parent, severity, blockers, labels; by ids or a filter; all lands or none does; yields updated_many { ids, changed, tickets, unchanged }.

ACT-2: `update`'s change to many tickets in one write, all or none —
the selection, the rules, the steps and the events are
[`crate::store::UpdatedMany`]'s module's. Yields exactly one event:
`UpdatedMany { ids, changed, tickets, unchanged }`, or one `Error`
naming every refused ticket and why.
   * @requires `tickets.update_many` on the scope of `ids`
   */
  updateMany(block?: TicketRef[] | null, filter?: FilterText | null, ids?: TicketRef[] | null, kind?: Kind | null, label?: Label[] | null, max?: number | null, message?: Message | null, parent?: TicketRef | null, severity?: SeverityChange | null, source?: CallerSource | null, status?: Status | null, tz?: Zone | null, unblock?: TicketRef[] | null, unlabel?: Label[] | null): AsyncGenerator<TicketsEvent>;
  /**
   * Subscribe to ticket changes (Created / StatusChanged / KindChanged / Amended / CommitsLinked / CommitsUnlinked / LabelsChanged / UnlocksChanged / UnlocksRemoved / BlockersChanged / SeverityChanged / ChildrenChanged / ParentChanged / ChildRemoved / GroupChanged / PlacementsChanged / ComponentChanged / ComponentRemoved / InputChanged / UpdatedMany / Deleted / Undeleted / Reshortcoded), until disconnect; Lagged { missed } where a slow subscriber lost changes; with a filter, Entered / Left / Recount as each write's last events; Entered / Left / Recount when the caller's grants move what it sees; WatchClosed { reason } when they end it.

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

SDLC-9: so is a `commits.link` or `commits.unlink` that changed
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
expired, held nowhere — it ends with `watch_closed { reason, message }`.
   * @requires `tickets.watch` anywhere it is held
   */
  watch(epic?: ShortcodeRef | null, filter?: FilterText | null, membership?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
}

/** Typed client implementation for tickets plugin */
class TicketsClientImpl implements TicketsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *amend(id: TicketRef, body?: Body | null, title?: Title | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.amend', { body, id, title });
    yield* extractData<TicketsEvent>(stream);
  }

  async *archive(dryRun?: boolean | null, filter?: FilterText | null, ids?: TicketRef[] | null, max?: number | null, message?: Message | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.archive', { dry_run: dryRun, filter, ids, max, message, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *awaitReply(id: TicketRef, input?: InputId | null, since?: number | null, timeoutMinutes?: number | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.await_reply', { id, input, since, timeout_minutes: timeoutMinutes });
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

  async *find(query: Query, after?: Cursor | null, filter?: FilterText | null, limit?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.find', { after, filter, limit, query, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *get(id: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.get', { id });
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

  async *label(add?: Label[] | null, id?: TicketRef | null, ids?: TicketRef[] | null, remove?: Label[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.label', { add, id, ids, remove });
    yield* extractData<TicketsEvent>(stream);
  }

  async *list(after?: Cursor | null, at?: number | null, epic?: ShortcodeRef | null, filter?: FilterText | null, keep?: TicketRef | null, limit?: number | null, parent?: TicketRef | null, status?: Status | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.list', { after, at, epic, filter, keep, limit, parent, status, summary, tz });
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

  async *requestChanges(body: Body, id: TicketRef, source?: CallerSource | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.request_changes', { body, id, source });
    yield* extractData<TicketsEvent>(stream);
  }

  async *reshortcode(ids: TicketRef[], to: ShortcodeRef, message?: Message | null, newScope?: Title | null, parent?: TicketRef | null, withChildren?: boolean | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.reshortcode', { ids, message, new_scope: newScope, parent, to, with_children: withChildren });
    yield* extractData<TicketsEvent>(stream);
  }

  async *restore(dryRun?: boolean | null, filter?: FilterText | null, ids?: TicketRef[] | null, max?: number | null, message?: Message | null, tz?: Zone | null, withChildren?: boolean | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.restore', { dry_run: dryRun, filter, ids, max, message, tz, with_children: withChildren });
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

  async *undelete(ids: TicketRef[]): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.undelete', { ids });
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
}

/** Create a typed tickets client from an RPC client */
export function createTicketsClient(rpc: RpcClient): TicketsClient {
  return new TicketsClientImpl(rpc);
}

/** What each tickets method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "tickets.amend": { grant: "tickets.amend", target: "id" },
  "tickets.archive": { grant: "tickets.archive", target: "ids" },
  "tickets.await_reply": { grant: "tickets.await_reply", target: null },
  "tickets.create": { grant: "tickets.create", target: null },
  "tickets.delete": { grant: "tickets.delete", target: "ids" },
  "tickets.find": { grant: "tickets.find", target: null },
  "tickets.get": { grant: "tickets.get", target: null },
  "tickets.health": { grant: "tickets.health", target: null },
  "tickets.history": { grant: "tickets.history", target: null },
  "tickets.label": { grant: "tickets.label", target: "id" },
  "tickets.list": { grant: "tickets.list", target: null },
  "tickets.queue": { grant: "tickets.queue", target: "id" },
  "tickets.queued": { grant: "tickets.queued", target: null },
  "tickets.request_changes": { grant: "tickets.request_changes", target: "id" },
  "tickets.reshortcode": { grant: "tickets.reshortcode", target: "ids" },
  "tickets.restore": { grant: "tickets.restore", target: "ids" },
  "tickets.schema": { grant: "tickets.schema", target: null },
  "tickets.search": { grant: "tickets.search", target: null },
  "tickets.set_kind": { grant: "tickets.set_kind", target: "id" },
  "tickets.set_parent": { grant: "tickets.set_parent", target: "id" },
  "tickets.set_status": { grant: "tickets.set_status", target: "id" },
  "tickets.snooze": { grant: "tickets.snooze", target: "id" },
  "tickets.undelete": { grant: "tickets.undelete", target: "ids" },
  "tickets.unqueue": { grant: "tickets.unqueue", target: "id" },
  "tickets.update": { grant: "tickets.update", target: "id" },
  "tickets.update_many": { grant: "tickets.update_many", target: "ids" },
  "tickets.watch": { grant: "tickets.watch", target: null },
} as const;

/** The tickets activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export const defaultRoles = {
  "editor": ["tickets.health", "tickets.get", "tickets.list", "tickets.find", "tickets.search", "tickets.watch", "tickets.history", "tickets.queued", "tickets.await_reply", "tickets.create", "tickets.set_status", "tickets.set_kind", "tickets.set_parent", "tickets.amend", "tickets.label", "tickets.update", "tickets.update_many", "tickets.queue", "tickets.unqueue", "tickets.snooze", "tickets.request_changes", "tickets.reshortcode", "tickets.archive", "tickets.restore"],
  "viewer": ["tickets.health", "tickets.get", "tickets.list", "tickets.find", "tickets.search", "tickets.watch", "tickets.history", "tickets.queued", "tickets.await_reply"],
} as const;