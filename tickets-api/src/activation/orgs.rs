//! ORG-4: a call's organizations — the one a `create` files into, what a
//! refusal names of the others, and where `create` is admitted (moved out
//! of `activation.rs` with it, MENGER-29).

use super::*;

/// ORG-4: an `ambiguous` answer as `caller` may hear it — only the
/// candidates in the organizations it holds a role in (every one for `*`,
/// and with login off); none left, `not_found`: another organization's
/// tickets and shortcodes are not a stranger's to learn. Every other
/// refusal as [`refusal`] says it.
pub(super) fn veiled(caller: &VerifiedUser, e: StoreError) -> TicketsEvent {
    let (StoreError::Ambiguous { text, candidates }, Some(held)) = (&e, caller.orgs()) else { return refusal(e) };
    let ours = |c: &String| c.split_once(':').is_some_and(|(o, _)| held.iter().any(|h| h.as_str() == o));
    let kept: Vec<String> = candidates.iter().filter(|c| ours(c)).cloned().collect();
    match kept.is_empty() {
        true => TicketsEvent::NotFound { id: text.clone() },
        false => TicketsEvent::Ambiguous { id: text.clone(), candidates: kept },
    }
}

/// ORG-4: `id` as `caller` names it — a short form read in the
/// organizations it holds a role in: the full address of the one ticket it
/// names there (another org's namesake, or a pin to one, is not in its
/// way); else `id` as given. Every caller of `*` (and login off) reads the
/// whole store, so `id` is its own.
pub(super) fn named_in(store: &Store, caller: &VerifiedUser, id: String) -> String {
    match caller.orgs() {
        Some(orgs) => store.address_in(&id, &orgs).unwrap_or(id),
        None => id,
    }
}

/// ORG-4: the `epic` a `create` files into, its organization settled — a
/// create with no `--org` goes to the caller's only organization, or is
/// refused naming the choices when it holds several:
///
/// - `org` given: the epic in it (an epic naming another org is refused;
///   one the caller holds no role in is no organization);
/// - else an epic that names its org, or a create under a `parent` (the
///   parent places it), as written;
/// - else the shortcode the epic names, when it is in one of the caller's
///   organizations; else the one organization of the caller's that holds a
///   shortcode of that name; else — a new shortcode — the caller's only
///   organization, or refused, naming them.
pub(super) fn epic_in(store: &Store, caller: &VerifiedUser, epic: String, org: Option<crate::address::OrgId>, parent: Option<&str>) -> Result<String, TicketsEvent> {
    let error = |message: String| TicketsEvent::Error { message };
    let Ok(asked) = crate::address::Address::parse(&epic) else { return Ok(epic) };
    let held: Vec<crate::address::OrgId> = match caller.orgs() {
        Some(h) => h,
        None => store.orgs().into_iter().map(|o| o.name).collect(),
    };
    let qualified = |o: &crate::address::OrgId| crate::address::Address { org: Some(o.clone()), ..asked.clone() }.to_string();
    if let Some(o) = org.as_ref().map(crate::address::OrgId::as_str) {
        let found = store.org(o).map(|x| x.name).filter(|n| held.contains(n));
        let Some(o) = found else { return Err(error(format!("no organization '{o}' (nothing was created)"))) };
        if asked.org.as_ref().is_some_and(|a| store.org(a.as_str()).map(|x| x.name).as_ref() != Some(&o)) {
            return Err(error(format!("epic {epic} names another organization than --org {o} (nothing was created)")));
        }
        return Ok(qualified(&o));
    }
    if asked.org.is_some() || parent.is_some() {
        return Ok(epic);
    }
    if store.org_of(&epic).is_some_and(|o| held.contains(&o)) {
        return Ok(epic);
    }
    let holding: Vec<&crate::address::OrgId> = held.iter().filter(|o| store.org_of(&qualified(o)).is_some()).collect();
    let names = || held.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(", ");
    match (holding.as_slice(), held.as_slice()) {
        ([one], _) => Ok(qualified(one)),
        ([], [only]) => Ok(qualified(only)),
        ([], []) => Ok(epic),
        ([], _) => Err(error(format!(
            "a new shortcode {epic}: name its organization with --org — you hold roles in {} (nothing was created)",
            names()
        ))),
        (_, _) => Err(error(format!(
            "{epic} is a shortcode in more than one of your organizations ({}): name one with --org (nothing was created)",
            holding.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}
