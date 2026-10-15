import type { RpcClient } from '../rpc';
import type { OrgId, OrgsEvent, Principal } from './types';
/** Typed client interface for orgs plugin */
export interface OrgsClient {
    /** Make an organization (admin on *), naming its first admin */
    create(name: OrgId, admin?: Principal | null): AsyncGenerator<OrgsEvent>;
    /** Delete an empty organization, and every grant on it */
    delete(name: OrgId): AsyncGenerator<OrgsEvent>;
    /** The organizations you hold a role in */
    list(): AsyncGenerator<OrgsEvent>;
    /** Rename an organization; its old name keeps resolving */
    rename(name: OrgId, to: OrgId): AsyncGenerator<OrgsEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
}
/** Create a typed orgs client from an RPC client */
export declare function createOrgsClient(rpc: RpcClient): OrgsClient;
