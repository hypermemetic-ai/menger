//! `notes.watch` as its stream — every write the store says, to a caller
//! that reads the note's organization: `created`, `updated` (every
//! change of the body: the Yjs update to apply), `renamed`, `deleted`,
//! `squashed`, and (V2-30) `asked`, `replied`, `resolved` — a thread whole
//! each; one note's with `id`, every one's without (so an agent
//! following every note hears a question asked of it). On one
//! note, who is on it too: `present` and `left` — and with `session`,
//! the watch is itself a presence on it for as long as it is open
//! ([`crate::presence::Held`]); an update that came from that session is
//! not said back to it. Held to its caller as `ui.watch` is (FRAME-5): asked again when
//! the gate's grants generation moves, when its sign-in's expiry comes and
//! every [`LOOK_EVERY`] of a quiet watch; refused, the stream ends with one
//! `watch_closed { reason, message }`. Each write is asked of the gate as
//! it stands then, so a role taken off an organization stops its writes
//! reaching the watch at once.
//!
//! NOTES-5: on one note, `mark_changed { id, mark }` and `mark_removed { id,
//! mark }` as marks come and go (on every note's watch too); and with
//! `text: true` the note's markdown as words, so an agent follows what is
//! written with no CRDT of its own: `text { id, revision, body, by, at }`
//! once as the watch opens, then after a change — [`TEXT_EVERY`] after the
//! first change not yet said, holding every change made meanwhile, so at
//! most one a second and the last change always said. Made in the service
//! from its CRDT, whoever wrote; held to the reach `notes.get` is.

use crate::caller::Caller;
use crate::events::NotesEvent;
use crate::seam::Seam;
use crate::store::{Change, NotesStore};
use async_stream::stream;
use futures::Stream;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

/// How often a quiet watch asks the gate whether its caller still stands.
pub const LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(15);

/// NOTES-5: how long a change waits before a watch with `text` says the
/// body — every change made meanwhile said with it.
pub const TEXT_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

/// What woke an open watch.
enum Woke {
    /// Something that could change its caller moved.
    Moved,
    /// NOTES-5: a body not yet said is due.
    Due,
    /// A write, or the channel's word.
    Heard(Result<Change, RecvError>),
}

/// The `text` event of `doc` as it stands (its body whole).
fn text_of(doc: &crate::events::Note) -> NotesEvent {
    NotesEvent::Text { id: doc.id.clone(), revision: doc.revision, body: doc.body.clone().unwrap_or_default(), by: doc.by.clone(), at: doc.at }
}

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

/// What the watch says of `change` — the note whole on a watch of it
/// (`one`), without its body on a watch of every one.
fn said(change: Change, one: bool) -> Option<NotesEvent> {
    let shown = |d: crate::events::Note| if one { d } else { d.summary() };
    Some(match change {
        Change::Created(doc) => NotesEvent::Created { note: shown(doc) },
        Change::Updated { doc, update, seq, session } => {
            NotesEvent::Updated { id: doc.id.clone(), seq, update: if one { update } else { String::new() }, at: doc.at, by: doc.by.clone(), session, note: Some(doc.summary()) }
        }
        Change::Renamed(doc) => NotesEvent::Renamed { id: doc.id.clone(), title: doc.title.clone(), revision: doc.revision, at: doc.at, by: doc.by.clone(), note: Some(shown(doc)) },
        Change::Deleted { id, at, by, .. } => NotesEvent::Deleted { id, at, by },
        Change::Squashed { id, until, removed, at, by, .. } => NotesEvent::Squashed { id, until, removed, at, by },
        Change::Present { present, .. } if one => NotesEvent::Present { present },
        Change::Left { id, session, client, .. } if one => NotesEvent::Left { id, session, client },
        Change::Present { .. } | Change::Left { .. } => return None,
        Change::Asked { thread, .. } => NotesEvent::Asked { thread },
        Change::Replied { thread, .. } => NotesEvent::Replied { thread },
        Change::Resolved { thread, .. } => NotesEvent::Resolved { thread },
        Change::Marked { mark, .. } => NotesEvent::MarkChanged { id: mark.id.clone(), mark },
        Change::Unmarked { id, mark, .. } => NotesEvent::MarkRemoved { id, mark },
    })
}

/// The stream: subscribed at the call, then `watching`, then each write.
pub(crate) fn watch(store: Arc<NotesStore>, seam: Seam, user: Caller, id: Option<String>, session: Option<String>, text: bool) -> impl Stream<Item = NotesEvent> + Send + 'static {
    let mut rx = store.subscribe();
    stream! {
        let mut _held = None;
        // NOTES-5: the body not yet said, and when it is due.
        let mut unsaid: Option<crate::events::Note> = None;
        let mut due: Option<tokio::time::Instant> = None;
        let mut opened = None;
        if text && id.is_none() {
            yield NotesEvent::Error { message: "text follows one note: name its id".into() };
            return;
        }
        if let Some(id) = &id {
            match crate::streams::reached(&store, &seam, &user, id, "notes.watch").await {
                Ok(doc) => {
                    if let Some(session) = session.as_deref().filter(|s| !store.taken(id, s, user.principal())) {
                        store.arrive(id, &doc.org, session, &user.who(), true, None, None);
                        _held = Some(crate::presence::Held { store: Arc::clone(&store), id: id.clone(), session: session.to_string() });
                    }
                    opened = Some(doc);
                }
                Err(ev) => {
                    yield ev;
                    return;
                }
            }
        }
        yield NotesEvent::Watching { id: id.clone() };
        if let (true, Some(doc)) = (text, opened.as_ref()) {
            yield text_of(doc);
        }
        let mut following = Following::of(user);
        loop {
            let woke = tokio::select! {
                biased;
                () = following.changed() => Woke::Moved,
                () = async { match due { Some(at) => tokio::time::sleep_until(at).await, None => std::future::pending().await } } => Woke::Due,
                r = rx.recv() => Woke::Heard(r),
            };
            if let Err((reason, message)) = following.caller.again() {
                yield NotesEvent::WatchClosed { reason, message };
                return;
            }
            match woke {
                Woke::Moved => continue,
                Woke::Due => {
                    due = None;
                    if let Some(doc) = unsaid.take() {
                        yield text_of(&doc);
                    }
                }
                Woke::Heard(Ok(change)) => {
                    if id.as_deref().is_some_and(|id| id != change.id()) {
                        continue;
                    }
                    match seam.holds(following.caller.context(), "notes.watch", change.org()) {
                        Ok(true) => {}
                        Ok(false) => continue,
                        Err(message) => {
                            yield NotesEvent::Error { message };
                            return;
                        }
                    }
                    if let (true, Change::Updated { doc, .. }) = (text, &change) {
                        unsaid = Some(doc.clone());
                        due.get_or_insert_with(|| tokio::time::Instant::now() + TEXT_EVERY);
                    }
                    if let (Change::Updated { session: Some(from), .. }, Some(mine)) = (&change, session.as_deref()) {
                        if from == mine {
                            continue;
                        }
                    }
                    let gone = matches!(change, Change::Deleted { .. }) && id.is_some();
                    if gone {
                        if let Some(doc) = unsaid.take() {
                            yield text_of(&doc);
                        }
                    }
                    if let Some(ev) = said(change, id.is_some()) {
                        yield ev;
                    }
                    if gone {
                        return;
                    }
                }
                Woke::Heard(Err(RecvError::Lagged(missed))) => {
                    yield NotesEvent::Lagged { missed };
                    // NOTES-5: what went by unheard may have changed the body: it is read again, and said when due.
                    if let (true, Some(id)) = (text, id.as_deref()) {
                        if let Ok(doc) = store.get(id).await {
                            unsaid = Some(doc);
                            due.get_or_insert_with(|| tokio::time::Instant::now() + TEXT_EVERY);
                        }
                    }
                }
                Woke::Heard(Err(RecvError::Closed)) => return,
            }
        }
    }
}
