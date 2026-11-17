//! What the `notes` methods do, beside the activation (its `impl` keeps
//! only the methods — MENGER-29's rule). Each takes the [`Caller`] the
//! dispatch resolved; whether it may read or write a note is the
//! gate's word on the note's organization, asked over the [`Seam`].
//! How the body is written, replayed and squashed is [`crate::crdt`]'s;
//! who is on a note, [`crate::presence`]'s.

use crate::caller::Caller;
use crate::events::{Note, NotesEvent};
use crate::seam::{grant_of, Seam};
use crate::store::{NotesStore, Listing, StoreError};
use menger_tickets_api::members::Role;
use std::sync::Arc;

/// One event, as a method's stream.
pub(crate) fn once(ev: impl std::future::Future<Output = NotesEvent> + Send + 'static) -> impl futures::Stream<Item = NotesEvent> + Send + 'static {
    futures::stream::once(ev)
}

/// Each of the events, as a method's stream.
pub(crate) fn many(evs: impl std::future::Future<Output = Vec<NotesEvent>> + Send + 'static) -> impl futures::Stream<Item = NotesEvent> + Send + 'static {
    use futures::StreamExt;
    futures::stream::once(evs).flat_map(futures::stream::iter)
}

/// A page of `list` when the call names none.
pub const LIST_PAGE: usize = 50;
/// The most a page of `list` (or `history`) may be.
pub const LIST_MAX: usize = 1_000;

/// The role a grant of this service takes, for a refusal's words.
fn role_of(grant: &str) -> &'static str {
    match grant {
        "notes.marks" | "notes.threads" | "notes.waiting" | "notes.get" | "notes.list" | "notes.history" | "notes.replay" | "notes.state" | "notes.present" | "notes.watch" | "notes.may" => "viewer",
        "notes.delete" => "admin",
        "notes.squash" => "notes.squash's grant (admin's by default)",
        _ => "editor",
    }
}

/// A store failure as the one event it is.
pub(crate) fn failed(e: StoreError) -> NotesEvent {
    match e {
        StoreError::NotFound(id) => NotesEvent::no_doc(&id),
        StoreError::NoRevision(id, r) => NotesEvent::NotFound { id: Some(id), revision: Some(r), org: None, thread: None, mark: None },
        StoreError::NoThread(t) => NotesEvent::NotFound { id: None, revision: None, org: None, thread: Some(t), mark: None },
        StoreError::NoMark(m) => NotesEvent::NotFound { id: None, revision: None, org: None, thread: None, mark: Some(m) },
        StoreError::BadRange(m) => NotesEvent::Error { message: format!("{m} (nothing was written)") },
        StoreError::Quota(m) => NotesEvent::Refused { reason: "quota".into(), message: format!("{m} (nothing was written)") },
        StoreError::Allowance(r) => NotesEvent::Refused { reason: r.reason, message: format!("{} (nothing was written)", r.message) },
        e @ StoreError::BadUpdate(_) => NotesEvent::Error { message: format!("{e} (nothing was written)") },
        StoreError::Db(m) => NotesEvent::Error { message: m },
    }
}

/// `forbidden`, naming what `grant` takes on `org`.
pub(crate) fn forbidden(user: &Caller, grant: &str, what: &str, org: &str) -> NotesEvent {
    NotesEvent::Refused { reason: "forbidden".into(), message: format!("{grant} on {what} needs {} on {org}: (or on *); {} does not hold it", role_of(grant), user.name()) }
}

/// The live note `id`, when `user` reads it (`notes.get` on its
/// organization) and holds `grant` there too. `not_found` for one it does
/// not read — as for none (ACCESS-10) — and `refused { forbidden }` for one
/// it reads and may not do this to.
pub(crate) async fn reached(store: &Arc<NotesStore>, seam: &Seam, user: &Caller, id: &str, grant: &str) -> Result<Note, NotesEvent> {
    let doc = store.get(id).await.map_err(failed)?;
    match seam.holds(user.context(), "notes.get", &doc.org) {
        Ok(true) => {}
        Ok(false) => return Err(NotesEvent::no_doc(id)),
        Err(message) => return Err(NotesEvent::Error { message }),
    }
    if grant != "notes.get" {
        match seam.holds(user.context(), grant, &doc.org) {
            Ok(true) => {}
            Ok(false) => return Err(forbidden(user, grant, id, &doc.org)),
            Err(message) => return Err(NotesEvent::Error { message }),
        }
    }
    Ok(doc)
}

/// The organizations `user` holds `grant` in, by the gate, of those it is in.
async fn orgs_holding(seam: &Seam, user: &Caller, grant: &str) -> Result<Vec<String>, NotesEvent> {
    let orgs = seam.orgs(user.context()).await.map_err(|message| NotesEvent::Error { message })?;
    let mut out = Vec::new();
    for org in orgs {
        if seam.holds(user.context(), grant, &org).map_err(|message| NotesEvent::Error { message })? {
            out.push(org);
        }
    }
    Ok(out)
}

/// `notes.create`: a note in `org` (else the first organization the
/// caller may write notes in), revision 1.
pub(crate) async fn create(store: Arc<NotesStore>, seam: Seam, user: Caller, title: String, body: String, org: Option<String>) -> NotesEvent {
    let org = match org {
        Some(org) => {
            let org = org.trim().trim_end_matches(':').to_string();
            match seam.orgs(user.context()).await {
                Ok(orgs) if orgs.contains(&org) => {}
                Ok(_) => return NotesEvent::NotFound { id: None, revision: None, org: Some(org), thread: None, mark: None },
                Err(message) => return NotesEvent::Error { message },
            }
            match seam.holds(user.context(), "notes.create", &org) {
                Ok(true) => org,
                Ok(false) => return forbidden(&user, "notes.create", &format!("{org}:"), &org),
                Err(message) => return NotesEvent::Error { message },
            }
        }
        None => match orgs_holding(&seam, &user, "notes.create").await {
            Ok(orgs) => match orgs.into_iter().next() {
                Some(org) => org,
                None => return NotesEvent::Refused { reason: "forbidden".into(), message: format!("{} may write notes in no organization: notes.create needs editor on one", user.name()) },
            },
            Err(ev) => return ev,
        },
    };
    match store.create(&org, &title, &body, &user.name(), user.principal()).await {
        Ok(doc) => NotesEvent::Created { note: doc },
        Err(e) => failed(e),
    }
}

/// `notes.get`: the note now, its body the markdown derived from its CRDT.
pub(crate) async fn get(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String) -> NotesEvent {
    match reached(&store, &seam, &user, &id, "notes.get").await {
        Ok(doc) => NotesEvent::Note { note: doc },
        Err(ev) => ev,
    }
}

/// `notes.list`: a page of the notes of the organizations the caller
/// reads notes in, newest made first, their titles searched.
pub(crate) async fn list(store: Arc<NotesStore>, seam: Seam, user: Caller, query: Option<String>, limit: Option<u32>, after: Option<String>) -> Vec<NotesEvent> {
    let orgs = match orgs_holding(&seam, &user, "notes.list").await {
        Ok(o) => o,
        Err(ev) => return vec![ev],
    };
    let limit = limit.map_or(LIST_PAGE, |l| (l as usize).clamp(1, LIST_MAX));
    match store.list(Listing { orgs: Some(orgs), query, limit, after }).await {
        Ok(page) => vec![
            NotesEvent::Rows { items: page.rows.into_iter().map(|doc| NotesEvent::Note { note: doc }).collect() },
            NotesEvent::Page { next: page.next, remaining: page.remaining },
        ],
        Err(e) => vec![failed(e)],
    }
}

/// `notes.rename`.
pub(crate) async fn rename(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, title: String) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.rename").await {
        return ev;
    }
    match store.rename(&id, &title, &user.name(), user.principal()).await {
        Ok(doc) => NotesEvent::Renamed { id: doc.id, title: doc.title, revision: doc.revision, at: doc.at, by: doc.by, note: None },
        Err(e) => failed(e),
    }
}

/// `notes.delete`.
pub(crate) async fn delete(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.delete").await {
        return ev;
    }
    match store.delete(&id, &user.name(), user.principal()).await {
        Ok((_, at)) => NotesEvent::Deleted { id, at, by: user.name() },
        Err(e) => failed(e),
    }
}

/// A writer with no watch open is present while it writes (an agent's
/// save over synapse): an entry under its own session, idling out.
fn writing(store: &NotesStore, user: &Caller, doc: &crate::Note, session: Option<&str>) {
    let session = session.map_or_else(|| format!("w-{}", user.principal()), str::to_string);
    if !store.taken(&doc.id, &session, user.principal()) {
        store.arrive(&doc.id, &doc.org, &session, &user.who(), false, None, None);
    }
}

/// `notes.update`: a Yjs update from the editor, merged and kept.
pub(crate) async fn update(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, update: String, session: Option<String>) -> NotesEvent {
    let doc = match reached(&store, &seam, &user, &id, "notes.update").await {
        Ok(d) => d,
        Err(ev) => return ev,
    };
    writing(&store, &user, &doc, session.as_deref());
    match store.update(&id, &update, &user.name(), user.principal(), session).await {
        Ok(w) => NotesEvent::Applied { id: w.doc.id, revision: w.doc.revision },
        Err(e) => failed(e),
    }
}

/// `notes.save`: a whole body, merged into the CRDT as its own edits.
pub(crate) async fn save(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, body: String, base: Option<u32>) -> NotesEvent {
    let doc = match reached(&store, &seam, &user, &id, "notes.save").await {
        Ok(d) => d,
        Err(ev) => return ev,
    };
    writing(&store, &user, &doc, None);
    match store.save(&id, &body, base, &user.name(), user.principal()).await {
        Ok(w) => NotesEvent::Saved { id: w.doc.id, revision: w.doc.revision, at: w.doc.at, by: w.doc.by, stale: w.stale },
        Err(e) => failed(e),
    }
}

/// `notes.restore`: the body as it stood after change `seq`, merged back.
pub(crate) async fn restore(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, seq: u32) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.restore").await {
        return ev;
    }
    match store.restore(&id, seq, &user.name(), user.principal()).await {
        Ok(w) => NotesEvent::Saved { id: w.doc.id, revision: w.doc.revision, at: w.doc.at, by: w.doc.by, stale: false },
        Err(e) => failed(e),
    }
}

/// `notes.state`: what the caller's CRDT lacks.
pub(crate) async fn state(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, vector: Option<String>) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.state").await {
        return ev;
    }
    match store.state(&id, vector).await {
        Ok(s) => NotesEvent::State { id, update: s.update, vector: s.vector, revision: s.revision },
        Err(e) => failed(e),
    }
}

/// `notes.replay`: the note as it stood after `seq`, or at `at`.
pub(crate) async fn replay(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, seq: Option<u32>, at: Option<i64>) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.replay").await {
        return ev;
    }
    match store.replay(&id, seq, at).await {
        Ok(doc) => NotesEvent::Note { note: doc },
        Err(e) => failed(e),
    }
}

/// `notes.history`: newest first.
pub(crate) async fn history(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, limit: Option<u32>, before: Option<u32>) -> Vec<NotesEvent> {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.history").await {
        return vec![ev];
    }
    match store.history(&id, limit.map_or(LIST_MAX, |l| (l as usize).clamp(1, LIST_MAX)), before).await {
        Ok(rows) => rows.into_iter().map(|change| NotesEvent::Change { change }).collect(),
        Err(e) => vec![failed(e)],
    }
}

/// `notes.squash`: its own grant on the note's organization, then the
/// changes up to `until` collapsed.
pub(crate) async fn squash(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, until: u32) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.squash").await {
        return ev;
    }
    match store.squash(&id, until, &user.name(), user.principal()).await {
        Ok(removed) => NotesEvent::Squashed { id, until, removed, at: crate::store::now(), by: user.name() },
        Err(e) => failed(e),
    }
}

/// `notes.present`: everyone on the note now.
pub(crate) async fn present(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String) -> Vec<NotesEvent> {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.present").await {
        return vec![ev];
    }
    store.present(&id).into_iter().map(|present| NotesEvent::Present { present }).collect()
}

/// `notes.here`: the caller's caret on the note, as its session.
pub(crate) async fn here(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, session: String, client: Option<u64>, awareness: Option<String>) -> NotesEvent {
    let doc = match reached(&store, &seam, &user, &id, "notes.here").await {
        Ok(d) => d,
        Err(ev) => return ev,
    };
    if store.taken(&id, &session, user.principal()) {
        return NotesEvent::Refused { reason: "forbidden".into(), message: format!("session {session} on {id} is someone else's") };
    }
    NotesEvent::Present { present: store.arrive(&doc.id, &doc.org, &session, &user.who(), false, client, awareness) }
}

/// `notes.may`: whether the caller holds `role` on the note — the grant
/// that role's methods take ([`grant_of`]) on its organization.
pub(crate) async fn may(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, role: Role) -> NotesEvent {
    let doc = match reached(&store, &seam, &user, &id, "notes.get").await {
        Ok(d) => d,
        Err(ev) => return ev,
    };
    let held = match grant_of(role) {
        None => false,
        Some(grant) => match seam.holds(user.context(), grant, &doc.org) {
            Ok(h) => h,
            Err(message) => return NotesEvent::Error { message },
        },
    };
    NotesEvent::May { id: doc.id, role, held }
}

/// V2-30: who a thread's hand is.
fn hand(user: &Caller) -> crate::threads::Hand {
    crate::threads::Hand { name: user.name(), principal: user.principal().to_string() }
}

/// `notes.ask`: a question on a range of the note.
pub(crate) async fn ask(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, from: u32, to: u32, text: String, to_whom: Option<String>) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.ask").await {
        return ev;
    }
    match store.ask(&id, from, to, &text, to_whom, hand(&user)).await {
        Ok(thread) => NotesEvent::Asked { thread },
        Err(e) => failed(e),
    }
}

/// `notes.reply`: an answer under a thread.
pub(crate) async fn reply(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, thread: String, body: String) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.reply").await {
        return ev;
    }
    match store.reply(&id, &thread, &body, hand(&user)).await {
        Ok(thread) => NotesEvent::Replied { thread },
        Err(e) => failed(e),
    }
}

/// `notes.resolve`: a thread folded, or opened again.
pub(crate) async fn resolve(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, thread: String, open: bool) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.resolve").await {
        return ev;
    }
    match store.resolve(&id, &thread, open, hand(&user)).await {
        Ok(thread) => NotesEvent::Resolved { thread },
        Err(e) => failed(e),
    }
}

/// `notes.threads`: the note's threads.
pub(crate) async fn threads(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String) -> Vec<NotesEvent> {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.threads").await {
        return vec![ev];
    }
    match store.threads(&id).await {
        Ok(rows) => rows.into_iter().map(|thread| NotesEvent::Thread { thread }).collect(),
        Err(e) => vec![failed(e)],
    }
}

/// NOTES-5: `notes.mark` — with `mark`, that mark changed (its range stays:
/// a range is moved by taking the mark off and making it again); without,
/// a mark made on `from..to`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn mark(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, mark: Option<String>, from: Option<u32>, to: Option<u32>, link: Option<String>, label: Option<String>) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.mark").await {
        return ev;
    }
    let made = match (mark, from, to) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => return NotesEvent::Error { message: "a mark's range is not moved: unmark it and mark again (nothing was written)".into() },
        (Some(m), None, None) => store.remark(&id, &m, crate::marks::Remark { link, label }, hand(&user)).await,
        (None, Some(from), Some(to)) => store.mark(&id, from, to, link, label, hand(&user)).await,
        (None, ..) => return NotesEvent::Error { message: "a new mark names its range, from and to; a change names the mark (nothing was written)".into() },
    };
    match made {
        Ok(mark) => NotesEvent::Marked { mark },
        Err(e) => failed(e),
    }
}

/// NOTES-5: `notes.unmark`.
pub(crate) async fn unmark(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String, mark: String) -> NotesEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.unmark").await {
        return ev;
    }
    match store.unmark(&id, &mark).await {
        Ok(()) => NotesEvent::Unmarked { id, mark },
        Err(e) => failed(e),
    }
}

/// NOTES-5: `notes.marks`: the note's marks, where each stands now.
pub(crate) async fn marks(store: Arc<NotesStore>, seam: Seam, user: Caller, id: String) -> Vec<NotesEvent> {
    if let Err(ev) = reached(&store, &seam, &user, &id, "notes.marks").await {
        return vec![ev];
    }
    match store.marks(&id).await {
        Ok(rows) => rows.into_iter().map(|mark| NotesEvent::Mark { mark }).collect(),
        Err(e) => vec![failed(e)],
    }
}

/// `notes.waiting`: open threads asked of someone, in the notes the caller reads.
pub(crate) async fn waiting(store: Arc<NotesStore>, seam: Seam, user: Caller, to: Option<String>) -> Vec<NotesEvent> {
    let orgs = match orgs_holding(&seam, &user, "notes.waiting").await {
        Ok(o) => o,
        Err(ev) => return vec![ev],
    };
    match store.waiting(orgs, to).await {
        Ok(rows) => rows.into_iter().map(|thread| NotesEvent::Thread { thread }).collect(),
        Err(e) => vec![failed(e)],
    }
}

/// `notes.usage`: every principal's notes made and bytes, for quota.recount — an admin of `*`'s.
/// V2-70: `daily`, by the day each change was kept.
pub(crate) async fn usage(store: Arc<NotesStore>, seam: Seam, user: Caller, daily: bool) -> Vec<NotesEvent> {
    if !seam.everywhere(user.context(), "quota.set") {
        return vec![NotesEvent::Error { message: "every principal's usage is an admin of *'s".into() }];
    }
    if daily {
        return match store.usage_daily().await {
            Ok(rows) => rows.into_iter().map(|(principal, day, count, bytes)| NotesEvent::UsageDay { principal, day, count, bytes }).collect(),
            Err(e) => vec![NotesEvent::Error { message: e.to_string() }],
        };
    }
    match store.usage_by_principal().await {
        Ok(rows) => rows.into_iter().map(|(principal, count, bytes)| NotesEvent::Usage { principal, count, bytes }).collect(),
        Err(e) => vec![NotesEvent::Error { message: e.to_string() }],
    }
}
