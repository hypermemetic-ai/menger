//! IDENTITY-21: the `members` activation — **the one declaration**, as
//! `tickets`, `comments` and `session` are (MFE-11): the server's
//! [`Members`] and the browser's `MembersClient`.
//!
//! Who holds which role on which scope, and the way an admin changes it.
//! An admin of a scope may grant and revoke roles on it and on every
//! shortcode hosted under it; `*` takes an admin of `*`. Grants are kept by
//! the store; `--admin` principals hold admin on `*` from the operator and
//! cannot be revoked here.
//!
//! AUTH-7: the protocol's `auth.grant`, `auth.revoke` and `auth.waiting`
//! say the same over the same table; this activation stays, unchanged on
//! the wire, for the browser app (its `MembersClient`). Its methods declare
//! no role: `list`, `grant`, `revoke` and `requests` are admin's (admin
//! holds every grant), `grant` and `revoke` held on their `scope` (the
//! target); `mine` is any signed-in caller's (the hub's signed-in grants).
//!
//! ACCESS-19: **on the frame** (README "The frame", ACCESS-13) —
//! `auth = "required"`, `request = MengerRequest`; every method's caller a
//! [`VerifiedUser`] the dispatch resolves before the body; every description
//! a `///` doc; every body a few lines through `members_activation/streams.rs`.
//!
//! ACCESS-22: `teams` is its child, `members_activation/teams.rs` —
//! `members.teams.<method>` on the wire — and `grant` / `revoke` take a
//! `team` in place of a `principal`.

use crate::members::{MembersEvent, Role};
// ACCESS-8: every parameter a type (MACRO-12's `typed_params` holds it).
use crate::vocab::{Principal, ScopeRef, TeamName};
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

// ACCESS-19: the methods' logic, beside the activation; ACCESS-22's child
// (not gated: its attribute emits `TeamsClient` in the client build) and
// its logic.
#[cfg(feature = "server")]
mod streams;
#[cfg(feature = "server")]
use streams::*;
mod teams;
pub use teams::TeamsClient;
#[cfg(feature = "server")]
mod team_streams;

/// The activation. Server-only: it holds the store and the rule.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Members {
    store: Arc<Store>,
    access: Access,
}

#[cfg(feature = "server")]
impl Members {
    pub fn new(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
/// Who holds which role on which shortcode, and the way admins change it.
#[plexus_macros::activation(
    namespace = "members",
    auth = "required",
    request = MengerRequest,
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core"
)]
impl Members {
    /// Who you are, then your teams, then each grant you hold (a team's as team:<name>).
    ///
    /// One `me { principal, name, admin_everywhere }`, then (ACCESS-22) one
    /// `team` per team the caller is in, then one `grant` per grant the
    /// caller holds — its own, and each team's (`principal: team:<name>`).
    /// Someone signed in without a grant gets `me` and nothing else.
    #[plexus_macros::method(group = "read")]
    pub async fn mine(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = MembersEvent> + Send + 'static {
        mine_stream(self.access.clone(), user)
    }

    /// The grants on the scopes you administer, by principal then scope.
    ///
    /// Every grant on the scopes the caller administers, by principal then
    /// scope (`--admin` ones included); `scope` narrows to one. Needs admin
    /// somewhere.
    #[plexus_macros::method(group = "read")]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Only this organization (acme:), shortcode, ticket (MENGER-12), subtree (under MENGER-12), or * (optional).
        scope: Option<ScopeRef>,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        list_stream(self.store.clone(), self.access.clone(), user, scope.map(String::from))
    }

    /// Give a principal or a team a role (viewer, editor, admin; deployer for a CI's key) on an organization, a shortcode in one, a ticket, a ticket's subtree, or *.
    ///
    /// Gives `principal` `role` on `scope` (a shortcode, `*`; ACCESS-21: a
    /// ticket alone, `MENGER-12`, or a ticket and everything under it,
    /// `under MENGER-12`, kept by the ticket's key so the grant follows it),
    /// replacing any role they held there. ACCESS-22: to `team` instead of
    /// `principal` (exactly one of the two), its members hold it. Needs
    /// admin on the scope, as the tree stands (ACCESS-11's `AdminOf`: for a
    /// ticket's scope, admin on a scope that covers it). Yields the grant as
    /// kept, or an in-band `error`.
    #[plexus_macros::method(
        group = "admin",
        target = "scope",
        values(role = ["viewer", "editor", "admin", "deployer"]),
        example(note = "make a person an editor on IDENTITY", params = r#"{"principal": "oidc:workos/user_01M4", "scope": "IDENTITY", "role": "editor"}"#),
        example(note = "make a team viewers of a ticket's subtree", params = r#"{"team": "release-crew", "scope": "under IDENTITY-12", "role": "viewer"}"#)
    )]
    pub async fn grant(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The Plexus principal, e.g. idp:c952acd3-…, oidc:auth0/google-oauth2|…, nostr:<hex> (or team).
        principal: Option<Principal>,
        /// ACCESS-22: a team instead of a principal, e.g. release-crew.
        team: Option<TeamName>,
        /// An organization (acme:), a shortcode in one (hypermemetic:IDENTITY, or IDENTITY — covers the shortcodes hosted under it), a ticket alone (IDENTITY-12), a ticket and its subtree (under IDENTITY-12), or *.
        scope: ScopeRef,
        /// viewer | editor | admin | deployer (a CI's key: the reads and deployments.record, DEPLOY-14).
        role: Role,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let (store, access) = (self.store.clone(), self.access.clone());
        futures::stream::once(async move { grant(store, access, user, (principal, team), scope.into(), role).await })
    }

    /// Take a principal's or a team's role on a scope (an organization, a shortcode, a ticket, a subtree, or *) away.
    ///
    /// Takes `principal`'s (ACCESS-22: or `team`'s) role on `scope` away.
    /// Needs admin on the scope, as the tree stands. Yields `revoked {
    /// principal, scope, existed }`.
    #[plexus_macros::method(group = "admin", target = "scope")]
    pub async fn revoke(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The Plexus principal (or team).
        principal: Option<Principal>,
        /// ACCESS-22: a team instead of a principal.
        team: Option<TeamName>,
        /// An organization (acme:), a shortcode in one, a ticket (IDENTITY-12), a subtree (under IDENTITY-12), or *.
        scope: ScopeRef,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let store = self.store.clone();
        futures::stream::once(async move { revoke(store, user, (principal, team), scope.into()).await })
    }

    /// Who signed in without a grant, waiting for an admin.
    ///
    /// Everyone who signed in and holds no grant yet, oldest first: one
    /// `waiting { principal, name, provider, since }` each. ACCESS-19: only
    /// an admin of `*` — the list is every sign-in, whatever shortcode it
    /// came for; anyone else is refused in band. Kept in memory, at most
    /// [`crate::access::WAITING_MAX`]: a restart forgets them until they
    /// come back.
    #[plexus_macros::method(group = "admin")]
    pub async fn requests(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = MembersEvent> + Send + 'static {
        requests_stream(self.access.clone(), user)
    }

    /// Teams — a named set of principals that holds grants (ACCESS-22): `members.teams.*`.
    #[plexus_macros::child]
    fn teams(&self) -> teams::Teams {
        teams::Teams { store: Arc::clone(&self.store), access: self.access.clone() }
    }
}
