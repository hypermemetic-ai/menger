// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { MethodStream, collectOne } from '../rpc';
import type { AuthEvent, GrantName, LoginFlow, MethodPath, ProviderName, RoleName, SchemaResult, ScopeName } from './types';
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
  grant(principal: Principal, role: RoleName, scope: ScopeName): MethodStream<'auth.grant', AuthEvent>;
  /**
   * Sign in — a person through the browser, an agent with its key
   *
   * Sign in — a person through the browser (`{browser: {redirect}}`,
   * then `{code: {code, state}}`), a password, or an agent's session on
   * a key-proved connection (`{session: {ttl_secs, ceiling}}`).
   * @requires `auth.login` anywhere it is held
   */
  login(flow: LoginFlow): MethodStream<'auth.login', AuthEvent>;
  /**
   * Sign out: end this session
   *
   * End this session.
   * @requires `auth.logout` anywhere it is held
   */
  logout(): MethodStream<'auth.logout', AuthEvent>;
  /**
   * The ways in this server offers
   * @requires `auth.providers` anywhere it is held
   */
  providers(): MethodStream<'auth.providers', AuthEvent>;
  /**
   * What a method needs: its grant, target, and the roles that list it
   *
   * What a method needs: its grant, its target and the roles that list
   * it.
   * @requires `auth.requires` anywhere it is held
   */
  requires(method: MethodPath): MethodStream<'auth.requires', AuthEvent>;
  /**
   * Take a principal's role on a scope away
   *
   * Take `principal`'s role (or every role) on `scope` away.
   * @requires `auth.revoke` on the scope of `scope`
   */
  revoke(principal: Principal, scope: ScopeName, role?: RoleName | null): MethodStream<'auth.revoke', AuthEvent>;
  /**
   * Set a role's grants (admin on *)
   *
   * Set a role's grants (admin on `*`). Each entry a grant
   * (`tickets.get`) or an existing role whose grants it includes.
   * @requires `auth.role_set` anywhere it is held
   */
  roleSet(grants: GrantName[], role: RoleName): MethodStream<'auth.role_set', AuthEvent>;
  /**
   * Every role and the grants it lists
   *
   * (admin: every grant).
   * @requires `auth.roles` anywhere it is held
   */
  roles(): MethodStream<'auth.roles', AuthEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `auth.schema` anywhere it is held
   */
  schema(method?: string | null): Promise<SchemaResult>;
  /**
   * Create an account, or ask for access
   *
   * a signed-in caller holding no
   * membership of its own ([`Memberships::own_held`]) joins the waiting
   * list for an admin.
   * @requires `auth.signup` anywhere it is held
   */
  signup(via?: ProviderName | null): MethodStream<'auth.signup', AuthEvent>;
  /**
   * Who asked for access and holds nothing yet (admin on *)
   *
   * Who signed up or asked for access and holds nothing of its own yet
   * ([`Memberships::own_held`]; admin on `*`).
   * @requires `auth.waiting` anywhere it is held
   */
  waiting(): MethodStream<'auth.waiting', AuthEvent>;
  /**
   * Who you are: principal, roles held where, ceiling
   *
   * Who this connection is: principal, name, the roles it holds where,
   * and its session's ceiling.
   * @requires `auth.whoami` anywhere it is held
   */
  whoami(): MethodStream<'auth.whoami', AuthEvent>;
}

/** Typed client implementation for auth plugin */
class AuthClientImpl implements AuthClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  grant(principal: Principal, role: RoleName, scope: ScopeName): MethodStream<'auth.grant', AuthEvent> {
    const stream = this.rpc.call('auth.grant', { principal, role, scope });
    return new MethodStream<'auth.grant', AuthEvent>('auth.grant', stream);
  }

  login(flow: LoginFlow): MethodStream<'auth.login', AuthEvent> {
    const stream = this.rpc.call('auth.login', { flow });
    return new MethodStream<'auth.login', AuthEvent>('auth.login', stream);
  }

  logout(): MethodStream<'auth.logout', AuthEvent> {
    const stream = this.rpc.call('auth.logout', {});
    return new MethodStream<'auth.logout', AuthEvent>('auth.logout', stream);
  }

  providers(): MethodStream<'auth.providers', AuthEvent> {
    const stream = this.rpc.call('auth.providers', {});
    return new MethodStream<'auth.providers', AuthEvent>('auth.providers', stream);
  }

  requires(method: MethodPath): MethodStream<'auth.requires', AuthEvent> {
    const stream = this.rpc.call('auth.requires', { method });
    return new MethodStream<'auth.requires', AuthEvent>('auth.requires', stream);
  }

  revoke(principal: Principal, scope: ScopeName, role?: RoleName | null): MethodStream<'auth.revoke', AuthEvent> {
    const stream = this.rpc.call('auth.revoke', { principal, role, scope });
    return new MethodStream<'auth.revoke', AuthEvent>('auth.revoke', stream);
  }

  roleSet(grants: GrantName[], role: RoleName): MethodStream<'auth.role_set', AuthEvent> {
    const stream = this.rpc.call('auth.role_set', { grants, role });
    return new MethodStream<'auth.role_set', AuthEvent>('auth.role_set', stream);
  }

  roles(): MethodStream<'auth.roles', AuthEvent> {
    const stream = this.rpc.call('auth.roles', {});
    return new MethodStream<'auth.roles', AuthEvent>('auth.roles', stream);
  }

  async schema(method?: string | null): Promise<SchemaResult> {
    const stream = this.rpc.call('auth.schema', { method });
    return collectOne<SchemaResult>(stream);
  }

  signup(via?: ProviderName | null): MethodStream<'auth.signup', AuthEvent> {
    const stream = this.rpc.call('auth.signup', { via });
    return new MethodStream<'auth.signup', AuthEvent>('auth.signup', stream);
  }

  waiting(): MethodStream<'auth.waiting', AuthEvent> {
    const stream = this.rpc.call('auth.waiting', {});
    return new MethodStream<'auth.waiting', AuthEvent>('auth.waiting', stream);
  }

  whoami(): MethodStream<'auth.whoami', AuthEvent> {
    const stream = this.rpc.call('auth.whoami', {});
    return new MethodStream<'auth.whoami', AuthEvent>('auth.whoami', stream);
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