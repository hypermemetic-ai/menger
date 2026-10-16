//! ACCESS-15: `tickets commits` — the commits that finished a ticket
//! (SDLC-9): `link` (was `tickets link_commits`), `unlink` (was
//! `tickets unlink_commits`).
//!
//! A `#[plexus_macros::child]` of `tickets` on the frame (ACCESS-13, README
//! "The frame"): mounted by `Tickets::commits`, so its methods are
//! `tickets.commits.link` / `tickets.commits.unlink` on the wire and its
//! client is `CommitsClient`, reached from the parent's as
//! `client.commits()` (MACRO-16). Every method takes the `VerifiedUser`
//! the dispatch resolves, typed parameters and `///` docs; its body is the
//! permit and the store's answer, through the helpers beside
//! `activation/mod.rs` (`answered!`, `cells_stream`).
//!
//! Its grants: the child's own `roles(...)` say `commits.<method>`; the hub
//! gates a child's call by its whole path (`tickets.commits.<method>`, with
//! the method's own `target`) and seeds its roles qualified the same way
//! (MACRO-20: plexus-core's `DynamicHub` indexes a static child; the call
//! reaches it with the caller whole — principal, session, ceiling).

use crate::types::TicketsEvent;
use crate::vocab::{CommitRef, TicketRef};
#[cfg(feature = "server")]
use super::*;
#[cfg(feature = "server")]
use crate::access::Access;
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use std::sync::Arc;

/// The commits that finished a ticket: linked and unlinked (SDLC-9).
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Commits {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

/// The commits that finished a ticket — link them, unlink them (SDLC-9).
#[plexus_macros::activation(
    namespace = "commits",
    auth = "required",
    request = MengerRequest,
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    roles(editor = ["link", "unlink"])
)]
impl Commits {
    /// Link commits to a ticket, or to many (ids; one write, all or none) — the commits that finished it; yields commits_linked { ticket, added } per ticket.
    ///
    /// SDLC-9: records which commits finished a ticket. Each entry is a
    /// commit reference — a hexadecimal object name of 7 to 64 characters
    /// (stored lowercase), optionally prefixed by a repository and `@`
    /// (`77df9a0`, `plexus-macros@4f7114a`) — compared as text: the server
    /// has no repository, so link the form you want kept. Every entry not
    /// already on the ticket is appended, in the order given. Yields
    /// exactly one event: `CommitsLinked { ticket, added }` — `added` is
    /// what was newly linked, empty when nothing was (every entry already
    /// linked, or none given), which writes and broadcasts nothing —
    /// `NotFound` if `id` names no ticket, or an in-band `Error` for a link
    /// past 256 commits on the ticket; an entry that is not a reference is
    /// `invalid_params` naming it, before the method runs. An error writes
    /// nothing. Moves `updated_unix` and nothing else — not the status.
    /// `watch` carries every `CommitsLinked` that added something. ACT-2:
    /// `ids` instead of `id` links them to each, in one write, all or none,
    /// one `CommitsLinked` per ticket (`cells_stream`).
    #[plexus_macros::method(
        group = "write",
        target = "id",
        example(note = "link the commit that finished it", params = r#"{"id": "MENGER-40", "commits": ["77df9a0"]}"#),
        example(note = "and one in another repository", params = r#"{"id": "MENGER-40", "commits": ["77df9a0", "plexus-macros@4f7114a"]}"#)
    )]
    pub async fn link(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. MNG-10 (or ids).
        id: Option<TicketRef>,
        /// Ticket ids, instead of id: every one gets the commits, in one write, all or none.
        ids: Option<Vec<TicketRef>>,
        /// Commit references, in link order: a hex object name of 7-64 characters, optionally repo@ before it — e.g. ["77df9a0", "plexus-macros@4f7114a"].
        commits: Vec<CommitRef>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        // AUTH-7: the gate held the grant on `id`; `ids` are asked there.
        cells_stream(self.store.clone(), user, id, ids, CellWrite::Commits(commits))
    }

    /// Unlink commits from a ticket; yields commits_unlinked { ticket, removed }.
    ///
    /// SDLC-9: the inverse of `link` — every entry the ticket carries comes
    /// off. The same references, compared the same way, all or nothing.
    /// Yields exactly one event: `CommitsUnlinked { ticket, removed }` —
    /// `removed` empty when none of them was linked, which writes and
    /// broadcasts nothing — or `NotFound`; an entry that is not a reference
    /// is `invalid_params` naming it. `watch` carries every
    /// `CommitsUnlinked` that removed something.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        example(note = "take a commit off", params = r#"{"id": "MENGER-40", "commits": ["77df9a0"]}"#)
    )]
    pub async fn unlink(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. MNG-10.
        id: TicketRef,
        /// Commit references to remove, as linked (the hash is compared lowercase).
        commits: Vec<CommitRef>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        answered!(store = self.store.clone(), user, Some(id.to_string()) => {
            let on = permit::<Editor>(&store, &user, &id)?;
            Ok(change_event(store.unlink_commits(on, commits).await?))
        })
    }
}
