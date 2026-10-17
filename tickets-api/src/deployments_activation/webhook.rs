//! DEPLOY-25: `deployments webhook` — each environment's webhook: where a
//! request (`deployments request`) is POSTed, and the secret its HMAC
//! signature is keyed by. `set`, `clear`, `show`; the secret is never said
//! back, only whether one is set.
//!
//! A `#[plexus_macros::child]` of `deployments`: mounted by
//! `DeploymentsActivation::webhook`, so its methods are
//! `deployments.webhook.<method>` on the wire, its client `WebhookClient`
//! (reached from the parent's as `client.webhook()`). It lists no role:
//! only admin holds its grants (admin holds every grant), and each method
//! asks admin on the project's shortcode itself (`streams.rs`).

use crate::deployments::DeploymentsEvent;
use crate::vocab::{EnvironmentName, Project, WebhookSecret, WebhookUrl};
#[cfg(feature = "server")]
use super::streams::{webhook_stream, HookCall};
#[cfg(feature = "server")]
use crate::access::Access;
#[cfg(feature = "server")]
use crate::store::Store;
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use plexus_core::plexus::{AuthContext, Reach};
#[cfg(feature = "server")]
use std::sync::Arc;

/// The webhooks — the store's, behind the same rule as the parent.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct WebhookActivation {
    store: Arc<Store>,
    access: Access,
}

#[cfg(feature = "server")]
impl WebhookActivation {
    pub(super) fn new(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

// Not `#[cfg]`-gated: the client-only build needs `WebhookClient` too.
#[plexus_macros::activation(
    description = "An environment's webhook, where deploy requests are POSTed signed (admin): set, clear, show",
    typed_params,
)]
impl WebhookActivation {
    /// Sets (or replaces) `project`'s `environment`'s webhook: every
    /// request made after it is POSTed to `url` as JSON, with
    /// `X-Menger-Signature: sha256=<hex HMAC-SHA256 of the body keyed by
    /// secret>` and `X-Menger-Delivery: <id>`. The URL is https, or http to
    /// this machine (a local CI). Yields exactly one `webhook { webhook }` —
    /// the URL and `secret_set: true`, never the secret — or an in-band
    /// `error`, nothing written. Admin on the project.
    #[plexus_macros::method(
        group = "admin",
        target = "project",
        description = "Set an environment's webhook (admin): url (https, or http to loopback) and the secret that signs each delivery; yields webhook { url, secret_set }",
        params(
            project = "Whose environment: a shortcode, any case (menger is MENGER)",
            environment = "The environment, e.g. prod",
            url = "Where to POST: https://…, or http://127.0.0.1:PORT/… for a local CI",
            secret = "The HMAC key, 16 to 256 bytes — pass it from a file (-p @params.json); it is never shown back"
        ),
        example(note = "a local CI's receiver", params = r#"{"project": "menger", "environment": "test", "url": "http://127.0.0.1:6091/menger", "secret": "<16+ bytes>"}"#)
    )]
    pub async fn set(
        &self, auth: Option<&AuthContext>, reach: Reach,
        project: Project,
        environment: EnvironmentName,
        url: WebhookUrl,
        secret: WebhookSecret,
    ) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        let call = HookCall::Set { url: url.into_string(), secret };
        webhook_stream(self.store.clone(), self.access.verify_call(auth, &reach), project.into_string(), environment.into_string(), call)
    }

    /// Clears `project`'s `environment`'s webhook: a request made after it
    /// is kept and says "no CI configured"; a delivery still due fails at
    /// its next attempt, saying so. Yields exactly one `webhook { webhook }`
    /// (`url: null`) or an in-band `error`. Admin on the project.
    #[plexus_macros::method(
        group = "admin",
        target = "project",
        description = "Clear an environment's webhook (admin); yields webhook { url: null, secret_set: false }",
        params(project = "Whose environment: a shortcode, any case", environment = "The environment, e.g. prod")
    )]
    pub async fn clear(&self, auth: Option<&AuthContext>, reach: Reach, project: Project, environment: EnvironmentName) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        webhook_stream(self.store.clone(), self.access.verify_call(auth, &reach), project.into_string(), environment.into_string(), HookCall::Clear)
    }

    /// `project`'s `environment`'s webhook: its URL, whether a secret is set,
    /// who set it when — never the secret. Yields exactly one `webhook
    /// { webhook }` (`url: null` when none is set) or an in-band `error`.
    /// Admin on the project.
    #[plexus_macros::method(
        group = "admin",
        target = "project",
        description = "Show an environment's webhook (admin): url, secret_set, set_at, set_by — never the secret",
        params(project = "Whose environment: a shortcode, any case", environment = "The environment, e.g. prod")
    )]
    pub async fn show(&self, auth: Option<&AuthContext>, reach: Reach, project: Project, environment: EnvironmentName) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
        webhook_stream(self.store.clone(), self.access.verify_call(auth, &reach), project.into_string(), environment.into_string(), HookCall::Show)
    }
}
