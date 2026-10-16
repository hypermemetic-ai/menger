import type { RpcClient } from '../rpc';
import type { MembersEvent, ScopeRef, TeamName } from './types';
import type { Principal, Role } from '../orgs/types';
import type { MembersTeamsClient } from '../members/teams/client';
/** Typed client interface for members plugin */
export interface MembersClient {
    /** Teams — a named set of principals that holds grants (ACCESS-22): `members.teams.*`. */
    readonly teams: MembersTeamsClient;
    /**
     * Give a principal or a team a role (viewer, editor, admin; deployer for a CI's key) on an organization, a shortcode in one, a ticket, a ticket's subtree, or *.
  
  Gives `principal` `role` on `scope` (a shortcode, `*`; ACCESS-21: a
  ticket alone, `MENGER-12`, or a ticket and everything under it,
  `under MENGER-12`, kept by the ticket's key so the grant follows it),
  replacing any role they held there. ACCESS-22: to `team` instead of
  `principal` (exactly one of the two), its members hold it. Needs
  admin on the scope, as the tree stands (ACCESS-11's `AdminOf`: for a
  ticket's scope, admin on a scope that covers it). Yields the grant as
  kept, or an in-band `error`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `members.grant` on the scope of `scope`
     */
    grant(role: Role, scope: ScopeRef, principal?: Principal | null, team?: TeamName | null): AsyncGenerator<MembersEvent>;
    /**
     * The grants on the scopes you administer, by principal then scope.
  
  Every grant on the scopes the caller administers, by principal then
  scope (`--admin` ones included); `scope` narrows to one. Needs admin
  somewhere.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `members.list` anywhere it is held
     */
    list(scope?: ScopeRef | null): AsyncGenerator<MembersEvent>;
    /**
     * Who you are, then your teams, then each grant you hold (a team's as team:<name>).
  
  One `me { principal, name, admin_everywhere }`, then (ACCESS-22) one
  `team` per team the caller is in, then one `grant` per grant the
  caller holds — its own, and each team's (`principal: team:<name>`).
  Someone signed in without a grant gets `me` and nothing else.
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
     * Take a principal's or a team's role on a scope (an organization, a shortcode, a ticket, a subtree, or *) away.
  
  Takes `principal`'s (ACCESS-22: or `team`'s) role on `scope` away.
  Needs admin on the scope, as the tree stands. Yields `revoked {
  principal, scope, existed }`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `members.revoke` on the scope of `scope`
     */
    revoke(scope: ScopeRef, principal?: Principal | null, team?: TeamName | null): AsyncGenerator<MembersEvent>;
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
