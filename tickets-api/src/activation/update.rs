//! MENGER-30: what `tickets.update` asks before the store — the call's
//! fields as the store's [`Update`] (an unknown kind, status or severity
//! named with the store's refusals), and the reach a parent or a blocker in
//! another shortcode takes.

use super::*;
use crate::store::Update;

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
    let home = shortcode_of(id.trim()).to_ascii_uppercase();
    for b in named {
        let epic = shortcode_of(b).to_ascii_uppercase();
        if visible.as_ref().is_some_and(|v| !v.contains(&epic)) {
            return Err(format!("blocked_by: {}", caller.lacks(&Grant::new(crate::access::SEES), Some(b))));
        }
        if let (true, Some(g), Some(ticket)) = (epic != home, caller.grant.as_ref(), store.ticket_id(b)) {
            caller.check(g, &ticket)?;
        }
    }
    Ok(())
}
