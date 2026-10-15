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

use crate::members::MembersEvent;
#[cfg(feature = "server")]
use crate::access::{Access, OPERATOR};
#[cfg(feature = "server")]
use crate::members::{Grant, Role};
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use plexus_core::plexus::{AuthContext, Reach};
#[cfg(feature = "server")]
use std::sync::Arc;

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

#[cfg(feature = "server")]
fn grant_event(grant: Grant) -> MembersEvent {
    MembersEvent::Grant { grant }
}

// Not `#[cfg]`-gated: the client-only build needs it too (see activation.rs).
#[plexus_macros::activation(
    namespace = "members",
    description = "Who holds which role on which shortcode, and the way admins change it",
    client,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core"
)]
impl Members {
    /// Who the caller is: one `me { principal, name, admin_everywhere }`,
    /// then one `grant` per grant they hold. Someone signed in without a
    /// grant gets `me` and nothing else.
    #[plexus_macros::method(group = "read", description = "Who you are, then each grant you hold")]
    pub async fn mine(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let admitted = self.access.admit(auth, &reach);
        let access = self.access.clone();
        stream! {
            let caller = match admitted {
                Ok(c) => c,
                Err(message) => {
                    yield MembersEvent::Error { message };
                    return;
                }
            };
            yield MembersEvent::Me {
                principal: caller.principal.clone(),
                name: caller.name.clone(),
                admin_everywhere: caller.administers("*"),
            };
            if let Some(p) = &caller.principal {
                for g in access.grants_of(p) {
                    yield grant_event(g);
                }
            }
        }
    }

    /// Every grant on the scopes the caller administers, by principal then
    /// scope (`--admin` ones included); `scope` narrows to one. Needs admin
    /// somewhere.
    #[plexus_macros::method(
        group = "read",
        description = "The grants on the scopes you administer, by principal then scope",
        params(scope = "Only this organization (acme:), shortcode, or * (optional)")
    )]
    pub async fn list(&self, auth: Option<&AuthContext>, reach: Reach, scope: Option<String>) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let admitted = self.access.admit(auth, &reach);
        let (store, access) = (self.store.clone(), self.access.clone());
        stream! {
            // AUTH-7: the gate admitted `members.list` held somewhere (admin
            // on a shortcode); the grants listed are those on scopes where it
            // is held, as it stands.
            if let Err(message) = admitted {
                yield MembersEvent::Error { message };
                return;
            }
            // ORG-4: a scope as the store keeps it (`hypermemetic:IDENTITY`).
            let only = match scope.as_deref().map(|s| store.scope_named(s)).transpose() {
                Ok(s) => s,
                Err(e) => {
                    yield MembersEvent::Error { message: e.to_string() };
                    return;
                }
            };
            let mut grants = store.grants();
            for admin in access.admins() {
                grants.extend(access.grants_of(&admin).into_iter().filter(|g| g.granted_by == OPERATOR));
            }
            grants.sort_by(|a, b| (&a.principal, &a.scope).cmp(&(&b.principal, &b.scope)));
            for g in grants {
                if only.as_ref().is_some_and(|o| *o != g.scope) || !reach.allows(&g.scope) {
                    continue;
                }
                yield grant_event(g);
            }
        }
    }

    /// Gives `principal` `role` on `scope` (a shortcode, or `*`), replacing
    /// any role they held there. Needs admin on the scope. Yields the grant
    /// as kept, or an in-band `error`.
    #[plexus_macros::method(
        group = "admin",
        target = "scope",
        description = "Give a principal a role (viewer, editor, admin; deployer for a CI's key) on an organization, a shortcode in one, or *",
        params(
            principal = "The Plexus principal, e.g. idp:c952acd3-…, oidc:auth0/google-oauth2|…, nostr:<hex>",
            scope = "An organization (acme:), a shortcode in one (hypermemetic:IDENTITY, or IDENTITY — covers the shortcodes hosted under it), or *",
            role = "viewer | editor | admin | deployer (a CI's key: the reads and deployments.record, DEPLOY-14)"
        ),
        values(role = ["viewer", "editor", "admin", "deployer"]),
        example(note = "make a person an editor on IDENTITY", params = r#"{"principal": "oidc:workos/user_01M4", "scope": "IDENTITY", "role": "editor"}"#)
    )]
    pub async fn grant(&self, auth: Option<&AuthContext>, reach: Reach, principal: String, scope: String, role: String) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let admitted = self.access.admit(auth, &reach);
        let (store, access) = (self.store.clone(), self.access.clone());
        stream! {
            let caller = match admitted {
                Ok(c) => c,
                Err(message) => {
                    yield MembersEvent::Error { message };
                    return;
                }
            };
            let (scope, role) = match (store.scope_named(&scope).map_err(|e| e.to_string()), role.parse::<Role>()) {
                (Ok(s), Ok(r)) => (s, r),
                (Err(message), _) | (_, Err(message)) => {
                    yield MembersEvent::Error { message };
                    return;
                }
            };
            let principal = principal.trim().to_string();
            if principal.is_empty() || principal.parse::<plexus_auth_core::identity::Principal>().is_err() {
                yield MembersEvent::Error { message: format!("`{principal}` is not a Plexus principal") };
                return;
            }
            let grant = Grant {
                principal,
                scope,
                role,
                granted_by: caller.principal.clone().unwrap_or_else(|| OPERATOR.into()),
                at: chrono::Utc::now().timestamp(),
            };
            match store.grant(grant).await {
                Ok(g) => {
                    access.granted(&g.principal);
                    yield grant_event(g)
                }
                Err(e) => yield MembersEvent::Error { message: e.to_string() },
            }
        }
    }

    /// Takes `principal`'s role on `scope` away. Needs admin on the scope.
    /// Yields `revoked { principal, scope, existed }`.
    #[plexus_macros::method(
        group = "admin",
        target = "scope",
        description = "Take a principal's role on a shortcode or * away",
        params(principal = "The Plexus principal", scope = "An organization (acme:), a shortcode in one, or *")
    )]
    pub async fn revoke(&self, auth: Option<&AuthContext>, reach: Reach, principal: String, scope: String) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let admitted = self.access.admit(auth, &reach);
        let store = self.store.clone();
        stream! {
            if let Err(message) = admitted {
                yield MembersEvent::Error { message };
                return;
            }
            let scope = match store.scope_named(&scope) {
                Ok(s) => s,
                Err(e) => {
                    yield MembersEvent::Error { message: e.to_string() };
                    return;
                }
            };
            let principal = principal.trim().to_string();
            match store.revoke(principal.clone(), scope.clone()).await {
                Ok(existed) => yield MembersEvent::Revoked { principal, scope, existed },
                Err(e) => yield MembersEvent::Error { message: e.to_string() },
            }
        }
    }

    /// Everyone who signed in and holds no grant yet, oldest first: one
    /// `waiting { principal, name, provider, since }` each. IDENTITY-37:
    /// needs admin on `*` — the list is every sign-in, whatever shortcode it
    /// came for. Kept in memory, at most [`crate::access::WAITING_MAX`]: a
    /// restart forgets them until they come back.
    #[plexus_macros::method(group = "admin", description = "Who signed in without a grant, waiting for an admin")]
    pub async fn requests(&self, auth: Option<&AuthContext>, reach: Reach) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let admitted = self.access.admit(auth, &reach);
        let access = self.access.clone();
        stream! {
            if let Err(message) = admitted {
                yield MembersEvent::Error { message };
                return;
            }
            // AUTH-7: held on `*` — the gate admitted it held somewhere.
            if !reach.is_everywhere() {
                yield MembersEvent::Error { message: "the waiting list needs admin on *".into() };
                return;
            }
            for w in access.waiting() {
                yield MembersEvent::Waiting { principal: w.principal, name: w.name, provider: w.provider, since: w.since };
            }
        }
    }
}
