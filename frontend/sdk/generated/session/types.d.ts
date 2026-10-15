/** IDENTITY-42: one entry of a session's ceiling — `role@SCOPE`
(`editor@IDENTITY`, `viewer@*`) or `grant@SCOPE` (`tickets.get@MENGER-12`),
the scope by [`ceiling_scope_of`]. Which roles there are, and the
shortcode a scope names, are the server's to say. */
export type Ceiling = string;
/** How a provider proves a caller. */
export type ProviderKind = "oidc" | "nostr";
/** The connection proved who it is. */
export interface SessionEventIdentity {
    type: 'identity';
    /** The name to show: a username, a nickname, or an npub. */
    name: string;
    /** The Plexus principal: `idp:<uuid>`, `oidc:<provider>/<sub>` or
  `nostr:<hex>`. */
    principal: string;
    /** The provider adapter that admitted it: `plexus-idp`, `auth0`,
  `nostr`, or a configured OIDC provider's name. */
    provider: string | null;
    /** The roles the provider granted, as given. */
    roles: string[];
    /** The tenant the identity belongs to, when its provider names one. */
    tenant: string | null;
}
/** The connection proved no one. */
export interface SessionEventAnonymous {
    type: 'anonymous';
}
/** IDENTITY-15, `providers`: one provider this menger admits callers
through — what a sign-in needs to reach it, and nothing secret. */
export interface SessionEventProvider {
    type: 'provider';
    /** The audience its tokens must be minted for. */
    audience: string | null;
    /** An OIDC provider's issuer: discovery is under it. */
    issuer: string | null;
    kind: ProviderKind;
    /** Its name, as `identity`'s `provider` says it. */
    name: string;
}
/** IDENTITY-42, `login`: a session minted for the caller. `token`
(`mgs_…`) opens later connections as this identity, presented as
`Authorization: Bearer mgs_…` or `Cookie: access_token=mgs_…`, until
`expires_at` (unix seconds) or `logout`. */
export interface SessionEventLogin {
    type: 'login';
    /** The agent id a key proof named, when one did. */
    agent: string | null;
    expiresAt: number;
    /** The caller's principal, which the session acts as. */
    principal: string;
    /** The session's ceiling, `role@SCOPE` each: every check passes only
  if the principal's grants AND this allow it. Empty: no ceiling
  beyond the principal's grants. */
    scope: string[];
    token: string;
}
/** IDENTITY-42, `logout`: the caller's session has ended. */
export interface SessionEventLoggedOut {
    type: 'logged_out';
}
/** A refusal, naming the problem; nothing was changed. */
export interface SessionEventError {
    type: 'error';
    message: string;
}
/** What the `session` activation's methods yield. */
export type SessionEvent = SessionEventIdentity | SessionEventAnonymous | SessionEventProvider | SessionEventLogin | SessionEventLoggedOut | SessionEventError;
export declare function isSessionEventIdentity(e: SessionEvent): e is SessionEventIdentity;
export declare function isSessionEventAnonymous(e: SessionEvent): e is SessionEventAnonymous;
export declare function isSessionEventProvider(e: SessionEvent): e is SessionEventProvider;
export declare function isSessionEventLogin(e: SessionEvent): e is SessionEventLogin;
export declare function isSessionEventLoggedOut(e: SessionEvent): e is SessionEventLoggedOut;
export declare function isSessionEventError(e: SessionEvent): e is SessionEventError;
