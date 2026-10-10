/** One grant. */
export interface Grant {
    /** When, unix seconds. */
    at: number;
    /** The principal who granted it, or `--admin` for the operator's. */
    grantedBy: string;
    /** The Plexus principal: `idp:<uuid>`, `oidc:<provider>/<sub>`, `nostr:<hex>`. */
    principal: string;
    role: Role;
    /** A shortcode, or `*`. */
    scope: string;
}
/** A grant as it stands (`list`, `mine`, and `grant`'s answer). */
export interface MembersEventGrant {
    type: 'grant';
    grant: Grant;
}
/** `revoke`'s answer: the grant that was removed, or `existed: false`. */
export interface MembersEventRevoked {
    type: 'revoked';
    existed: boolean;
    principal: string;
    scope: string;
}
/** `requests`: someone signed in who holds no grant yet. */
export interface MembersEventWaiting {
    type: 'waiting';
    name: string | null;
    principal: string;
    provider: string | null;
    /** When they first signed in without access, unix seconds. */
    since: number;
}
/** `mine`: who the caller is — before their grants. */
export interface MembersEventMe {
    type: 'me';
    adminEverywhere: boolean;
    name: string | null;
    principal: string | null;
}
/** A refusal, naming the problem; nothing was changed. */
export interface MembersEventError {
    type: 'error';
    message: string;
}
/** What the `members` activation's methods yield. */
export type MembersEvent = MembersEventGrant | MembersEventRevoked | MembersEventWaiting | MembersEventMe | MembersEventError;
export declare function isMembersEventGrant(e: MembersEvent): e is MembersEventGrant;
export declare function isMembersEventRevoked(e: MembersEvent): e is MembersEventRevoked;
export declare function isMembersEventWaiting(e: MembersEvent): e is MembersEventWaiting;
export declare function isMembersEventMe(e: MembersEvent): e is MembersEventMe;
export declare function isMembersEventError(e: MembersEvent): e is MembersEventError;
/** A role, ordered: each includes the ones before it. */
export type Role = "viewer" | "editor" | "admin";
