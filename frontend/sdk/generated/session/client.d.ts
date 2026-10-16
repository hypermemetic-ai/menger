import type { RpcClient } from '../rpc';
import type { Ceiling, SessionEvent } from './types';
/** Typed client interface for session plugin */
export interface SessionClient {
    /**
     * Mint a session for this identity: login { token, principal, agent, expires_at, scope }.
  
  IDENTITY-42: a session for this connection's identity. Yields
  `login { token, principal, agent, expires_at, scope }`, or an in-band
  `error`. The token (`mgs_…`) opens later connections as this
  identity — `Authorization: Bearer mgs_…` or `Cookie:
  access_token=mgs_…` — until `expires_at` or `logout`. `ttl_secs`:
  default 43200 (12 h), at most 86400. `scope`: the session's ceiling,
  `role@SCOPE` each (`editor@IDENTITY`, `viewer@*`, `editor@MENGER-12`):
  every check then passes only if the principal's grants AND the scope
  allow it. A session minted from a scoped session keeps that scope too.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `session.login` anywhere it is held
     */
    login(scope?: Ceiling[] | null, ttlSecs?: number | null): AsyncGenerator<SessionEvent>;
    /**
     * End this connection's session: logged_out.
  
  IDENTITY-42: ends the session this connection is on (its token stops
  opening connections, and its open connections stop answering).
  Yields `logged_out`, or an in-band `error` when the connection holds
  no session.
     * @requires `session.logout` anywhere it is held
     */
    logout(): AsyncGenerator<SessionEvent>;
    /**
     * The providers this menger admits callers through: provider { name, kind, issuer, audience }.
  
  IDENTITY-15: one `provider { name, kind, issuer, audience }` each, in
  the order they were configured; none when login is off. Nothing here
  is secret: it is what a sign-in needs to find the provider — so it is
  `public` (ACCESS-19), answered before anyone has signed in.
     * @requires `session.providers` anywhere it is held
     */
    providers(): AsyncGenerator<SessionEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `session.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
    /**
     * Who this connection is: identity { principal, name, roles, tenant, provider }, or anonymous.
  
  Yields exactly one event: `identity` — `principal` (`idp:<uuid>`,
  `oidc:<provider>/<sub>` or `nostr:<hex>`), `name`, `roles`, `tenant`
  and `provider` — or `anonymous` with login off. The identity is
  settled when the connection opens; reconnect to change it.
  ACCESS-19 (ACCESS-3 #14): the caller is verified as every method's
  is — a sign-in signed out or expired is refused `unauthenticated`,
  never reported as signed in.
     * @requires `session.whoami` anywhere it is held
     */
    whoami(): AsyncGenerator<SessionEvent>;
}
/** Create a typed session client from an RPC client */
export declare function createSessionClient(rpc: RpcClient): SessionClient;
/** What each session method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "session.login": {
        readonly grant: "session.login";
        readonly target: null;
    };
    readonly "session.logout": {
        readonly grant: "session.logout";
        readonly target: null;
    };
    readonly "session.providers": {
        readonly grant: "session.providers";
        readonly target: null;
    };
    readonly "session.schema": {
        readonly grant: "session.schema";
        readonly target: null;
    };
    readonly "session.whoami": {
        readonly grant: "session.whoami";
        readonly target: null;
    };
};
