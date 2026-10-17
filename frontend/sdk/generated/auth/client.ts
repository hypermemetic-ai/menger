// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
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

/** Typed client implementation for auth plugin */
class AuthClientImpl implements AuthClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *grant(principal: Principal, role: RoleName, scope: ScopeName): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.grant', { principal, role, scope });
    yield* extractData<AuthEvent>(stream);
  }

  async *login(flow: LoginFlow): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.login', { flow });
    yield* extractData<AuthEvent>(stream);
  }

  async *logout(): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.logout', {});
    yield* extractData<AuthEvent>(stream);
  }

  async *providers(): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.providers', {});
    yield* extractData<AuthEvent>(stream);
  }

  async *requires(method: MethodPath): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.requires', { method });
    yield* extractData<AuthEvent>(stream);
  }

  async *revoke(principal: Principal, scope: ScopeName, role?: RoleName | null): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.revoke', { principal, role, scope });
    yield* extractData<AuthEvent>(stream);
  }

  async *roleSet(grants: GrantName[], role: RoleName): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.role_set', { grants, role });
    yield* extractData<AuthEvent>(stream);
  }

  async *roles(): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.roles', {});
    yield* extractData<AuthEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('auth.schema', {});
    return collectOne<unknown>(stream);
  }

  async *signup(via?: ProviderName | null): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.signup', { via });
    yield* extractData<AuthEvent>(stream);
  }

  async *waiting(): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.waiting', {});
    yield* extractData<AuthEvent>(stream);
  }

  async *whoami(): AsyncGenerator<AuthEvent> {
    const stream = this.rpc.call('auth.whoami', {});
    yield* extractData<AuthEvent>(stream);
  }
}

/** Create a typed auth client from an RPC client */
export function createAuthClient(rpc: RpcClient): AuthClient {
  return new AuthClientImpl(rpc);
}

/** What each auth method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "auth.grant": { grant: "auth.grant", target: "scope" },
  "auth.login": { grant: "auth.login", target: null },
  "auth.logout": { grant: "auth.logout", target: null },
  "auth.providers": { grant: "auth.providers", target: null },
  "auth.requires": { grant: "auth.requires", target: null },
  "auth.revoke": { grant: "auth.revoke", target: "scope" },
  "auth.role_set": { grant: "auth.role_set", target: null },
  "auth.roles": { grant: "auth.roles", target: null },
  "auth.schema": { grant: "auth.schema", target: null },
  "auth.signup": { grant: "auth.signup", target: null },
  "auth.waiting": { grant: "auth.waiting", target: null },
  "auth.whoami": { grant: "auth.whoami", target: null },
} as const;