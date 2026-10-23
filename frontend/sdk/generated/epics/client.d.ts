import type { RpcClient } from '../rpc';
import type { EpicName, EpicsEvent } from './types';
import type { OrgId } from '../orgs/types';
/** Typed client interface for epics plugin */
export interface EpicsClient {
    /**
     * Make an epic in the organization, empty (admin on it, or on *): its shortcodes start at 1. Move tickets in with tickets reshortcode --to org:epic:CODE; create in it with create --epic org:epic:CODE. Yields created { epic }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.create` anywhere it is held
     */
    create(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * The epic you are in, in the organization — your choice (switch), else its newest; yields current { epic }. A filtered read answers there unless its filter names epic (epic any: every one).
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.current` anywhere it is held
     */
    current(org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Delete an empty epic (admin on its organization, or on *) — never the organization's first; whoever was in it is in the newest again. Yields deleted { org, name }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.delete` anywhere it is held
     */
    delete(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * One epic of the organization by name; `not_found` when it has none of that name.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.get` anywhere it is held
     */
    get(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * The organization's epics, oldest first: one `epic { org, name, first, current, shortcodes, tickets, created_unix }` each.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.list` anywhere it is held
     */
    list(org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Rename an epic (admin on its organization, or on *): every address naming it moves to the new name. Yields renamed { from, epic }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.rename` anywhere it is held
     */
    rename(name: EpicName, to: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `epics.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
    /**
     * Be in another epic of the organization (your own setting): short forms resolve there first, and filtered reads answer there. Yields current { epic }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.switch` anywhere it is held
     */
    switch(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
}
/** Create a typed epics client from an RPC client */
export declare function createEpicsClient(rpc: RpcClient): EpicsClient;
/** What each epics method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "epics.create": {
        readonly grant: "epics.create";
        readonly target: null;
    };
    readonly "epics.current": {
        readonly grant: "epics.current";
        readonly target: null;
    };
    readonly "epics.delete": {
        readonly grant: "epics.delete";
        readonly target: null;
    };
    readonly "epics.get": {
        readonly grant: "epics.get";
        readonly target: null;
    };
    readonly "epics.list": {
        readonly grant: "epics.list";
        readonly target: null;
    };
    readonly "epics.rename": {
        readonly grant: "epics.rename";
        readonly target: null;
    };
    readonly "epics.schema": {
        readonly grant: "epics.schema";
        readonly target: null;
    };
    readonly "epics.switch": {
        readonly grant: "epics.switch";
        readonly target: null;
    };
};
