import type { RpcClient } from '../rpc';
import type { EpicName, EpicsEvent } from './types';
import type { OrgId } from '../orgs/types';
/** Typed client interface for epics plugin */
export interface EpicsClient {
    /**
     * Archive an epic (admin on its organization, or on *), in one write: everyone on it moves to the next newer epic (else the next older), the default too if it was this one; its tickets leave every view unless a filter names the epic or archived. The last live epic is refused. Yields archived { epic, moved, to, tickets, dry_run }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.archive` anywhere it is held
     */
    archive(name: EpicName, dryRun?: boolean | null, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Make an epic in the organization, empty (admin on it, or on *): its shortcodes start at 1. Move tickets in with tickets reshortcode --to org:epic:CODE; create in it with create --epic org:epic:CODE. Yields created { epic }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.create` anywhere it is held
     */
    create(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * The epic you are in, in the organization — your choice (switch), else its default; yields current { epic, moved? } (moved: an archive moved you here, until dismiss). A filtered read answers there unless its filter names epic (epic any: every live one).
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.current` anywhere it is held
     */
    current(org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Delete an empty epic (admin on its organization, or on *) — never the organization's first; whoever was in it is on the default again. Yields deleted { org, name }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.delete` anywhere it is held
     */
    delete(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Dismiss the notice that an archive moved you (your own setting): current stops carrying moved. Yields dismissed { org }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.dismiss` anywhere it is held
     */
    dismiss(org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * One epic of the organization by name; `not_found` when it has none of that name.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.get` anywhere it is held
     */
    get(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * The organization's live epics, oldest first: one `epic { org, name, first, default, current, members_on, shortcodes, tickets, created_unix, archived? }` each (--archived true: the archived ones too).
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.list` anywhere it is held
     */
    list(archived?: boolean | null, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Rename an epic (admin on its organization, or on *): every address naming it moves to the new name. Yields renamed { from, epic }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.rename` anywhere it is held
     */
    rename(name: EpicName, to: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Restore an archived epic (admin on its organization, or on *): back in the picker and the views; nobody moves. Yields restored { epic }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.restore` anywhere it is held
     */
    restore(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `epics.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
    /**
     * Make an epic the organization's default (admin on it, or on *) — where everyone who never switched is. Yields default_set { epic }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.set_default` anywhere it is held
     */
    setDefault(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Be in another epic of the organization (your own setting): short forms resolve there first, and filtered reads answer there. Yields current { epic }.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `epics.switch` anywhere it is held
     */
    switch(name: EpicName, org?: OrgId | null): AsyncGenerator<EpicsEvent>;
    /**
     * Follow your organizations' epics, open-ended: epic_changed { org, name, what } (created, renamed, deleted, archived, restored, default_moved), and moved { org, moved } when an archive moves you; lagged { missed }; watch_closed { reason, message } when your grants end it.
     * @requires `epics.watch` anywhere it is held
     */
    watch(): AsyncGenerator<EpicsEvent>;
}
/** Create a typed epics client from an RPC client */
export declare function createEpicsClient(rpc: RpcClient): EpicsClient;
/** What each epics method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "epics.archive": {
        readonly grant: "epics.archive";
        readonly target: null;
    };
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
    readonly "epics.dismiss": {
        readonly grant: "epics.dismiss";
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
    readonly "epics.restore": {
        readonly grant: "epics.restore";
        readonly target: null;
    };
    readonly "epics.schema": {
        readonly grant: "epics.schema";
        readonly target: null;
    };
    readonly "epics.set_default": {
        readonly grant: "epics.set_default";
        readonly target: null;
    };
    readonly "epics.switch": {
        readonly grant: "epics.switch";
        readonly target: null;
    };
    readonly "epics.watch": {
        readonly grant: "epics.watch";
        readonly target: null;
    };
};
