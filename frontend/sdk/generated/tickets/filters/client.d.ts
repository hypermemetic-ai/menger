import type { RpcClient } from '../../rpc';
import { MethodStream } from '../../rpc';
import type { FilterName, FilterSource, SchemaResult } from './types';
import type { TicketsEvent } from '../../tickets/types';
/** Typed client interface for tickets.filters plugin */
export interface TicketsFiltersClient {
    /**
     * Read a filter without saving it: yields filter_checked with the filter as canonical text and how many tickets in reach it admits — or an in-band error naming the chunk and where it stands, a name that does not resolve, a cycle. Writes nothing.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `filters.check` anywhere it is held
     */
    check(filter: FilterSource): MethodStream<'tickets.filters.check', TicketsEvent>;
    /**
     * Delete a saved filter by name; an in-band error when there is none. Yields filter_deleted. A filter that names it stops resolving.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `filters.delete` anywhere it is held
     */
    delete(name: FilterName): MethodStream<'tickets.filters.delete', TicketsEvent>;
    /**
     * One saved filter, by name (matched by its slug: `Open Builds` is `open-builds`), with its count; an in-band error when there is none.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `filters.get` anywhere it is held
     */
    get(name: FilterName): MethodStream<'tickets.filters.get', TicketsEvent>;
    /**
     * Every saved filter of the caller's organization, by name: its filter, how many tickets in reach it admits, who saved it and when (`Needs you` among them until one is saved under that name).
     * @requires `filters.list` anywhere it is held
     */
    list(): MethodStream<'tickets.filters.list', TicketsEvent>;
    /**
     * Give a saved filter another name, keeping its filter and who saved it; refused in band when the new name is another filter's. Yields filter_saved with `renamed_from`. A filter that names the old name stops resolving.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `filters.rename` anywhere it is held
     */
    rename(name: FilterName, to: FilterName): MethodStream<'tickets.filters.rename', TicketsEvent>;
    /**
     * Save a filter under a name in the caller's organization — new, or replacing the one of that name (who first saved it is kept). Refused in band, nothing written: a filter the language cannot read (the chunk and where it stands are named), one holding `text`, a `saved "…"` that names none or makes a cycle. Yields filter_saved with the row.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `filters.save` anywhere it is held
     */
    save(filter: FilterSource, name: FilterName): MethodStream<'tickets.filters.save', TicketsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `filters.schema` anywhere it is held
     */
    schema(method?: string | null): Promise<SchemaResult>;
}
/** Create a typed tickets.filters client from an RPC client */
export declare function createTicketsFiltersClient(rpc: RpcClient): TicketsFiltersClient;
/** What each tickets.filters method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "filters.check": {
        readonly grant: "filters.check";
        readonly target: null;
    };
    readonly "filters.delete": {
        readonly grant: "filters.delete";
        readonly target: null;
    };
    readonly "filters.get": {
        readonly grant: "filters.get";
        readonly target: null;
    };
    readonly "filters.list": {
        readonly grant: "filters.list";
        readonly target: null;
    };
    readonly "filters.rename": {
        readonly grant: "filters.rename";
        readonly target: null;
    };
    readonly "filters.save": {
        readonly grant: "filters.save";
        readonly target: null;
    };
    readonly "filters.schema": {
        readonly grant: "filters.schema";
        readonly target: null;
    };
};
/** The tickets.filters activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export declare const defaultRoles: {
    readonly editor: readonly ["filters.list", "filters.get", "filters.check", "filters.save", "filters.rename", "filters.delete"];
    readonly viewer: readonly ["filters.list", "filters.get", "filters.check"];
};
