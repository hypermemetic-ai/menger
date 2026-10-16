//! ACCESS-11: what a write asks of its caller before the store — the proofs
//! the store's writes take, minted through the [`VerifiedUser`] for exactly
//! the tickets (or scopes) the write names, as the tree stands: a
//! [`Permit`] per ticket, a [`Filing`] for a birth, a [`Parenting`] for a
//! move, an [`Updating`] for an update, an [`AdminOf`] or an [`On`] for a
//! scope. What the caller may not see is answered as what is not there
//! ([`Hidden`]); the store checks, under its write lock, that the proofs
//! are for what it writes. Kept beside the activation so its file keeps
//! only its methods (MENGER-29).

use super::*;
use crate::access::{AdminOf, Editor, Hidden, Permit, RoleMark, Viewer};
use crate::address::TicketId;
use crate::members::Scope;
use crate::vocab::TicketRef;
use crate::store::{Blocker, Filing, Parenting, Rehost, Under, Update, Updating};

/// The key `id` names, as a write resolves it (any address): `Err` the
/// event — `not_found` (named as asked), or the ambiguity as `caller` may
/// hear it.
pub(super) fn key(store: &Store, caller: &VerifiedUser, id: &str) -> Result<TicketId, TicketsEvent> {
    match store.resolve(id) {
        Ok(k) => Ok(TicketId::new(k)),
        Err(StoreError::NotFound(_)) => Err(TicketsEvent::NotFound { id: Asked::stored(id.trim()) }),
        Err(e) => Err(veiled(caller, e)),
    }
}

/// A role `R` permit for the ticket `id` names — `not_found` for one that
/// is not there and for one `caller` may not have, alike.
pub(super) fn permit<R: RoleMark>(store: &Store, caller: &VerifiedUser, id: &str) -> Result<Permit<R>, TicketsEvent> {
    let k = key(store, caller, id)?;
    caller.permit::<R>(&k, store).map_err(|Hidden| TicketsEvent::NotFound { id: Asked::stored(id.trim()) })
}

/// [`permit`] for a ticket a write names beside its target: refused in
/// words — the call's grant on the id's shortcode, as the gate says it —
/// the same for a ticket not there as for one out of reach.
fn beside<R: RoleMark>(store: &Store, caller: &VerifiedUser, id: &str, what: &str) -> Result<Permit<R>, String> {
    beside_as::<R>(store, caller, id, what, false)
}

/// ACCESS-21: [`beside`] for a parent — `below`: the role held on what is
/// below it ([`VerifiedUser::permit_below`]), never on the ticket alone.
fn beside_as<R: RoleMark>(store: &Store, caller: &VerifiedUser, id: &str, what: &str, below: bool) -> Result<Permit<R>, String> {
    let refused = || {
        let g = caller.grant().cloned().unwrap_or_else(|| Grant::new(what));
        caller.check(&g, id.trim()).err().unwrap_or_else(|| caller.lacks(&g, Some(id.trim())))
    };
    let k = store.resolve(id).map_err(|e| match e {
        e @ StoreError::Ambiguous { .. } => e.to_string(),
        _ => refused(),
    })?;
    let k = TicketId::new(k);
    match below {
        true => caller.permit_below::<R>(&k, store),
        false => caller.permit::<R>(&k, store),
    }
    .map_err(|Hidden| refused())
}

/// A blocker `caller` names: a ticket it can view, or — one who sees every
/// ticket — a name not born yet; else refused by the id's shortcode, so a
/// hidden ticket and an id naming none get the same answer (IDENTITY-37).
/// `editor`: and, in another shortcode than `home`, the call's grant on it
/// (a write that moves a blocker's `unlocks` there — `update`'s rule).
fn blocker(store: &Store, caller: &VerifiedUser, name: &str, home: Option<&crate::address::ShortcodeId>, editor: bool) -> Result<Blocker, String> {
    let name = name.trim();
    let hidden = || format!("blocked_by: {}", caller.lacks(&Grant::new(crate::access::SEES), Some(name)));
    let Some(k) = store.ticket_id(name) else {
        return caller.forward(name, store).map(Blocker::Forward).map_err(|Hidden| hidden());
    };
    let seen = caller.permit::<Viewer>(&TicketId::new(k.clone()), store).map_err(|Hidden| hidden())?;
    if editor && store.shortcode_of(&k).as_ref() != home {
        if let Some(g) = caller.grant() {
            caller.check(g, &k)?;
            caller.permit::<Editor>(&TicketId::new(k.clone()), store).map_err(|Hidden| caller.lacks(g, Some(&k)))?;
        }
    }
    Ok(Blocker::Seen(seen))
}

/// IDENTITY-21 / AUTH-7 / ACCESS-11: the [`Filing`] a `create` in `epic`
/// under `parent` takes — an editor's permit on the parent, or on the
/// shortcode's root; a shortcode with neither, editor on `*` (ORG-4: or on
/// the organization the epic names); and a [`Blocker`] per `blocked_by`
/// entry. Or the event to answer.
pub(super) fn filing(store: &Store, caller: &VerifiedUser, epic: &str, parent: Option<&str>, blocked_by: Option<&[TicketRef]>) -> Result<Filing, TicketsEvent> {
    let error = |message: String| TicketsEvent::Error { message };
    let anchor = match parent {
        Some(p) => Some(p.to_string()),
        None => store.root_of(epic).map_err(|e| veiled(caller, e))?,
    };
    let under = match anchor {
        Some(a) => match store.ticket_id(a.trim()) {
            // ACCESS-21: editor on what is below the parent — `under` it or
            // wider; editor on the parent alone files nothing under it.
            Some(_) => Under::Parent(beside_as::<Editor>(store, caller, &a, "tickets.create", true).map_err(error)?),
            // A parent that is no ticket: the store says so — to one who
            // sees every ticket; to anyone else it is out of reach.
            None => match caller.forward(&a, store) {
                Ok(f) => Under::Named(f),
                Err(Hidden) => return Err(error(beside::<Editor>(store, caller, &a, "tickets.create").err().unwrap_or_default())),
            },
        },
        None => {
            // A new shortcode in a named organization is held on it (`acme:`).
            let org = crate::address::Address::parse(epic).ok().and_then(|a| a.org);
            let scope = org.clone().map_or(Scope::Everywhere, Scope::Org);
            let on = caller.on::<Editor>(&scope, store).map_err(|_| {
                let g = caller.grant().map_or_else(|| "tickets.create".to_string(), |g| g.to_string());
                error(format!("a new shortcode {} needs {g} on {}", epic.to_ascii_uppercase(), org.map_or("*".to_string(), |o| format!("{o}:"))))
            })?;
            Under::Founding(on)
        }
    };
    let blockers = blocked_by.into_iter().flatten().map(|b| blocker(store, caller, b, None, false)).collect::<Result<Vec<_>, _>>().map_err(error)?;
    Ok(Filing::new(under, blockers))
}

/// MNG-12 / IDENTITY-37 / ACCESS-11: what moving the ticket `child` (a key)
/// under `parent` takes — editor on the parent it has now and on the one it
/// goes to; a shortcode's root re-hosts its shortcode, so admin on the host
/// it leaves (itself, hanging nowhere) and on the one it joins (ACCESS-3
/// #2). `Err` the refusal in words; a parent that is no ticket, the
/// store's own answer (`None`: let the store say it).
pub(super) fn parenting(store: &Store, caller: &VerifiedUser, child: &TicketId, parent: &str) -> Result<Option<Parenting>, String> {
    let Some(to) = store.ticket_id(parent.trim()) else {
        // Not there: to one who sees every ticket, the store's refusal;
        // to anyone else, as a ticket out of reach.
        return match caller.forward(parent, store) {
            Ok(_) => Ok(None),
            Err(Hidden) => Err(beside::<Editor>(store, caller, parent, "tickets.set_parent").err().unwrap_or_default()),
        };
    };
    let now = store.parent_of(child);
    let root = store.is_root(child);
    let what = "tickets.set_parent";
    let rehost = match root {
        false => None,
        true => {
            // A host the caller cannot see is not named (ACCESS-3 #8).
            let admin = |id: &str| -> Result<AdminOf, String> {
                let hidden = || format!("{} needs admin on a host out of your reach", caller.principal().map_or("you", |p| p.as_str()));
                let scope = store.shortcode_scope_of(id).ok_or_else(hidden)?;
                let seen = matches!(caller.sight_filter(), Ok(None)) || sees_id(store, &caller.sight(), id);
                caller.admin_of(&scope, store).map_err(|r| if seen { r.to_string() } else { hidden() })
            };
            let moving = |m: String| format!("moving a shortcode's root takes admin on the host it leaves and on its new host: {m}");
            let leaving = admin(now.as_ref().map_or(child.as_str(), |p| p.as_str())).map_err(moving)?;
            let joining = admin(&to).map_err(moving)?;
            Some(Rehost { leaving, joining })
        }
    };
    let from = now.map(|p| beside::<Editor>(store, caller, p.as_str(), what)).transpose()?;
    // ACCESS-21: the new parent as a parent — what is below it.
    let to = beside_as::<Editor>(store, caller, &to, what, true)?;
    Ok(Some(Parenting::new(from, to, rehost)))
}

/// MENGER-30 / ACCESS-11: the [`Updating`] an update of `id` (a key) takes
/// — its permit, a [`Parenting`] for a parent it names (as `set_parent`
/// takes it), and a [`Blocker`] per id it blocks or unblocks (as `create`
/// takes it — and, in another shortcode, the call's grant there too).
pub(super) fn updating(store: &Store, caller: &VerifiedUser, ticket: Permit<Editor>, update: &Update) -> Result<Updating, String> {
    let id = ticket.id().clone();
    let parent = match update.parent.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => parenting(store, caller, &id, p)?,
        None => None,
    };
    let home = store.shortcode_of(&id);
    let named = update.block.iter().chain(&update.unblock).map(|b| b.trim()).filter(|b| !b.is_empty());
    let blockers = named.map(|b| blocker(store, caller, b, home.as_ref(), true)).collect::<Result<Vec<_>, _>>()?;
    Ok(Updating::new(ticket, parent, blockers))
}

/// ACT-2 / ACT-5 / ACT-6: a role `R` permit for each of `ids` (the store's
/// selection) — the first refused names it as the call's grant would. An
/// id that names no ticket has none: to one who would see it, the store
/// refuses it by name; to anyone else it is refused as a hidden one is.
pub(super) fn permits<R: RoleMark>(store: &Store, caller: &VerifiedUser, ids: &[String]) -> Result<Vec<Permit<R>>, String> {
    let lacks = |id: &str| match caller.grant() {
        Some(g) => caller.lacks(g, Some(id)),
        None => format!("no ticket {id}"),
    };
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        match store.ticket_id(id) {
            Some(k) => out.push(caller.permit::<R>(&TicketId::new(k), store).map_err(|Hidden| lacks(id))?),
            None if caller.forward(id, store).is_ok() => {}
            None => return Err(lacks(id)),
        }
    }
    Ok(out)
}

/// ACT-5 / ACCESS-11: admin, for each deleted id, on its shortcode's scope.
pub(super) fn undeleting(store: &Store, caller: &VerifiedUser, ids: &[String]) -> Result<Vec<(String, AdminOf)>, String> {
    ids.iter()
        .map(|id| {
            let g = caller.grant().cloned().unwrap_or_else(|| Grant::new(ADMINISTER));
            let scope = store.deleted_scope(id).ok_or_else(|| caller.lacks(&g, Some(id)))?;
            let a = caller.admin_of(&scope, store).map_err(|_| caller.lacks(&g, Some(id)))?;
            Ok((id.clone(), a))
        })
        .collect()
}


/// ACCESS-11 (ACCESS-3 #8): a write's refusal as `caller` may hear it — the
/// tickets it names that `caller` cannot see said as out of reach (one who
/// sees every ticket hears it whole, an id not born yet included).
pub(super) fn said(store: &Store, caller: &VerifiedUser, e: StoreError) -> TicketsEvent {
    let everything = matches!(caller.sight_filter(), Ok(None));
    TicketsEvent::Error { message: redacted(e, |id| everything || sees_id(store, &caller.sight(), id)) }
}

/// NOTE-2 / ACCESS-11: [`filing`] for a note's upgrade (`notes.upgrade`),
/// its refusal in words.
pub(crate) fn filing_text(store: &Store, caller: &VerifiedUser, epic: &str, parent: Option<&str>) -> Result<Filing, String> {
    filing(store, caller, epic, parent, None).map_err(|ev| match ev {
        TicketsEvent::Error { message } => message,
        TicketsEvent::NotFound { id } => format!("no ticket {id}"),
        TicketsEvent::Ambiguous { id, candidates } => format!("{id} names several tickets: {}", candidates.join(", ")),
        other => format!("{other:?}"),
    })
}

/// COMMENT-2 / ACCESS-11: a role `R` permit for each of `named` that names
/// a ticket `caller` holds `R` on — what a comment write takes for the
/// tickets it is on (one that names none the store refuses as it always
/// did; one out of reach was refused before, by the call's grant).
pub(crate) fn comment_permits<R: RoleMark>(store: &Store, caller: &VerifiedUser, named: &[&str]) -> Vec<Permit<R>> {
    named.iter().filter_map(|n| store.ticket_id(n.trim())).filter_map(|k| caller.permit::<R>(&TicketId::new(k), store).ok()).collect()
}

/// ACCESS-11: the scope a shortcode (or `*`, or an organization) named by
/// text is, as a grant on it is stored.
pub(crate) fn scope_named(store: &Store, text: &str) -> Result<Scope, String> {
    store.scope_named(text).map_err(|e| e.to_string())
}
