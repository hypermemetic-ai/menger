//! ACCESS-19: what the `members` methods do, beside the activation (its
//! `impl` keeps only the methods — MENGER-29, ACCESS-13's frame). Each
//! takes the [`VerifiedUser`] the dispatch resolved.

use crate::access::{Access, AdminOf, VerifiedUser};
use crate::members::{Grant, MembersEvent, Role, Scope};
use crate::store::Store;
use crate::vocab::{GrantedBy, Grantee, Principal, TeamName};
use async_stream::stream;
use futures::Stream;
use std::sync::Arc;

fn grant_event(grant: Grant) -> MembersEvent {
    MembersEvent::Grant { grant }
}

/// A scope named by text, as the store keeps it (ORG-4:
/// `hypermemetic:IDENTITY`) — refused in band.
fn scope_of(store: &Store, text: &str) -> Result<Scope, MembersEvent> {
    store.scope_named(text).map_err(|e| MembersEvent::Error { message: e.to_string() })
}

/// ACCESS-11: admin on the stored scope `scope`, minted against the tree as
/// it stands — what a grant or a revoke on it takes; refused in words.
fn admin_on(store: &Store, user: &VerifiedUser, scope: &Scope) -> Result<AdminOf, MembersEvent> {
    user.admin_of(scope, store).map_err(|r| MembersEvent::Error { message: r.to_string() })
}

/// `members.mine`: who the caller is, then each grant it holds.
pub(super) fn mine_stream(access: Access, user: VerifiedUser) -> impl Stream<Item = MembersEvent> + Send + 'static {
    stream! {
        yield MembersEvent::Me { principal: user.principal_text(), name: user.name().map(str::to_string), admin_everywhere: user.administers("*") };
        // ACCESS-22: the teams it is in, before the grants each holds.
        for team in user.principal().map(|p| access.teams_of(p.as_str())).unwrap_or_default() {
            yield MembersEvent::Team { team };
        }
        for g in user.principal().map(|p| access.grants_of(p)).unwrap_or_default() {
            yield grant_event(g);
        }
    }
}

/// `members.list`: AUTH-7 — the gate admitted `members.list` held somewhere
/// (admin on a shortcode); the grants listed are those on scopes where it
/// is held, as it stands.
pub(super) fn list_stream(store: Arc<Store>, access: Access, user: VerifiedUser, scope: Option<String>) -> impl Stream<Item = MembersEvent> + Send + 'static {
    stream! {
        let only = match scope.as_deref().map(|s| scope_of(&store, s)).transpose() {
            Ok(s) => s,
            Err(refused) => {
                yield refused;
                return;
            }
        };
        let mut grants = access.stored();
        for admin in access.admins() {
            grants.extend(access.grants_of(&admin).into_iter().filter(|g| g.granted_by == GrantedBy::Operator));
        }
        grants.sort_by_cached_key(|g| (g.principal.to_string(), g.scope.text()));
        let held = |s: &Scope| user.gate_reach().is_none_or(|r| r.allows(&s.text()));
        for g in grants {
            if only.as_ref().is_none_or(|o| *o == g.scope) && held(&g.scope) {
                yield grant_event(g);
            }
        }
    }
}

/// ACCESS-22: who a grant or a revoke names — a principal or a team,
/// exactly one.
fn grantee(named: (Option<Principal>, Option<TeamName>)) -> Result<Grantee, MembersEvent> {
    match named {
        (Some(p), None) => Ok(Grantee::Principal(p)),
        (None, Some(t)) => Ok(Grantee::Team(t)),
        _ => Err(MembersEvent::Error { message: "name a principal or a team (--principal or --team), one of the two".into() }),
    }
}

/// `members.grant`: ACCESS-11 — admin on the scope as it stands (ACCESS-3
/// #2, #11), the grant signed by the caller. ACCESS-22: to a principal or
/// a team, as the same rule.
pub(super) async fn grant(store: Arc<Store>, access: Access, user: VerifiedUser, named: (Option<Principal>, Option<TeamName>), scope: String, role: Role) -> MembersEvent {
    let principal = match grantee(named) {
        Ok(g) => g,
        Err(refused) => return refused,
    };
    let scope = match scope_of(&store, &scope) {
        Ok(s) => s,
        Err(refused) => return refused,
    };
    let admin = match admin_on(&store, &user, &scope) {
        Ok(a) => a,
        Err(refused) => return refused,
    };
    let granted_by = user.principal().cloned().map_or(GrantedBy::Operator, GrantedBy::Principal);
    let grant = Grant { principal, scope, role, granted_by, at: chrono::Utc::now().timestamp() };
    match store.grant(admin, grant).await {
        Ok(g) => {
            access.granted(&g.principal.to_string());
            grant_event(g)
        }
        Err(e) => MembersEvent::Error { message: e.to_string() },
    }
}

/// `members.revoke`: ACCESS-11 — admin on the scope as it stands.
pub(super) async fn revoke(store: Arc<Store>, user: VerifiedUser, named: (Option<Principal>, Option<TeamName>), scope: String) -> MembersEvent {
    let principal = match grantee(named) {
        Ok(g) => g,
        Err(refused) => return refused,
    };
    let scope = match scope_of(&store, &scope) {
        Ok(s) => s,
        Err(refused) => return refused,
    };
    let admin = match admin_on(&store, &user, &scope) {
        Ok(a) => a,
        Err(refused) => return refused,
    };
    match store.revoke(admin, principal.clone(), scope.clone()).await {
        Ok(existed) => MembersEvent::Revoked { principal, scope, existed },
        Err(e) => MembersEvent::Error { message: e.to_string() },
    }
}

/// `members.requests`: ACCESS-19 (ACCESS-3 #10; ACCESS-1's second question,
/// as proposed) — the waiting list is every sign-in, whatever it came for,
/// so only an admin of `*` reads it; anyone else holding `members.requests`
/// (an admin of a shortcode, a role an admin edited) is refused.
pub(super) fn requests_stream(access: Access, user: VerifiedUser) -> impl Stream<Item = MembersEvent> + Send + 'static {
    stream! {
        if !user.administers("*") {
            yield MembersEvent::Error { message: "the waiting list needs admin on *".into() };
            return;
        }
        for w in access.waiting() {
            yield MembersEvent::Waiting { principal: w.principal, name: w.name, provider: w.provider, since: w.since };
        }
    }
}
