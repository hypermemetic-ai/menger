//! The wave reads' answers (WAVE-4; MENGER-31) — what `waves`, `waves_under`
//! and `waves_to` yield, worked out here so the activation's methods stay
//! an attribute and a call.
//!
//! MENGER-31: **`waves_under { id, kinds? }`** is the wave board of a
//! ticket's subtree — what "Wave board for <parent>" opens: one `Placed` per
//! ticket of `id`'s subtree (it and every ticket under it at any depth, in
//! any shortcode hosted there — the kept tree, FILTER-13's `under`) whose
//! kind is in `kinds` (default build, spike, gap; never a doc), then one
//! `WaveCounts` — `waves`' answer over that set instead of a shortcode's.
//!
//! **`waves_to { id, kinds? }`** is the wave board whose final state is one
//! ticket — what "Waves to <ticket>" opens. Its set is what must happen
//! first: the ticket's blockers, theirs, transitively over `blocked_by` (into
//! any shortcode), and — for a scope or an execution — its open descendants
//! (the kept tree) and what they wait on. A done blocker is in it, `done`,
//! and not followed past. One `Placed` per ticket of the set whose kind is in
//! `kinds` (default all five; never a doc), each where the store keeps it
//! (the set holds every blocker of its open tickets, so a kept wave is the
//! wave here), and the ticket itself, always, alone in the last wave — one
//! past the highest of the rest, `missing_blocker` when anything it waits on
//! is, `done` when it is done; `waves`' order, the ticket last. Then one
//! `WaveCycle { ids }` per `blocked_by` cycle among the set's open tickets
//! (only a hand edit makes one), then one `WaveCounts`.
//!
//! Both: `NotFound` for an id that names no ticket (or one out of the
//! caller's reach); a doc target (`waves_to`) or a kind outside the six is
//! one in-band `Error`. Rows only; live through `watch` as every board is.

use super::*;

/// `waves { epics, kinds }` (WAVE-4): `epics` empty, a shortcode out of
/// reach or a kind outside the six is one `Error`.
pub(super) fn waves_answer(store: &Store, admitted: Result<Caller, String>, epics: Vec<String>, kinds: Option<Vec<String>>) -> Vec<TicketsEvent> {
    let caller = match admitted {
        Ok(caller) => caller,
        Err(message) => return vec![TicketsEvent::Error { message }],
    };
    let visible = match visible_epics(store, &caller) {
        Ok(v) => v,
        Err(message) => return vec![TicketsEvent::Error { message }],
    };
    if epics.is_empty() {
        return vec![TicketsEvent::Error { message: "waves: epics is empty — name at least one shortcode, e.g. [\"MNG\"]".into() }];
    }
    // ORG-3: each name is letters, as `in`'s — every shortcode with them
    // must be in reach.
    if let Some(v) = &visible {
        if let Some(hidden) = epics.iter().find(|e| store.shortcodes_in(e).iter().any(|sc| !v.contains(sc))) {
            return vec![TicketsEvent::Error { message: format!("waves: no access to {hidden}") }];
        }
    }
    match kinds_of(kinds, &crate::store::waves::DEFAULT_KINDS) {
        Ok(kinds) => wave_events(store.waves(&epics, &kinds), Vec::new()),
        Err(evs) => evs,
    }
}

/// `waves_under { id, kinds? }` (MENGER-31).
pub(super) fn waves_under_answer(store: &Store, admitted: Result<Caller, String>, id: String, kinds: Option<Vec<String>>) -> Vec<TicketsEvent> {
    let (visible, kinds) = match waves_of_one(store, admitted, &id, kinds, &crate::store::waves::DEFAULT_KINDS) {
        Ok(v) => v,
        Err(evs) => return evs,
    };
    match store.waves_under(&id, &kinds, visible.as_ref()) {
        Ok(answer) => wave_events(answer, Vec::new()),
        Err(StoreError::NotFound(_)) => vec![TicketsEvent::NotFound { id }],
        Err(e) => vec![refusal(e)],
    }
}

/// `waves_to { id, kinds? }` (MENGER-31).
pub(super) fn waves_to_answer(store: &Store, admitted: Result<Caller, String>, id: String, kinds: Option<Vec<String>>) -> Vec<TicketsEvent> {
    let every = [Kind::Scope, Kind::Execution, Kind::Build, Kind::Gap, Kind::Spike];
    let (visible, kinds) = match waves_of_one(store, admitted, &id, kinds, &every) {
        Ok(v) => v,
        Err(evs) => return evs,
    };
    match store.waves_to(&id, &kinds, visible.as_ref()) {
        Ok(answer) => wave_events(answer.waves, answer.cycles),
        Err(StoreError::NotFound(_)) => vec![TicketsEvent::NotFound { id }],
        Err(StoreError::DocHasNoLifecycle { .. }) => {
            vec![TicketsEvent::Error { message: format!("waves_to: {id} is a doc — a doc has no place in the order of work") }]
        }
        Err(e) => vec![refusal(e)],
    }
}

/// The shortcodes a caller sees (`None`: all).
type Visible = Option<std::collections::HashSet<crate::address::ShortcodeId>>;

/// The kinds asked (`default` with none), or the `Error` naming the bad one.
fn kinds_of(kinds: Option<Vec<String>>, default: &[Kind]) -> Result<Vec<Kind>, Vec<TicketsEvent>> {
    match kinds {
        None => Ok(default.to_vec()),
        Some(given) => given.iter().map(|k| k.parse::<Kind>()).collect::<Result<_, _>>().map_err(|message| vec![TicketsEvent::Error { message }]),
    }
}

/// What `waves_under` and `waves_to` share before they read — the caller
/// admitted, the ticket in its reach (`NotFound` otherwise, as `get`
/// answers), the shortcodes it sees, and the kinds asked parsed. Each
/// refusal is the one event the call yields.
fn waves_of_one(
    store: &Store,
    admitted: Result<Caller, String>,
    id: &str,
    kinds: Option<Vec<String>>,
    default: &[Kind],
) -> Result<(Visible, Vec<Kind>), Vec<TicketsEvent>> {
    let caller = admitted.map_err(|message| vec![TicketsEvent::Error { message }])?;
    if caller.require(&[id], store).is_err() {
        return Err(vec![TicketsEvent::NotFound { id: id.to_string() }]);
    }
    let visible = visible_epics(store, &caller).map_err(|message| vec![TicketsEvent::Error { message }])?;
    Ok((visible, kinds_of(kinds, default)?))
}

/// A wave answer as the wire carries it — each `Placed`, then each cycle
/// (`waves_to`'s), then the one `WaveCounts`.
fn wave_events(answer: crate::store::waves::Waves, cycles: Vec<Vec<String>>) -> Vec<TicketsEvent> {
    let mut out: Vec<TicketsEvent> = answer
        .placed
        .into_iter()
        .map(|p| TicketsEvent::Placed { summary: p.summary, placement: p.placement, waiting_on: p.waiting_on })
        .collect();
    out.extend(cycles.into_iter().map(|ids| TicketsEvent::WaveCycle { ids }));
    out.push(TicketsEvent::WaveCounts { waves: answer.waves, missing: answer.missing, done: answer.done });
    out
}
