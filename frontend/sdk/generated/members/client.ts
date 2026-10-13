// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { MembersEvent } from './types';

/** Typed client interface for members plugin */
export interface MembersClient {
  /** Give a principal a role (viewer, editor, admin; deployer for a CI's key) on a shortcode or * */
  grant(principal: string, role: string, scope: string): AsyncGenerator<MembersEvent>;
  /** The grants on the scopes you administer, by principal then scope */
  list(scope?: string | null): AsyncGenerator<MembersEvent>;
  /** Who you are, then each grant you hold */
  mine(): AsyncGenerator<MembersEvent>;
  /** Who signed in without a grant, waiting for an admin */
  requests(): AsyncGenerator<MembersEvent>;
  /** Take a principal's role on a shortcode or * away */
  revoke(principal: string, scope: string): AsyncGenerator<MembersEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
}

/** Typed client implementation for members plugin */
class MembersClientImpl implements MembersClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *grant(principal: string, role: string, scope: string): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.grant', { principal, role, scope });
    yield* extractData<MembersEvent>(stream);
  }

  async *list(scope?: string | null): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.list', { scope });
    yield* extractData<MembersEvent>(stream);
  }

  async *mine(): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.mine', {});
    yield* extractData<MembersEvent>(stream);
  }

  async *requests(): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.requests', {});
    yield* extractData<MembersEvent>(stream);
  }

  async *revoke(principal: string, scope: string): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.revoke', { principal, scope });
    yield* extractData<MembersEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('members.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed members client from an RPC client */
export function createMembersClient(rpc: RpcClient): MembersClient {
  return new MembersClientImpl(rpc);
}