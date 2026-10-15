//! ACCESS-9: **`System`** — the capability of acting for no user: the
//! builder's startup, `--verify-derived`, seeding, the access layer's own
//! reads of the grants. ACCESS-10: every store read takes a
//! [`crate::access::VerifiedUser`] or this ([`super::Reader`]); the
//! system's reads answer everything.
//!
//! Only this module mints one, once per store, at its open; whoever opened
//! the store takes it ([`Store::system`]) and hands it on (the builder to
//! the access layer, [`crate::access::Access::attach_with`]). No `Clone`, no
//! `Default`, no `Deserialize`, no public constructor:
//!
//! ```compile_fail
//! // A System cannot be built outside the store: its field is private.
//! let forged = menger_tickets_api::store::System(());
//! ```

use super::*;

/// Holding one proves the holder is the code that opened the store — the
/// one caller that may act for no user. Minted once per store
/// ([`Store::system`]).
#[derive(Debug)]
pub struct System(());

impl System {
    /// The one minting: the store's own, at its open.
    pub(super) fn minted() -> Self {
        Self(())
    }

    /// A `System` for a test in another module (compiled out of the server).
    #[cfg(any(test, feature = "test-support"))]
    pub const fn for_tests() -> Self {
        Self(())
    }
}

/// ACCESS-10: the [`System`] a test of this crate reads as — `store.get(SYS,
/// id)` — [`System::for_tests`], once.
#[cfg(test)]
pub(crate) const SYS: &System = &System::for_tests();

impl Store {
    /// ACCESS-9: the store's one [`System`], to whoever opened it — `Some`
    /// the first time, `None` after: a second taker gets nothing. The
    /// builder takes it as it attaches the store to the access layer
    /// ([`crate::access::Access::attach`]).
    pub fn system(&self) -> Option<System> {
        self.system.lock().expect("system lock poisoned").take()
    }

    /// ACCESS-9: the grants generation — moved by every grant, revoke and
    /// team change ([`members::Members`]); what a reach records and a
    /// watch compares (ACCESS-12). From the cache.
    pub fn grants_generation(&self) -> u64 {
        self.tickets.read().expect("store lock poisoned").members.generation()
    }

    /// ACCESS-9: the teams `principal` is in — the input a reach adds each
    /// team's grants from (`team:<name>`'s). From the cache; empty until
    /// ACCESS-22 keeps teams.
    pub fn teams_of(&self, principal: &str) -> Vec<crate::vocab::TeamName> {
        self.tickets.read().expect("store lock poisoned").members.teams_of(principal)
    }

    /// ACCESS-9, tests only until ACCESS-22 stores teams: `principal` in
    /// `team` or out of it, in the cache alone (nothing is written).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_team_for_tests(&self, principal: &str, team: crate::vocab::TeamName, member: bool) {
        let generation = {
            let mut cache = self.tickets.write().expect("store lock poisoned");
            cache.members.set_team(principal, team, member);
            cache.members.generation()
        };
        self.tx.grants_at(generation);
    }

    /// ACCESS-12: what wakes an open watch when the grants generation moves
    /// — a receiver of the store's own; the watch reads the generation
    /// itself ([`Store::grants_generation`]).
    pub fn grants_moved(&self) -> tokio::sync::watch::Receiver<u64> {
        self.tx.grants.subscribe()
    }
}
