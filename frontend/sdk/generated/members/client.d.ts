import type { RpcClient } from '../rpc';
import type { MembersEvent } from './types';
/** Typed client interface for members plugin */
export interface MembersClient {
    /** Give a principal a role (viewer, editor, admin) on a shortcode or * */
    grant(principal: string, role: string, scope: string): AsyncGenerator<MembersEvent>;
    /** The grants on the scopes you administer, by principal then scope */
    list(scope?: string | null): AsyncGenerator<MembersEvent>;
    /** Who you are, then each grant you hold */
    mine(): AsyncGenerator<MembersEvent>;
    /** Who signed in without a grant, waiting for an admin */
    requests(): AsyncGenerator<MembersEvent>;
    /** Take a principal's role on a shortcode or * away */
    revoke(principal: string, scope: string): AsyncGenerator<MembersEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
}
/** Create a typed members client from an RPC client */
export declare function createMembersClient(rpc: RpcClient): MembersClient;
