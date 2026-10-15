import type { RpcClient } from '../rpc';
import type { MembersEvent, ScopeRef } from './types';
import type { Principal, Role } from '../orgs/types';
/** Typed client interface for members plugin */
export interface MembersClient {
    /** Give a principal a role (viewer, editor, admin; deployer for a CI's key) on an organization, a shortcode in one, or *.
  
  Gives `principal` `role` on `scope` (a shortcode, or `*`), replacing
  any role they held there. Needs admin on the scope, as the tree
  stands (ACCESS-11's `AdminOf`). Yields the grant as kept, or an
  in-band `error`. */
    grant(principal: Principal, role: Role, scope: ScopeRef): AsyncGenerator<MembersEvent>;
    /** The grants on the scopes you administer, by principal then scope.
  
  Every grant on the scopes the caller administers, by principal then
  scope (`--admin` ones included); `scope` narrows to one. Needs admin
  somewhere. */
    list(scope?: ScopeRef | null): AsyncGenerator<MembersEvent>;
    /** Who you are, then each grant you hold.
  
  One `me { principal, name, admin_everywhere }`, then one `grant` per
  grant the caller holds. Someone signed in without a grant gets `me`
  and nothing else. */
    mine(): AsyncGenerator<MembersEvent>;
    /** Who signed in without a grant, waiting for an admin.
  
  Everyone who signed in and holds no grant yet, oldest first: one
  `waiting { principal, name, provider, since }` each. ACCESS-19: only
  an admin of `*` — the list is every sign-in, whatever shortcode it
  came for; anyone else is refused in band. Kept in memory, at most
  [`crate::access::WAITING_MAX`]: a restart forgets them until they
  come back. */
    requests(): AsyncGenerator<MembersEvent>;
    /** Take a principal's role on a shortcode or * away.
  
  Takes `principal`'s role on `scope` away. Needs admin on the scope,
  as the tree stands. Yields `revoked { principal, scope, existed }`. */
    revoke(principal: Principal, scope: ScopeRef): AsyncGenerator<MembersEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
}
/** Create a typed members client from an RPC client */
export declare function createMembersClient(rpc: RpcClient): MembersClient;
