//! ACCESS-12: **an open watch's caller, followed** — the [`VerifiedUser`] a
//! subscription holds, the [`AuthContext`] it was verified from, and the
//! gate's reach it was admitted with, verified again whenever something
//! that could change the answer moves, and only then:
//!
//! - the **grants generation** ([`Store::grants_generation`]) moved past the
//!   one the user's reach was resolved at — a grant, a revoke, a team or
//!   an organization changed, a role's grants edited;
//! - a **sign-out** through this access layer ([`Access::logout`]), or its
//!   session gone from the table however it went (asked at each change, and
//!   every [`SESSION_LOOK_EVERY`] of a quiet watch: a lookup, no store read);
//! - its sign-in's **expiry** came.
//!
//! [`Following::changed`] waits for any of them (the watch selects on it
//! beside its channel, so a revoke ends a quiet watch at once);
//! [`Following::again`] answers what they did — the same reach, a reach
//! that moved ([`Followed::Moved`]: the watch recomputes what it hears and
//! says what entered and left), or [`Closed`] with a typed reason
//! ([`WatchEnd`]). Nothing here holds a lock across an await: each ask
//! reads the cache and lets go.
//!
//! ```text
//!   [*] → Streaming ──(generation moved / signed out / expired)──→ Reverifying
//!   Reverifying ──admitted──→ Streaming      (reach moved: entered / left)
//!   Reverifying ──refused───→ Closed { reason }  → [*]
//! ```

use super::*;
use crate::types::WatchEnd;

/// How often a quiet watch whose identity is a session asks whether the
/// session still stands — the sign-outs that do not pass through
/// [`Access::logout`] (the shell's `/logout`, the login backend's own).
/// A liveness lookup: no store read.
pub const SESSION_LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(15);

/// Why a watch ends: its reason and the words for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closed {
    pub reason: WatchEnd,
    pub message: String,
}

impl Closed {
    fn of(r: Refusal) -> Self {
        let reason = match &r {
            Refusal::SignedOut => WatchEnd::SignedOut,
            Refusal::Expired => WatchEnd::Expired,
            Refusal::Nowhere { .. } | Refusal::OutsideCeiling { .. } | Refusal::Lacks(_) | Refusal::Unreadable(_) => WatchEnd::Revoked,
            Refusal::SignIn | Refusal::NoPrincipal | Refusal::Ceiling(_) => WatchEnd::Refused,
        };
        Self { reason, message: format!("your access changed: {r}") }
    }

    /// The grants moved and the reach holds nothing any more.
    pub fn revoked(why: impl std::fmt::Display) -> Self {
        Self { reason: WatchEnd::Revoked, message: format!("your access changed: {why}") }
    }
}

/// What [`Following::again`] found.
#[derive(Debug)]
pub enum Followed {
    /// Nothing that moves what the watch hears.
    Same,
    /// Verified again, and where the call's grant is held moved: the user
    /// is the one verified now; `was`, the one before.
    Moved { was: VerifiedUser },
}

/// An open watch's caller, as the grants stand.
pub struct Following {
    access: Access,
    store: Arc<Store>,
    who: Option<AuthContext>,
    reach: GateReach,
    user: VerifiedUser,
    key: Option<ReachKey>,
    grants: tokio::sync::watch::Receiver<u64>,
    ended: tokio::sync::watch::Receiver<u64>,
    expires: Option<i64>,
    /// A wake [`Following::changed`] saw, for [`Following::again`] to act on.
    woke: bool,
}

impl Following {
    /// The caller of a watch, verified now — its refusal in words, as the
    /// method answers it before the stream starts. The wakes are
    /// subscribed first, so nothing moving after this verify is missed.
    pub fn open(access: &Access, store: &Arc<Store>, who: Option<&AuthContext>, reach: &GateReach) -> Result<Self, String> {
        let (mut grants, mut ended) = (store.grants_moved(), access.ended.subscribe());
        grants.mark_unchanged();
        ended.mark_unchanged();
        let user = access.verify_call(who, reach)?;
        let key = user.reach_key()?;
        let expires = who.and_then(|c| c.metadata.get("expires_at")).and_then(|v| v.as_i64());
        Ok(Self { access: access.clone(), store: Arc::clone(store), who: who.cloned(), reach: reach.clone(), user, key, grants, ended, expires, woke: false })
    }

    /// The caller, as it was last verified.
    pub fn user(&self) -> &VerifiedUser {
        &self.user
    }

    /// Waits until something that could change the caller moves — the
    /// grants generation, a sign-out, the sign-in's expiry. Cancel-safe:
    /// the watch selects on it beside its channel. With login off, never.
    pub async fn changed(&mut self) {
        if !self.access.login() {
            return std::future::pending().await;
        }
        let until = self.expires.map(|at| {
            let secs = at.saturating_sub(chrono::Utc::now().timestamp()).max(0) as u64;
            tokio::time::Instant::now() + std::time::Duration::from_secs(secs)
        });
        let expiry = async {
            match until {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        // A session signed out by a path that does not pass through this
        // access layer (the session table's own) is found within this.
        let session = self.who.as_ref().and_then(|c| Some(c.get_metadata_string(self.access.session_key()?)?)).is_some();
        tokio::select! {
            r = self.grants.changed() => if r.is_err() { std::future::pending::<()>().await },
            r = self.ended.changed() => if r.is_err() { std::future::pending::<()>().await },
            () = expiry => {}
            () = tokio::time::sleep(SESSION_LOOK_EVERY), if session => return,
        }
        self.woke = true;
    }

    /// The caller as it stands: verified again only when the generation
    /// moved past the user's, or a wake came — else its sign-in's liveness
    /// asked (the expiry, the session table), nothing more. `Closed` when it
    /// is refused, or its reach reaches nothing.
    pub fn again(&mut self) -> Result<Followed, Closed> {
        if !self.access.login() {
            return Ok(Followed::Same);
        }
        let ctx = self.who.as_ref().filter(|c| c.is_authenticated()).ok_or_else(|| Closed::of(Refusal::SignIn))?;
        let woke = std::mem::take(&mut self.woke);
        // The sign-in first: signed out or expired is said as such, before
        // the gate (which refuses it too) is asked.
        self.access.liveness(ctx).map_err(Closed::of)?;
        if !woke && self.store.grants_generation() == self.user.reach().generation() {
            return Ok(Followed::Same);
        }
        // AUTH-7: the gate's live reach — held nowhere any more, the watch
        // ends.
        let reach = self.reach.current().ok_or_else(|| Closed::revoked("you hold this nowhere any more"))?;
        let user = self.access.verify(ctx).map_err(Closed::of)?.for_call(reach.grant().cloned());
        let key = user.reach_key().map_err(Closed::revoked)?;
        // A reach no read can narrow to reads nothing: the watch ends.
        user.read_filter().map_err(Closed::revoked)?;
        let moved = key != self.key;
        let was = std::mem::replace(&mut self.user, user);
        self.key = key;
        Ok(if moved { Followed::Moved { was } } else { Followed::Same })
    }
}
