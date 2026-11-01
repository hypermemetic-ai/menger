//! ARCHIVE-2: `archive` and `restore` as their streams — the selection
//! (ACT-2's: `ids`, or a filter in the caller's reach), an editor's permit
//! for every ticket the write marks (each descendant included: ACCESS-11),
//! then the store's one write ([`crate::store::ArchiveAnswer`]'s module).
//! And ARCHIVE-1's `child-of-archived`: the chain a create or a move under
//! an archived parent restores, its permits minted before the write it
//! follows. Kept beside the activation so its file keeps only its methods
//! (MENGER-29).

use super::*;
use crate::access::Permit;
use crate::filter::Filter;
use crate::store::{ArchiveAnswer, RestoreAnswer, Selection, UPDATE_MANY_MAX};

/// What an `archive` or a `restore` names: `ids`, or a filter (read in
/// `tz`, at most `max`).
pub(super) struct Picked {
    pub(super) ids: Option<Vec<TicketRef>>,
    pub(super) filter: Option<FilterText>,
    pub(super) max: Option<u32>,
    pub(super) tz: Option<Zone>,
}

/// The selection `picked` names, in `caller`'s reach — `archived`: a
/// restore's filter, ANDed with `archived` (it brings back only what is in
/// the archive).
fn selection(store: &Store, caller: &VerifiedUser, picked: Picked, archived: bool) -> Result<Selection, String> {
    match (picked.ids, picked.filter) {
        (Some(ids), None) if !ids.is_empty() => Ok(Selection::Ids(texts(ids))),
        (None, Some(filter)) => {
            let mut f = caller.scoped_text(&filter)?;
            if archived {
                f = f.and(&Filter::parse("archived").map_err(|e| e.to_string())?);
            }
            let lens = zoned_lens(store, caller, Some(&f), picked.tz.as_deref()).map_err(|e| e.to_string())?.unwrap_or_else(Lens::live);
            Ok(Selection::Filter { lens, max: picked.max.map_or(UPDATE_MANY_MAX, |m| m as usize) })
        }
        _ => Err("name the tickets: ids, or a filter — one of them".to_string()),
    }
}

/// ACCESS-11: an editor's permit for each of `ids` — every ticket the write
/// marks. Refused, the count outside the caller's reach, never their names
/// (ARCHIVE-1: a subtree may hold what the caller cannot see).
pub(super) fn marking(store: &Store, caller: &VerifiedUser, ids: &[String], what: &str) -> Result<Vec<Permit<crate::access::Editor>>, String> {
    let (mut out, mut outside) = (Vec::with_capacity(ids.len()), 0usize);
    for id in ids {
        match store.ticket_id(id).map(|k| caller.permit::<crate::access::Editor>(&crate::address::TicketId::new(k), store)) {
            Some(Ok(p)) => out.push(p),
            _ => outside += 1,
        }
    }
    match outside {
        0 => Ok(out),
        n => Err(format!("{what}: {n} of the {} tickets it would mark {} outside your reach; nothing written", ids.len(), if n == 1 { "is" } else { "are" })),
    }
}

/// ARCHIVE-2: `archive` — the selection, the permits, the write.
pub(super) fn archive_stream(store: Arc<Store>, caller: VerifiedUser, picked: Picked, act: Result<Act, String>, dry_run: bool) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let (select, act) = match selection(&store, &caller, picked, false).and_then(|s| Ok((s, act?))) {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let caller = Arc::new(caller);
        let (gate, at) = (Arc::clone(&caller), store.clone());
        let may = move |ids: &[String]| marking(&at, &gate, ids, "archive");
        match store.archive_by(select, act, dry_run, may).await {
            Ok(ArchiveAnswer { ids, live, unqueued, mark, dry_run }) => yield TicketsEvent::Archived { ids, live, unqueued, mark, dry_run },
            Err(e) => yield said(&store, &caller, e),
        }
    }
}

/// ARCHIVE-2: `restore` — the selection (of archived tickets), the permits,
/// the write.
pub(super) fn restore_stream(
    store: Arc<Store>,
    caller: VerifiedUser,
    picked: Picked,
    with_children: bool,
    act: Result<Act, String>,
    dry_run: bool,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let (select, act) = match selection(&store, &caller, picked, true).and_then(|s| Ok((s, act?))) {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let caller = Arc::new(caller);
        let (gate, at) = (Arc::clone(&caller), store.clone());
        let may = move |ids: &[String]| marking(&at, &gate, ids, "restore");
        match store.restore_by(select, with_children, act, dry_run, may).await {
            Ok(r) => yield restored(r),
            Err(e) => yield said(&store, &caller, e),
        }
    }
}

/// A restore's answer as its event.
fn restored(r: RestoreAnswer) -> TicketsEvent {
    let RestoreAnswer { ids, ancestors, still_archived, by, dry_run } = r;
    TicketsEvent::Restored { ids, ancestors, still_archived, by, dry_run }
}

/// ARCHIVE-1 `child-of-archived`: what a create or a move under `parent`
/// will restore — its archived chain, and the caller's permits for it,
/// minted before the write it follows (`Err`: the refusal, nothing
/// written). Empty when nothing above is archived.
pub(super) fn chain_of(store: &Store, caller: &VerifiedUser, parent: Option<&str>) -> Result<(Vec<String>, Vec<Permit<crate::access::Editor>>), String> {
    let chain = parent.map(|p| store.archived_chain(p)).unwrap_or_default();
    let permits = marking(store, caller, &chain, "restoring the archived parent chain")?;
    Ok((chain, permits))
}

/// [`chain_of`] for a create in `epic` under `parent` — or, without one,
/// under the shortcode's root (a new shortcode has none: nothing above).
pub(super) fn chain_under(store: &Store, caller: &VerifiedUser, epic: &str, parent: Option<&str>) -> Result<(Vec<String>, Vec<Permit<crate::access::Editor>>), String> {
    let anchor = match parent {
        Some(p) => Some(p.to_string()),
        None => store.root_of(epic).ok().flatten(),
    };
    chain_of(store, caller, anchor.as_deref())
}

/// …and, once that write is in, the chain brought back: its `restored`
/// event, or none when nothing of it is still archived.
pub(super) async fn restore_chain(store: &Store, caller: &VerifiedUser, (chain, permits): (Vec<String>, Vec<Permit<crate::access::Editor>>), why: String) -> Option<TicketsEvent> {
    if chain.is_empty() {
        return None;
    }
    let mut act = act_of(caller, None, None).ok()?;
    act.message = Some(why);
    match store.restore_chain_by(chain, act, permits).await {
        Ok(r) => r.map(restored),
        Err(e) => Some(said(store, caller, e)),
    }
}
