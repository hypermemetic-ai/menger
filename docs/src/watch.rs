//! `docs.watch` as its stream — every write the store says, to a caller
//! that reads the document's organization: `created`, `updated` (every
//! change of the body: the Yjs update to apply), `renamed`, `deleted`,
//! `squashed`, and (V2-30) `asked`, `replied`, `resolved` — a thread whole
//! each; one document's with `id`, every one's without (so an agent
//! following every document hears a question asked of it). On one
//! document, who is on it too: `present` and `left` — and with `session`,
//! the watch is itself a presence on it for as long as it is open
//! ([`crate::presence::Held`]); an update that came from that session is
//! not said back to it. Held to its caller as `ui.watch` is (FRAME-5): asked again when
//! the gate's grants generation moves, when its sign-in's expiry comes and
//! every [`LOOK_EVERY`] of a quiet watch; refused, the stream ends with one
//! `watch_closed { reason, message }`. Each write is asked of the gate as
//! it stands then, so a role taken off an organization stops its writes
//! reaching the watch at once.

use crate::caller::Caller;
use crate::events::DocsEvent;
use crate::seam::Seam;
use crate::store::{Change, DocsStore};
use async_stream::stream;
use futures::Stream;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

/// How often a quiet watch asks the gate whether its caller still stands.
pub const LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(15);

/// An open watch's caller and what wakes it (the ui service's `Following`).
struct Following {
    caller: Caller,
    moved: Option<tokio::sync::watch::Receiver<u64>>,
}

impl Following {
    fn of(caller: Caller) -> Self {
        let moved = caller.reach().generation().map(|g| {
            let mut rx = g.subscribe();
            rx.mark_unchanged();
            rx
        });
        Self { caller, moved }
    }

    /// Waits until something that could change the caller moves.
    async fn changed(&mut self) {
        let expiry = async {
            match self.caller.expires_at() {
                Some(at) => {
                    let until = std::time::Duration::from_secs((at - crate::store::now()).max(0) as u64);
                    tokio::time::sleep(until + std::time::Duration::from_millis(5)).await
                }
                None => std::future::pending().await,
            }
        };
        let generation = async {
            match self.moved.as_mut() {
                Some(rx) => {
                    if rx.changed().await.is_err() {
                        std::future::pending::<()>().await
                    }
                }
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = generation => {}
            () = expiry => {}
            () = tokio::time::sleep(LOOK_EVERY) => {}
        }
    }
}

/// What the watch says of `change` — the document whole on a watch of it
/// (`one`), without its body on a watch of every one.
fn said(change: Change, one: bool) -> Option<DocsEvent> {
    let shown = |d: crate::events::Doc| if one { d } else { d.summary() };
    Some(match change {
        Change::Created(doc) => DocsEvent::Created { doc: shown(doc) },
        Change::Updated { doc, update, seq, session } => {
            DocsEvent::Updated { id: doc.id.clone(), seq, update: if one { update } else { String::new() }, at: doc.at, by: doc.by.clone(), session, doc: Some(doc.summary()) }
        }
        Change::Renamed(doc) => DocsEvent::Renamed { id: doc.id.clone(), title: doc.title.clone(), revision: doc.revision, at: doc.at, by: doc.by.clone(), doc: Some(shown(doc)) },
        Change::Deleted { id, at, by, .. } => DocsEvent::Deleted { id, at, by },
        Change::Squashed { id, until, removed, at, by, .. } => DocsEvent::Squashed { id, until, removed, at, by },
        Change::Present { present, .. } if one => DocsEvent::Present { present },
        Change::Left { id, session, client, .. } if one => DocsEvent::Left { id, session, client },
        Change::Present { .. } | Change::Left { .. } => return None,
        Change::Asked { thread, .. } => DocsEvent::Asked { thread },
        Change::Replied { thread, .. } => DocsEvent::Replied { thread },
        Change::Resolved { thread, .. } => DocsEvent::Resolved { thread },
    })
}

/// The stream: subscribed at the call, then `watching`, then each write.
pub(crate) fn watch(store: Arc<DocsStore>, seam: Seam, user: Caller, id: Option<String>, session: Option<String>) -> impl Stream<Item = DocsEvent> + Send + 'static {
    let mut rx = store.subscribe();
    stream! {
        let mut _held = None;
        if let Some(id) = &id {
            match crate::streams::reached(&store, &seam, &user, id, "docs.watch").await {
                Ok(doc) => {
                    if let Some(session) = session.as_deref().filter(|s| !store.taken(id, s, user.principal())) {
                        store.arrive(id, &doc.org, session, &user.who(), true, None, None);
                        _held = Some(crate::presence::Held { store: Arc::clone(&store), id: id.clone(), session: session.to_string() });
                    }
                }
                Err(ev) => {
                    yield ev;
                    return;
                }
            }
        }
        yield DocsEvent::Watching { id: id.clone() };
        let mut following = Following::of(user);
        loop {
            let woke = tokio::select! {
                biased;
                () = following.changed() => None,
                r = rx.recv() => Some(r),
            };
            if let Err((reason, message)) = following.caller.again() {
                yield DocsEvent::WatchClosed { reason, message };
                return;
            }
            match woke {
                None => continue,
                Some(Ok(change)) => {
                    if id.as_deref().is_some_and(|id| id != change.id()) {
                        continue;
                    }
                    match seam.holds(following.caller.context(), "docs.watch", change.org()) {
                        Ok(true) => {}
                        Ok(false) => continue,
                        Err(message) => {
                            yield DocsEvent::Error { message };
                            return;
                        }
                    }
                    if let (Change::Updated { session: Some(from), .. }, Some(mine)) = (&change, session.as_deref()) {
                        if from == mine {
                            continue;
                        }
                    }
                    let gone = matches!(change, Change::Deleted { .. }) && id.is_some();
                    if let Some(ev) = said(change, id.is_some()) {
                        yield ev;
                    }
                    if gone {
                        return;
                    }
                }
                Some(Err(RecvError::Lagged(missed))) => yield DocsEvent::Lagged { missed },
                Some(Err(RecvError::Closed)) => return,
            }
        }
    }
}
