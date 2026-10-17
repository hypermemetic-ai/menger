//! The `deployments` methods' logic — beside the activation, so each method
//! is its attribute, the call's admission and one of these (MENGER-29;
//! DEPLOY-25 moved `record`'s and `promotes`' here with its own `request`,
//! `cancel_request` and the webhook child's), and the helpers the reads
//! and `watch` share.

use crate::access::{Deployer, VerifiedUser};
use crate::deployments::{DeploymentsEvent, Record};
use crate::store::{RequestAsk, Store, StoreError};
use std::collections::HashSet;
use crate::vocab::WebhookSecret;
use futures::Stream;
use std::sync::Arc;

/// Who the caller is, as a request or a webhook records it.
fn by(caller: &VerifiedUser) -> String {
    caller.principal_text()
}

/// One answer, or the refusal in band.
fn once(answer: impl std::future::Future<Output = Result<DeploymentsEvent, String>> + Send + 'static) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
    futures::stream::once(async move { answer.await.unwrap_or_else(|message| DeploymentsEvent::Error { message }) })
}

/// The projects `caller` holds its call's grant on, of those deployments
/// were recorded for — `None`: every one (login off, or held on `*`).
pub(super) fn allowed(caller: &VerifiedUser, store: &Store) -> Result<Option<HashSet<String>>, String> {
    let Some(grant) = caller.grant().cloned() else { return Ok(None) };
    if caller.reach_filter()?.is_none() {
        return Ok(None);
    }
    Ok(Some(store.deploy_projects().into_iter().filter(|p| caller.check(&grant, p).is_ok()).collect()))
}

/// The project an event of `watch` is about.
pub(super) fn project_of(ev: &DeploymentsEvent) -> Option<&str> {
    match ev {
        DeploymentsEvent::Deployed { deployment, .. } | DeploymentsEvent::Deployment { deployment, .. } | DeploymentsEvent::Request { deployment, .. } => {
            Some(&deployment.project)
        }
        DeploymentsEvent::EnvironmentChanged { environment } | DeploymentsEvent::Environment { environment } => Some(&environment.project),
        DeploymentsEvent::Promotes { project, .. } => Some(project),
        _ => None,
    }
}

/// A refusal, in band.
pub(super) fn refused(e: StoreError) -> DeploymentsEvent {
    DeploymentsEvent::Error { message: e.to_string() }
}

/// `record`: deployer on the project's shortcode (ACCESS-11, DEPLOY-14),
/// then the store's write.
pub(super) fn record_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, rec: Record) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
    once(async move {
        let caller = admitted?;
        let scope = crate::activation::scope_named(&store, &rec.project)?;
        let on = caller.on::<Deployer>(&scope, &store).map_err(|r| r.to_string())?;
        store.record_deployment(on, rec, by(&caller)).await.map_err(|e| e.to_string())
    })
}

/// `promotes` (DEPLOY-21): admin on the project's shortcode, then the store's write.
pub(super) fn promotes_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, project: String, environment: String, from: Option<String>) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
    once(async move {
        let caller = admitted?;
        let scope = crate::activation::scope_named(&store, &project)?;
        let on = caller.admin_of(&scope, &store).map_err(|r| r.to_string())?;
        store.set_promotes(on, &project, &environment, from.as_deref(), by(&caller)).await.map_err(|e| e.to_string())
    })
}

/// `request`: deployer on the project (ACCESS-11), then the store's write.
pub(super) fn request_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, ask: RequestAsk) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
    once(async move {
        let caller = admitted?;
        let scope = crate::activation::scope_named(&store, &ask.project)?;
        let on = caller.on::<Deployer>(&scope, &store).map_err(|r| r.to_string())?;
        store.request_deployment(on, ask, by(&caller)).await.map_err(|e| e.to_string())
    })
}

/// `cancel_request`: deployer on the request's project, then the store's write.
pub(super) fn cancel_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, id: u64) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
    once(async move {
        let caller = admitted?;
        let project = store.request_project(id).ok_or_else(|| format!("no request {id} (nothing was canceled)"))?;
        let scope = crate::activation::scope_named(&store, &project)?;
        let on = caller.on::<Deployer>(&scope, &store).map_err(|r| r.to_string())?;
        store.cancel_request(on, id, by(&caller)).await.map_err(|e| e.to_string())
    })
}

/// What a webhook method does once its caller is admin on the project.
pub(super) enum HookCall {
    Set { url: String, secret: WebhookSecret },
    Clear,
    Show,
}

/// `webhook set|clear|show`: admin on the project (ACCESS-11), then the store.
pub(super) fn webhook_stream(store: Arc<Store>, admitted: Result<VerifiedUser, String>, project: String, environment: String, call: HookCall) -> impl Stream<Item = DeploymentsEvent> + Send + 'static {
    once(async move {
        let caller = admitted?;
        let scope = crate::activation::scope_named(&store, &project)?;
        let on = caller.admin_of(&scope, &store).map_err(|r| r.to_string())?;
        let answer = match call {
            HookCall::Set { url, secret } => store.set_webhook(on, &project, &environment, &url, secret.into_secret(), by(&caller)).await,
            HookCall::Clear => store.clear_webhook(on, &project, &environment).await,
            HookCall::Show => store.webhook(on, &project, &environment),
        };
        answer.map_err(|e| e.to_string())
    })
}
