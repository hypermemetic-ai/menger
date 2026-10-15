// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { OrgsEvent } from './types';

/** Typed client interface for orgs plugin */
export interface OrgsClient {
  /** Make an organization (admin on *), naming its first admin */
  create(name: string, admin?: string | null): AsyncGenerator<OrgsEvent>;
  /** Delete an empty organization, and every grant on it */
  delete(name: string): AsyncGenerator<OrgsEvent>;
  /** The organizations you hold a role in */
  list(): AsyncGenerator<OrgsEvent>;
  /** Rename an organization; its old name keeps resolving */
  rename(name: string, to: string): AsyncGenerator<OrgsEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
}

/** Typed client implementation for orgs plugin */
class OrgsClientImpl implements OrgsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *create(name: string, admin?: string | null): AsyncGenerator<OrgsEvent> {
    const stream = this.rpc.call('orgs.create', { admin, name });
    yield* extractData<OrgsEvent>(stream);
  }

  async *delete(name: string): AsyncGenerator<OrgsEvent> {
    const stream = this.rpc.call('orgs.delete', { name });
    yield* extractData<OrgsEvent>(stream);
  }

  async *list(): AsyncGenerator<OrgsEvent> {
    const stream = this.rpc.call('orgs.list', {});
    yield* extractData<OrgsEvent>(stream);
  }

  async *rename(name: string, to: string): AsyncGenerator<OrgsEvent> {
    const stream = this.rpc.call('orgs.rename', { name, to });
    yield* extractData<OrgsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('orgs.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed orgs client from an RPC client */
export function createOrgsClient(rpc: RpcClient): OrgsClient {
  return new OrgsClientImpl(rpc);
}