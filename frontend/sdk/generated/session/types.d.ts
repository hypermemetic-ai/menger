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
/** What the `session` activation's methods yield. */
export type SessionEvent = SessionEventIdentity | SessionEventAnonymous | SessionEventProvider;
export declare function isSessionEventIdentity(e: SessionEvent): e is SessionEventIdentity;
export declare function isSessionEventAnonymous(e: SessionEvent): e is SessionEventAnonymous;
export declare function isSessionEventProvider(e: SessionEvent): e is SessionEventProvider;
