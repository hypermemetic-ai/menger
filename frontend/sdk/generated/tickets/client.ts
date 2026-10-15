// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { CallerSource, CommitRef, Cursor, DeleteChildren, FilterText, InputId, InputSource, InputState, InputValue, Label, MengerPath, Message, Reason, Severity, SeverityChange, Status, TicketsEvent, Urgency, Zone } from './types';
import type { Body, Kind, Query, ShortcodeRef, TicketRef, Title } from '../notes/types';
import type { OrgId } from '../orgs/types';

/** Typed client interface for tickets plugin */
export interface TicketsClient {
  /** Replace a ticket's title and/or body (whole values); yields amended { ticket, changed } */
  amend(id: TicketRef, body?: Body | null, title?: Title | null): AsyncGenerator<TicketsEvent>;
  /** Answer an input: record a value (checked against its kind and options) with a rationale, a source and the time; yields input_changed */
  answer(input: InputId, ticket: TicketRef, value: InputValue, rationale?: Message | null, source?: InputSource | null): AsyncGenerator<TicketsEvent>;
  /** One ticket's blockers resolved to current statuses; dangling ids marked */
  blockers(id: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Clear an input's answer (open again; kept in its history, with a source); yields input_changed */
  clearAnswer(input: InputId, ticket: TicketRef, source?: InputSource | null): AsyncGenerator<TicketsEvent>;
  /** Close an input: freeze its answer (or its lack of one); yields input_changed */
  closeInput(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Every component in the registry, in name order: its source, its owner (null: an orphan), what references it, its docs (summary, typed attributes, slots, examples, help; null without <docs>) and its preview attributes */
  components(): AsyncGenerator<TicketsEvent>;
  /** Each shortcode's number of tickets matching a filter, in shortcode order — the filtered extents (every ticket without one) */
  counts(filter?: FilterText | null, keep?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Create a ticket; allocates the next EPIC-N id, status starts Pending (a doc's: Doc, for good); the first ticket of a prefix must be a scope or a doc */
  create(epic: ShortcodeRef, title: Title, blockedBy?: TicketRef[] | null, body?: Body | null, kind?: Kind | null, org?: OrgId | null, parent?: TicketRef | null, path?: MengerPath | null, severity?: Severity | null): AsyncGenerator<TicketsEvent>;
  /** Delete tickets for good (admin): refused while children, blockers, a shortcode, a component, a theme or a live input point at one, unless children reparent|delete and unblock say what moves; a tombstone kept (get answers deleted), the body 30 days for undelete; yields deleted { ids, tombstones } */
  delete(children?: DeleteChildren | null, filter?: FilterText | null, ids?: TicketRef[] | null, max?: number | null, message?: Message | null, tz?: Zone | null, unblock?: boolean | null): AsyncGenerator<TicketsEvent>;
  /** Each shortcode's last sequence (a deleted ticket's too: numbers are never reused) and the holes below it, in shortcode order — where each shortcode ends in the store's order */
  extents(): AsyncGenerator<TicketsEvent>;
  /** Menu counts: per field (status, category, kind, severity, blocked, ready, has commits, menger doc, in), the matches of base and filter without filter's own groups for that field, by value */
  facets(base?: FilterText | null, filter?: FilterText | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Find tickets whose id or title contains the query (case-insensitive), optionally under a filter; summary rows, paged with after + limit */
  find(query: Query, after?: Cursor | null, filter?: FilterText | null, limit?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Get one ticket by id, or by path address SHORTCODE/path (e.g. PLEXUS/_MENGER/board) */
  get(id: TicketRef): AsyncGenerator<TicketsEvent>;
  /** One roll-up per shortcode: done/total over its subtree's work, its docs, its root (with the root's title, status and kind), and the host it nests under */
  groups(): AsyncGenerator<TicketsEvent>;
  /** Heartbeat — version, unix time, ticket count */
  health(): AsyncGenerator<TicketsEvent>;
  /** A ticket's history: each status and kind transition (from, to, when, who, source, message, cause), oldest first, then those it caused in other tickets, then its queue records (MENGER-39), then history_end (when the log began; what a restore would put back; since when it has held its status) */
  history(id: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Every input (a body's <m-decision id> / <m-input id kind>) with its kind, question, options, recommendation, state, answer and history; filtered by ticket, shortcode, state and a filter on its ticket */
  inputs(epic?: ShortcodeRef | null, filter?: FilterText | null, state?: InputState | null, ticket?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Add or remove a ticket's labels (e.g. needs-you), or many tickets' (ids; one write, all or none); yields labels_changed { ticket, added, removed } per ticket */
  label(add?: Label[] | null, id?: TicketRef | null, ids?: TicketRef[] | null, remove?: Label[] | null): AsyncGenerator<TicketsEvent>;
  /** Link commits to a ticket, or to many (ids; one write, all or none) — the commits that finished it; yields commits_linked { ticket, added } per ticket */
  linkCommits(commits: CommitRef[], id?: TicketRef | null, ids?: TicketRef[] | null): AsyncGenerator<TicketsEvent>;
  /** List tickets, optionally filtered by epic, status, parent and/or a filter; paged with after + limit, or at a position with at */
  list(after?: Cursor | null, at?: number | null, epic?: ShortcodeRef | null, filter?: FilterText | null, keep?: TicketRef | null, limit?: number | null, parent?: TicketRef | null, status?: Status | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Which of these tickets a filter admits: one member { id, matched } per id, in the order given (an unknown id: false); at most 1000 ids */
  member(filter: FilterText, ids: TicketRef[], tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Queue a ticket for the owner — urgency whenever | today | now | blocking (blocking names the input that answers it), and why; queued again it is raised, never lowered; yields queued { entry, ticket, action } */
  queue(id: TicketRef, reason: Reason, urgency: Urgency, input?: InputId | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** The owner's queue, most urgent first, then oldest: a queue_row { entry, ticket } each, then queue_end { count, snoozed, now_unix } */
  queued(snoozed?: boolean | null): AsyncGenerator<TicketsEvent>;
  /** Tickets that can start now: Ready, with every blocker Complete, Superseded or Canceled; optionally under a filter */
  readySet(after?: Cursor | null, epic?: ShortcodeRef | null, filter?: FilterText | null, limit?: number | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Reopen a closed input; yields input_changed */
  reopenInput(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Move tickets into another shortcode, renumbered there in the order given (a scope moved into a new code becomes its root; --new-scope makes the shortcode first; a root moves with --with-children as a rename); old ids keep resolving (get says moved_from); all or none; yields reshortcoded { moves, tickets } */
  reshortcode(ids: TicketRef[], to: ShortcodeRef, message?: Message | null, newScope?: Title | null, parent?: TicketRef | null, withChildren?: boolean | null): AsyncGenerator<TicketsEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /** Search inside tickets: the tickets whose title or body holds every word of the query (prefixes and typos too), best first, each with the passage that matched; optionally under a filter, paged with offset + limit */
  search(query: Query, filter?: FilterText | null, limit?: number | null, offset?: number | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's kind; everything kept by kind moves with it (to a doc: status Doc; from a doc: the status given, else Epic for a scope and Pending otherwise) */
  setKind(id: TicketRef, kind: Kind, message?: Message | null, source?: CallerSource | null, status?: Status | null): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's parent — the ticket it is an output of; adopts a parentless ticket or moves one */
  setParent(id: TicketRef, parent: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's status (not a doc's: a doc has no lifecycle); Canceled cancels its open descendants in the same write, and leaving Canceled may restore them (restore_children) */
  setStatus(id: TicketRef, status: Status, message?: Message | null, restoreChildren?: boolean | null, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** Snooze a queued ticket for some minutes (1 to a week): off the alerting surfaces, still queued; yields snoozed { entry, ticket } */
  snooze(id: TicketRef, minutes: number, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** The nav under a filter: a parent's children (or a shortcode's top level) that match or hold a match, each with matched and below (matches strictly under it), then a page; matches_only: the matches nearest below instead, then the hosted shortcodes drawn there */
  tree(filter: FilterText, after?: Cursor | null, epic?: ShortcodeRef | null, keep?: TicketRef | null, limit?: number | null, matchesOnly?: boolean | null, parent?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** Bring deleted tickets back (admin) within 30 days of the delete, whole, children and blocks put back; yields undeleted { ids, tickets } */
  undelete(ids: TicketRef[]): AsyncGenerator<TicketsEvent>;
  /** Unlink commits from a ticket; yields commits_unlinked { ticket, removed } */
  unlinkCommits(commits: CommitRef[], id: TicketRef): AsyncGenerator<TicketsEvent>;
  /** Take a ticket out of the owner's queue (done with it, or withdrawn); yields unqueued { id, entry, action, by, at_unix }, or not_queued */
  unqueue(id: TicketRef, source?: CallerSource | null): AsyncGenerator<TicketsEvent>;
  /** Change any part of a ticket in one write — title, body, kind, status, parent, severity, blockers (block / unblock); all lands or none does; yields updated { ticket, changed } */
  update(id: TicketRef, block?: TicketRef[] | null, body?: Body | null, kind?: Kind | null, message?: Message | null, parent?: TicketRef | null, severity?: SeverityChange | null, source?: CallerSource | null, status?: Status | null, title?: Title | null, unblock?: TicketRef[] | null): AsyncGenerator<TicketsEvent>;
  /** The same update to many tickets in one write — kind, status, parent, severity, blockers, labels; by ids or a filter; all lands or none does; yields updated_many { ids, changed, tickets, unchanged } */
  updateMany(block?: TicketRef[] | null, filter?: FilterText | null, ids?: TicketRef[] | null, kind?: Kind | null, label?: Label[] | null, max?: number | null, message?: Message | null, parent?: TicketRef | null, severity?: SeverityChange | null, source?: CallerSource | null, status?: Status | null, tz?: Zone | null, unblock?: TicketRef[] | null, unlabel?: Label[] | null): AsyncGenerator<TicketsEvent>;
  /** Subscribe to ticket changes (Created / StatusChanged / KindChanged / Amended / CommitsLinked / CommitsUnlinked / LabelsChanged / UnlocksChanged / UnlocksRemoved / BlockersChanged / SeverityChanged / ChildrenChanged / ParentChanged / ChildRemoved / GroupChanged / PlacementsChanged / ComponentChanged / ComponentRemoved / InputChanged / UpdatedMany / Deleted / Undeleted / Reshortcoded), until disconnect; Lagged { missed } where a slow subscriber lost changes; with a filter, Entered / Left / Recount as each write's last events */
  watch(epic?: ShortcodeRef | null, filter?: FilterText | null, membership?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /** The order of work: each ticket of the shortcodes placed in its wave over the whole blocked_by graph (or missing_blocker, or done) with what it waits on, then the wave counts */
  waves(epics: ShortcodeRef[], kinds?: Kind[] | null): AsyncGenerator<TicketsEvent>;
  /** The wave board whose final state is one ticket: its blockers transitively (and a scope's open descendants) placed in waves, done blockers done, the ticket alone in the last wave; cycles named; then the wave counts */
  wavesTo(id: TicketRef, kinds?: Kind[] | null): AsyncGenerator<TicketsEvent>;
  /** The order of work under one ticket: each ticket of its subtree placed in its wave (or missing_blocker, or done) with what it waits on, then the wave counts */
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