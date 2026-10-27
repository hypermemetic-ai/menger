// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../../rpc';
import { extractData, collectOne } from '../../rpc';
import type { MembersEvent } from '../../members/types';
import type { Principal } from '../../orgs/types';

/** Typed client interface for members.profiles plugin */
export interface MembersProfilesClient {
  /**
   * One principal's profile. Admin of * only.

One `profile { profile }`, or an in-band `error` when the server
has never seen it sign in.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `profiles.get` anywhere it is held
   */
  get(principal: Principal): AsyncGenerator<MembersEvent>;
  /**
   * Everyone who signed in, newest sign-in first; one profile each, with holds_grant (false: waiting). Admin of * only.

One `profile { profile }` per principal the server has seen sign
in: `provider`, `method` (`githuboauth`, `email_code`, `key`, …),
`email`, `name`, `picture`, `signed_in_at`, `first_seen`, and
`holds_grant` — whether it holds a grant of its own. Anyone but an
admin of `*` is refused in band.
   * @requires `profiles.list` anywhere it is held
   */
  list(): AsyncGenerator<MembersEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `profiles.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
}

/** Typed client implementation for members.profiles plugin */
class MembersProfilesClientImpl implements MembersProfilesClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *get(principal: Principal): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.profiles.get', { principal });
    yield* extractData<MembersEvent>(stream);
  }

  async *list(): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.profiles.list', {});
    yield* extractData<MembersEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('members.profiles.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed members.profiles client from an RPC client */
export function createMembersProfilesClient(rpc: RpcClient): MembersProfilesClient {
  return new MembersProfilesClientImpl(rpc);
}

/** What each members.profiles method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "profiles.get": { grant: "profiles.get", target: null },
  "profiles.list": { grant: "profiles.list", target: null },
  "profiles.schema": { grant: "profiles.schema", target: null },
} as const;