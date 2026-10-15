//! MENGER-30: the rules a write after birth is refused by, each in one
//! place — asked by the single writes (`set_kind`, `set_status`,
//! `set_parent`; `create` for its blockers) and by `update`, which applies
//! several of them in one write and must refuse exactly what they refuse.

use super::*;

/// The distinct ids of `ids`, in the order first named.
pub(super) fn distinct(ids: &[String]) -> Vec<String> {
    ids.iter().fold(Vec::new(), |mut seen, i| {
        if !seen.contains(i) {
            seen.push(i.clone());
        }
        seen
    })
}

/// SDLC-11: of `ids`, the distinct ones that name a doc — a doc never
/// completes, so nothing may wait on one.
pub(super) fn docs_among(cache: &Cache, ids: &[String]) -> Vec<String> {
    let docs: Vec<String> = ids.iter().filter(|b| cache.get(b).is_some_and(|bt| !bt.kind.has_lifecycle())).cloned().collect();
    distinct(&docs)
}

/// SDLC-13: the status a ticket of `previous` kind, at `current`, takes in
/// becoming `kind` with `status` asked: across the doc line, `Doc` for a doc
/// (any other status refused), and from a doc the status given (never
/// `Doc`), else `Epic` for a scope and `Pending` otherwise; between two
/// kinds of work its own, unless one is given.
pub(super) fn kind_status(id: &str, previous: Kind, current: Status, kind: Kind, status: Option<Status>) -> Result<Status, StoreError> {
    if kind.has_lifecycle() {
        match status {
            Some(Status::Doc) => Err(StoreError::DocStatusForWork { id: id.to_string(), kind }),
            Some(s) => Ok(s),
            None if previous.has_lifecycle() => Ok(current),
            None if kind == Kind::Scope => Ok(Status::Epic),
            None => Ok(Status::Pending),
        }
    } else {
        match status {
            None | Some(Status::Doc) => Ok(Status::Doc),
            Some(s) => Err(StoreError::DocStatusForDoc { id: id.to_string(), status: s }),
        }
    }
}

/// SDLC-13: whether `row` may become `kind`, waiting on `blocked_by` (its
/// own, or what the write leaves it): a doc is never in the `blocked_by`
/// graph at either end; a shortcode's root stays a kind that roots one; a
/// path holder stays a doc.
pub(super) fn kind_refusal(cache: &Cache, row: &Row, blocked_by: &[String], kind: Kind) -> Result<(), StoreError> {
    if !kind.has_lifecycle() && (!blocked_by.is_empty() || !row.unlocks.is_empty()) {
        return Err(StoreError::DocInGraph {
            blocked_by: distinct(blocked_by),
            unlocks: distinct(&row.unlocks),
            id: row.id.clone(),
        });
    }
    if !kind.roots_a_shortcode() && cache.rollups.root(&row.shortcode) == Some(row.id.as_str()) {
        return Err(StoreError::RootKind { id: row.id.clone(), epic: row.epic.to_string(), kind });
    }
    if let (Some(path), true) = (&row.path, kind.has_lifecycle()) {
        return Err(StoreError::PathKind { path: path.clone(), id: row.id.clone(), kind });
    }
    Ok(())
}

/// SDLC-11: whether `row` may be set to `status` — a doc has no lifecycle,
/// and `Doc` is a doc's own.
pub(super) fn status_refusal(row: &Row, status: Status) -> Result<(), StoreError> {
    if !row.kind.has_lifecycle() {
        return Err(StoreError::DocHasNoLifecycle { id: row.id.clone() });
    }
    if !Status::LIFECYCLE.contains(&status) {
        return Err(StoreError::DocStatusOnWork { id: row.id.clone(), kind: row.kind });
    }
    Ok(())
}

/// MNG-12/14: whether `child` may be filed under `parent` — which must
/// exist, be inside the child's shortcode unless the child is its
/// shortcode's root, and not make the child its own ancestor.
pub(super) fn parent_refusal(cache: &Cache, child: &Row, parent: &str) -> Result<(), StoreError> {
    let Some(parent_ticket) = cache.get(parent) else {
        return Err(StoreError::ParentNotFound { id: child.id.clone(), parent: parent.to_string() });
    };
    let is_root = shortcode_root(cache, &child.shortcode).is_some_and(|r| r.id == child.id);
    if !is_root && parent_ticket.shortcode != child.shortcode {
        return Err(StoreError::ParentOutsideShortcode { id: child.id.clone(), parent: parent.to_string() });
    }
    if let Some(path) = find_parent_cycle(cache, &child.id, parent) {
        return Err(StoreError::ParentCycle { id: child.id.clone(), path });
    }
    Ok(())
}
