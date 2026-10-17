import type { RpcClient } from '../rpc';
import type { OrgId, OrgsEvent, Principal } from './types';
/** Typed client interface for orgs plugin */
export interface OrgsClient {
    /**
     * Make an organization (admin on *), naming its first admin
     *
     * Makes the organization `name`, empty — lower-case letters, digits
     * and `-`, unique app-wide (among every name an organization has or
     * had). `admin` names its first admin, who holds admin on `name:`
     * from this grant. Needs admin on `*`. Yields `created { org, admin }`.
     * @requires `orgs.create` anywhere it is held
     */
    create(name: OrgId, admin?: Principal | null): AsyncGenerator<OrgsEvent>;
    /**
     * Delete an empty organization, and every grant on it
     *
     * Deletes the organization `name` — refused while it holds a shortcode
     * (a ticket, or a deleted ticket kept for its undo), and for the first
     * organization. Every grant on it goes with it. Needs admin on it or
     * on `*`. Yields `deleted { name, grants }`.
     * @requires `orgs.delete` anywhere it is held
     */
    delete(name: OrgId): AsyncGenerator<OrgsEvent>;
    /**
     * The organizations you hold a role in
     *
     * The organizations the caller holds a role in — every one for a role
     * on `*` — in the order they were made: one `org
     * { name, aliases, roots, shortcodes, tickets }` each.
     * @requires `orgs.list` anywhere it is held
     */
    list(): AsyncGenerator<OrgsEvent>;
    /**
     * Rename an organization; its old name keeps resolving
     *
     * Renames the organization `name` to `to`. The old name stays an
     * alias: every address and grant scope written with it keeps
     * resolving; every grant on it is rewritten to the new name. Needs
     * admin on it (`name:`) or on `*`. Yields `renamed { from, org }`.
     * @requires `orgs.rename` anywhere it is held
     */
    rename(name: OrgId, to: OrgId): AsyncGenerator<OrgsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `orgs.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
}
/** Create a typed orgs client from an RPC client */
export declare function createOrgsClient(rpc: RpcClient): OrgsClient;
/** What each orgs method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "orgs.create": {
        readonly grant: "orgs.create";
        readonly target: null;
    };
    readonly "orgs.delete": {
        readonly grant: "orgs.delete";
        readonly target: null;
    };
    readonly "orgs.list": {
        readonly grant: "orgs.list";
        readonly target: null;
    };
    readonly "orgs.rename": {
        readonly grant: "orgs.rename";
        readonly target: null;
    };
    readonly "orgs.schema": {
        readonly grant: "orgs.schema";
        readonly target: null;
    };
};
