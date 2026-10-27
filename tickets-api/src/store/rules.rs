//! MENGER-30: the rules a write after birth is refused by, each in one
//! place — asked by the single writes (`set_kind`, `set_status`,
//! `set_parent`; `create` for its blockers) and by `update`, which applies
//! several of them in one write and must refuse exactly what they refuse.

use super::*;

/// The distinct ids of `ids`, in the order first named.
pub(super) fn distinct<T: Clone + PartialEq>(ids: &[T]) -> Vec<T> {
    ids.iter().fold(Vec::new(), |mut seen, i| {
        if !seen.contains(i) {
            seen.push(i.clone());
        }
        seen
    })
}

/// SDLC-11: of `ids`, the distinct ones that name a doc — a doc never
/// completes, so nothing may wait on one.
pub(super) fn docs_among(cache: &Cache, ids: &[TicketName]) -> Vec<TicketName> {
    let docs: Vec<TicketName> = ids.iter().filter(|b| cache.get(b).is_some_and(|bt| !bt.kind.has_lifecycle())).cloned().collect();
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
pub(super) fn kind_refusal(cache: &Cache, row: &Row, blocked_by: &[TicketName], kind: Kind) -> Result<(), StoreError> {
    // TICKETS-6: a frozen scope stays one until it is unfrozen.
    if row.frozen && kind != Kind::Scope {
        return Err(StoreError::Frozen { scope: row.id.to_string(), numbers: None });
    }
    if !kind.has_lifecycle() && (!blocked_by.is_empty() || !row.unlocks.is_empty()) {
        return Err(StoreError::DocInGraph {
            blocked_by: distinct(blocked_by),
            unlocks: distinct(&row.unlocks),
            id: row.id.to_string(),
        });
    }
    if !kind.roots_a_shortcode() && cache.rollups.root(&row.shortcode) == Some(row.id.as_str()) {
        return Err(StoreError::RootKind { id: row.id.to_string(), epic: row.epic.to_string(), kind });
    }
    // ORG-5: a scope roots its shortcode — a ticket inside one that another
    // roots does not become a scope there; it moves out first.
    if kind == Kind::Scope && row.kind != Kind::Scope && !legacy_scopes() {
        if let Some(root) = shortcode_root(cache, &row.shortcode).filter(|r| r.id != row.id && r.seq < row.seq) {
            return Err(StoreError::ScopeNotRoot {
                id: row.id.to_string(),
                root: root.id.to_string(),
                parent: row.parent.as_ref().unwrap_or(&root.id).to_string(),
            });
        }
    }
    if let (Some(path), true) = (&row.path, kind.has_lifecycle()) {
        return Err(StoreError::PathKind { path: path.clone(), id: row.id.to_string(), kind });
    }
    Ok(())
}

/// SDLC-11: whether `row` may be set to `status` — a doc has no lifecycle,
/// and `Doc` is a doc's own.
///
/// TICKETS-30: and a ticket comes to `Complete` only when every child is —
/// the transition into it; a ticket already Complete is not asked again.
/// Canceled and Superseded children never complete and do not hold it; a
/// doc is not counted. The refusal names each open child.
pub(super) fn status_refusal(cache: &Cache, row: &Row, status: Status) -> Result<(), StoreError> {
    if !row.kind.has_lifecycle() {
        return Err(StoreError::DocHasNoLifecycle { id: row.id.to_string() });
    }
    if !Status::LIFECYCLE.contains(&status) {
        return Err(StoreError::DocStatusOnWork { id: row.id.to_string(), kind: row.kind });
    }
    if status == Status::Complete && row.status != Status::Complete {
        let open: Vec<String> = cache
            .children
            .of(&row.id)
            .into_iter()
            .filter_map(|(shortcode, seq)| cache.order.get(&OrderKey { shortcode, seq }))
            .filter(|c| c.kind.has_lifecycle() && !matches!(c.status, Status::Complete | Status::Canceled | Status::Superseded))
            .map(|c| c.id.to_string())
            .collect();
        if !open.is_empty() {
            return Err(StoreError::OpenChildren { id: row.id.to_string(), open });
        }
    }
    Ok(())
}

/// MNG-12/14: whether `child` may be filed under `parent` — which must
/// exist, be inside the child's shortcode unless the child is its
/// shortcode's root, and not make the child its own ancestor.
pub(super) fn parent_refusal(cache: &Cache, child: &Row, parent: &str) -> Result<(), StoreError> {
    let Some(parent_ticket) = cache.get(parent) else {
        return Err(StoreError::ParentNotFound { id: child.id.to_string(), parent: parent.to_string() });
    };
    let is_root = shortcode_root(cache, &child.shortcode).is_some_and(|r| r.id == child.id);
    if !is_root && parent_ticket.shortcode != child.shortcode {
        return Err(StoreError::ParentOutsideShortcode { id: child.id.to_string(), parent: parent.to_string() });
    }
    if let Some(path) = find_parent_cycle(cache, &child.id, parent) {
        return Err(StoreError::ParentCycle { id: child.id.to_string(), path });
    }
    frozen_refusal(cache, Some(parent), None)
}

/// TICKETS-6: whether a ticket may come to hang under `parent` (a birth or
/// a move) and — `numbered` — take a new number in that shortcode (a birth
/// or a reshortcode into it, never a move inside it): refused when the
/// parent is a frozen scope (its children, and the shortcodes hosted under
/// it, do not grow), else when the shortcode's root is (no new numbers).
pub(super) fn frozen_refusal(cache: &Cache, parent: Option<&str>, numbered: Option<&ShortcodeId>) -> Result<(), StoreError> {
    if let Some(p) = parent.and_then(|p| cache.get(p)).filter(|p| p.frozen) {
        return Err(StoreError::Frozen { scope: p.id.to_string(), numbers: None });
    }
    if let Some(root) = numbered.and_then(|sc| shortcode_root(cache, sc)).filter(|r| r.frozen) {
        return Err(StoreError::Frozen { scope: root.id.to_string(), numbers: Some(root.epic.to_string()) });
    }
    Ok(())
}

/// ORG-5: the refusal of a scope that would not root its shortcode — born
/// under `parent` (or with none) into `epic`, which `root` roots already.
pub(super) fn scope_not_root(parent: Option<&str>, epic: &str, root: &str) -> StoreError {
    StoreError::Invalid(match parent {
        Some(p) => format!("a scope under {p} roots its own shortcode; pass a new --epic (nothing was created)"),
        None => format!(
            "a scope roots its own shortcode, and {epic} is rooted by {root} — pass a new --epic, and --parent where it hangs (nothing was created)"
        ),
    })
}

#[cfg(test)]
thread_local! {
    /// ORG-5, tests only: a `set_kind` to a scope below a root let through,
    /// to make the shape a file from before ORG-5 holds (MENGER-2 inside
    /// MENGER) — what `reshortcode` moves out.
    pub(super) static LEGACY_SCOPES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn legacy_scopes() -> bool {
    #[cfg(test)]
    return LEGACY_SCOPES.with(std::cell::Cell::get);
    #[cfg(not(test))]
    false
}
