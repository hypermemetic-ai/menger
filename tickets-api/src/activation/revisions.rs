//! TICKETS-7: `tickets revisions` — a ticket's versions (`list`, `get`,
//! `diff`) and putting one back (`restore`). Every write that changes a
//! ticket's own fields keeps a revision (`store/revisions.rs`); a restore is
//! an `update` of the chosen fields to a revision's values, under the
//! update's rules, and is itself a new revision — nothing is lost.
//!
//! A `#[plexus_macros::child]` of `tickets` on the frame (ACCESS-13, README
//! "The frame"): `tickets.revisions.<method>` on the wire, `RevisionsClient`
//! reached from the parent's as `client.revisions()`. A child and not four
//! verbs on `tickets`: `tickets.restore` is ARCHIVE-2's already, and the
//! children are the house's shape for a ticket's records (ACCESS-14..18).

use super::*;
use crate::vocab::RevisionField;

/// A ticket's revisions child. Server-only: it holds the store.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct Revisions {
    pub(super) store: Arc<Store>,
    pub(super) access: Access,
}

/// `list` — the page, then `revisions_end`.
#[cfg(feature = "server")]
fn list_stream(store: Arc<Store>, caller: VerifiedUser, id: String, before: Option<u32>, limit: Option<u32>) -> impl Stream<Item = TicketsEvent> + Send + 'static {
    stream! {
        match store.revisions(&caller, &id, before, limit).await {
            Ok(page) => {
                for revision in page.revisions {
                    yield TicketsEvent::Revision { revision };
                }
                yield TicketsEvent::RevisionsEnd { count: page.count, latest: page.latest, next: page.next };
            }
            Err(StoreError::NotFound(_)) => yield TicketsEvent::NotFound { id: Asked::stored(id.as_str()) },
            Err(e) => yield veiled(&caller, e),
        }
    }
}

/// What a restore's revision says: what it put back and from where, then
/// the caller's own words.
#[cfg(feature = "server")]
fn restore_message(rev: u32, fields: &[String], said: Option<String>) -> String {
    let put = format!("restored {} from revision {rev}", fields.join(", "));
    match said {
        Some(m) => format!("{put}: {m}"),
        None => put,
    }
}

/// `restore` — the update a revision asks, written as one.
#[cfg(feature = "server")]
async fn restore_write(store: &Arc<Store>, user: &VerifiedUser, id: &TicketRef, rev: u32, fields: Option<Vec<RevisionField>>, act: Act) -> Result<TicketsEvent, Failed> {
    let on = permit::<Editor>(store, user, id)?;
    let asked = fields.map(|f| f.into_iter().map(|f| f.as_str().to_string()).collect());
    let crate::store::Restoring { update, fields } = store.restoring(user, on.id(), rev, asked).await?;
    if fields.is_empty() {
        let ticket = store.get(user, on.id()).await?;
        return Ok(TicketsEvent::RevisionRestored { ticket, from_rev: rev, changed: Vec::new(), rev: None });
    }
    let at = updating(store, user, on, &update)?;
    let act = Act { message: Some(restore_message(rev, &fields, act.message)), ..act };
    crate::store::say_act(&act);
    let crate::store::Updated { ticket, changed } = store.update_by(at, update, act).await?;
    let latest = store.revisions(user, &ticket.id, None, Some(1)).await.ok().map(|p| p.latest);
    Ok(TicketsEvent::RevisionRestored { ticket, from_rev: rev, changed, rev: latest })
}

// The attribute is NOT `#[cfg]`-gated: it has to run in the client-only
// build too, or there would be no `RevisionsClient`.
/// A ticket's revisions: every version it had, open any, diff two, restore one (TICKETS-7).
#[plexus_macros::activation(
    namespace = "revisions",
    auth = "required",
    request = MengerRequest,
    client,
    typed_params,
    server_cfg = "feature = \"server\"",
    crate_path = "plexus_core",
    roles(viewer = ["list", "get", "diff"], editor = [viewer, "restore"])
)]
impl Revisions {
    /// A ticket's revisions, newest first: rev, when, who, from where, why, what changed, the body's hash, size and +/- lines; then revisions_end { count, latest, next }.
    ///
    /// TICKETS-7: yields one `Revision` per revision of the ticket, newest
    /// first — at most `limit` (100; at most 500), below `before` when
    /// given — then `RevisionsEnd { count, latest, next }` (`next`: the
    /// `before` of the next page, `null` at the oldest). A ticket older than
    /// revisions has none until it is first changed; that write keeps its
    /// version before (`source: baseline`) and its own. Transitions stay in
    /// `history`. `NotFound` for a ticket out of the caller's reach. From
    /// storage, not the cache.
    #[plexus_macros::method(
        group = "read",
        example(note = "what changed in DEPLOY-9, newest first", params = r#"{"id": "DEPLOY-9"}"#)
    )]
    pub async fn list(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. DEPLOY-9 (a deleted ticket's too).
        id: TicketRef,
        /// Only revisions older than this number (optional; the page before, from revisions_end's next).
        before: Option<u32>,
        /// At most this many (optional; default 100, at most 500).
        limit: Option<u32>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        list_stream(self.store.clone(), user, id.into_string(), before, limit)
    }

    /// The ticket exactly as it was at a revision; yields revision_at { revision, ticket }.
    ///
    /// TICKETS-7: every field a revision keeps (title, body, status, kind,
    /// severity, parent, blockers, labels, commits) as it was then; the rest
    /// (its id, address, children, placement) as it is now. A body that uses
    /// a component or a theme renders with today's definitions. `NotFound`
    /// for a ticket out of reach; an `Error` for a revision it does not have.
    #[plexus_macros::method(
        group = "read",
        example(note = "DEPLOY-9 as it was at its third revision", params = r#"{"id": "DEPLOY-9", "rev": 3}"#)
    )]
    pub async fn get(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. DEPLOY-9.
        id: TicketRef,
        /// The revision's number, from 1.
        rev: u32,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        answered!(store = self.store.clone(), user, Some(id.to_string()) => {
            let (revision, ticket) = store.revision(&user, &id, rev).await?;
            Ok(TicketsEvent::RevisionAt { revision, ticket })
        })
    }

    /// What changed between two revisions: field by field, the frontmatter key by key, the body as a line diff with words within a changed line; yields revision_diff { diff }.
    ///
    /// TICKETS-7: `from` → `to` — `to` omitted, the latest; `from` omitted,
    /// the revision before `to`. Each field that differs with its value in
    /// each; the body's frontmatter key by key; the body's line diff in hunks
    /// (three lines of context; a line changed in place carries its words),
    /// of its source — or, `text: true`, of the text a reader sees (no
    /// markup, no scripts or styles, no frontmatter). At most 4,000 lines of
    /// hunks (`truncated` says so).
    #[plexus_macros::method(
        group = "read",
        example(note = "revision 3 against 5", params = r#"{"id": "DEPLOY-9", "from": 3, "to": 5}"#),
        example(note = "the last change, as a reader sees it", params = r#"{"id": "DEPLOY-9", "text": true}"#)
    )]
    pub async fn diff(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. DEPLOY-9.
        id: TicketRef,
        /// The older revision (optional; default the one before to).
        from: Option<u32>,
        /// The newer revision (optional; default the latest).
        to: Option<u32>,
        /// Diff the visible text instead of the source (optional; default false).
        text: Option<bool>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        let view = if text.unwrap_or(false) { crate::store::DiffView::Text } else { crate::store::DiffView::Source };
        answered!(store = self.store.clone(), user, Some(id.to_string()) => {
            Ok(TicketsEvent::RevisionDiff { diff: store.revision_diff(&user, &id, from, to, view).await? })
        })
    }

    /// Put a revision's fields back — all it keeps but its commits, or those named — as one update, itself a new revision; yields revision_restored { ticket, from_rev, changed, rev }.
    ///
    /// TICKETS-7: writes the chosen fields of revision `rev` (default: title,
    /// body, status, kind, severity, parent, blocked_by, labels — each that
    /// differs from now) through `update`'s rules: a parent that no longer
    /// exists, a blocker cycle, a duplicate input id, a component another
    /// ticket defines are refused, naming why, and nothing is written. The
    /// revision it keeps says what it restored, from which revision, and
    /// `message`. Nothing differing writes nothing (`changed: []`, `rev:
    /// null`). A deleted ticket is undeleted first; commits are never
    /// restored; a parent is moved, never cleared.
    #[plexus_macros::method(
        group = "write",
        target = "id",
        values(source = ["app", "api", "pipeline"]),
        example(note = "put revision 3 back", params = r#"{"id": "DEPLOY-9", "rev": 3, "message": "the planned version, for comparison"}"#),
        example(note = "only its body", params = r#"{"id": "DEPLOY-9", "rev": 3, "fields": ["body"]}"#)
    )]
    pub async fn restore(
        &self,
        #[from_auth(self.access.verify)] user: VerifiedUser,
        /// Ticket id, e.g. DEPLOY-9.
        id: TicketRef,
        /// The revision to restore from.
        rev: u32,
        /// Which fields (optional; default every one it keeps but commits): title, body, status, kind, severity, parent, blocked_by, labels.
        fields: Option<Vec<RevisionField>>,
        /// Where the change comes from: app | api | pipeline (optional, default api).
        source: Option<CallerSource>,
        /// Why, kept in the revision after what it restored (optional).
        message: Option<Message>,
    ) -> impl Stream<Item = TicketsEvent> + Send + 'static {
        answered!(store = self.store.clone(), user, Some(id.to_string()) => {
            let act = act_of(&user, source, message)?;
            restore_write(&store, &user, &id, rev, fields, act).await
        })
    }
}
