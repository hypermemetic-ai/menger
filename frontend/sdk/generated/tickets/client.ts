// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { InputValue, TicketsEvent } from './types';

/** Typed client interface for tickets plugin */
export interface TicketsClient {
  /** Replace a ticket's title and/or body (whole values); yields amended { ticket, changed } */
  amend(id: string, body?: string | null, title?: string | null): AsyncGenerator<TicketsEvent>;
  /** Answer an input: record a value (checked against its kind and options) with a rationale, a source and the time; yields input_changed */
  answer(input: string, ticket: string, value: InputValue, rationale?: string | null, source?: string | null): AsyncGenerator<TicketsEvent>;
  /** One ticket's blockers resolved to current statuses; dangling ids marked */
  blockers(id: string): AsyncGenerator<TicketsEvent>;
  /** Clear an input's answer (open again; kept in its history, with a source); yields input_changed */
  clearAnswer(input: string, ticket: string, source?: string | null): AsyncGenerator<TicketsEvent>;
  /** Close an input: freeze its answer (or its lack of one); yields input_changed */
  closeInput(input: string, ticket: string): AsyncGenerator<TicketsEvent>;
  /** Every component in the registry, in name order: its source, its owner (null: an orphan), what references it, its docs (summary, typed attributes, slots, examples, help; null without <docs>) and its preview attributes */
  components(): AsyncGenerator<TicketsEvent>;
  /** Each shortcode's number of tickets matching a filter, in shortcode order — the filtered extents (every ticket without one) */
  counts(filter?: string | null, keep?: string | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Create a ticket; allocates the next EPIC-N id, status starts Pending (a doc's: Doc, for good); the first ticket of a prefix must be a scope or a doc */
  create(epic: string, title: string, blockedBy?: string[] | null, body?: string | null, kind?: string | null, parent?: string | null, path?: string | null, severity?: string | null): AsyncGenerator<TicketsEvent>;
  /** Each shortcode's last sequence, in shortcode order — where each shortcode ends in the store's order */
  extents(): AsyncGenerator<TicketsEvent>;
  /** Menu counts: per field (status, category, kind, severity, blocked, ready, has commits, menger doc, in), the matches of base and filter without filter's own groups for that field, by value */
  facets(base?: string | null, filter?: string | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Find tickets whose id or title contains the query (case-insensitive), optionally under a filter; summary rows, paged with after + limit */
  find(query: string, after?: string | null, filter?: string | null, limit?: number | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Get one ticket by id, or by path address SHORTCODE/path (e.g. PLEXUS/_MENGER/board) */
  get(id: string): AsyncGenerator<TicketsEvent>;
  /** One roll-up per shortcode: done/total over its subtree's work, its docs, its root (with the root's title, status and kind), and the host it nests under */
  groups(): AsyncGenerator<TicketsEvent>;
  /** Heartbeat — version, unix time, ticket count */
  health(): AsyncGenerator<TicketsEvent>;
  /** A ticket's history: each status and kind transition (from, to, when, who, source, message, cause), oldest first, then those it caused in other tickets, then history_end (when the log began; what a restore would put back; since when it has held its status) */
  history(id: string): AsyncGenerator<TicketsEvent>;
  /** Every input (a body's <m-decision id> / <m-input id kind>) with its kind, question, options, recommendation, state, answer and history; filtered by ticket, shortcode, state and a filter on its ticket */
  inputs(epic?: string | null, filter?: string | null, state?: string | null, ticket?: string | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Add or remove a ticket's labels (e.g. needs-you); yields labels_changed { ticket, added, removed } */
  label(id: string, add?: string[] | null, remove?: string[] | null): AsyncGenerator<TicketsEvent>;
  /** Link commits to a ticket (the commits that finished it); yields commits_linked { ticket, added } */
  linkCommits(commits: string[], id: string): AsyncGenerator<TicketsEvent>;
  /** List tickets, optionally filtered by epic, status, parent and/or a filter; paged with after + limit, or at a position with at */
  list(after?: string | null, at?: number | null, epic?: string | null, filter?: string | null, keep?: string | null, limit?: number | null, parent?: string | null, status?: string | null, summary?: boolean | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Which of these tickets a filter admits: one member { id, matched } per id, in the order given (an unknown id: false); at most 1000 ids */
  member(filter: string, ids: string[], tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Tickets that can start now: Ready, with every blocker Complete, Superseded or Canceled; optionally under a filter */
  readySet(after?: string | null, epic?: string | null, filter?: string | null, limit?: number | null, summary?: boolean | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Reopen a closed input; yields input_changed */
  reopenInput(input: string, ticket: string): AsyncGenerator<TicketsEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /** Search inside tickets: the tickets whose title or body holds every word of the query (prefixes and typos too), best first, each with the passage that matched; optionally under a filter, paged with offset + limit */
  search(query: string, filter?: string | null, limit?: number | null, offset?: number | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's kind; everything kept by kind moves with it (to a doc: status Doc; from a doc: the status given, else Epic for a scope and Pending otherwise) */
  setKind(id: string, kind: string, message?: string | null, source?: string | null, status?: string | null): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's parent — the ticket it is an output of; adopts a parentless ticket or moves one */
  setParent(id: string, parent: string): AsyncGenerator<TicketsEvent>;
  /** Set a ticket's status (not a doc's: a doc has no lifecycle); Canceled cancels its open descendants in the same write, and leaving Canceled may restore them (restore_children) */
  setStatus(id: string, status: string, message?: string | null, restoreChildren?: boolean | null, source?: string | null): AsyncGenerator<TicketsEvent>;
  /** The nav under a filter: a parent's children (or a shortcode's top level) that match or hold a match, each with matched and below (matches strictly under it), then a page; matches_only: the matches nearest below instead, then the hosted shortcodes drawn there */
  tree(filter: string, after?: string | null, epic?: string | null, keep?: string | null, limit?: number | null, matchesOnly?: boolean | null, parent?: string | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** Unlink commits from a ticket; yields commits_unlinked { ticket, removed } */
  unlinkCommits(commits: string[], id: string): AsyncGenerator<TicketsEvent>;
  /** Change any part of a ticket in one write — title, body, kind, status, parent, severity, blockers (block / unblock); all lands or none does; yields updated { ticket, changed } */
  update(id: string, block?: string[] | null, body?: string | null, kind?: string | null, message?: string | null, parent?: string | null, severity?: string | null, source?: string | null, status?: string | null, title?: string | null, unblock?: string[] | null): AsyncGenerator<TicketsEvent>;
  /** Subscribe to ticket changes (Created / StatusChanged / KindChanged / Amended / CommitsLinked / CommitsUnlinked / LabelsChanged / UnlocksChanged / UnlocksRemoved / BlockersChanged / SeverityChanged / ChildrenChanged / ParentChanged / ChildRemoved / GroupChanged / PlacementsChanged / ComponentChanged / ComponentRemoved / InputChanged), until disconnect; Lagged { missed } where a slow subscriber lost changes; with a filter, Entered / Left / Recount as each write's last events */
  watch(epic?: string | null, filter?: string | null, membership?: boolean | null, tz?: string | null): AsyncGenerator<TicketsEvent>;
  /** The order of work: each ticket of the shortcodes placed in its wave over the whole blocked_by graph (or missing_blocker, or done) with what it waits on, then the wave counts */
  waves(epics: string[], kinds?: string[] | null): AsyncGenerator<TicketsEvent>;
  /** The wave board whose final state is one ticket: its blockers transitively (and a scope's open descendants) placed in waves, done blockers done, the ticket alone in the last wave; cycles named; then the wave counts */
  wavesTo(id: string, kinds?: string[] | null): AsyncGenerator<TicketsEvent>;
  /** The order of work under one ticket: each ticket of its subtree placed in its wave (or missing_blocker, or done) with what it waits on, then the wave counts */
  wavesUnder(id: string, kinds?: string[] | null): AsyncGenerator<TicketsEvent>;
}

/** Typed client implementation for tickets plugin */
class TicketsClientImpl implements TicketsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *amend(id: string, body?: string | null, title?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.amend', { body, id, title });
    yield* extractData<TicketsEvent>(stream);
  }

  async *answer(input: string, ticket: string, value: InputValue, rationale?: string | null, source?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.answer', { input, rationale, source, ticket, value });
    yield* extractData<TicketsEvent>(stream);
  }

  async *blockers(id: string): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.blockers', { id });
    yield* extractData<TicketsEvent>(stream);
  }

  async *clearAnswer(input: string, ticket: string, source?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.clear_answer', { input, source, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async *closeInput(input: string, ticket: string): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.close_input', { input, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async *components(): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.components', {});
    yield* extractData<TicketsEvent>(stream);
  }

  async *counts(filter?: string | null, keep?: string | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.counts', { filter, keep, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *create(epic: string, title: string, blockedBy?: string[] | null, body?: string | null, kind?: string | null, parent?: string | null, path?: string | null, severity?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.create', { blocked_by: blockedBy, body, epic, kind, parent, path, severity, title });
    yield* extractData<TicketsEvent>(stream);
  }

  async *extents(): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.extents', {});
    yield* extractData<TicketsEvent>(stream);
  }

  async *facets(base?: string | null, filter?: string | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.facets', { base, filter, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *find(query: string, after?: string | null, filter?: string | null, limit?: number | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.find', { after, filter, limit, query, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *get(id: string): AsyncGenerator<TicketsEvent> {
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

  async *history(id: string): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.history', { id });
    yield* extractData<TicketsEvent>(stream);
  }

  async *inputs(epic?: string | null, filter?: string | null, state?: string | null, ticket?: string | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.inputs', { epic, filter, state, ticket, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *label(id: string, add?: string[] | null, remove?: string[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.label', { add, id, remove });
    yield* extractData<TicketsEvent>(stream);
  }

  async *linkCommits(commits: string[], id: string): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.link_commits', { commits, id });
    yield* extractData<TicketsEvent>(stream);
  }

  async *list(after?: string | null, at?: number | null, epic?: string | null, filter?: string | null, keep?: string | null, limit?: number | null, parent?: string | null, status?: string | null, summary?: boolean | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.list', { after, at, epic, filter, keep, limit, parent, status, summary, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *member(filter: string, ids: string[], tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.member', { filter, ids, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *readySet(after?: string | null, epic?: string | null, filter?: string | null, limit?: number | null, summary?: boolean | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.ready_set', { after, epic, filter, limit, summary, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *reopenInput(input: string, ticket: string): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.reopen_input', { input, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('tickets.schema', {});
    return collectOne<unknown>(stream);
  }

  async *search(query: string, filter?: string | null, limit?: number | null, offset?: number | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.search', { filter, limit, offset, query, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *setKind(id: string, kind: string, message?: string | null, source?: string | null, status?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.set_kind', { id, kind, message, source, status });
    yield* extractData<TicketsEvent>(stream);
  }

  async *setParent(id: string, parent: string): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.set_parent', { id, parent });
    yield* extractData<TicketsEvent>(stream);
  }

  async *setStatus(id: string, status: string, message?: string | null, restoreChildren?: boolean | null, source?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.set_status', { id, message, restore_children: restoreChildren, source, status });
    yield* extractData<TicketsEvent>(stream);
  }

  async *tree(filter: string, after?: string | null, epic?: string | null, keep?: string | null, limit?: number | null, matchesOnly?: boolean | null, parent?: string | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.tree', { after, epic, filter, keep, limit, matches_only: matchesOnly, parent, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *unlinkCommits(commits: string[], id: string): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.unlink_commits', { commits, id });
    yield* extractData<TicketsEvent>(stream);
  }

  async *update(id: string, block?: string[] | null, body?: string | null, kind?: string | null, message?: string | null, parent?: string | null, severity?: string | null, source?: string | null, status?: string | null, title?: string | null, unblock?: string[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.update', { block, body, id, kind, message, parent, severity, source, status, title, unblock });
    yield* extractData<TicketsEvent>(stream);
  }

  async *watch(epic?: string | null, filter?: string | null, membership?: boolean | null, tz?: string | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.watch', { epic, filter, membership, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *waves(epics: string[], kinds?: string[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.waves', { epics, kinds });
    yield* extractData<TicketsEvent>(stream);
  }

  async *wavesTo(id: string, kinds?: string[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.waves_to', { id, kinds });
    yield* extractData<TicketsEvent>(stream);
  }

  async *wavesUnder(id: string, kinds?: string[] | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.waves_under', { id, kinds });
    yield* extractData<TicketsEvent>(stream);
  }
}

/** Create a typed tickets client from an RPC client */
export function createTicketsClient(rpc: RpcClient): TicketsClient {
  return new TicketsClientImpl(rpc);
}