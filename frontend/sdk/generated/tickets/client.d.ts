import type { RpcClient } from '../rpc';
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
    counts(filter?: string | null): AsyncGenerator<TicketsEvent>;
    /** Create a ticket; allocates the next EPIC-N id, status starts Pending (a doc's: Doc, for good); the first ticket of a prefix must be a scope or a doc */
    create(epic: string, title: string, blockedBy?: string[] | null, body?: string | null, kind?: string | null, parent?: string | null, path?: string | null, severity?: string | null): AsyncGenerator<TicketsEvent>;
    /** Each shortcode's last sequence, in shortcode order — where each shortcode ends in the store's order */
    extents(): AsyncGenerator<TicketsEvent>;
    /** Menu counts: per field (status, category, kind, severity, blocked, ready, has commits, in), the matches of base and filter without filter's own groups for that field, by value */
    facets(base?: string | null, filter?: string | null): AsyncGenerator<TicketsEvent>;
    /** Find tickets whose id or title contains the query (case-insensitive); summary rows, paged with after + limit */
    find(query: string, after?: string | null, limit?: number | null): AsyncGenerator<TicketsEvent>;
    /** Get one ticket by id, or by path address SHORTCODE/path (e.g. PLEXUS/_MENGER/board) */
    get(id: string): AsyncGenerator<TicketsEvent>;
    /** One roll-up per shortcode: done/total over its subtree's work, its docs, its root (with the root's title, status and kind), and the host it nests under */
    groups(): AsyncGenerator<TicketsEvent>;
    /** Heartbeat — version, unix time, ticket count */
    health(): AsyncGenerator<TicketsEvent>;
    /** Every input (a body's <m-decision id> / <m-input id kind>) with its kind, question, options, recommendation, state, answer and history; filtered by ticket, shortcode and state */
    inputs(epic?: string | null, state?: string | null, ticket?: string | null): AsyncGenerator<TicketsEvent>;
    /** Link commits to a ticket (the commits that finished it); yields commits_linked { ticket, added } */
    linkCommits(commits: string[], id: string): AsyncGenerator<TicketsEvent>;
    /** List tickets, optionally filtered by epic, status, parent and/or a filter; paged with after + limit, or at a position with at */
    list(after?: string | null, at?: number | null, epic?: string | null, filter?: string | null, limit?: number | null, parent?: string | null, status?: string | null, summary?: boolean | null): AsyncGenerator<TicketsEvent>;
    /** Tickets that can start now: Ready, with every blocker Complete or Superseded */
    readySet(after?: string | null, epic?: string | null, limit?: number | null, summary?: boolean | null): AsyncGenerator<TicketsEvent>;
    /** Reopen a closed input; yields input_changed */
    reopenInput(input: string, ticket: string): AsyncGenerator<TicketsEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Set a ticket's parent — the ticket it is an output of; adopts a parentless ticket or moves one */
    setParent(id: string, parent: string): AsyncGenerator<TicketsEvent>;
    /** Set a ticket's status (not a doc's: a doc has no lifecycle) */
    setStatus(id: string, status: string): AsyncGenerator<TicketsEvent>;
    /** Unlink commits from a ticket; yields commits_unlinked { ticket, removed } */
    unlinkCommits(commits: string[], id: string): AsyncGenerator<TicketsEvent>;
    /** Subscribe to ticket changes (Created / StatusChanged / Amended / CommitsLinked / CommitsUnlinked / UnlocksChanged / ChildrenChanged / ParentChanged / ChildRemoved / GroupChanged / PlacementsChanged / ComponentChanged / ComponentRemoved / InputChanged), until disconnect; Lagged { missed } where a slow subscriber lost changes; with a filter, Entered / Left / Recount as each write's last events */
    watch(epic?: string | null, filter?: string | null, membership?: boolean | null): AsyncGenerator<TicketsEvent>;
    /** The order of work: each ticket of the shortcodes placed in its wave over the whole blocked_by graph (or missing_blocker, or done) with what it waits on, then the wave counts */
    waves(epics: string[], kinds?: string[] | null): AsyncGenerator<TicketsEvent>;
}
/** Create a typed tickets client from an RPC client */
export declare function createTicketsClient(rpc: RpcClient): TicketsClient;
