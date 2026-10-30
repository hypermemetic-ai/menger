// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { MethodStream, collectOne } from '../rpc';
import type { MembersEvent, PairingCode, SchemaResult, ScopeRef, TeamName } from './types';
import type { Principal, Role } from '../orgs/types';
import type { MembersProfilesClient } from '../members/profiles/client';
import { createMembersProfilesClient } from '../members/profiles/client';
import type { MembersTeamsClient } from '../members/teams/client';
import { createMembersTeamsClient } from '../members/teams/client';

/** Typed client interface for members plugin */
export interface MembersClient {
  /** Profiles — what each principal's sign-in said of them (IDENTITY-46): `members.profiles.*`. */
  readonly profiles: MembersProfilesClient;
  /** Teams — a named set of principals that holds grants (ACCESS-22): `members.teams.*`. */
  readonly teams: MembersTeamsClient;
  /**
   * Give a principal, a team or everyone signed in a role (viewer, editor, admin; deployer for a CI's key) on an organization, a shortcode in one, a ticket, a ticket's subtree, or *.

Gives `principal` `role` on `scope` (a shortcode, `*`; ACCESS-21: a
ticket alone, `MENGER-12`, or a ticket and everything under it,
`under MENGER-12`, kept by the ticket's key so the grant follows it),
replacing any role they held there. ACCESS-22: to `team` instead of
`principal` (exactly one), its members hold it; ACCESS-57: to
`everyone`, every signed-in principal holds it (never admin). Needs
admin on the scope, as the tree stands (ACCESS-11's `AdminOf`: for a
ticket's scope, admin on a scope that covers it). Yields the grant as
kept, or an in-band `error`.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `members.grant` on the scope of `scope`
   */
  grant(role: Role, scope: ScopeRef, everyone?: boolean | null, principal?: Principal | null, team?: TeamName | null): MethodStream<'members.grant', MembersEvent>;
  /**
   * The grants on the scopes you administer, by principal then scope.

Every grant on the scopes the caller administers, by principal then
scope (`--admin` ones included); `scope` narrows to one. Needs admin
somewhere.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `members.list` anywhere it is held
   */
  list(scope?: ScopeRef | null): MethodStream<'members.list', MembersEvent>;
  /**
   * Who you are, then your teams, then each grant you hold (a team's as team:<name>, everyone's as everyone).

One `me { principal, name, admin_everywhere }`, then (ACCESS-22) one
`team` per team the caller is in, then one `grant` per grant the
caller holds — its own, each team's (`principal: team:<name>`) and
(ACCESS-57) everyone's (`principal: everyone`).
   * @requires `members.mine` anywhere it is held
   */
  mine(): MethodStream<'members.mine', MembersEvent>;
  /**
   * The machines waiting whose pairing code is this one (TETHER-12: a code typed on #/pair).

One `waiting { principal, name, provider, since }` per key on the
waiting list whose code ([`crate::pairing`]) is `code` — none, one,
or (at 40 bits, rarely) several; nothing else of the list. Needs
admin somewhere: whoever can grant a machine anything.
   * @requiresAuth — resolver: self.access.verify
   * @server-derived auth
   * @requires `members.pairing` anywhere it is held
   */
  pairing(code: PairingCode): MethodStream<'members.pairing', MembersEvent>;
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
  requests(): MethodStream<'members.requests', MembersEvent>;
  /**
   * Take a principal's or a team's role on a scope (an organization, a shortcode, a ticket, a subtree, or *) away.

Takes `principal`'s (ACCESS-22: or `team`'s; ACCESS-57: or
`everyone`'s) role on `scope` away.
Needs admin on the scope, as the tree stands. Yields `revoked {
principal, scope, existed }`.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `members.revoke` on the scope of `scope`
   */
  revoke(scope: ScopeRef, everyone?: boolean | null, principal?: Principal | null, team?: TeamName | null): MethodStream<'members.revoke', MembersEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `members.schema` anywhere it is held
   */
  schema(method?: string | null): Promise<SchemaResult>;
}

/** Typed client implementation for members plugin */
class MembersClientImpl implements MembersClient {
  private rpc: RpcClient;
  readonly profiles: MembersProfilesClient;
  readonly teams: MembersTeamsClient;
  constructor(rpc: RpcClient) {
    this.rpc = rpc;
    this.profiles = createMembersProfilesClient(rpc);
    this.teams = createMembersTeamsClient(rpc);
  }

  grant(role: Role, scope: ScopeRef, everyone?: boolean | null, principal?: Principal | null, team?: TeamName | null): MethodStream<'members.grant', MembersEvent> {
    const stream = this.rpc.call('members.grant', { everyone, principal, role, scope, team });
    return new MethodStream<'members.grant', MembersEvent>('members.grant', stream);
  }

  list(scope?: ScopeRef | null): MethodStream<'members.list', MembersEvent> {
    const stream = this.rpc.call('members.list', { scope });
    return new MethodStream<'members.list', MembersEvent>('members.list', stream);
  }

  mine(): MethodStream<'members.mine', MembersEvent> {
    const stream = this.rpc.call('members.mine', {});
    return new MethodStream<'members.mine', MembersEvent>('members.mine', stream);
  }

  pairing(code: PairingCode): MethodStream<'members.pairing', MembersEvent> {
    const stream = this.rpc.call('members.pairing', { code });
    return new MethodStream<'members.pairing', MembersEvent>('members.pairing', stream);
  }

  requests(): MethodStream<'members.requests', MembersEvent> {
    const stream = this.rpc.call('members.requests', {});
    return new MethodStream<'members.requests', MembersEvent>('members.requests', stream);
  }

  revoke(scope: ScopeRef, everyone?: boolean | null, principal?: Principal | null, team?: TeamName | null): MethodStream<'members.revoke', MembersEvent> {
    const stream = this.rpc.call('members.revoke', { everyone, principal, scope, team });
    return new MethodStream<'members.revoke', MembersEvent>('members.revoke', stream);
  }

  async schema(method?: string | null): Promise<SchemaResult> {
    const stream = this.rpc.call('members.schema', { method });
    return collectOne<SchemaResult>(stream);
  }
}

/** Create a typed members client from an RPC client */
export function createMembersClient(rpc: RpcClient): MembersClient {
  return new MembersClientImpl(rpc);
}

/** What each members method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "members.grant": { grant: "members.grant", target: "scope" },
  "members.list": { grant: "members.list", target: null },
  "members.mine": { grant: "members.mine", target: null },
  "members.pairing": { grant: "members.pairing", target: null },
  "members.requests": { grant: "members.requests", target: null },
  "members.revoke": { grant: "members.revoke", target: "scope" },
  "members.schema": { grant: "members.schema", target: null },
} as const;