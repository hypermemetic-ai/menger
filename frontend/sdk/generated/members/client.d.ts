import type { RpcClient } from '../rpc';
import type { MembersEvent, ScopeRef } from './types';
import type { Principal, Role } from '../orgs/types';
/** Typed client interface for members plugin */
export interface MembersClient {
    /**
     * Give a principal a role (viewer, editor, admin; deployer for a CI's key) on an organization, a shortcode in one, or *.
  
  Gives `principal` `role` on `scope` (a shortcode, or `*`), replacing
  any role they held there. Needs admin on the scope, as the tree
  stands (ACCESS-11's `AdminOf`). Yields the grant as kept, or an
  in-band `error`.
     * @requires `members.grant` on the scope of `scope`
     */
    grant(principal: Principal, role: Role, scope: ScopeRef): AsyncGenerator<MembersEvent>;
    /**
     * The grants on the scopes you administer, by principal then scope.
  
  Every grant on the scopes the caller administers, by principal then
  scope (`--admin` ones included); `scope` narrows to one. Needs admin
  somewhere.
     * @requires `members.list` anywhere it is held
     */
    list(scope?: ScopeRef | null): AsyncGenerator<MembersEvent>;
    /**
     * Who you are, then each grant you hold.
  
  One `me { principal, name, admin_everywhere }`, then one `grant` per
  grant the caller holds. Someone signed in without a grant gets `me`
  and nothing else.
     * @requires `members.mine` anywhere it is held
     */
    mine(): AsyncGenerator<MembersEvent>;
    /**
     * Who signed in without a grant, waiting for an admin.
  
  Everyone who signed in and holds no grant yet, oldest first: one
  `waiting { principal, name, provider, since }` each. ACCESS-19: only
  an admin of `*` — the list is every sign-in, whatever shortcode it
  came for; anyone else is refused in band. Kept in memory, at most
  [`crate::access::WAITING_MAX`]: a restart forgets them until they
  come back.
     * @requires `members.requests` anywhere it is held
     */
    requests(): AsyncGenerator<MembersEvent>;
    /**
     * Take a principal's role on a shortcode or * away.
  
  Takes `principal`'s role on `scope` away. Needs admin on the scope,
  as the tree stands. Yields `revoked { principal, scope, existed }`.
     * @requires `members.revoke` on the scope of `scope`
     */
    revoke(principal: Principal, scope: ScopeRef): AsyncGenerator<MembersEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `members.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
}
/** Create a typed members client from an RPC client */
export declare function createMembersClient(rpc: RpcClient): MembersClient;
/** What each members method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "members.grant": {
        readonly grant: "members.grant";
        readonly target: "scope";
    };
    readonly "members.list": {
        readonly grant: "members.list";
        readonly target: null;
    };
    readonly "members.mine": {
        readonly grant: "members.mine";
        readonly target: null;
    };
    readonly "members.requests": {
        readonly grant: "members.requests";
        readonly target: null;
    };
    readonly "members.revoke": {
        readonly grant: "members.revoke";
        readonly target: "scope";
    };
    readonly "members.schema": {
        readonly grant: "members.schema";
        readonly target: null;
    };
};
