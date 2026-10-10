import type { RpcClient } from '../rpc';
import type { SessionEvent } from './types';
/** Typed client interface for session plugin */
export interface SessionClient {
    /** The providers this menger admits callers through: provider { name, kind, issuer, audience } */
    providers(): AsyncGenerator<SessionEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Who this connection is: identity { principal, name, roles, tenant, provider }, or anonymous */
    whoami(): AsyncGenerator<SessionEvent>;
}
/** Create a typed session client from an RPC client */
export declare function createSessionClient(rpc: RpcClient): SessionClient;
