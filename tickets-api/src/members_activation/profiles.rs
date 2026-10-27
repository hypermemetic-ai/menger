//! IDENTITY-46: `members profiles` — what each principal's sign-in told the
//! server about them (provider, how they signed in, email, name, picture),
//! kept by the store ([`crate::store::Store::note_profile`]) so an admin
//! finds who signed in and how after a restart, when the waiting list
//! (in memory) has forgotten them.
//!
//! A `#[plexus_macros::child]` of `members` on the frame (ACCESS-13, README
//! "The frame"), as `teams` is: `members.profiles.<method>` on the wire,
//! its client `ProfilesClient`. It lists no role — only admin holds its
//! grants — and each method answers an admin of `*` alone: every sign-in
//! is everyone's, whatever shortcode it came for.

use crate::members::MembersEvent;
use crate::vocab::Principal;
#[cfg(feature = "server")]
use crate::access::{Access, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The principals' profiles (IDENTITY-46).
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct ProfilesActivation {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

// Not `#[cfg]`-gated: the client-only build needs `ProfilesClient` too.
/// Profiles: what each principal's sign-in said of them — provider, method, email, name, picture.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
)]
impl ProfilesActivation {
    /// Everyone who signed in, newest sign-in first; one profile each, with holds_grant (false: waiting). Admin of * only.
    ///
    /// One `profile { profile }` per principal the server has seen sign
    /// in: `provider`, `method` (`githuboauth`, `email_code`, `key`, …),
    /// `email`, `name`, `picture`, `signed_in_at`, `first_seen`, and
    /// `holds_grant` — whether it holds a grant of its own. Anyone but an
    /// admin of `*` is refused in band.
    #[plexus_macros::method(group = "admin")]
    pub async fn list(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let events = answer(&self.store, &self.access, &user, None);
        futures::stream::iter(events)
    }

    /// One principal's profile. Admin of * only.
    ///
    /// One `profile { profile }`, or an in-band `error` when the server
    /// has never seen it sign in.
    #[plexus_macros::method(group = "admin")]
    pub async fn get(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The principal, e.g. oidc:workos/user_01… or key:54a9….
        principal: Principal,
    ) -> impl Stream<Item = MembersEvent> + Send + 'static {
        let events = answer(&self.store, &self.access, &user, Some(principal.as_str()));
        futures::stream::iter(events)
    }
}

/// The profiles `user` may read — every one, or `principal`'s — each with
/// whether it holds a grant of its own; a refusal in band.
#[cfg(feature = "server")]
fn answer(store: &Store, access: &Access, user: &VerifiedUser, principal: Option<&str>) -> Vec<MembersEvent> {
    let Ok(admin) = user.admin_of(&crate::members::Scope::Everywhere, store) else {
        return vec![MembersEvent::Error { message: "profiles need admin on *".into() }];
    };
    let found = match principal {
        None => store.profiles(&admin),
        Some(p) => match store.profile(&admin, p) {
            Some(found) => vec![found],
            None => return vec![MembersEvent::Error { message: format!("{p} has not signed in here") }],
        },
    };
    found
        .into_iter()
        .map(|mut profile| {
            profile.holds_grant = !access.own_grants_of(&profile.principal).is_empty();
            MembersEvent::Profile { profile }
        })
        .collect()
}
