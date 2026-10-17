import type { RpcClient } from '../rpc';
import type { AuthEvent, GrantName, LoginFlow, MethodPath, ProviderName, RoleName, ScopeName } from './types';
import type { Principal } from '../orgs/types';
/** Typed client interface for auth plugin */
export interface AuthClient {
    /**
     * Give a principal a role on a scope
     *
     * Give `principal` `role` on `scope` (whoever holds `auth.grant` on
     * the scope: admin there).
     * @requires `auth.grant` on the scope of `scope`
     */
    grant(principal: Principal, role: RoleName, scope: ScopeName): AsyncGenerator<AuthEvent>;
    /**
     * Sign in — a person through the browser, an agent with its key
     *
     * Sign in — a person through the browser (`{browser: {redirect}}`,
     * then `{code: {code, state}}`), a password, or an agent's session on
     * a key-proved connection (`{session: {ttl_secs, ceiling}}`).
     * @requires `auth.login` anywhere it is held
     */
    login(flow: LoginFlow): AsyncGenerator<AuthEvent>;
    /**
     * Sign out: end this session
     *
     * End this session.
     * @requires `auth.logout` anywhere it is held
     */
    logout(): AsyncGenerator<AuthEvent>;
    /**
     * The ways in this server offers
     * @requires `auth.providers` anywhere it is held
     */
    providers(): AsyncGenerator<AuthEvent>;
    /**
     * What a method needs: its grant, target, and the roles that list it
     *
     * What a method needs: its grant, its target and the roles that list
     * it.
     * @requires `auth.requires` anywhere it is held
     */
    requires(method: MethodPath): AsyncGenerator<AuthEvent>;
    /**
     * Take a principal's role on a scope away
     *
     * Take `principal`'s role (or every role) on `scope` away.
     * @requires `auth.revoke` on the scope of `scope`
     */
    revoke(principal: Principal, scope: ScopeName, role?: RoleName | null): AsyncGenerator<AuthEvent>;
    /**
     * Set a role's grants (admin on *)
     *
     * Set a role's grants (admin on `*`). Each entry a grant
     * (`tickets.get`) or an existing role whose grants it includes.
     * @requires `auth.role_set` anywhere it is held
     */
    roleSet(grants: GrantName[], role: RoleName): AsyncGenerator<AuthEvent>;
    /**
     * Every role and the grants it lists
     *
     * (admin: every grant).
     * @requires `auth.roles` anywhere it is held
     */
    roles(): AsyncGenerator<AuthEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `auth.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
    /**
     * Create an account, or ask for access
     *
     * a signed-in caller holding no
     * membership joins the waiting list for an admin.
     * @requires `auth.signup` anywhere it is held
     */
    signup(via?: ProviderName | null): AsyncGenerator<AuthEvent>;
    /**
     * Who asked for access and holds nothing yet (admin on *)
     *
     * Who signed up or asked for access and holds nothing yet (admin on
     * `*`).
     * @requires `auth.waiting` anywhere it is held
     */
    waiting(): AsyncGenerator<AuthEvent>;
    /**
     * Who you are: principal, roles held where, ceiling
     *
     * Who this connection is: principal, name, the roles it holds where,
     * and its session's ceiling.
     * @requires `auth.whoami` anywhere it is held
     */
    whoami(): AsyncGenerator<AuthEvent>;
}
/** Create a typed auth client from an RPC client */
export declare function createAuthClient(rpc: RpcClient): AuthClient;
/** What each auth method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "auth.grant": {
        readonly grant: "auth.grant";
        readonly target: "scope";
    };
    readonly "auth.login": {
        readonly grant: "auth.login";
        readonly target: null;
    };
    readonly "auth.logout": {
        readonly grant: "auth.logout";
        readonly target: null;
    };
    readonly "auth.providers": {
        readonly grant: "auth.providers";
        readonly target: null;
    };
    readonly "auth.requires": {
        readonly grant: "auth.requires";
        readonly target: null;
    };
    readonly "auth.revoke": {
        readonly grant: "auth.revoke";
        readonly target: "scope";
    };
    readonly "auth.role_set": {
        readonly grant: "auth.role_set";
        readonly target: null;
    };
    readonly "auth.roles": {
        readonly grant: "auth.roles";
        readonly target: null;
    };
    readonly "auth.schema": {
        readonly grant: "auth.schema";
        readonly target: null;
    };
    readonly "auth.signup": {
        readonly grant: "auth.signup";
        readonly target: null;
    };
    readonly "auth.waiting": {
        readonly grant: "auth.waiting";
        readonly target: null;
    };
    readonly "auth.whoami": {
        readonly grant: "auth.whoami";
        readonly target: null;
    };
};
