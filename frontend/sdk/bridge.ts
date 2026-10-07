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
 * scripts/sdk-codegen.sh bundles this file (bun, IIFE) with the generated
 * client into the region between the GENERATED markers of menger-sdk.js. The
 * bundle assigns the shell's own `mengerTyped` variable — it defines no
 * global: a body's scripts cannot reach it, only the frozen `window.menger`.
 */

import type { RpcClient } from './generated/rpc';
import type { PlexusStreamItem, StreamMetadata } from './generated/types';
import { createTicketsClient, type TicketsClient } from './generated/tickets/client';

/** The shell's `menger.call`: a read over the bridge, answered with the
 *  call's items (the wire's spelling), or rejected with the reason. */
export type BridgeCall = (method: string, params: unknown) => Promise<unknown[]>;

/** What the bundle hands the shell. */
export interface Typed {
  /** The typed `tickets` client over `call`. */
  tickets(call: BridgeCall): TicketsClient;
}

/** The activation the generated client addresses (`tickets.waves`, …); the
 *  host's methods are its bare names (`waves`). */
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

/** The shell's variable (menger-sdk.js declares it just above the region). */
declare let mengerTyped: Typed | null;

mengerTyped = {
  tickets: (call: BridgeCall) => createTicketsClient(bridgeRpc(call)),
};
