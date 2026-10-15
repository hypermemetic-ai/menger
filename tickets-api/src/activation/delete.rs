//! ACT-5: `delete` and `undelete` as their streams — the selection (ACT-2's:
//! `ids`, or a filter in the caller's reach), the caller's grant on every
//! ticket the write touches, then the store's one write. ACT-6:
//! `reshortcode`'s too — the grant on every ticket that moves and on where
//! they go (editor on both shortcodes). Kept beside the activation so its
//! file keeps only its methods (MENGER-29).

use super::*;
use crate::store::{Children, Deleted, Deletion, Reshortcode, Reshortcoded, Selection, Undeleted, UPDATE_MANY_MAX};

/// ACT-5: `delete` — what to delete, how, and the store's write.
pub(super) fn delete_stream(
    store: Arc<Store>,
    admitted: Result<Caller, String>,
    ids: Option<Vec<String>>,
    filter: Option<String>,
    max: Option<u32>,
    tz: Option<String>,
    children: Option<String>,
    unblock: Option<bool>,
    message: Option<String>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let chosen = admitted.and_then(|caller| {
            let select = match (ids, filter) {
                (Some(ids), None) if !ids.is_empty() => Selection::Ids(ids),
                (None, Some(filter)) => {
                    let text = caller.scoped_text(&filter)?;
                    let lens = zoned_lens(&store, &caller, Some(&text), tz.as_deref()).map_err(|e| e.to_string())?.unwrap_or_else(Lens::all);
                    Selection::Filter { lens, max: max.map_or(UPDATE_MANY_MAX, |m| m as usize) }
                }
                _ => return Err("name the tickets: ids, or a filter — one of them".to_string()),
            };
            let children = children.as_deref().map(str::parse::<Children>).transpose()?;
            let act = act_of(&caller, None, message)?;
            Ok((caller, select, Deletion { children, unblock: unblock.unwrap_or(false) }, act))
        });
        let (caller, select, how, act) = match chosen {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let sight = sight_lens(&caller).ok().flatten();
        let (gate, at) = (caller.clone(), store.clone());
        let may = move |ids: &[String]| gate.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &at);
        match store.delete_by(select, how, act, may).await {
            Ok(Deleted { ids, tombstones }) => yield TicketsEvent::Deleted { ids, tombstones },
            Err(e) => yield TicketsEvent::Error { message: redacted(e, |id| sees_id(&store, sight.as_ref(), id)) },
        }
    }
}

/// ACT-5: `undelete` — the caller's grant on each id, then the store's write.
pub(super) fn undelete_stream(store: Arc<Store>, admitted: Result<Caller, String>, ids: Vec<String>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let caller = match admitted {
            Ok(caller) => caller,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let act = match act_of(&caller, None, None) {
            Ok(a) => a,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let (gate, at) = (caller.clone(), store.clone());
        let may = move |ids: &[String]| gate.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &at);
        match store.undelete_by(&ids, act, may).await {
            Ok(Undeleted { ids, tickets }) => yield TicketsEvent::Undeleted { ids, tickets },
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// ACT-6: `reshortcode` — who asks, the grants, and the store's write.
pub(super) fn reshortcode_stream(
    store: Arc<Store>,
    admitted: Result<Caller, String>,
    mut ask: Reshortcode,
    message: Option<String>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let (caller, act) = match admitted.and_then(|caller| act_of(&caller, None, message).map(|act| (caller, act))) {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        // ORG-4: the ids, `to` and `parent` read in the caller's organizations.
        ask.orgs = caller.orgs();
        let sight = sight_lens(&caller).ok().flatten();
        let (gate, at) = (caller.clone(), store.clone());
        let may = move |ids: &[String]| gate.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &at);
        match store.reshortcode_by(ask, act, may).await {
            Ok(Reshortcoded { moves, tickets }) => yield TicketsEvent::Reshortcoded { moves, tickets },
            Err(e) => yield TicketsEvent::Error { message: redacted(e, |id| sees_id(&store, sight.as_ref(), id)) },
        }
    }
}
