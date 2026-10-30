//! FILTER-46: `tickets filters` — the saved filters, a table of their own
//! (store/filters.rs): `list`, `get`, `save`, `rename`, `delete`, `check`,
//! at `tickets.filters.<method>`; a `#[plexus_macros::child]` of `tickets`
//! on the frame, mounted in `activation/mod.rs` as `fn filters`.
//!
//! The reads are a viewer's, the writes an editor's, in the caller's
//! organization (the first it holds a role in, as a chat outside tickets
//! opens in); a `save`'s refusals (a filter the language cannot read, a
//! cycle) come back in band, naming the chunk and where it stands. Each
//! filter is answered with the tickets it admits in the caller's reach
//! (`count`). A write is said to every open `watch` as `filter_saved` /
//! `filter_deleted`. Its tests are `store/tests_filters.rs` (the store) and
//! `tests/wire_filters.rs` (the wire).

#[cfg(feature = "server")]
use crate::saved_filters::SavedFilter;
use crate::types::TicketsEvent;
use crate::vocab::{FilterName, FilterSource};
#[cfg(feature = "server")]
use crate::access::{Access, Editor, MengerRequest, VerifiedUser};
#[cfg(feature = "server")]
use crate::address::OrgId;
#[cfg(feature = "server")]
use crate::members::Scope;
#[cfg(feature = "server")]
use crate::store::{Store, StoreError};
#[cfg(feature = "server")]
use futures::Stream;
#[cfg(feature = "server")]
use std::sync::Arc;

/// Saved filters: a name and a filter, listed, saved, renamed, deleted and checked.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct FiltersActivation {
    store: Arc<Store>,
    access: Access,
}

#[cfg(feature = "server")]
impl FiltersActivation {
    /// Over the parent's store and access rule.
    pub fn new(store: Arc<Store>, access: Access) -> Self {
        Self { store, access }
    }
}

/// Saved filters — `tickets.filters.<method>`: a name and a filter, listed, saved, renamed, deleted and checked.
#[plexus_macros::activation(
    auth = "required",
    request = MengerRequest,
    typed_params,
    roles(
        viewer = ["list", "get", "check"],
        editor = [viewer, "save", "rename", "delete"],
    )
)]
impl FiltersActivation {
    /// Every saved filter of the caller's organization, by name: its filter, how many tickets in reach it admits, who saved it and when (`Needs you` among them until one is saved under that name).
    #[plexus_macros::method(group = "filters")]
    pub async fn list(&self, #[from_auth(self.access.verify)] user: VerifiedUser) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(read(&self.store, &user, |org| Ok(self.store.saved_filters(org))))
    }

    /// One saved filter, by name (matched by its slug: `Open Builds` is `open-builds`), with its count; an in-band error when there is none.
    #[plexus_macros::method(group = "filters", example(note = "one saved filter", params = r#"{"name": "Shipped today"}"#))]
    pub async fn get(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The filter's name, e.g. 'Shipped today'.
        name: FilterName,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        futures::stream::iter(read(&self.store, &user, |org| self.store.saved_filter(org, &name).map(|f| vec![f])))
    }

    /// Save a filter under a name in the caller's organization — new, or replacing the one of that name (who first saved it is kept). Refused in band, nothing written: a filter the language cannot read (the chunk and where it stands are named), one holding `text`, a `saved "…"` that names none or makes a cycle. Yields filter_saved with the row.
    #[plexus_macros::method(
        group = "filters",
        example(note = "save a filter", params = r#"{"name": "Shipped today", "filter": "deployed app-prod today"}"#)
    )]
    pub async fn save(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The filter's name, e.g. 'Shipped today'; saving under a name that exists replaces it.
        name: FilterName,
        /// The filter, in FILTER-3's language, e.g. 'deployed app-prod today'.
        filter: FilterSource,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        write(self.store.clone(), user, move |store, on, org, by| async move {
            store.save_filter(on, &org, &name, &filter, by).await.map(|(f, _)| TicketsEvent::FilterSaved { filter: f, renamed_from: None })
        })
    }

    /// Give a saved filter another name, keeping its filter and who saved it; refused in band when the new name is another filter's. Yields filter_saved with `renamed_from`; a watch hears it and then filter_deleted with the old name. A filter that names the old name stops resolving.
    #[plexus_macros::method(group = "filters")]
    pub async fn rename(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The filter's name now.
        name: FilterName,
        /// The name it takes.
        to: FilterName,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        write(self.store.clone(), user, move |store, on, org, _| async move {
            let was = store.saved_filter(&org, &name)?.name;
            store.rename_filter(on, &org, &name, &to).await.map(|f| TicketsEvent::FilterSaved { filter: f, renamed_from: Some(was) })
        })
    }

    /// Delete a saved filter by name; an in-band error when there is none. Yields filter_deleted. A filter that names it stops resolving.
    #[plexus_macros::method(group = "filters")]
    pub async fn delete(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The filter's name.
        name: FilterName,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        write(self.store.clone(), user, move |store, on, org, _| async move {
            store.delete_filter(on, &org, &name).await.map(|name| TicketsEvent::FilterDeleted { name })
        })
    }

    /// Read a filter without saving it: yields filter_checked with the filter as canonical text and how many tickets in reach it admits — or an in-band error naming the chunk and where it stands, a name that does not resolve, a cycle. Writes nothing.
    #[plexus_macros::method(group = "filters", example(note = "check a filter", params = r#"{"filter": "deployed app-prod today"}"#))]
    pub async fn check(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// The filter, in FILTER-3's language.
        filter: FilterSource,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let one = match self.store.check_filter(None, &filter) {
            Ok(f) => counted(&self.store, &user, f.format()).map(|c| TicketsEvent::FilterChecked { filter: f.format(), count: c.unwrap_or(0) }),
            Err(e) => Err(e),
        };
        futures::stream::iter([one.unwrap_or_else(refused)])
    }
}

/// The in-band refusal.
#[cfg(feature = "server")]
fn refused(e: StoreError) -> TicketsEvent {
    TicketsEvent::Error { message: e.to_string() }
}

/// The caller's organization — the first it holds a role in.
#[cfg(feature = "server")]
fn org_of(store: &Store, user: &VerifiedUser) -> Result<OrgId, StoreError> {
    user.home_org(store).ok_or_else(|| StoreError::Filter("the caller holds a role in no organization".into()))
}

/// How many tickets in the caller's reach `text` admits — `None` when it does not resolve.
#[cfg(feature = "server")]
fn counted(store: &Store, user: &VerifiedUser, text: String) -> Result<Option<u64>, StoreError> {
    let Ok(lens) = store.lens(&text) else { return Ok(None) };
    Ok(Some(store.counts_read(user, &lens, None)?.into_iter().map(|(_, n)| n).sum()))
}

/// `rows` of the caller's organization as `filter_saved` events, each counted.
#[cfg(feature = "server")]
fn read(store: &Store, user: &VerifiedUser, rows: impl FnOnce(&OrgId) -> Result<Vec<SavedFilter>, StoreError>) -> Vec<TicketsEvent> {
    let org = match user.read_filter().map_err(StoreError::Filter).and_then(|_| org_of(store, user)) {
        Ok(org) => org,
        Err(e) => return vec![refused(e)],
    };
    let with_count = |mut f: SavedFilter| {
        f.count = counted(store, user, f.filter.clone()).ok().flatten();
        TicketsEvent::FilterSaved { filter: f, renamed_from: None }
    };
    rows(&org).map_or_else(|e| vec![refused(e)], |rows| rows.into_iter().map(with_count).collect())
}

/// A write by the caller as an editor of its organization: `f` is given the store, the proof, the organization and the caller's principal.
#[cfg(feature = "server")]
fn write<F, Fut>(store: Arc<Store>, user: VerifiedUser, f: F) -> impl Stream<Item = TicketsEvent> + Send + 'static
where
    F: FnOnce(Arc<Store>, crate::access::On<Editor>, OrgId, String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<TicketsEvent, StoreError>> + Send + 'static,
{
    async_stream::stream! {
        let proof = org_of(&store, &user).and_then(|org| {
            let on = user.on::<Editor>(&Scope::Org(org.clone()), &store).map_err(|r| StoreError::Filter(r.to_string()))?;
            Ok((on, org))
        });
        yield match proof {
            Ok((on, org)) => f(store.clone(), on, org, user.principal_text()).await.unwrap_or_else(refused),
            Err(e) => refused(e),
        };
    }
}
