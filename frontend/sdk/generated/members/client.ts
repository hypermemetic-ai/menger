// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { MembersEvent, ScopeRef } from './types';
import type { Principal, Role } from '../orgs/types';

/** Typed client interface for members plugin */
export interface MembersClient {
  /** Give a principal a role (viewer, editor, admin; deployer for a CI's key) on an organization, a shortcode in one, or * */
  grant(principal: Principal, role: Role, scope: ScopeRef): AsyncGenerator<MembersEvent>;
  /** The grants on the scopes you administer, by principal then scope */
  list(scope?: ScopeRef | null): AsyncGenerator<MembersEvent>;
  /** Who you are, then each grant you hold */
  mine(): AsyncGenerator<MembersEvent>;
  /** Who signed in without a grant, waiting for an admin */
  requests(): AsyncGenerator<MembersEvent>;
  /** Take a principal's role on a shortcode or * away */
  revoke(principal: Principal, scope: ScopeRef): AsyncGenerator<MembersEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
}

/** Typed client implementation for members plugin */
class MembersClientImpl implements MembersClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *grant(principal: Principal, role: Role, scope: ScopeRef): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.grant', { principal, role, scope });
    yield* extractData<MembersEvent>(stream);
  }

  async *list(scope?: ScopeRef | null): AsyncGenerator<MembersEvent> {
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

  async *revoke(principal: Principal, scope: ScopeRef): AsyncGenerator<MembersEvent> {
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