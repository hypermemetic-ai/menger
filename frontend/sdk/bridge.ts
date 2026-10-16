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
 * implements `RpcClient` over the bridge instead: the method's path under
 * `tickets` (`get`, `work.waves`) and the params go to the host as the shell's own
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

/** The typed `tickets` a frame gets: the generated client as generated —
 *  its children (hub-codegen ≥ 0.5.0) its own typed sub-clients,
 *  `menger.tickets.work.waves([…])`, `.nav.groups()`, `.inputs.list(…)`,
 *  `.components.list()` (ACCESS-20, SDK 2.0: no alias at the old names). */
export type FrameTickets = TicketsClient;

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

/** The activation the generated client addresses (`tickets.get`,
 *  `tickets.work.waves`, …); the host names a method by its path under it
 *  (`get`, `work.waves`), as a frame's `menger.call` does (ACCESS-20). */
const NAMESPACE = 'tickets.';

function metadata(method: string): StreamMetadata {
  return { provenance: ['menger', 'bridge', method], plexusHash: '', timestamp: Math.floor(Date.now() / 1000) };
}

/** `RpcClient` over the bridge — see the header. */
function bridgeRpc(call: BridgeCall): RpcClient {
  return {
    async *call(method: string, params?: unknown): AsyncGenerator<PlexusStreamItem> {
      const name = method.startsWith(NAMESPACE) ? method.slice(NAMESPACE.length) : method;
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

mengerTyped = {
  tickets: (call: BridgeCall) => createTicketsClient(bridgeRpc(call)),
  typed: <T>(content: unknown): Promise<T> => collectOne<T>(one(content)),
};
