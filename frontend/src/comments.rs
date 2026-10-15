//! COMMENT-3: the app's copy of the comments (COMMENT-2) — what annotation
//! mode pins, what `#/comments` lists, what a ticket page's side panel and
//! the header count.
//!
//! **Asked for by what shows it** — the registry's and the inputs' rule
//! (src/registry.rs, src/inputs.rs). Nothing at startup asks `comments`. The
//! first view that needs them — annotation mode, the comments page, a ticket
//! page's count — calls [`want`]; from then on, on every new connection,
//! `comments.watch` is subscribed and then `comments.list` fetched, page by
//! page ([`refresh`]), and between fetches the watch keeps the copy:
//! `added`, `resolved` and `reopened` each carry the whole comment, so
//! applying one is an upsert ([`Held::apply`]); `lagged` fetches again.
//!
//! **No change is lost to the fetch.** The watch is subscribed before the
//! list is asked for, so an answer may be older than an event heard, or
//! newer: what is heard while the fetch is out is replayed onto its answer
//! ([`settle`]). An upsert of a whole comment gives the newest state
//! whichever side of the answer it fell on.
//!
//! **The writes** — `add`, `resolve`, `reopen` — are the app's calls on the
//! same connection, typed by `CommentsClient` (the activation's own
//! declaration); the comment each answers with is applied at once, so the
//! view that wrote it does not wait for its own echo on the watch.
//!
//! What decides — the order, applying an event, the replay, the counts — is
//! pure and pinned by native tests; the rest is the session's socket.

use menger_tickets_api::vocab::CommentId;
use std::cell::{Cell, RefCell};

use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::{Anchor, Comment, CommentState, CommentsEvent};
use plexus_client::ClientEvent;
use wasm_bindgen_futures::spawn_local;

use crate::session;

/// How many comments one `list` page asks for.
const PAGE: u32 = 500;

/// The comments as the app holds them: newest first (by id — ids are given
/// in the order comments are added).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Held {
    list: Vec<Comment>,
}

impl Held {
    /// `list` answers: one `comment` per comment; anything else contributes
    /// nothing.
    pub(crate) fn from_events(events: impl IntoIterator<Item = CommentsEvent>) -> Self {
        let mut held = Held::default();
        for ev in events {
            if let ev @ CommentsEvent::Comment { .. } = ev {
                if let Some(c) = ev.into_comment() {
                    held.upsert(c);
                }
            }
        }
        held
    }

    /// Apply a `watch` event; whether the copy moved.
    pub(crate) fn apply(&mut self, ev: &CommentsEvent) -> bool {
        match ev {
            CommentsEvent::Added { comment } | CommentsEvent::Resolved { comment } | CommentsEvent::Reopened { comment } => {
                self.upsert(comment.clone())
            }
            _ => false,
        }
    }

    /// The whole comment, in place of the one with its id, or new in its
    /// place by id. Whether anything moved.
    pub(crate) fn upsert(&mut self, c: Comment) -> bool {
        match self.list.binary_search_by(|x| c.id.cmp(&x.id)) {
            Ok(at) if self.list[at] == c => false,
            Ok(at) => {
                self.list[at] = c;
                true
            }
            Err(at) => {
                self.list.insert(at, c);
                true
            }
        }
    }

    /// Every comment, newest first.
    pub(crate) fn list(&self) -> &[Comment] {
        &self.list
    }

    pub(crate) fn get(&self, id: u64) -> Option<&Comment> {
        self.list.iter().find(|c| c.id == id)
    }

    /// How many are open — the header's badge.
    pub(crate) fn open_count(&self) -> usize {
        self.list.iter().filter(|c| c.state == CommentState::Open).count()
    }

    /// How many open ones are on `ticket` — the ticket page's side panel.
    pub(crate) fn open_on(&self, ticket: &str) -> usize {
        self.list
            .iter()
            .filter(|c| c.state == CommentState::Open && c.anchor.ticket.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(ticket)))
            .count()
    }
}

/// The fetch's rule: `answer`, then every event heard while it was out, in
/// order.
pub(crate) fn settle(answer: Vec<CommentsEvent>, heard: &[CommentsEvent]) -> Held {
    let mut held = Held::from_events(answer);
    for ev in heard {
        held.apply(ev);
    }
    held
}

/// What the ticket page's side panel says of `n` open comments.
pub(crate) fn count_words(n: usize) -> String {
    match n {
        0 => "none open".to_string(),
        1 => "1 open".to_string(),
        n => format!("{n} open"),
    }
}

thread_local! {
    /// The comments — `None` until the first fetch lands.
    static HELD: ArcRwSignal<Option<Held>> = ArcRwSignal::new(None);
    /// Whether some view has asked for them; from then on they are kept.
    static WANTED: Cell<bool> = const { Cell::new(false) };
    /// The newest fetch; an older one's answer is dropped.
    static FETCH: Cell<u32> = const { Cell::new(0) };
    /// While a fetch is out: the events heard since it went.
    static HEARD: RefCell<Option<Vec<CommentsEvent>>> = const { RefCell::new(None) };
    /// The watch's pump on this connection — ended when a new one starts.
    static PUMP: RefCell<Option<futures::future::AbortHandle>> = const { RefCell::new(None) };
}

/// The comments, as a signal.
pub(crate) fn signal() -> ArcRwSignal<Option<Held>> {
    HELD.with(Clone::clone)
}

/// A view needs the comments: watch and fetch them now if this is the first
/// ask (and the socket is up — else the session's [`refresh`] will).
pub(crate) fn want() {
    if !WANTED.with(|w| w.replace(true)) {
        start();
    }
}

/// A new connection: the watch subscribed on it, and the comments fetched
/// again — if anything wants them.
pub(crate) fn refresh() {
    if WANTED.with(Cell::get) {
        start();
    }
}

fn start() {
    let Some(client) = session::comments_client() else {
        return; // not connected: the session refreshes once it is
    };
    let (abort, reg) = futures::future::AbortHandle::new_pair();
    if let Some(old) = PUMP.with(|p| p.borrow_mut().replace(abort)) {
        old.abort();
    }
    // The subscribe frame is written as the call is made (MACRO-6), when
    // this task first runs — before the fetch's task, spawned after it: the
    // watch is live before the list is asked for.
    let pump = async move {
        let mut stream = client.watch();
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(ClientEvent::Data(CommentsEvent::Lagged { missed })) => {
                    leptos::logging::log!("comments watch lagged: {missed} change(s) missed; fetching again");
                    fetch();
                }
                Ok(ClientEvent::Data(ev)) => heard(&ev),
                Ok(_) => {}
                Err(_) => break, // the socket: the next session starts it again
            }
        }
    };
    spawn_local(async move {
        let _ = futures::future::Abortable::new(pump, reg).await;
    });
    fetch();
}

/// A `watch` event: the copy takes it (and keeps it for replay while a
/// fetch is out).
fn heard(ev: &CommentsEvent) {
    HEARD.with(|h| {
        if let Some(buf) = h.borrow_mut().as_mut() {
            buf.push(ev.clone());
        }
    });
    HELD.with(|s| {
        s.maybe_update(|held| held.as_mut().is_some_and(|held| held.apply(ev)));
    });
}

/// A comment a write answered with, applied at once.
fn took(c: Comment) {
    heard(&CommentsEvent::Added { comment: c });
}

fn fetch() {
    let Some(client) = session::comments_client() else { return };
    let n = FETCH.with(|f| {
        let n = f.get().wrapping_add(1);
        f.set(n);
        n
    });
    HEARD.with(|h| *h.borrow_mut() = Some(Vec::new()));
    spawn_local(async move {
        let mut answer = Vec::new();
        let mut after = None;
        loop {
            match client.list_all(None, None, None, None, None, None, None, after, Some(PAGE)).await {
                Ok(events) => {
                    let mut next = None;
                    for ev in events {
                        match ev {
                            CommentsEvent::Page { next: n } => next = n,
                            CommentsEvent::Error { message } => leptos::logging::log!("comments: {message}"),
                            ev => answer.push(ev),
                        }
                    }
                    match next {
                        Some(n) => after = Some(CommentId::new(n)),
                        None => break,
                    }
                }
                Err(e) => {
                    leptos::logging::log!("comments: {}", session::describe(&e));
                    return;
                }
            }
        }
        if FETCH.with(Cell::get) != n {
            return; // a newer fetch went out; its answer is the one kept
        }
        let heard = HEARD.with(|h| h.borrow_mut().take()).unwrap_or_default();
        let held = settle(answer, &heard);
        HELD.with(|s| s.set(Some(held)));
    });
}

/// The one comment a write answers with, or what the server said instead.
fn answer_of(what: &str, events: Result<Vec<CommentsEvent>, plexus_client::ClientError>) -> Result<Comment, String> {
    let events = events.map_err(|e| format!("{what}: {}", session::describe(&e)))?;
    for ev in events {
        match ev {
            CommentsEvent::Error { message } => return Err(message),
            ev => {
                if let Some(c) = ev.into_comment() {
                    took(c.clone());
                    return Ok(c);
                }
            }
        }
    }
    Err(format!("{what}: no answer"))
}

/// `comments.add { anchor, body }` — the comment as kept.
pub(crate) async fn add(anchor: Anchor, body: String) -> Result<Comment, String> {
    let client = session::comments_client().ok_or_else(|| "not connected — nothing was kept".to_string())?;
    answer_of("add", client.add_all(anchor, crate::typed::typed(&body)?, None).await)
}

/// `comments.resolve { id }`.
pub(crate) async fn resolve(id: u64) -> Result<Comment, String> {
    let client = session::comments_client().ok_or_else(|| "not connected".to_string())?;
    answer_of("resolve", client.resolve_all(CommentId::new(id)).await)
}

/// `comments.reopen { id }`.
pub(crate) async fn reopen(id: u64) -> Result<Comment, String> {
    let client = session::comments_client().ok_or_else(|| "not connected".to_string())?;
    answer_of("reopen", client.reopen_all(CommentId::new(id)).await)
}

/// `comments.list { under }` — the ids of every comment whose ticket is in
/// `under`'s lineage (the server's to say: it holds the tree).
pub(crate) async fn ids_under(under: String) -> Result<std::collections::HashSet<u64>, String> {
    let client = session::comments_client().ok_or_else(|| "not connected".to_string())?;
    let mut out = std::collections::HashSet::new();
    let mut after = None;
    let under: menger_tickets_api::vocab::Under = crate::typed::typed(&under)?;
    loop {
        let events = client
            .list_all(None, Some(under.clone()), None, None, None, None, None, after, Some(PAGE))
            .await
            .map_err(|e| session::describe(&e))?;
        let mut next = None;
        for ev in events {
            match ev {
                CommentsEvent::Comment { id, .. } => {
                    out.insert(id);
                }
                CommentsEvent::Page { next: n } => next = n,
                CommentsEvent::Error { message } => return Err(message),
                _ => {}
            }
        }
        match next {
            Some(n) => after = Some(CommentId::new(n)),
            None => return Ok(out),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A comment on `ticket` (none: `None`) at `path`, open.
    pub(crate) fn comment(id: u64, route: &str, path: &str, ticket: Option<&str>) -> Comment {
        Comment {
            id: menger_tickets_api::vocab::CommentId::new(id),
            state: CommentState::Open,
            author: "owner".into(),
            at: 1_791_510_000 + id as i64,
            anchor: Anchor {
                route: route.into(),
                path: path.into(),
                label: format!("label {id}"),
                excerpt: format!("excerpt {id}"),
                ticket: ticket.map(str::to_string),
                version: ticket.map(|_| 1),
            },
            body: format!("body {id}"),
            principal: None,
        }
    }

    pub(crate) fn resolved(mut c: Comment) -> Comment {
        c.state = CommentState::Resolved;
        c
    }

    fn ids(h: &Held) -> Vec<u64> {
        h.list().iter().map(|c| c.id.get()).collect()
    }

    /// The copy is newest first, whatever order its comments came in; an
    /// event upserts the whole comment; one the copy holds as it is moves
    /// nothing.
    #[test]
    fn the_copy_is_newest_first_and_takes_whole_comments() {
        let a = comment(1, "#/ticket/D-2", "#t-status", Some("D-2"));
        let b = comment(2, "#/", "#annotate", None);
        let c = comment(3, "#/ticket/D-2", "#t-parent", Some("D-2"));
        let mut h = Held::from_events([CommentsEvent::from(c.clone()), CommentsEvent::from(a.clone()), CommentsEvent::Page { next: None }]);
        assert_eq!(ids(&h), [3, 1]);
        assert!(h.apply(&CommentsEvent::Added { comment: b.clone() }));
        assert_eq!(ids(&h), [3, 2, 1]);
        assert!(!h.apply(&CommentsEvent::Added { comment: b.clone() }), "held as it is");
        assert!(h.apply(&CommentsEvent::Resolved { comment: resolved(a.clone()) }));
        assert_eq!(h.get(1).map(|c| c.state), Some(CommentState::Resolved));
        assert_eq!(ids(&h), [3, 2, 1], "a state change keeps its place");
        assert!(h.apply(&CommentsEvent::Reopened { comment: a.clone() }));
        assert!(!h.apply(&CommentsEvent::Lagged { missed: 3 }));
        assert!(!h.apply(&CommentsEvent::Error { message: "x".into() }));
        // The counts: open across the copy, and open on one ticket.
        assert!(h.apply(&CommentsEvent::Resolved { comment: resolved(c.clone()) }));
        assert_eq!(h.open_count(), 2);
        assert_eq!(h.open_on("D-2"), 1);
        assert_eq!(h.open_on("d-2"), 1, "a ticket id, whatever its case");
        assert_eq!(h.open_on("D-3"), 0);
        assert_eq!(count_words(0), "none open");
        assert_eq!(count_words(1), "1 open");
        assert_eq!(count_words(4), "4 open");
    }

    /// What is heard while a fetch is out is replayed onto its answer: a
    /// comment born meanwhile is kept, and a resolve the answer predates
    /// wins.
    #[test]
    fn a_fetch_takes_what_was_heard_while_it_was_out() {
        let a = comment(1, "#/", "#annotate", None);
        let b = comment(2, "#/", "#conn", None);
        let heard = [CommentsEvent::Added { comment: b.clone() }, CommentsEvent::Resolved { comment: resolved(a.clone()) }];
        let h = settle(vec![CommentsEvent::from(a.clone()), CommentsEvent::Page { next: None }], &heard);
        assert_eq!(ids(&h), [2, 1]);
        assert_eq!(h.get(1).map(|c| c.state), Some(CommentState::Resolved));
        // An answer newer than what was heard: the same comment, unchanged.
        let h = settle(vec![CommentsEvent::from(b.clone()), CommentsEvent::from(a.clone())], &[CommentsEvent::Added { comment: b.clone() }]);
        assert_eq!(ids(&h), [2, 1]);
    }
}
