// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { SessionEvent } from './types';

/** Typed client interface for session plugin */
export interface SessionClient {
  /** The providers this menger admits callers through: provider { name, kind, issuer, audience } */
  providers(): AsyncGenerator<SessionEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /** Who this connection is: identity { principal, name, roles, tenant, provider }, or anonymous */
  whoami(): AsyncGenerator<SessionEvent>;
}

/** Typed client implementation for session plugin */
class SessionClientImpl implements SessionClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *providers(): AsyncGenerator<SessionEvent> {
    const stream = this.rpc.call('session.providers', {});
    yield* extractData<SessionEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('session.schema', {});
    return collectOne<unknown>(stream);
  }

  async *whoami(): AsyncGenerator<SessionEvent> {
    const stream = this.rpc.call('session.whoami', {});
    yield* extractData<SessionEvent>(stream);
  }
}

/** Create a typed session client from an RPC client */
export function createSessionClient(rpc: RpcClient): SessionClient {
  return new SessionClientImpl(rpc);
}