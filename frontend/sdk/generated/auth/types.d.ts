/** `providers`: one way in. */
export interface AuthEventProvider {
    type: 'provider';
    kind: ProviderKind;
    label: string | null;
    name: string;
}
/** `login {browser}` / `signup`: open this URL. */
export interface AuthEventLoginUrl {
    type: 'login_url';
    state: string;
    url: string;
}
/** `login`: signed in. */
export interface AuthEventSession {
    type: 'session';
    agent: string | null;
    ceiling: string[];
    expiresAt: number;
    name: string | null;
    principal: string;
    token: string;
}
/** `signup` (asking for access) / `waiting` (an admin's list). */
export interface AuthEventWaiting {
    type: 'waiting';
    name: string | null;
    principal: string;
    provider: string | null;
    since: number;
}
/** `whoami`. */
export interface AuthEventWhoami {
    type: 'whoami';
    agent: string | null;
    ceiling: string[];
    memberships: MembershipEntry[];
    name: string | null;
    principal: string;
}
/** `logout`. */
export interface AuthEventLoggedOut {
    type: 'logged_out';
    principal: string;
}
/** `requires {method}`: its grant, its target, the roles that list it. */
export interface AuthEventRequires {
    type: 'requires';
    grant: string;
    method: string;
    public: boolean;
    roles: string[];
    target: string | null;
}
/** `roles` / `role_set`: a role and its grants (`all`: every grant —
admin). */
export interface AuthEventRole {
    type: 'role';
    all: boolean;
    grants: string[];
    role: string;
}
/** `grant`. */
export interface AuthEventGranted {
    type: 'granted';
    principal: string;
    role: string;
    scope: string;
}
/** `revoke`: the roles it removed there. */
export interface AuthEventRevoked {
    type: 'revoked';
    principal: string;
    roles: string[];
    scope: string;
}
/** Anything refused or failed. A refusal (`error_kind: "forbidden"`)
carries AUTH-2's fields. */
export interface AuthEventError {
    type: 'error';
    errorKind: string | null;
    grant: string | null;
    heldOn: string[] | null;
    message: string;
    roles: string[] | null;
    scope: string | null;
}
/** Every event the `auth` activation sends (`{"type": …}`). */
export type AuthEvent = AuthEventProvider | AuthEventLoginUrl | AuthEventSession | AuthEventWaiting | AuthEventWhoami | AuthEventLoggedOut | AuthEventRequires | AuthEventRole | AuthEventGranted | AuthEventRevoked | AuthEventError;
export declare function isAuthEventProvider(e: AuthEvent): e is AuthEventProvider;
export declare function isAuthEventLoginUrl(e: AuthEvent): e is AuthEventLoginUrl;
export declare function isAuthEventSession(e: AuthEvent): e is AuthEventSession;
export declare function isAuthEventWaiting(e: AuthEvent): e is AuthEventWaiting;
export declare function isAuthEventWhoami(e: AuthEvent): e is AuthEventWhoami;
export declare function isAuthEventLoggedOut(e: AuthEvent): e is AuthEventLoggedOut;
export declare function isAuthEventRequires(e: AuthEvent): e is AuthEventRequires;
export declare function isAuthEventRole(e: AuthEvent): e is AuthEventRole;
export declare function isAuthEventGranted(e: AuthEvent): e is AuthEventGranted;
export declare function isAuthEventRevoked(e: AuthEvent): e is AuthEventRevoked;
export declare function isAuthEventError(e: AuthEvent): e is AuthEventError;
/** One entry of a role's grants: a grant (`tickets.get`) or a role
whose grants it includes (`viewer`). No `@`, no whitespace. */
export type GrantName = string;
/** How a caller signs in (`auth.login {flow: …}`). On the wire, externally
tagged: `{"browser": {"redirect": …}}`, `{"code": {"code", "state"}}`,
`{"password": {"username", "password"}}`, `{"session": {"ttl_secs",
"ceiling"}}`. */
export type LoginFlow = never;
/** A membership as `whoami` says it. */
export interface MembershipEntry {
    role: string;
    scope: string;
}
/** A method, as `<activation>.<method>` — `tickets.set_status`. Two
or more dot-separated segments, none empty, no whitespace. */
export type MethodPath = string;
/** How a provider signs someone in. */
export type ProviderKind = "browser" | "password" | "key";
/** A sign-in provider's name, as `auth.providers` lists it
(`workos`, `plexus-idp`). One word. */
export type ProviderName = string;
/** A role's name — `editor`. One word: no `.` (that is a grant), no
`@` (that is a ceiling entry), no whitespace. */
export type RoleName = string;
/** Where a role is held: a scope name (`MENGER`) or `*`, everywhere.
What a name means is the service's; it is not blank. */
export type ScopeName = string;
