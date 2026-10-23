//! ORG-13: what the `epics` methods do, beside the activation (its `impl`
//! keeps only the methods — MENGER-29, ACCESS-13's frame).

use crate::access::{AdminOf, VerifiedUser, Viewer};
use crate::address::{EpicName, OrgId};
use crate::epics::EpicsEvent;
use crate::members::Scope;
use crate::store::{Store, StoreError};
use futures::Stream;
use std::sync::Arc;

/// A store refusal as the one event it is — an epic or an organization
/// there is not is `not_found`.
fn refused(e: StoreError) -> EpicsEvent {
    match e {
        StoreError::NotFound(name) | StoreError::NoOrg(name) => EpicsEvent::NotFound { name },
        e => EpicsEvent::Error { message: e.to_string() },
    }
}

/// The organization a call reads in — `asked`, else the caller's own; one
/// it holds no role in is not found (ORG-4: another org's names are not a
/// stranger's to learn).
fn org_of(store: &Store, user: &VerifiedUser, asked: Option<OrgId>) -> Result<OrgId, EpicsEvent> {
    let org = match asked {
        Some(o) => store.org(o.as_str()).map(|x| x.name).ok_or(EpicsEvent::NotFound { name: o.to_string() })?,
        None => user.home_org(store).ok_or(EpicsEvent::Error { message: "the caller holds a role in no organization".into() })?,
    };
    match user.orgs().is_some_and(|held| !held.contains(&org)) {
        true => Err(EpicsEvent::NotFound { name: org.to_string() }),
        false => Ok(org),
    }
}

/// A read in the caller's organization: `f`'s events, or its refusal.
pub(super) fn read(
    store: Arc<Store>,
    user: VerifiedUser,
    org: Option<OrgId>,
    f: impl FnOnce(&Store, &OrgId, &str) -> Result<Vec<EpicsEvent>, StoreError>,
) -> impl Stream<Item = EpicsEvent> + Send + 'static {
    let events = match org_of(&store, &user, org) {
        Ok(org) => f(&store, &org, &user.principal_text()).unwrap_or_else(|e| vec![refused(e)]),
        Err(ev) => vec![ev],
    };
    futures::stream::iter(events)
}

/// `switch`: the caller's current epic in its organization — a viewer's
/// own setting there (ACCESS-11: `On<Viewer>` on the organization).
pub(super) fn switch(store: Arc<Store>, user: VerifiedUser, org: Option<OrgId>, name: EpicName) -> impl Stream<Item = EpicsEvent> + Send + 'static {
    async_stream::stream! {
        let proof = org_of(&store, &user, org).and_then(|org| {
            let on = user.on::<Viewer>(&Scope::Org(org.clone()), &store).map_err(|r| EpicsEvent::Error { message: r.to_string() })?;
            Ok((on, org))
        });
        yield match proof {
            Ok((on, org)) => store.switch_epic(on, &org, &name, &user.principal_text()).await.map(|epic| EpicsEvent::Current { epic }).unwrap_or_else(refused),
            Err(ev) => ev,
        };
    }
}

/// An admin's write on the caller's organization: `f` is given the store,
/// admin on it (ACCESS-11: `AdminOf`), the organization and the caller.
pub(super) fn admin<F, Fut>(store: Arc<Store>, user: VerifiedUser, org: Option<OrgId>, f: F) -> impl Stream<Item = EpicsEvent> + Send + 'static
where
    F: FnOnce(Arc<Store>, AdminOf, OrgId, String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<EpicsEvent, StoreError>> + Send + 'static,
{
    async_stream::stream! {
        let proof = org_of(&store, &user, org).and_then(|org| {
            let by = user.admin_of(&Scope::Org(org.clone()), &store).map_err(|_| EpicsEvent::Error {
                message: format!("{} needs admin on {org}: (or on *)", user.principal().as_str()),
            })?;
            Ok((by, org))
        });
        yield match proof {
            Ok((by, org)) => f(store.clone(), by, org, user.principal_text()).await.unwrap_or_else(refused),
            Err(ev) => ev,
        };
    }
}
