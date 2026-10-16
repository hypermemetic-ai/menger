import type { RpcClient } from '../../rpc';
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
/** Create a typed members.teams client from an RPC client */
export declare function createMembersTeamsClient(rpc: RpcClient): MembersTeamsClient;
/** What each members.teams method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "teams.add": {
        readonly grant: "teams.add";
        readonly target: null;
    };
    readonly "teams.create": {
        readonly grant: "teams.create";
        readonly target: null;
    };
    readonly "teams.delete": {
        readonly grant: "teams.delete";
        readonly target: null;
    };
    readonly "teams.list": {
        readonly grant: "teams.list";
        readonly target: null;
    };
    readonly "teams.remove": {
        readonly grant: "teams.remove";
        readonly target: null;
    };
    readonly "teams.rename": {
        readonly grant: "teams.rename";
        readonly target: null;
    };
    readonly "teams.schema": {
        readonly grant: "teams.schema";
        readonly target: null;
    };
    readonly "teams.show": {
        readonly grant: "teams.show";
        readonly target: null;
    };
};
