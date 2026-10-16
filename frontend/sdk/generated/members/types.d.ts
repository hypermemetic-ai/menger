import type { Grant, GrantedBy, Grantee, OrgId, Principal, Scope } from '../orgs/types';
/** A grant as it stands (`list`, `mine`, and `grant`'s answer). */
export interface MembersEventGrant {
    type: 'grant';
    grant: Grant;
}
/** `revoke`'s answer: the grant that was removed, or `existed: false`. */
export interface MembersEventRevoked {
    type: 'revoked';
    existed: boolean;
    principal: Grantee;
    scope: Scope;
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
/** `mine`: who the caller is — before their teams and grants. */
export interface MembersEventMe {
    type: 'me';
    adminEverywhere: boolean;
    name: string | null;
    principal: string | null;
}
/** ACCESS-22: a team as it stands — `teams.list`, `teams.show`'s first,
`teams.create` and `teams.rename`'s answer, and `mine`'s, one per
team the caller is in (before its grants: a grant held through a team
is the team's, `principal: team:<name>`). */
export interface MembersEventTeam {
    type: 'team';
    team: Team;
}
/** ACCESS-22: a principal in a team — `teams.show`'s, after the team;
`teams.add`'s answer. */
export interface MembersEventTeamMember {
    type: 'team_member';
    member: TeamMember;
}
/** ACCESS-22: `teams.remove`'s answer — whether they were in it. */
export interface MembersEventTeamLeft {
    type: 'team_left';
    existed: boolean;
    principal: Principal;
    team: TeamName;
}
/** ACCESS-22: `teams.delete`'s answer — the team, with every grant it
held (`grants`, how many) and every member. */
export interface MembersEventTeamDeleted {
    type: 'team_deleted';
    grants: number;
    members: number;
    team: TeamName;
}
/** A refusal, naming the problem; nothing was changed. */
export interface MembersEventError {
    type: 'error';
    message: string;
}
/** What the `members` activation's methods yield. */
export type MembersEvent = MembersEventGrant | MembersEventRevoked | MembersEventWaiting | MembersEventMe | MembersEventTeam | MembersEventTeamMember | MembersEventTeamLeft | MembersEventTeamDeleted | MembersEventError;
export declare function isMembersEventGrant(e: MembersEvent): e is MembersEventGrant;
export declare function isMembersEventRevoked(e: MembersEvent): e is MembersEventRevoked;
export declare function isMembersEventWaiting(e: MembersEvent): e is MembersEventWaiting;
export declare function isMembersEventMe(e: MembersEvent): e is MembersEventMe;
export declare function isMembersEventTeam(e: MembersEvent): e is MembersEventTeam;
export declare function isMembersEventTeamMember(e: MembersEvent): e is MembersEventTeamMember;
export declare function isMembersEventTeamLeft(e: MembersEvent): e is MembersEventTeamLeft;
export declare function isMembersEventTeamDeleted(e: MembersEvent): e is MembersEventTeamDeleted;
export declare function isMembersEventError(e: MembersEvent): e is MembersEventError;
/** ORG-4: a scope as a call writes it (ACCESS-37's grammar) — `*`, an
organization (`acme:`), or a shortcode in one by any short form of
its address (`acme:MENGER/THEME`, `IDENTITY`), its codes upper case:
[`crate::members::scope_of`]. ACCESS-21: or a ticket alone by any
short form of its address (`MENGER-12`), or `under` one (`under
MENGER-12`, it and its subtree). The store resolves it to the
[`crate::members::Scope`] it keeps. */
export type ScopeRef = string;
/** ACCESS-22: a team — a named set of principals that holds grants as one
grantee (`team:<name>`): each member holds what it holds. Its name is
unique app-wide (so `team:<name>` names one), and it lives in one
organization (ACCESS-37), where `members teams list` shows it. */
export interface Team {
    /** When, unix seconds. */
    at: number;
    /** Who made it — a principal, or the operator's. */
    createdBy: GrantedBy;
    /** How many principals are in it. */
    members: number;
    name: TeamName;
    org: OrgId;
}
/** ACCESS-22: one principal in a team. */
export interface TeamMember {
    /** Who added them. */
    addedBy: GrantedBy;
    /** When, unix seconds. */
    at: number;
    principal: Principal;
    team: TeamName;
}
/** ACCESS-22: a team's name — lower-kebab, starting with a letter, at
most 32 (`release-crew`). */
export type TeamName = string;
