//! ACCESS-22: what the `members teams` methods do, beside the child (its
//! `impl` keeps only the methods — MENGER-29, ACCESS-13's frame). Each
//! takes the [`VerifiedUser`] the dispatch resolved.

use crate::access::{Access, AdminOf, VerifiedUser};
use crate::address::OrgId;
use crate::members::{MembersEvent, Role, Scope};
use crate::store::{OnTeam, Store};
use crate::vocab::{GrantedBy, Principal, TeamName};
use async_stream::stream;
use futures::Stream;
use std::sync::Arc;

fn error(message: impl Into<String>) -> MembersEvent {
    MembersEvent::Error { message: message.into() }
}

/// Who signs a change: the caller's principal (ACCESS-24: always someone).
fn signer(user: &VerifiedUser) -> Result<GrantedBy, MembersEvent> {
    Ok(GrantedBy::Principal(user.principal().clone()))
}

/// Admin somewhere, as a proof: the first scope the caller holds admin on,
/// minted within its ceiling — what making a team takes, and what changing
/// one that holds nothing takes.
fn admin_somewhere(store: &Store, user: &VerifiedUser) -> Result<AdminOf, MembersEvent> {
    let scopes: Vec<Scope> = user.reach().grants().iter().filter(|(_, r)| *r == Role::Admin).map(|(s, _)| s.clone()).collect();
    scopes.iter().find_map(|s| user.admin_of(s, store).ok()).ok_or_else(|| error("a team is made and changed by an admin: you hold admin nowhere"))
}

/// ACCESS-22 (owner, 2026-10-09): admin on every scope `team` holds, minted
/// against the tree as it stands — or, for a team that holds nothing, admin
/// somewhere. The first scope missing is named.
fn on_team(store: &Store, user: &VerifiedUser, team: &TeamName) -> Result<OnTeam, MembersEvent> {
    signer(user)?;
    let scopes = store.team_scopes(team).ok_or_else(|| error(format!("no team {team}")))?;
    if scopes.is_empty() {
        return Ok(OnTeam::new(vec![admin_somewhere(store, user)?]));
    }
    let admins = scopes
        .iter()
        .map(|s| user.admin_of(s, store).map_err(|_| error(format!("changing team {team} needs admin on every scope it holds — you are not admin on {s}"))))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OnTeam::new(admins))
}

/// Whether `org` is one the caller holds a role in.
fn in_reach(user: &VerifiedUser, org: &OrgId) -> bool {
    user.orgs().is_none_or(|held| held.contains(org))
}

fn said<T>(r: Result<T, crate::store::StoreError>, ok: impl FnOnce(T) -> MembersEvent) -> MembersEvent {
    r.map_or_else(|e| error(e.to_string()), ok)
}

/// `teams.list`.
pub(super) fn list_stream(access: Access, user: VerifiedUser, org: Option<OrgId>) -> impl Stream<Item = MembersEvent> + Send + 'static {
    stream! {
        for team in access.teams(user.orgs().as_deref()) {
            if org.as_ref().is_none_or(|o| *o == team.org) {
                yield MembersEvent::Team { team };
            }
        }
    }
}

/// `teams.show`: the team, its members, its grants on scopes the caller
/// administers.
pub(super) fn show_stream(access: Access, user: VerifiedUser, name: TeamName) -> impl Stream<Item = MembersEvent> + Send + 'static {
    stream! {
        let Some((team, members, grants)) = access.team(&name).filter(|(t, _, _)| in_reach(&user, &t.org)) else {
            yield error(format!("no team {name}"));
            return;
        };
        yield MembersEvent::Team { team };
        for member in members {
            yield MembersEvent::TeamMember { member };
        }
        let held = |s: &Scope| user.gate_reach().is_none_or(|r| r.allows(&s.text()));
        for grant in grants.into_iter().filter(|g| held(&g.scope)) {
            yield MembersEvent::Grant { grant };
        }
    }
}

/// `teams.create`: admin somewhere; in `org`, or the caller's only one.
pub(super) async fn create(store: Arc<Store>, user: VerifiedUser, name: TeamName, org: Option<OrgId>) -> MembersEvent {
    let made = || -> Result<(AdminOf, GrantedBy, OrgId), MembersEvent> {
        let by = signer(&user)?;
        let admin = admin_somewhere(&store, &user)?;
        let org = match (org, user.orgs()) {
            (Some(o), _) if in_reach(&user, &o) => o,
            (Some(o), _) => return Err(error(format!("no organization {o} of yours"))),
            (None, Some(held)) if held.len() == 1 => held[0].clone(),
            (None, Some(_)) => return Err(error("you are in more than one organization: name the team's with --org")),
            (None, None) => store.orgs().first().map(|o| o.name.clone()).ok_or_else(|| error("there is no organization"))?,
        };
        Ok((admin, by, org))
    };
    match made() {
        Ok((admin, by, org)) => said(store.create_team(admin, name, &org, by).await, |team| MembersEvent::Team { team }),
        Err(refused) => refused,
    }
}

/// `teams.rename`.
pub(super) async fn rename(store: Arc<Store>, user: VerifiedUser, team: TeamName, to: TeamName) -> MembersEvent {
    match on_team(&store, &user, &team) {
        Ok(on) => said(store.rename_team(on, &team, to).await, |team| MembersEvent::Team { team }),
        Err(refused) => refused,
    }
}

/// `teams.delete`.
pub(super) async fn delete(store: Arc<Store>, user: VerifiedUser, team: TeamName) -> MembersEvent {
    match on_team(&store, &user, &team) {
        Ok(on) => said(store.delete_team(on, &team).await, |(grants, members)| MembersEvent::TeamDeleted { team, grants, members }),
        Err(refused) => refused,
    }
}

/// `teams.add`.
pub(super) async fn add(store: Arc<Store>, user: VerifiedUser, team: TeamName, principal: Principal) -> MembersEvent {
    let proofs = on_team(&store, &user, &team).and_then(|on| Ok((on, signer(&user)?)));
    match proofs {
        Ok((on, by)) => said(store.add_to_team(on, &team, &principal, by).await, |member| MembersEvent::TeamMember { member }),
        Err(refused) => refused,
    }
}

/// `teams.remove`.
pub(super) async fn remove(store: Arc<Store>, user: VerifiedUser, team: TeamName, principal: Principal) -> MembersEvent {
    match on_team(&store, &user, &team) {
        Ok(on) => said(store.remove_from_team(on, &team, &principal).await, |existed| MembersEvent::TeamLeft { team, principal, existed }),
        Err(refused) => refused,
    }
}
