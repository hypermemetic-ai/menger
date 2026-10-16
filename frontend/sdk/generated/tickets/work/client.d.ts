import type { RpcClient } from '../../rpc';
import type { Kind, ShortcodeRef, TicketRef } from '../../notes/types';
import type { Cursor, FilterText, TicketsEvent, Zone } from '../../tickets/types';
/** Typed client interface for tickets.work plugin */
export interface TicketsWorkClient {
    /** One ticket's blockers resolved to current statuses; dangling ids marked; those out of your reach counted, never named.
  
  "Why can't this start?" in one call. Yields, in `blocked_by`
  order, one `Blocker` event per blocker that exists (the full
  ticket, at its current status) and one `DanglingBlocker` event per
  id that names no ticket yet; `NotFound` if the queried ticket
  itself doesn't exist (or is out of the caller's reach). An empty
  `blocked_by` yields nothing. ACCESS-17 (ACCESS-10): a blocker the
  caller may not see — and, to a reader restricted to part of the
  store, an id that names no ticket — is neither resolved nor named:
  the answer ends with one `HiddenBlockers { count }` saying how many,
  yielded only when there are any. */
    blockers(id: TicketRef): AsyncGenerator<TicketsEvent>;
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
  narrows nothing (empty, a display part alone) is no filter.
  ACCESS-17: was `tickets ready_set`. */
    ready(after?: Cursor | null, epic?: ShortcodeRef | null, filter?: FilterText | null, limit?: number | null, summary?: boolean | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
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
  `waiting_on` is its unsatisfied direct blockers in the caller's
  reach, in `blocked_by` order, distinct (one out of reach holds it
  back, never named). Order: waves ascending, inside a wave Coding, In
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
/** Create a typed tickets.work client from an RPC client */
export declare function createTicketsWorkClient(rpc: RpcClient): TicketsWorkClient;
