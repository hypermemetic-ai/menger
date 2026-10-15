// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../../rpc';
import { extractData, collectOne } from '../../rpc';
import type { ShortcodeRef, TicketRef } from '../../notes/types';
import type { FilterText, InputId, InputState, InputValue, Message, TicketsEvent, Zone } from '../../tickets/types';

/** Typed client interface for tickets.inputs plugin */
export interface TicketsInputsClient {
  /**
   * Answer an input: record a value (checked against its kind and options) with a rationale, the time, and where it came from (said by the server); yields input_changed.

INPUT-3: answers an input — records `value` against `ticket`'s input
`input`, with its `rationale` and the time. ACCESS-14: its `source`
is the server's — `app` for a person signed in through a provider,
`api` for an agent's key and for a call that proves no one. The value
is typed by the input's kind and checked against the options its
body declares: `{"type": "choice", "option": …}` names one,
`{"type": "multi", "options": […]}` a set (kept in the body's order,
each once; empty: none ticked), `{"type": "text", "text": …}` a
reply, not blank. Every answer is a history entry — a changed one,
and the same one again. Yields exactly one event: `InputChanged` (the
input as it is now, its answer and history included), `NotFound` for
a ticket that is none, or an in-band `Error` — the ticket declares no
such input, the value does not answer it, it is closed, or it was
removed from its body — and nothing is written. Never touches the
body. `tickets.watch` carries the `InputChanged`.
   * @requires `inputs.answer` on the scope of `ticket`
   */
  answer(input: InputId, ticket: TicketRef, value: InputValue, rationale?: Message | null): AsyncGenerator<TicketsEvent>;
  /**
   * Clear an input's answer (open again; kept in its history, with where it came from, said by the server); yields input_changed.

INPUT-3: clears an input's answer — `open` again, the clear in its
history, with its source (ACCESS-14: the server's, as for `answer`).
Yields exactly one event: `InputChanged`; `Input` when it held no
answer (nothing written); `NotFound`; or an in-band `Error` — no
such input, closed, removed.
   * @requires `inputs.clear` on the scope of `ticket`
   */
  clear(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent>;
  /**
   * Close an input: freeze its answer (or its lack of one); yields input_changed.

INPUT-3: closes an input — frozen (a decision's DECIDED), with its
answer or without one; an answer or a clear is refused until it is
reopened. Yields exactly one event: `InputChanged`; `Input` when it
was closed already; `NotFound`; or an in-band `Error` — no such
input, removed.
   * @requires `inputs.close` on the scope of `ticket`
   */
  close(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent>;
  /**
   * Every input (a body's <m-decision id> / <m-input id kind>) with its kind, question, options, recommendation, state, answer and history; filtered by ticket, shortcode, state and a filter on its ticket.

INPUT-3: the inputs store. Yields one `Input` per input of `ticket`
(every ticket's, without one) whose ticket is in the shortcode `epic`
and which is in `state` — its ticket, id, kind (`choice`, `multi`,
`text`), question, options (`{ value, label, recommended }`, the
body's order), recommendation, state (`open`, `answered`, `closed`,
`removed`), current answer (`{ value, rationale, source, at_unix }`,
`null` with none) and whole history, oldest first — in the store's
order of tickets, each ticket's inputs in its body's order and the
removed after. Nothing trails them; the stream's own `done` is the
whole answer. `NotFound` when `ticket` names no ticket. From the
cache; `tickets.watch` keeps a client's copy current with
`InputChanged`.

FILTER-15: `filter` (the language's text, FILTER-10) keeps the inputs
whose ticket the filter admits — each such ticket one lookup in the
filter's admitted cells; every other parameter as without it. A
filter not taken, or one that asks `under` or `text`, is one in-band
`error`; one that narrows nothing (empty, a display part alone) is no
filter.
   * @requires `inputs.list` anywhere it is held
   */
  list(epic?: ShortcodeRef | null, filter?: FilterText | null, state?: InputState | null, ticket?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent>;
  /**
   * Reopen a closed input; yields input_changed.

INPUT-3: reopens a closed input — `answered` again when it holds an
answer, else `open`. Yields exactly one event: `InputChanged`;
`Input` when it was not closed; `NotFound`; or an in-band `Error` —
no such input, removed.
   * @requires `inputs.reopen` on the scope of `ticket`
   */
  reopen(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `inputs.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
}

/** Typed client implementation for tickets.inputs plugin */
class TicketsInputsClientImpl implements TicketsInputsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *answer(input: InputId, ticket: TicketRef, value: InputValue, rationale?: Message | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.inputs.answer', { input, rationale, ticket, value });
    yield* extractData<TicketsEvent>(stream);
  }

  async *clear(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.inputs.clear', { input, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async *close(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.inputs.close', { input, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async *list(epic?: ShortcodeRef | null, filter?: FilterText | null, state?: InputState | null, ticket?: TicketRef | null, tz?: Zone | null): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.inputs.list', { epic, filter, state, ticket, tz });
    yield* extractData<TicketsEvent>(stream);
  }

  async *reopen(input: InputId, ticket: TicketRef): AsyncGenerator<TicketsEvent> {
    const stream = this.rpc.call('tickets.inputs.reopen', { input, ticket });
    yield* extractData<TicketsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('tickets.inputs.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed tickets.inputs client from an RPC client */
export function createTicketsInputsClient(rpc: RpcClient): TicketsInputsClient {
  return new TicketsInputsClientImpl(rpc);
}

/** What each tickets.inputs method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "inputs.answer": { grant: "inputs.answer", target: "ticket" },
  "inputs.clear": { grant: "inputs.clear", target: "ticket" },
  "inputs.close": { grant: "inputs.close", target: "ticket" },
  "inputs.list": { grant: "inputs.list", target: null },
  "inputs.reopen": { grant: "inputs.reopen", target: "ticket" },
  "inputs.schema": { grant: "inputs.schema", target: null },
} as const;

/** The tickets.inputs activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export const defaultRoles = {
  "editor": ["inputs.list", "inputs.answer", "inputs.clear", "inputs.close", "inputs.reopen"],
  "viewer": ["inputs.list"],
} as const;