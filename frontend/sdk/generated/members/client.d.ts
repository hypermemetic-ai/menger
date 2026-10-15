import type { RpcClient } from '../rpc';
import type { MembersEvent, ScopeRef } from './types';
import type { Principal, Role } from '../orgs/types';
/** Typed client interface for members plugin */
export interface MembersClient {
    /** Give a principal a role (viewer, editor, admin; deployer for a CI's key) on an organization, a shortcode in one, or * */
    grant(principal: Principal, role: Role, scope: ScopeRef): AsyncGenerator<MembersEvent>;
    /** The grants on the scopes you administer, by principal then scope */
    list(scope?: ScopeRef | null): AsyncGenerator<MembersEvent>;
    /** Who you are, then each grant you hold */
    mine(): AsyncGenerator<MembersEvent>;
    /** Who signed in without a grant, waiting for an admin */
    requests(): AsyncGenerator<MembersEvent>;
    /** Take a principal's role on a shortcode or * away */
    revoke(principal: Principal, scope: ScopeRef): AsyncGenerator<MembersEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
}
/** Create a typed members client from an RPC client */
export declare function createMembersClient(rpc: RpcClient): MembersClient;
