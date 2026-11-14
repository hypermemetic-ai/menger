//! TICKETS-39: **who made each write**, as `watch { actor }` hears it.
//!
//! A write runs as its caller ([`super::revisions::acting`]: the act the
//! activation verified — the gate's principal, never a name a client or a
//! body supplies), and its recording holds that act from before its
//! transaction to after its events. Each change the write broadcasts goes,
//! while an actor watch is open, on a channel of its own with that writer
//! ([`Wrote`]); a change no call's write made (a store's own pass) carries
//! none, and no actor watch hears it.

use super::{Broadcast, Change};
use super::history::Act;
use std::sync::Arc;

/// Who made a write: the caller's principal and its display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writer {
    pub actor: String,
    pub name: Option<String>,
}

impl Writer {
    /// The writer an act is.
    pub(super) fn of(act: &Act) -> Self {
        Self { actor: act.actor.clone(), name: act.name.clone() }
    }
}

/// A change, with who made the write it is of (`None`: no call's write).
#[derive(Debug, Clone)]
pub struct Wrote {
    pub by: Option<Arc<Writer>>,
    pub change: Change,
}

impl Wrote {
    /// Whether the write was `actor`'s — by principal, exactly.
    pub fn by(&self, actor: &str) -> bool {
        self.by.as_deref().is_some_and(|w| w.actor == actor)
    }
}

impl Broadcast {
    /// Sends `change` with the writer of the write on this thread — only
    /// while an actor watch is open, so a store with none pays nothing.
    pub(super) fn wrote(&self, change: &Change) {
        if self.wrote.receiver_count() > 0 {
            let _ = self.wrote.send(Wrote { by: super::revisions::writer(), change: change.clone() });
        }
    }
}
