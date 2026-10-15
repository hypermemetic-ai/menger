//! MENGER-30: what `tickets.update` asks before the store — the call's
//! fields as the store's [`Update`] (ACCESS-8: typed at dispatch), and the reach a parent or a blocker in
//! another shortcode takes. ACT-2: and the writes over many tickets —
//! `update_many`, `label` and `commits.link` given `ids` — as their streams,
//! so the activation keeps only its methods (MENGER-29).

use super::*;
use crate::store::{Selection, Update, UpdatedMany, UPDATE_MANY_MAX};
use crate::vocab::CommitRef;

/// The call's fields as an [`Update`]. ACCESS-8: each one already a type
/// — an unknown kind, status or severity was refused at dispatch, naming
/// its parameter — so the store's collected refusal holds only the rules
/// that need the cache; `severity` `none` clears it.
pub(super) fn update_of(
    title: Option<Title>,
    body: Option<Body>,
    kind: Option<Kind>,
    status: Option<Status>,
    parent: Option<TicketRef>,
    severity: Option<SeverityChange>,
    block: Option<Vec<TicketRef>>,
    unblock: Option<Vec<TicketRef>>,
) -> Update {
    Update {
        title: title.map(String::from),
        body: body.map(String::from),
        kind,
        status,
        parent: parent.map(String::from),
        severity: severity.map(SeverityChange::severity),
        block: block.unwrap_or_default(),
        unblock: unblock.unwrap_or_default(),
        label: Vec::new(),
        unlabel: Vec::new(),
        refused: Vec::new(),
    }
}

/// ACT-2: a write `label` or `commits.link` makes, to one ticket or many.
pub(super) enum CellWrite {
    /// `add`, `remove`.
    Labels(Vec<Label>, Vec<Label>),
    Commits(Vec<CommitRef>),
}

/// ACT-2: `label` and `commits.link`, over `id` or `ids` — exactly one. One
/// ticket: the gate held the grant on it; one event, `not_found` for an id
/// that names none. Many: the caller's grant on every one asked first (one
/// missing refuses the write, naming it), then the store's one write, all
/// or none — one event per ticket, in the order named.
pub(super) fn cells_stream(
    store: Arc<Store>,
    caller: VerifiedUser,
    id: Option<TicketRef>,
    ids: Option<Vec<TicketRef>>,
    write: CellWrite,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let target = match one_or_many(id.map(String::from), ids.map(texts)) {
            Ok(t) => t,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let id = match target {
            Ok(id) => id,
            Err(ids) => {
                // ACCESS-11: an editor's permit per ticket, the ids as the
                // store resolves them; the store writes those.
                // An id that names no ticket refuses the write, by name, as
                // the store does (to one who would see it; to anyone else
                // as a hidden one, `permits`).
                let on = match permits(&store, &caller, &ids) {
                    Ok(on) if on.len() == ids.len() => on,
                    Ok(_) => {
                        let missing = ids.iter().filter(|id| store.ticket_id(id).is_none());
                        let each = missing.map(|id| (id.trim().to_ascii_uppercase(), vec![StoreError::NotFound(id.trim().to_ascii_uppercase()).to_string()])).collect();
                        yield TicketsEvent::Error { message: crate::store::refused_many(each) };
                        return;
                    }
                    Err(message) => {
                        yield TicketsEvent::Error { message };
                        return;
                    }
                };
                let written = match write {
                    CellWrite::Labels(add, remove) => store.label_many(on, add, remove).await,
                    CellWrite::Commits(commits) => store.link_commits_many(on, commits).await,
                };
                match written {
                    Ok(changes) => {
                        for change in changes {
                            yield change_event(change);
                        }
                    }
                    Err(e) => yield said(&store, &caller, e),
                }
                return;
            }
        };
        let on = match permit::<crate::access::Editor>(&store, &caller, &id) {
            Ok(on) => on,
            Err(ev) => {
                yield ev;
                return;
            }
        };
        let written = match write {
            CellWrite::Labels(add, remove) => store.label(on, add, remove).await,
            CellWrite::Commits(commits) => store.link_commits(on, commits).await,
        };
        match written {
            Ok(change) => yield change_event(change),
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: Asked::stored(id.as_str()) },
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// ACT-2: what a write over one ticket or many names — `id` or `ids`,
/// exactly one of them: `Ok(Ok(id))` one, `Ok(Err(ids))` many.
fn one_or_many(id: Option<String>, ids: Option<Vec<String>>) -> Result<Result<String, Vec<String>>, String> {
    match (id, ids) {
        (Some(id), None) => Ok(Ok(id)),
        (None, Some(ids)) if !ids.is_empty() => Ok(Err(ids)),
        _ => Err("name the tickets: id, or ids — one of them".into()),
    }
}

/// ACT-2: `update_many` — the selection (`ids`, or `filter` in the caller's
/// reach, read in `tz`, at most `max`), the caller's grant and an update's
/// reach on every ticket selected, then the store's one write.
pub(super) fn update_many_stream(
    store: Arc<Store>,
    caller: VerifiedUser,
    ids: Option<Vec<TicketRef>>,
    filter: Option<FilterText>,
    max: Option<u32>,
    tz: Option<Zone>,
    asked: Update,
    source: Option<CallerSource>,
    message: Option<Message>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let chosen = Ok::<_, String>(caller).and_then(|caller| {
            let select = match (ids, filter) {
                (Some(ids), None) => Selection::Ids(texts(ids)),
                (None, Some(filter)) => {
                    let text = caller.scoped_text(&filter)?;
                    let lens = zoned_lens(&store, &caller, Some(&text), tz.as_deref()).map_err(|e| e.to_string())?.unwrap_or_else(Lens::all);
                    Selection::Filter { lens, max: max.map_or(UPDATE_MANY_MAX, |m| m as usize) }
                }
                _ => return Err("name the tickets: ids, or a filter — one of them".to_string()),
            };
            let act = act_of(&caller, source, message)?;
            Ok((caller, select, act))
        });
        let (caller, select, act) = match chosen {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        // ARCHIVE-2: moved under an archived parent, its chain comes back after.
        let chain = match chain_of(&store, &caller, asked.parent.as_deref()) {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let caller = Arc::new(caller);
        // ACCESS-11: for each ticket selected, its editor's permit and the
        // proofs for the parent and blockers the update names.
        let (gate, reach_of, at) = (Arc::clone(&caller), asked.clone(), store.clone());
        let may = move |ids: &[String]| -> Result<Vec<crate::store::Updating>, String> {
            let on = permits::<crate::access::Editor>(&at, &gate, ids)?;
            on.into_iter().map(|p| updating(&at, &gate, p, &reach_of)).collect()
        };
        match store.update_many_by(select, asked, act, may).await {
            Ok(UpdatedMany { tickets, ids, unchanged, changed }) => {
                let moved = ids.len();
                yield TicketsEvent::UpdatedMany { ids, changed, tickets, unchanged };
                if moved > 0 {
                    if let Some(ev) = restore_chain(&store, &caller, chain, format!("{moved} ticket(s) were moved under it")).await {
                        yield ev;
                    }
                }
            }
            Err(e) => yield said(&store, &caller, e),
        }
    }
}
