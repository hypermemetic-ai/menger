// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../../rpc';
import { extractData, collectOne } from '../../rpc';
import type { MembersEvent, TeamName } from '../../members/types';
import type { OrgId, Principal } from '../../orgs/types';

/** Typed client interface for members.teams plugin */
export interface MembersTeamsClient {
  /**
   * Put a principal in a team — they hold what it holds; needs admin on every scope it holds.

Yields `team_member { member }`. Their open sockets follow at once
(the grants generation moves, ACCESS-12).
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `teams.add` anywhere it is held
   */
  add(principal: Principal, team: TeamName): AsyncGenerator<MembersEvent>;
  /**
   * Make a team, empty, in an organization; yields the team. Needs admin somewhere.

Makes the team `name` (lower-kebab, unique app-wide) in `org` — the
caller's only organization when not given. Needs admin on some
scope; it holds nothing until granted (`members grant --team`).
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `teams.create` anywhere it is held
   */
  create(name: TeamName, org?: OrgId | null): AsyncGenerator<MembersEvent>;
  /**
   * Delete a team — every grant it holds goes with it; needs admin on every scope it holds.

Yields `team_deleted { team, grants, members }`: how many grants and
members went with it.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `teams.delete` anywhere it is held
   */
  delete(team: TeamName): AsyncGenerator<MembersEvent>;
  /**
   * The teams in your organizations, by name; one team { name, org, created_by, at, members } each.

Every team in an organization the caller holds a role in (every
team, for a role on `*`), by name — `org` narrows to one. Admin's.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `teams.list` anywhere it is held
   */
  list(org?: OrgId | null): AsyncGenerator<MembersEvent>;
  /**
   * Take a principal out of a team; needs admin on every scope it holds.

Yields `team_left { team, principal, existed }`. Their open watches
stop hearing the team's scopes at once (ACCESS-12).
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `teams.remove` anywhere it is held
   */
  remove(principal: Principal, team: TeamName): AsyncGenerator<MembersEvent>;
  /**
   * Rename a team — its members and grants go with it; needs admin on every scope it holds.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `teams.rename` anywhere it is held
   */
  rename(team: TeamName, to: TeamName): AsyncGenerator<MembersEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `teams.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
  /**
   * A team: then each member (team_member), then each grant it holds on a scope you administer.

One `team`, then a `team_member { member }` per principal in it, then
a `grant { grant }` per grant it holds on a scope where the caller
is admin. A team outside the caller's organizations, or none, is an
in-band `error`.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `teams.show` anywhere it is held
   */
  show(team: TeamName): AsyncGenerator<MembersEvent>;
}

/** Typed client implementation for members.teams plugin */
class MembersTeamsClientImpl implements MembersTeamsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *add(principal: Principal, team: TeamName): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.teams.add', { principal, team });
    yield* extractData<MembersEvent>(stream);
  }

  async *create(name: TeamName, org?: OrgId | null): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.teams.create', { name, org });
    yield* extractData<MembersEvent>(stream);
  }

  async *delete(team: TeamName): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.teams.delete', { team });
    yield* extractData<MembersEvent>(stream);
  }

  async *list(org?: OrgId | null): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.teams.list', { org });
    yield* extractData<MembersEvent>(stream);
  }

  async *remove(principal: Principal, team: TeamName): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.teams.remove', { principal, team });
    yield* extractData<MembersEvent>(stream);
  }

  async *rename(team: TeamName, to: TeamName): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.teams.rename', { team, to });
    yield* extractData<MembersEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('members.teams.schema', {});
    return collectOne<unknown>(stream);
  }

  async *show(team: TeamName): AsyncGenerator<MembersEvent> {
    const stream = this.rpc.call('members.teams.show', { team });
    yield* extractData<MembersEvent>(stream);
  }
}

/** Create a typed members.teams client from an RPC client */
export function createMembersTeamsClient(rpc: RpcClient): MembersTeamsClient {
  return new MembersTeamsClientImpl(rpc);
}

/** What each members.teams method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "teams.add": { grant: "teams.add", target: null },
  "teams.create": { grant: "teams.create", target: null },
  "teams.delete": { grant: "teams.delete", target: null },
  "teams.list": { grant: "teams.list", target: null },
  "teams.remove": { grant: "teams.remove", target: null },
  "teams.rename": { grant: "teams.rename", target: null },
  "teams.schema": { grant: "teams.schema", target: null },
  "teams.show": { grant: "teams.show", target: null },
} as const;