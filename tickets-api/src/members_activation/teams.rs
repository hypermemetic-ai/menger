//! ACCESS-22: `members teams` — teams, a named set of principals that holds
//! grants as one grantee (`team:<name>`): each member holds what the team
//! holds. `list`, `show`, `create`, `rename`, `delete`, `add`, `remove`;
//! a grant to a team is `members grant --team T` (the parent's).
//!
//! A `#[plexus_macros::child]` of `members` on the frame (ACCESS-13, README
//! "The frame"): mounted by `Members::teams`, so its methods are
//! `members.teams.<method>` on the wire, its client `TeamsClient` (reached
//! from the parent's as `client.teams()`), and the hub gates each call by
//! its whole path (MACRO-20). It lists no role: only admin holds its
//! grants (admin holds every grant), so every caller is an admin of
//! somewhere before a method runs. The rest of the rule (owner,
//! 2026-10-09) is the method's, through `team_streams.rs` beside it:
//! changing who is in a team, its name or its end takes admin on **every**
//! scope it holds a grant on, checked again by the store under its lock
//! ([`crate::store::OnTeam`]).

use crate::members::MembersEvent;
use crate::vocab::{Principal, TeamName};
use crate::address::OrgId;
#[cfg(feature = "server")]
use super::team_streams::*;
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The teams a grant may name (ACCESS-22).
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Teams {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

// Not `#[cfg]`-gated: the client-only build needs `TeamsClient` too.
/// Teams: a named set of principals that holds grants; its members hold what it holds.
#[plexus_macros::activation(
    namespace = "teams",
    auth = "required",
    request = MengerRequest,
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core"
)]
impl Teams {
    /// The teams in your organizations, by name; one team { name, org, created_by, at, members } each.
    ///
    /// Every team in an organization the caller holds a role in (every
    /// team, for a role on `*`), by name — `org` narrows to one. Admin's.
    #[plexus_macros::method(group = "read")]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only this organization's teams (optional).
        org: Option<OrgId>,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        list_stream(self.access.clone(), user, org)
    }

    /// A team: then each member (team_member), then each grant it holds on a scope you administer.
    ///
    /// One `team`, then a `team_member { member }` per principal in it, then
    /// a `grant { grant }` per grant it holds on a scope where the caller
    /// is admin. A team outside the caller's organizations, or none, is an
    /// in-band `error`.
    #[plexus_macros::method(group = "read")]
    pub async fn show(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The team's name, e.g. release-crew.
        team: TeamName,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        show_stream(self.access.clone(), user, team)
    }

    /// Make a team, empty, in an organization; yields the team. Needs admin somewhere.
    ///
    /// Makes the team `name` (lower-kebab, unique app-wide) in `org` — the
    /// caller's only organization when not given. Needs admin on some
    /// scope; it holds nothing until granted (`members grant --team`).
    #[plexus_macros::method(group = "admin", example(note = "a crew for releases", params = r#"{"name": "release-crew"}"#))]
    pub async fn create(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The new team's name: lower case letters, digits and -, starting with a letter, at most 32 (release-crew).
        name: TeamName,
        /// Its organization (optional: the caller's only one).
        org: Option<OrgId>,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { create(store, user, name, org).await })
    }

    /// Rename a team — its members and grants go with it; needs admin on every scope it holds.
    #[plexus_macros::method(group = "admin")]
    pub async fn rename(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The team's name now.
        team: TeamName,
        /// Its new name, unique as a new team's is.
        to: TeamName,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { rename(store, user, team, to).await })
    }

    /// Delete a team — every grant it holds goes with it; needs admin on every scope it holds.
    ///
    /// Yields `team_deleted { team, grants, members }`: how many grants and
    /// members went with it.
    #[plexus_macros::method(group = "admin")]
    pub async fn delete(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The team's name.
        team: TeamName,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { delete(store, user, team).await })
    }

    /// Put a principal in a team — they hold what it holds; needs admin on every scope it holds.
    ///
    /// Yields `team_member { member }`. Their open sockets follow at once
    /// (the grants generation moves, ACCESS-12).
    #[plexus_macros::method(group = "admin", example(note = "Ada joins the release crew", params = r#"{"team": "release-crew", "principal": "oidc:workos/user_01M4"}"#))]
    pub async fn add(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The team's name.
        team: TeamName,
        /// The Plexus principal, e.g. idp:c952acd3-…, oidc:auth0/google-oauth2|…, key:<hex>.
        principal: Principal,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { add(store, user, team, principal).await })
    }

    /// Take a principal out of a team; needs admin on every scope it holds.
    ///
    /// Yields `team_left { team, principal, existed }`. Their open watches
    /// stop hearing the team's scopes at once (ACCESS-12).
    #[plexus_macros::method(group = "admin")]
    pub async fn remove(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The team's name.
        team: TeamName,
        /// The Plexus principal.
        principal: Principal,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { remove(store, user, team, principal).await })
    }
}
