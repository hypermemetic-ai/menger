//! `tickets.watch` as its stream — kept beside the activation so its file
//! keeps only its methods (MENGER-29).
//!
//! ACCESS-12: the watch holds its caller ([`Sight`]: the `VerifiedUser`, the
//! `AuthContext` it re-verifies from, its reach compiled once) and wakes on
//! three things — a change on its channel, the caller's grants or sign-in
//! moving ([`Sight::changed`]), and (a relative filter) time. At every wake
//! the caller is asked again first ([`Sight::again`]: a comparison of the
//! grants generation, unless it moved): refused, the stream ends with one
//! `watch_closed { reason, message }`; a reach that moved, the watch hears
//! through the new one from here on — its filter compiled again within it,
//! and the tickets that entered or left what it hears said as `entered` /
//! `left` (past 256, `recount`), the components it no longer sees as
//! `component_removed`. Then the change, cut to the reach by the store.

use super::*;
use crate::store::Watcher;

/// What a `watch` call asks.
pub(super) struct WatchAsk {
    pub(super) epic: Option<ShortcodeRef>,
    pub(super) filter: Option<FilterText>,
    pub(super) membership: bool,
    pub(super) tz: Option<Zone>,
    /// TICKETS-7: and the revisions every write keeps (`revised`).
    pub(super) revisions: bool,
}

/// TICKETS-7: the next revision a write kept, when the watch asked for them —
/// never, when it did not.
async fn revision(rx: &mut Option<tokio::sync::broadcast::Receiver<Change>>) -> Result<Change, tokio::sync::broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Why the watch woke.
enum Woke {
    Access,
    Change(Result<Change, tokio::sync::broadcast::error::RecvError>),
    Tick,
}

/// The stream. The caller is verified, and the channel subscribed, at the
/// call — before the stream is first polled.
pub(super) fn watch_stream(store: Arc<Store>, access: Access, user: VerifiedUser, ask: WatchAsk) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    let WatchAsk { epic, filter: asked, membership, tz, revisions } = ask;
    let (epic, ambiguous) = watched_shortcode(&store, epic.as_deref());
    // IDENTITY-19, IDENTITY-21: who is watching, and through what reach.
    let opened = Sight::open(&store, &access, user).and_then(|sight| {
        if let Some(why) = &ambiguous {
            return Err(why.clone());
        }
        let filter = sight.caller().scoped(asked.as_deref())?;
        // MENGER-13: how its filter reads time.
        let clock = clock_of(sight.caller(), tz.as_deref())?;
        Ok((filter, sight, clock))
    });
    // FILTER-4: a filtered watch reads the filtered channel, which carries
    // every change and each write's cell moves after them.
    let (opened, filtered) = match opened {
        Ok((filter, sight, clock)) => {
            let filtered = filter.is_some() || membership;
            let watcher = filtered.then(|| watcher_for(&store, filter.as_ref(), epic.as_ref(), &clock));
            (Ok((sight, clock, watcher)), filtered)
        }
        Err(e) => (Err(e), false),
    };
    let mut rx = if filtered { store.subscribe_filtered() } else { store.subscribe() };
    let mut revised = (revisions && !membership).then(|| store.subscribe_revisions());
    stream! {
        let (mut sight, clock, watcher) = match opened {
            Ok(o) => o,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        let mut watcher: Option<Watcher> = match watcher.transpose() {
            Ok(w) => w,
            Err(message) => {
                yield TicketsEvent::Error { message };
                return;
            }
        };
        loop {
            // MENGER-13: a filter with a relative window is read again as
            // time passes, between writes.
            let relative = watcher.as_ref().is_some_and(Watcher::relative);
            let wait = watcher.as_ref().map_or(AGE_AT_MOST, |w| w.wait(&store, AGE_AT_MOST));
            let woke = tokio::select! {
                biased;
                () = sight.changed() => Woke::Access,
                r = rx.recv() => Woke::Change(r),
                r = revision(&mut revised) => Woke::Change(r),
                () = tokio::time::sleep(wait), if relative => Woke::Tick,
            };
            // ACCESS-12: the caller as it stands, before anything is relayed.
            match sight.again(&store) {
                Err(closed) => {
                    yield TicketsEvent::WatchClosed { reason: closed.reason, message: closed.message };
                    return;
                }
                Ok(None) => {}
                Ok(Some(moved)) => {
                    // What it heard through, and what it hears through now:
                    // a filtered watch's filter compiled again within the
                    // new reach; a plain one's, the reach itself.
                    let before = match &watcher {
                        Some(w) => Some(w.lens().clone()),
                        None => moved.was.lens().cloned(),
                    };
                    if let Some(w) = watcher.as_mut() {
                        match sight.caller().scoped(asked.as_deref()).and_then(|f| watcher_for(&store, f.as_ref(), epic.as_ref(), &clock)) {
                            Ok(again) => *w = again,
                            Err(message) => {
                                yield TicketsEvent::WatchClosed { reason: crate::types::WatchEnd::Revoked, message };
                                return;
                            }
                        }
                    }
                    let after = match &watcher {
                        Some(w) => Some(w.lens().clone()),
                        None => sight.sight().lens().cloned(),
                    };
                    for m in store.moved_between(before.as_ref(), after.as_ref(), epic.as_ref()) {
                        yield membership_event(&store, m);
                    }
                    if !membership {
                        for name in moved.gone {
                            yield TicketsEvent::ComponentRemoved { name };
                        }
                    }
                }
            }
            let next = match woke {
                Woke::Access => continue,
                Woke::Tick => {
                    if let Some(w) = watcher.as_mut() {
                        for m in w.tick(&store, epic.as_ref()) {
                            yield membership_event(&store, m);
                        }
                    }
                    continue;
                }
                Woke::Change(next) => next,
            };
            match (next, watcher.as_mut()) {
                (Ok(Change::CellsMoved { moves }), Some(w)) => {
                    for m in w.membership(&store, &moves, epic.as_ref()) {
                        yield membership_event(&store, m);
                    }
                }
                // FILTER-13: a filter asking `under` — what the write
                // worked out for this watch.
                (Ok(Change::UnderMoved { moves }), Some(w)) => {
                    for m in w.under_membership(&moves) {
                        yield membership_event(&store, m);
                    }
                }
                (Ok(change), _) if !membership => {
                    // WAVE-6: of a `placements_changed`, the changes in the
                    // epic. ACCESS-12: then cut to the reach by the store.
                    if let Some(ev) = change.in_epic(epic.as_ref()).and_then(|c| sight.relay(&store, c)) {
                        yield ev;
                    }
                }
                (Ok(_), _) => {}
                (Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)), _) => {
                    yield TicketsEvent::Lagged { missed };
                }
                (Err(tokio::sync::broadcast::error::RecvError::Closed), _) => break,
            }
            // FILTER-36: a saved filter it names, changed — said after the
            // change that changed it.
            if let Some(w) = watcher.as_mut() {
                match w.resaved(&store, epic.as_ref()) {
                    Ok(moved) => for m in moved { yield membership_event(&store, m); },
                    Err(message) => { yield TicketsEvent::Error { message }; return; }
                }
            }
        }
    }
}
