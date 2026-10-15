import type { RpcClient } from '../rpc';
import type { Ceiling, SessionEvent } from './types';
/** Typed client interface for session plugin */
export interface SessionClient {
    /** Mint a session for this identity: login { token, principal, agent, expires_at, scope } */
    login(scope?: Ceiling[] | null, ttlSecs?: number | null): AsyncGenerator<SessionEvent>;
    /** End this connection's session: logged_out */
    logout(): AsyncGenerator<SessionEvent>;
    /** The providers this menger admits callers through: provider { name, kind, issuer, audience } */
    providers(): AsyncGenerator<SessionEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Who this connection is: identity { principal, name, roles, tenant, provider }, or anonymous */
    whoami(): AsyncGenerator<SessionEvent>;
}
/** Create a typed session client from an RPC client */
export declare function createSessionClient(rpc: RpcClient): SessionClient;
