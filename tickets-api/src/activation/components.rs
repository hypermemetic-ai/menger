//! ACCESS-18: `tickets components` — the component registry (COMP-3), a
//! child of `tickets` on the frame (README "The frame"): `list` (was
//! `tickets.components`). Reached as `tickets.components.list`; its grants
//! are `components.<method>`, listed in its own `roles(...)`, which the hub
//! gates and seeds by the path it is mounted at (`tickets.components.list`,
//! MACRO-20); the parent's client reaches it as `client.components()`
//! (MACRO-16).
//!
//! Every answer is through the caller's reach (ACCESS-10, IDENTITY-37): a
//! caller who cannot view every shortcode gets the components whose owner
//! it can view — no orphan — each with only the references it can view. So
//! the frames' component scripts are only those the viewer may read
//! (ACCESS-26 reads this).

use super::*;

/// The component registry child. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Components {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

// The attribute is NOT `#[cfg]`-gated: it has to run in the client-only
// build too, or there would be no `ComponentsClient`.
/// The component registry: every component a ticket defines, through the caller's reach (COMP-3).
#[plexus_macros::activation(
    namespace = "components",
    auth = "required",
    request = MengerRequest,
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    roles(viewer = ["list"], editor = [viewer])
)]
impl Components {
    /// Every component in the registry, in name order: its source, its owner (null: an orphan), what references it, its docs (summary, typed attributes, slots, examples, help; null without <docs>) and its preview attributes.
    ///
    /// COMP-3: the component registry. Yields one `Component` per component
    /// a ticket defines or once defined, in name order — its `source` (the
    /// ` ```component <name> ` block's content), its `owner` (`null` for an
    /// orphan: no ticket defines it any more, and something still uses it)
    /// and its `references` (the tickets whose bodies hold `<name` as an
    /// HTML tag, in the store's order, then the components whose sources
    /// do). COMP-8: and its help — `docs`, the source's `<docs>` parsed
    /// (summary, typed attributes, slots, examples, markdown help; `null`
    /// without one) — and `preview`, the attributes on the block's opening
    /// line. Nothing trails them — the stream's own `done` is the whole
    /// answer, as for `nav groups`. From the cache alone; `tickets.watch`
    /// keeps a client's copy current with `ComponentChanged` and
    /// `ComponentRemoved`. IDENTITY-37: a caller who cannot view every
    /// shortcode gets the components whose owner it can view — no orphan —
    /// each with only the references it can view.
    #[plexus_macros::method(group = "overview")]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        listed(&self.store, &user, |store, user| store.components(user).into_iter().map(|component| TicketsEvent::Component { component }).collect())
    }
}
