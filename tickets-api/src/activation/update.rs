//! MENGER-30: what `tickets.update` asks before the store — the call's
//! fields as the store's [`Update`] (an unknown kind, status or severity
//! named with the store's refusals), and the reach a parent or a blocker in
//! another shortcode takes. ACT-2: and the writes over many tickets —
//! `update_many`, `label` and `link_commits` given `ids` — as their streams,
//! so the activation keeps only its methods (MENGER-29).

use super::*;
use crate::store::{Selection, Update, UpdatedMany, UPDATE_MANY_MAX};

/// The call's fields as an [`Update`]. `severity` `none` clears it.
pub(super) fn update_of(
    title: Option<String>,
    body: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    parent: Option<String>,
    severity: Option<String>,
    block: Option<Vec<String>>,
    unblock: Option<Vec<String>>,
) -> Update {
    fn parsed<T>(r: Option<Result<T, String>>, refused: &mut Vec<String>) -> Option<T> {
        match r {
            Some(Err(e)) => {
                refused.push(e);
                None
            }
            Some(Ok(v)) => Some(v),
            None => None,
        }
    }
    let mut refused = Vec::new();
    let kind = parsed(kind.as_deref().map(str::parse::<Kind>), &mut refused);
    let status = parsed(status.as_deref().map(str::parse::<Status>), &mut refused);
    let severity = parsed(severity.as_deref().map(|s| match s.trim() {
        s if s.eq_ignore_ascii_case("none") => Ok(None),
        s => s.parse::<Severity>().map(Some).map_err(|e| format!("{e} — or none, to clear it")),
    }), &mut refused);
    Update {
        title,
        body,
        kind,
        status,
        parent,
        severity,
        block: block.unwrap_or_default(),
        unblock: unblock.unwrap_or_default(),
        label: Vec::new(),
        unlabel: Vec::new(),
        refused,
    }
}

/// The reach an update takes beyond its target (the gate held its grant on
/// `id`): a parent as `set_parent` takes it — a shortcode's root moving
/// takes admin on it and on its new host, any other ticket the grant on
/// both sides —, and a blocker as `create` takes it (one the caller cannot
/// view is refused, by its shortcode) and, in another shortcode, the grant
/// there too.
pub(super) fn update_reach(store: &Store, caller: &Caller, id: &str, update: &Update) -> Result<(), String> {
    if let Some(parent) = &update.parent {
        let root = store.groups().into_iter().any(|g| g.root.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(id.trim())));
        let wanted = if root { Some(Grant::new(ADMINISTER)) } else { caller.grant.clone() };
        if let Some(g) = wanted {
            caller.check(&g, id).and_then(|()| caller.check(&g, parent.trim())).map_err(|message| {
                if root {
                    format!("moving a shortcode's root takes admin on it and on its new host: {message}")
                } else {
                    message
                }
            })?;
        }
    }
    let named = update.block.iter().chain(&update.unblock).map(|b| b.trim()).filter(|b| !b.is_empty());
    let visible = sight_epics(store, caller)?;
    // ORG-3: each by the shortcode its address names it in.
    let home = store.shortcode_of(id.trim());
    for b in named {
        let epic = store.shortcode_of(b);
        if visible.as_ref().is_some_and(|v| epic.as_ref().is_none_or(|e| !v.contains(e))) {
            return Err(format!("blocked_by: {}", caller.lacks(&Grant::new(crate::access::SEES), Some(b))));
        }
        if let (true, Some(g), Some(ticket)) = (epic != home, caller.grant.as_ref(), store.ticket_id(b)) {
            caller.check(g, &ticket)?;
        }
    }
    Ok(())
}

/// ACT-2: a write `label` or `link_commits` makes, to one ticket or many.
pub(super) enum CellWrite {
    /// `add`, `remove`.
    Labels(Vec<String>, Vec<String>),
    Commits(Vec<String>),
}

/// ACT-2: `label` and `link_commits`, over `id` or `ids` — exactly one. One
/// ticket: the gate held the grant on it; one event, `not_found` for an id
/// that names none. Many: the caller's grant on every one asked first (one
/// missing refuses the write, naming it), then the store's one write, all
/// or none — one event per ticket, in the order named.
pub(super) fn cells_stream(
    store: Arc<Store>,
    admitted: Result<Caller, String>,
    id: Option<String>,
    ids: Option<Vec<String>>,
    write: CellWrite,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let (caller, target) = match admitted.and_then(|c| one_or_many(id, ids).map(|t| (c, t))) {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let id = match target {
            Ok(id) => id,
            Err(ids) => {
                if let Err(message) = caller.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &store) {
                    yield TicketsEvent::Error { message };
                    return;
                }
                let sight = sight_lens(&caller).ok().flatten();
                let written = match write {
                    CellWrite::Labels(add, remove) => store.label_many(&ids, add, remove).await,
                    CellWrite::Commits(commits) => store.link_commits_many(&ids, commits).await,
                };
                match written {
                    Ok(changes) => {
                        for change in changes {
                            yield change_event(change);
                        }
                    }
                    Err(e) => yield TicketsEvent::Error { message: redacted(e, |id| sees_id(&store, sight.as_ref(), id)) },
                }
                return;
            }
        };
        let written = match write {
            CellWrite::Labels(add, remove) => store.label(&id, add, remove).await,
            CellWrite::Commits(commits) => store.link_commits(&id, commits).await,
        };
        match written {
            Ok(change) => yield change_event(change),
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id },
            Err(e) => yield refusal(e),
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
    admitted: Result<Caller, String>,
    ids: Option<Vec<String>>,
    filter: Option<String>,
    max: Option<u32>,
    tz: Option<String>,
    asked: Update,
    source: Option<String>,
    message: Option<String>,
) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        let chosen = admitted.and_then(|caller| {
            let select = match (ids, filter) {
                (Some(ids), None) => Selection::Ids(ids),
                (None, Some(filter)) => {
                    let text = caller.scoped_text(&filter)?;
                    let lens = zoned_lens(&caller, Some(&text), tz.as_deref()).map_err(|e| e.to_string())?.unwrap_or_else(Lens::all);
                    Selection::Filter { lens, max: max.map_or(UPDATE_MANY_MAX, |m| m as usize) }
                }
                _ => return Err("name the tickets: ids, or a filter — one of them".to_string()),
            };
            let act = act_of(&caller, source.as_deref(), message)?;
            Ok((caller, select, act))
        });
        let (caller, select, act) = match chosen {
            Ok(c) => c,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let sight = sight_lens(&caller).ok().flatten();
        let (gate, reach_of, at) = (caller.clone(), asked.clone(), store.clone());
        let may = move |ids: &[String]| -> Result<(), String> {
            gate.require(&ids.iter().map(String::as_str).collect::<Vec<_>>(), &at)?;
            if reach_of.parent.is_some() || !reach_of.block.is_empty() || !reach_of.unblock.is_empty() {
                for id in ids {
                    update_reach(&at, &gate, id, &reach_of)?;
                }
            }
            Ok(())
        };
        match store.update_many_by(select, asked, act, may).await {
            Ok(UpdatedMany { tickets, ids, unchanged, changed }) => yield TicketsEvent::UpdatedMany { ids, changed, tickets, unchanged },
            Err(e) => yield TicketsEvent::Error { message: redacted(e, |id| sees_id(&store, sight.as_ref(), id)) },
        }
    }
}
