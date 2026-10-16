/* COMP-10: the bridge transport — the generated client's transport over the
 * frame's postMessage channel, and the entry point of the SDK's generated
 * region.
 *
 * `menger.tickets` is `createTicketsClient` from the client hub-codegen
 * generates from the `tickets` activation's schema (./generated/, written by
 * scripts/sdk-codegen.sh — never edited by hand). That client's only seam is
 * Layer 1, `RpcClient` (./generated/rpc.ts): `call(method, params)` → an
 * `AsyncGenerator<PlexusStreamItem>`. A frame cannot open the WebSocket the
 * generated `PlexusRpcClient` would (its origin is opaque), so this file
 * implements `RpcClient` over the bridge instead: the method's name without
 * its namespace and the params go to the host as the shell's own
 * `menger.call` sends them (`menger:call`), and the host's answer comes back
 * as the stream items the generated client unwraps —
 *
 *   menger:result { items }  →  { type: "data", content } per item, then { type: "done" }
 *   menger:result { error }  →  { type: "error", message }   (the client throws a PlexusError)
 *
 * So the host's read-only allow-list (frontend/src/bridge.rs) is the only
 * gate: a read is answered, a write (`setStatus`, `amend`, …) rejects with
 * "read-only: …", and nothing here knows which is which.
 *
 * INPUT-4: and it hands the shell `typed` — a value the host sent in the
 * wire's spelling, in the typed client's (`at_unix` → `atUnix`), by the
 * generated runtime's own transform (`collectOne`), so what
 * `menger.input.submit` resolves to is the generated `Answer`, spelled as
 * `menger.tickets.inputs.list()` spells an input's answer.
 *
 * scripts/sdk-codegen.sh bundles this file (bun, IIFE) with the generated
 * client into the region between the GENERATED markers of menger-sdk.js. The
 * bundle assigns the shell's own `mengerTyped` variable — it defines no
 * global: a body's scripts cannot reach it, only the frozen `window.menger`.
 */

import { collectOne, type RpcClient } from './generated/rpc';
import type { PlexusStreamItem, StreamMetadata } from './generated/types';
import { createTicketsClient, type TicketsClient } from './generated/tickets/client';
import { createTicketsInputsClient, type TicketsInputsClient } from './generated/tickets/inputs/client';
import { createTicketsWorkClient, type TicketsWorkClient } from './generated/tickets/work/client';
import { createTicketsNavClient, type TicketsNavClient } from './generated/tickets/nav/client';
import { createTicketsComponentsClient, type TicketsComponentsClient } from './generated/tickets/components/client';

/** ACCESS-14: `menger.tickets.inputs` — the `tickets inputs` child's client
 *  (`list`, and the writes, which the host refuses), and callable as
 *  `list` itself, as `menger.tickets.inputs(epic?, filter?, state?,
 *  ticket?, tz?)` was before the move (the same parameters, the same
 *  order). */
export type InputsChild = TicketsInputsClient & TicketsInputsClient['list'];

/** ACCESS-18: `menger.tickets.components` — the `tickets components`
 *  child's client (`list`), and callable as `list` itself, as
 *  `menger.tickets.components()` was before the move. */
export type ComponentsChild = TicketsComponentsClient & TicketsComponentsClient['list'];

/** The typed `tickets` a frame gets: the generated client, its `inputs`
 *  the child's ([`InputsChild`]) — hub-codegen gives a `#[child]` accessor
 *  no client of its own on the parent — and (ACCESS-17) `tickets work` as
 *  `work`, with that child's `waves` and `blockers` kept where a body has
 *  always called them (`menger.tickets.waves([…])`) until ACCESS-20 moves
 *  the frames' SDK — and (ACCESS-16) its child `nav` (`tickets.nav.groups`, …)
 *  — and (ACCESS-18) its `components` the child's ([`ComponentsChild`]). */
export type FrameTickets = Omit<TicketsClient, 'inputs' | 'work' | 'nav' | 'components'> &
  Pick<TicketsWorkClient, 'waves' | 'blockers'> & {
    readonly inputs: InputsChild;
    readonly work: TicketsWorkClient;
    readonly nav: TicketsNavClient;
    readonly components: ComponentsChild;
  };

/** The shell's `menger.call`: a read over the bridge, answered with the
 *  call's items (the wire's spelling), or rejected with the reason. */
export type BridgeCall = (method: string, params: unknown) => Promise<unknown[]>;

/** What the bundle hands the shell. */
export interface Typed {
  /** The typed `tickets` client over `call`. */
  tickets(call: BridgeCall): FrameTickets;
  /** INPUT-4: `content` (the wire's spelling) in the typed client's — the
   *  generated runtime's key transform, as every typed event gets it. */
  typed<T>(content: unknown): Promise<T>;
}

/** The activation the generated client addresses (`tickets.get`, …); the
 *  host's methods are its bare names (`get`). ACCESS-16: its child's too
 *  (`tickets.nav.groups` → `groups`), as the host's `menger.call` names
 *  them. */
const NAMESPACE = 'tickets.';
const NAV = 'tickets.nav.';

/** ACCESS-14 / ACCESS-17: a child's method by the host's name for it — the
 *  frame's read of inputs is still the host's `inputs`, and `work`'s `waves`
 *  and `blockers` the host's `waves` / `blockers` (its `menger.call` names
 *  move with every client in ACCESS-20); any other child method goes as
 *  `<child>.<method>`, which the host refuses. */
const HOST_NAMES: Record<string, string> = {
  'inputs.list': 'inputs',
  'work.waves': 'waves',
  'work.blockers': 'blockers',
  // ACCESS-18: the registry is still the host's `components`.
  'components.list': 'components',
};

function metadata(method: string): StreamMetadata {
  return { provenance: ['menger', 'bridge', method], plexusHash: '', timestamp: Math.floor(Date.now() / 1000) };
}

/** `RpcClient` over the bridge — see the header. */
function bridgeRpc(call: BridgeCall): RpcClient {
  return {
    async *call(method: string, params?: unknown): AsyncGenerator<PlexusStreamItem> {
      const bare = method.startsWith(NAMESPACE) ? method.slice(NAMESPACE.length) : method;
      const name = HOST_NAMES[bare] ?? (method.startsWith(NAV) ? method.slice(NAV.length) : bare);
      let items: unknown[];
      try {
        items = await call(name, params ?? null);
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        yield { type: 'error', metadata: metadata(method), message, recoverable: false };
        return;
      }
      for (const content of items) {
        yield { type: 'data', metadata: metadata(method), contentType: method, content };
      }
      yield { type: 'done', metadata: metadata(method) };
    },
  };
}

/** One data item, as the generated runtime reads a stream. */
async function* one(content: unknown): AsyncGenerator<PlexusStreamItem> {
  yield { type: 'data', metadata: metadata('typed'), contentType: 'typed', content };
}

/** The shell's variable (menger-sdk.js declares it just above the region). */
declare let mengerTyped: Typed | null;

/** ACCESS-14: the generated client with `inputs` the child's ([`InputsChild`]);
 *  ACCESS-17: and `work`; ACCESS-16: and `nav`. */
function frameTickets(rpc: RpcClient): FrameTickets {
  const tickets = createTicketsClient(rpc);
  const child = createTicketsInputsClient(rpc);
  const inputs: InputsChild = Object.assign(child.list.bind(child), {
    list: child.list.bind(child),
    answer: child.answer.bind(child),
    clear: child.clear.bind(child),
    close: child.close.bind(child),
    reopen: child.reopen.bind(child),
    schema: child.schema.bind(child),
  });
  Object.defineProperty(tickets, 'inputs', { value: inputs });
  // ACCESS-17: `tickets work`, and its `waves` / `blockers` where they were.
  const work = createTicketsWorkClient(rpc);
  Object.assign(tickets, { work, waves: work.waves.bind(work), blockers: work.blockers.bind(work) });
  // ACCESS-16: `tickets nav`.
  Object.defineProperty(tickets, 'nav', { value: createTicketsNavClient(rpc) });
  // ACCESS-18: `tickets components`, callable as `list`.
  const registry = createTicketsComponentsClient(rpc);
  const components: ComponentsChild = Object.assign(registry.list.bind(registry), {
    list: registry.list.bind(registry),
    schema: registry.schema.bind(registry),
  });
  Object.defineProperty(tickets, 'components', { value: components });
  return tickets as unknown as FrameTickets;
}

mengerTyped = {
  tickets: (call: BridgeCall) => frameTickets(bridgeRpc(call)),
  typed: <T>(content: unknown): Promise<T> => collectOne<T>(one(content)),
};
