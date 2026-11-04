//! What the `docs` methods do, beside the activation (its `impl` keeps
//! only the methods — MENGER-29's rule). Each takes the [`Caller`] the
//! dispatch resolved; whether it may read or write a document is the
//! gate's word on the document's organization, asked over the [`Seam`].
//! How the body is written, replayed and squashed is [`crate::crdt`]'s;
//! who is on a document, [`crate::presence`]'s.

use crate::caller::Caller;
use crate::events::{Doc, DocsEvent};
use crate::seam::{grant_of, Seam};
use crate::store::{DocsStore, Listing, StoreError};
use menger_tickets_api::members::Role;
use std::sync::Arc;

/// One event, as a method's stream.
pub(crate) fn once(ev: impl std::future::Future<Output = DocsEvent> + Send + 'static) -> impl futures::Stream<Item = DocsEvent> + Send + 'static {
    futures::stream::once(ev)
}

/// Each of the events, as a method's stream.
pub(crate) fn many(evs: impl std::future::Future<Output = Vec<DocsEvent>> + Send + 'static) -> impl futures::Stream<Item = DocsEvent> + Send + 'static {
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
        "docs.threads" | "docs.waiting" | "docs.get" | "docs.list" | "docs.history" | "docs.replay" | "docs.state" | "docs.present" | "docs.watch" | "docs.may" => "viewer",
        "docs.delete" => "admin",
        "docs.squash" => "docs.squash's grant (admin's by default)",
        _ => "editor",
    }
}

/// A store failure as the one event it is.
pub(crate) fn failed(e: StoreError) -> DocsEvent {
    match e {
        StoreError::NotFound(id) => DocsEvent::no_doc(&id),
        StoreError::NoRevision(id, r) => DocsEvent::NotFound { id: Some(id), revision: Some(r), org: None, thread: None },
        StoreError::NoThread(t) => DocsEvent::NotFound { id: None, revision: None, org: None, thread: Some(t) },
        StoreError::BadRange(m) => DocsEvent::Error { message: format!("{m} (nothing was written)") },
        StoreError::Quota(m) => DocsEvent::Refused { reason: "quota".into(), message: format!("{m} (nothing was written)") },
        e @ StoreError::BadUpdate(_) => DocsEvent::Error { message: format!("{e} (nothing was written)") },
        StoreError::Db(m) => DocsEvent::Error { message: m },
    }
}

/// `forbidden`, naming what `grant` takes on `org`.
pub(crate) fn forbidden(user: &Caller, grant: &str, what: &str, org: &str) -> DocsEvent {
    DocsEvent::Refused { reason: "forbidden".into(), message: format!("{grant} on {what} needs {} on {org}: (or on *); {} does not hold it", role_of(grant), user.name()) }
}

/// The live document `id`, when `user` reads it (`docs.get` on its
/// organization) and holds `grant` there too. `not_found` for one it does
/// not read — as for none (ACCESS-10) — and `refused { forbidden }` for one
/// it reads and may not do this to.
pub(crate) async fn reached(store: &Arc<DocsStore>, seam: &Seam, user: &Caller, id: &str, grant: &str) -> Result<Doc, DocsEvent> {
    let doc = store.get(id).await.map_err(failed)?;
    match seam.holds(user.context(), "docs.get", &doc.org) {
        Ok(true) => {}
        Ok(false) => return Err(DocsEvent::no_doc(id)),
        Err(message) => return Err(DocsEvent::Error { message }),
    }
    if grant != "docs.get" {
        match seam.holds(user.context(), grant, &doc.org) {
            Ok(true) => {}
            Ok(false) => return Err(forbidden(user, grant, id, &doc.org)),
            Err(message) => return Err(DocsEvent::Error { message }),
        }
    }
    Ok(doc)
}

/// The organizations `user` holds `grant` in, by the gate, of those it is in.
async fn orgs_holding(seam: &Seam, user: &Caller, grant: &str) -> Result<Vec<String>, DocsEvent> {
    let orgs = seam.orgs(user.context()).await.map_err(|message| DocsEvent::Error { message })?;
    let mut out = Vec::new();
    for org in orgs {
        if seam.holds(user.context(), grant, &org).map_err(|message| DocsEvent::Error { message })? {
            out.push(org);
        }
    }
    Ok(out)
}

/// `docs.create`: a document in `org` (else the first organization the
/// caller may write documents in), revision 1.
pub(crate) async fn create(store: Arc<DocsStore>, seam: Seam, user: Caller, title: String, body: String, org: Option<String>) -> DocsEvent {
    let org = match org {
        Some(org) => {
            let org = org.trim().trim_end_matches(':').to_string();
            match seam.orgs(user.context()).await {
                Ok(orgs) if orgs.contains(&org) => {}
                Ok(_) => return DocsEvent::NotFound { id: None, revision: None, org: Some(org), thread: None },
                Err(message) => return DocsEvent::Error { message },
            }
            match seam.holds(user.context(), "docs.create", &org) {
                Ok(true) => org,
                Ok(false) => return forbidden(&user, "docs.create", &format!("{org}:"), &org),
                Err(message) => return DocsEvent::Error { message },
            }
        }
        None => match orgs_holding(&seam, &user, "docs.create").await {
            Ok(orgs) => match orgs.into_iter().next() {
                Some(org) => org,
                None => return DocsEvent::Refused { reason: "forbidden".into(), message: format!("{} may write documents in no organization: docs.create needs editor on one", user.name()) },
            },
            Err(ev) => return ev,
        },
    };
    match store.create(&org, &title, &body, &user.name(), user.principal()).await {
        Ok(doc) => DocsEvent::Created { doc },
        Err(e) => failed(e),
    }
}

/// `docs.get`: the document now, its body the markdown derived from its CRDT.
pub(crate) async fn get(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String) -> DocsEvent {
    match reached(&store, &seam, &user, &id, "docs.get").await {
        Ok(doc) => DocsEvent::Doc { doc },
        Err(ev) => ev,
    }
}

/// `docs.list`: a page of the documents of the organizations the caller
/// reads documents in, newest made first, their titles searched.
pub(crate) async fn list(store: Arc<DocsStore>, seam: Seam, user: Caller, query: Option<String>, limit: Option<u32>, after: Option<String>) -> Vec<DocsEvent> {
    let orgs = match orgs_holding(&seam, &user, "docs.list").await {
        Ok(o) => o,
        Err(ev) => return vec![ev],
    };
    let limit = limit.map_or(LIST_PAGE, |l| (l as usize).clamp(1, LIST_MAX));
    match store.list(Listing { orgs: Some(orgs), query, limit, after }).await {
        Ok(page) => vec![
            DocsEvent::Rows { items: page.rows.into_iter().map(|doc| DocsEvent::Doc { doc }).collect() },
            DocsEvent::Page { next: page.next, remaining: page.remaining },
        ],
        Err(e) => vec![failed(e)],
    }
}

/// `docs.rename`.
pub(crate) async fn rename(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, title: String) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.rename").await {
        return ev;
    }
    match store.rename(&id, &title, &user.name(), user.principal()).await {
        Ok(doc) => DocsEvent::Renamed { id: doc.id, title: doc.title, revision: doc.revision, at: doc.at, by: doc.by, doc: None },
        Err(e) => failed(e),
    }
}

/// `docs.delete`.
pub(crate) async fn delete(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.delete").await {
        return ev;
    }
    match store.delete(&id, &user.name(), user.principal()).await {
        Ok((_, at)) => DocsEvent::Deleted { id, at, by: user.name() },
        Err(e) => failed(e),
    }
}

/// A writer with no watch open is present while it writes (an agent's
/// save over synapse): an entry under its own session, idling out.
fn writing(store: &DocsStore, user: &Caller, doc: &crate::Doc, session: Option<&str>) {
    let session = session.map_or_else(|| format!("w-{}", user.principal()), str::to_string);
    if !store.taken(&doc.id, &session, user.principal()) {
        store.arrive(&doc.id, &doc.org, &session, &user.who(), false, None, None);
    }
}

/// `docs.update`: a Yjs update from the editor, merged and kept.
pub(crate) async fn update(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, update: String, session: Option<String>) -> DocsEvent {
    let doc = match reached(&store, &seam, &user, &id, "docs.update").await {
        Ok(d) => d,
        Err(ev) => return ev,
    };
    writing(&store, &user, &doc, session.as_deref());
    match store.update(&id, &update, &user.name(), user.principal(), session).await {
        Ok(w) => DocsEvent::Applied { id: w.doc.id, revision: w.doc.revision },
        Err(e) => failed(e),
    }
}

/// `docs.save`: a whole body, merged into the CRDT as its own edits.
pub(crate) async fn save(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, body: String, base: Option<u32>) -> DocsEvent {
    let doc = match reached(&store, &seam, &user, &id, "docs.save").await {
        Ok(d) => d,
        Err(ev) => return ev,
    };
    writing(&store, &user, &doc, None);
    match store.save(&id, &body, base, &user.name(), user.principal()).await {
        Ok(w) => DocsEvent::Saved { id: w.doc.id, revision: w.doc.revision, at: w.doc.at, by: w.doc.by, stale: w.stale },
        Err(e) => failed(e),
    }
}

/// `docs.restore`: the body as it stood after change `seq`, merged back.
pub(crate) async fn restore(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, seq: u32) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.restore").await {
        return ev;
    }
    match store.restore(&id, seq, &user.name(), user.principal()).await {
        Ok(w) => DocsEvent::Saved { id: w.doc.id, revision: w.doc.revision, at: w.doc.at, by: w.doc.by, stale: false },
        Err(e) => failed(e),
    }
}

/// `docs.state`: what the caller's CRDT lacks.
pub(crate) async fn state(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, vector: Option<String>) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.state").await {
        return ev;
    }
    match store.state(&id, vector).await {
        Ok(s) => DocsEvent::State { id, update: s.update, vector: s.vector, revision: s.revision },
        Err(e) => failed(e),
    }
}

/// `docs.replay`: the document as it stood after `seq`, or at `at`.
pub(crate) async fn replay(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, seq: Option<u32>, at: Option<i64>) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.replay").await {
        return ev;
    }
    match store.replay(&id, seq, at).await {
        Ok(doc) => DocsEvent::Doc { doc },
        Err(e) => failed(e),
    }
}

/// `docs.history`: newest first.
pub(crate) async fn history(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, limit: Option<u32>, before: Option<u32>) -> Vec<DocsEvent> {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.history").await {
        return vec![ev];
    }
    match store.history(&id, limit.map_or(LIST_MAX, |l| (l as usize).clamp(1, LIST_MAX)), before).await {
        Ok(rows) => rows.into_iter().map(|change| DocsEvent::Change { change }).collect(),
        Err(e) => vec![failed(e)],
    }
}

/// `docs.squash`: its own grant on the document's organization, then the
/// changes up to `until` collapsed.
pub(crate) async fn squash(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, until: u32) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.squash").await {
        return ev;
    }
    match store.squash(&id, until, &user.name(), user.principal()).await {
        Ok(removed) => DocsEvent::Squashed { id, until, removed, at: crate::store::now(), by: user.name() },
        Err(e) => failed(e),
    }
}

/// `docs.present`: everyone on the document now.
pub(crate) async fn present(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String) -> Vec<DocsEvent> {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.present").await {
        return vec![ev];
    }
    store.present(&id).into_iter().map(|present| DocsEvent::Present { present }).collect()
}

/// `docs.here`: the caller's caret on the document, as its session.
pub(crate) async fn here(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, session: String, client: Option<u64>, awareness: Option<String>) -> DocsEvent {
    let doc = match reached(&store, &seam, &user, &id, "docs.here").await {
        Ok(d) => d,
        Err(ev) => return ev,
    };
    if store.taken(&id, &session, user.principal()) {
        return DocsEvent::Refused { reason: "forbidden".into(), message: format!("session {session} on {id} is someone else's") };
    }
    DocsEvent::Present { present: store.arrive(&doc.id, &doc.org, &session, &user.who(), false, client, awareness) }
}

/// `docs.may`: whether the caller holds `role` on the document — the grant
/// that role's methods take ([`grant_of`]) on its organization.
pub(crate) async fn may(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, role: Role) -> DocsEvent {
    let doc = match reached(&store, &seam, &user, &id, "docs.get").await {
        Ok(d) => d,
        Err(ev) => return ev,
    };
    let held = match grant_of(role) {
        None => false,
        Some(grant) => match seam.holds(user.context(), grant, &doc.org) {
            Ok(h) => h,
            Err(message) => return DocsEvent::Error { message },
        },
    };
    DocsEvent::May { id: doc.id, role, held }
}

/// V2-30: who a thread's hand is.
fn hand(user: &Caller) -> crate::threads::Hand {
    crate::threads::Hand { name: user.name(), principal: user.principal().to_string() }
}

/// `docs.ask`: a question on a range of the document.
pub(crate) async fn ask(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, from: u32, to: u32, text: String, to_whom: Option<String>) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.ask").await {
        return ev;
    }
    match store.ask(&id, from, to, &text, to_whom, hand(&user)).await {
        Ok(thread) => DocsEvent::Asked { thread },
        Err(e) => failed(e),
    }
}

/// `docs.reply`: an answer under a thread.
pub(crate) async fn reply(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, thread: String, body: String) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.reply").await {
        return ev;
    }
    match store.reply(&id, &thread, &body, hand(&user)).await {
        Ok(thread) => DocsEvent::Replied { thread },
        Err(e) => failed(e),
    }
}

/// `docs.resolve`: a thread folded, or opened again.
pub(crate) async fn resolve(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String, thread: String, open: bool) -> DocsEvent {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.resolve").await {
        return ev;
    }
    match store.resolve(&id, &thread, open, hand(&user)).await {
        Ok(thread) => DocsEvent::Resolved { thread },
        Err(e) => failed(e),
    }
}

/// `docs.threads`: the document's threads.
pub(crate) async fn threads(store: Arc<DocsStore>, seam: Seam, user: Caller, id: String) -> Vec<DocsEvent> {
    if let Err(ev) = reached(&store, &seam, &user, &id, "docs.threads").await {
        return vec![ev];
    }
    match store.threads(&id).await {
        Ok(rows) => rows.into_iter().map(|thread| DocsEvent::Thread { thread }).collect(),
        Err(e) => vec![failed(e)],
    }
}

/// `docs.waiting`: open threads asked of someone, in the documents the caller reads.
pub(crate) async fn waiting(store: Arc<DocsStore>, seam: Seam, user: Caller, to: Option<String>) -> Vec<DocsEvent> {
    let orgs = match orgs_holding(&seam, &user, "docs.waiting").await {
        Ok(o) => o,
        Err(ev) => return vec![ev],
    };
    match store.waiting(orgs, to).await {
        Ok(rows) => rows.into_iter().map(|thread| DocsEvent::Thread { thread }).collect(),
        Err(e) => vec![failed(e)],
    }
}

/// `docs.usage`: every principal's documents made and bytes, for quota.recount — an admin of `*`'s.
pub(crate) async fn usage(store: Arc<DocsStore>, seam: Seam, user: Caller) -> Vec<DocsEvent> {
    if !seam.everywhere(user.context(), "quota.set") {
        return vec![DocsEvent::Error { message: "every principal's usage is an admin of *'s".into() }];
    }
    match store.usage_by_principal().await {
        Ok(rows) => rows.into_iter().map(|(principal, count, bytes)| DocsEvent::Usage { principal, count, bytes }).collect(),
        Err(e) => vec![DocsEvent::Error { message: e.to_string() }],
    }
}
