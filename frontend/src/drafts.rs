//! COMMENT-9: a comment is never lost on its way to the server.
//!
//! Owner, 2026-10-18, on a phone: *"left a comment here"*, then *"you can
//! see the failed submission?"* — the comment never reached the server,
//! and nothing anywhere said so. Three things keep it now:
//!
//! - **The draft is kept** in the browser (`localStorage`, [`STORE`]) by
//!   where it is being left ([`key`]: the anchor's route and path) from the
//!   first keystroke until the server answers `comment`; a box opened at
//!   the same place again (the Conversation after a reload or a trip to
//!   another page, the annotation box on the same element) starts from it.
//! - **A send has an answer**: [`send`] gives `comments.add` [`ANSWER_MS`]
//!   to answer — a socket that is down, or open and dead (a phone's
//!   backgrounded tab), is a reason in the box with a Retry, the draft
//!   still there ([`words`]). If the comment lands after all, the watch's
//!   `added` carrying the same text at the same place clears the draft
//!   ([`landed`]), so a Retry does not post it twice.
//! - **The failure is reported**: each one goes to an outbox in the
//!   browser ([`OUTBOX`]) and from there to `comments.report` on this
//!   connection or the next ([`flush`], called on every connection), so an
//!   agent reads it with `comments failures` — what it said, where, why.
//!
//! What decides — the key, the stored map, the outbox, whether a comment is
//! the draft landing, the words — is pure and pinned by native tests; the
//! rest is the browser.

use std::cell::Cell;
use std::collections::BTreeMap;

use leptos::prelude::*;
use menger_tickets_api::{Anchor, Comment, CommentsEvent};
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

use crate::{comments, dom, session};

/// Where the drafts are kept: one JSON map, [`key`] → [`Draft`].
pub(crate) const STORE: &str = "menger.comment.drafts";

/// Where the failures wait for the server: a JSON list of [`Failed`].
pub(crate) const OUTBOX: &str = "menger.comment.failures";

/// How long `comments.add` (and `comments.report`) has to answer.
pub(crate) const ANSWER_MS: i32 = 15_000;

/// The most failures the outbox keeps (the newest).
pub(crate) const OUTBOX_MAX: usize = 20;

/// A draft older than this many seconds is let go (a week).
pub(crate) const DRAFT_TTL: i64 = 7 * 24 * 3600;

/// One kept draft.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Draft {
    pub(crate) body: String,
    /// When it was last written, unix seconds.
    pub(crate) at: i64,
}

/// One failure, waiting to be reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Failed {
    pub(crate) anchor: Anchor,
    pub(crate) body: String,
    pub(crate) reason: String,
    pub(crate) failed_at: i64,
}

/// Where a draft is kept: the anchor's route and path — the Conversation
/// of a ticket, or one element on one page.
pub(crate) fn key(a: &Anchor) -> String {
    format!("{}|{}", a.route.trim(), a.path.trim())
}

/// COMMENT-8: where a reply's draft is kept — its thread's root.
pub(crate) fn reply_key(root: u64) -> String {
    format!("reply|{root}")
}

/// Where the draft `c` would have been is kept: its thread's, for a reply.
pub(crate) fn key_of(c: &Comment) -> String {
    c.reply_to.map_or_else(|| key(&c.anchor), |r| reply_key(r.get()))
}

/// The stored map, read — anything unreadable is none, and a draft past
/// [`DRAFT_TTL`] is gone.
pub(crate) fn read_map(text: Option<&str>, now: i64) -> BTreeMap<String, Draft> {
    let mut map: BTreeMap<String, Draft> = text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    map.retain(|_, d| now - d.at < DRAFT_TTL && !d.body.trim().is_empty());
    map
}

/// `map` with `key` holding `body` — or without it, when `body` is blank.
pub(crate) fn put(mut map: BTreeMap<String, Draft>, key: &str, body: &str, now: i64) -> BTreeMap<String, Draft> {
    if body.trim().is_empty() {
        map.remove(key);
    } else {
        map.insert(key.to_string(), Draft { body: body.to_string(), at: now });
    }
    map
}

/// Whether `c` is the draft `body`, kept at `key` — its place and its text
/// (trimmed): the comment landed after all.
pub(crate) fn is_landing(key_: &str, body: &str, c: &Comment) -> bool {
    !body.trim().is_empty() && key_of(c) == key_ && c.body.trim() == body.trim()
}

/// The outbox, read.
pub(crate) fn read_outbox(text: Option<&str>) -> Vec<Failed> {
    text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default()
}

/// The outbox with `f` at its end, the oldest let go past [`OUTBOX_MAX`].
pub(crate) fn push(mut out: Vec<Failed>, f: Failed) -> Vec<Failed> {
    out.push(f);
    let over = out.len().saturating_sub(OUTBOX_MAX);
    out.drain(..over);
    out
}

/// What the box says when a comment was not kept.
pub(crate) fn words(reason: &str) -> String {
    format!("Not kept: {} — your draft is still here.", reason.trim().trim_end_matches('.'))
}

/// Why no answer came in time.
pub(crate) fn timed_out() -> String {
    format!("no answer from the server in {} s (the connection may be down)", ANSWER_MS / 1000)
}

// ---------------------------------------------------------------------
// The browser

fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

fn now() -> i64 {
    crate::now_unix()
}

fn load_map() -> BTreeMap<String, Draft> {
    read_map(storage().and_then(|s| s.get_item(STORE).ok().flatten()).as_deref(), now())
}

fn store_map(map: &BTreeMap<String, Draft>) {
    if let (Some(s), Ok(text)) = (storage(), serde_json::to_string(map)) {
        let _ = s.set_item(STORE, &text);
    }
}

/// The draft kept at `key`, if there is one.
pub(crate) fn load(key: &str) -> Option<String> {
    load_map().remove(key).map(|d| d.body)
}

/// `body` kept at `key` (blank: let go).
pub(crate) fn save(key: &str, body: &str) {
    store_map(&put(load_map(), key, body, now()));
}

thread_local! {
    /// The newest comment heard that was a kept draft landing: its key and
    /// body — what an open box at that key compares with its own text.
    static LANDED: ArcRwSignal<Option<(String, String)>> = ArcRwSignal::new(None);
    /// A flush is out: another waits for it.
    static FLUSHING: Cell<bool> = const { Cell::new(false) };
}

/// The newest draft that landed, as a signal (key, body).
pub(crate) fn landed_signal() -> ArcRwSignal<Option<(String, String)>> {
    LANDED.with(Clone::clone)
}

/// A comment the watch (or a write) carried: if it is a kept draft
/// landing, the draft is let go and an open box there told.
pub(crate) fn landed(c: &Comment) {
    let k = key_of(c);
    let mut map = load_map();
    if map.get(&k).is_some_and(|d| is_landing(&k, &d.body, c)) {
        map.remove(&k);
        store_map(&map);
    }
    LANDED.with(|l| l.set(Some((k, c.body.clone()))));
}

/// `fut`, or `Err(timed_out())` after [`ANSWER_MS`].
async fn in_time<T>(fut: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    use futures::future::{select, Either};
    let fut = std::pin::pin!(fut);
    let timer = std::pin::pin!(dom::sleep_ms(ANSWER_MS));
    match select(fut, timer).await {
        Either::Left((r, _)) => r,
        Either::Right(_) => Err(timed_out()),
    }
}

/// `comments.add`, with an answer in time: the comment as kept (its draft
/// let go), or the reason — the draft kept and the failure reported.
pub(crate) async fn send(anchor: Anchor, body: String) -> Result<Comment, String> {
    let k = key(&anchor);
    kept(k, anchor.clone(), body.clone(), comments::add(anchor, body), None).await
}

/// COMMENT-8: [`send`] for a reply to `root` — kept by its thread, and
/// reported on the root's anchor, saying which comment it answered.
pub(crate) async fn send_reply(root: &Comment, body: String) -> Result<Comment, String> {
    let to = root.id.get();
    kept(reply_key(to), root.anchor.clone(), body.clone(), comments::reply(to, body), Some(to)).await
}

/// The draft at `k` kept while `write` goes out, let go when it answers in
/// time; else the failure reported (on `anchor`; a reply's, naming its root).
async fn kept(k: String, anchor: Anchor, body: String, write: impl std::future::Future<Output = Result<Comment, String>>, reply_to: Option<u64>) -> Result<Comment, String> {
    save(&k, &body);
    match in_time(write).await {
        Ok(c) => {
            save(&k, "");
            Ok(c)
        }
        Err(reason) => {
            let said = reply_to.map_or_else(|| reason.clone(), |to| format!("a reply to comment {to}: {reason}"));
            report(anchor, body, said);
            Err(reason)
        }
    }
}

/// A failure to keep a comment, into the outbox and on to the server.
pub(crate) fn report(anchor: Anchor, body: String, reason: String) {
    leptos::logging::warn!("COMMENT-9: a comment was not kept at {} — {reason}", key(&anchor));
    let out = push(read_outbox(storage().and_then(|s| s.get_item(OUTBOX).ok().flatten()).as_deref()), Failed { anchor, body, reason, failed_at: now() });
    store_outbox(&out);
    flush();
}

fn store_outbox(out: &[Failed]) {
    if let (Some(s), Ok(text)) = (storage(), serde_json::to_string(out)) {
        let _ = if out.is_empty() { s.remove_item(OUTBOX) } else { s.set_item(OUTBOX, &text) };
    }
}

/// The outbox, sent to `comments.report` — each taken out once the server
/// answers `reported`. Called on every connection, and after each failure.
pub(crate) fn flush() {
    if FLUSHING.with(|f| f.replace(true)) {
        return;
    }
    spawn_local(async move {
        loop {
            let out = read_outbox(storage().and_then(|s| s.get_item(OUTBOX).ok().flatten()).as_deref());
            let Some(first) = out.first().cloned() else { break };
            let Some(client) = session::comments_client() else { break };
            let agent = dom::window().navigator().user_agent().ok().and_then(|a| crate::typed::typed(&a).ok());
            let sent = async {
                let body = crate::typed::typed(&first.body)?;
                let reason = crate::typed::typed(&first.reason)?;
                client.report_all(first.anchor.clone(), body, reason, Some(first.failed_at), agent).await.map_err(|e| session::describe(&e))
            };
            match in_time(sent).await {
                Ok(events) if events.iter().any(|e| matches!(e, CommentsEvent::Reported { .. })) => {}
                Ok(events) => leptos::logging::warn!("COMMENT-9: the report was not taken: {events:?}"),
                Err(e) => {
                    leptos::logging::warn!("COMMENT-9: the report waits for the next connection: {e}");
                    break;
                }
            }
            // Taken (or refused for good): out of the outbox either way.
            let mut out = read_outbox(storage().and_then(|s| s.get_item(OUTBOX).ok().flatten()).as_deref());
            if out.first() == Some(&first) {
                out.remove(0);
            }
            store_outbox(&out);
        }
        FLUSHING.with(|f| f.set(false));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::tests::comment;

    fn conv(ticket: &str) -> Anchor {
        crate::conversation::anchor(ticket)
    }

    /// A draft is kept by where it is being left: the Conversation of one
    /// ticket, one element on one page.
    #[test]
    fn a_draft_is_kept_by_its_place() {
        assert_eq!(key(&conv("KILN-2")), "#/ticket/KILN-2|conversation");
        let mut el = conv("KILN-2");
        el.path = "#t-status".into();
        assert_ne!(key(&el), key(&conv("KILN-2")));
        assert_ne!(key(&conv("KILN-3")), key(&conv("KILN-2")));
    }

    /// Written, it is there; blank, it is gone; a week old, it is let go;
    /// what cannot be read is no drafts, never a panic.
    #[test]
    fn the_stored_map_keeps_writes_and_lets_go_of_blank_and_old() {
        let k = key(&conv("KILN-2"));
        let map = put(BTreeMap::new(), &k, "the kiln is cold", 100);
        let text = serde_json::to_string(&map).unwrap();
        assert_eq!(read_map(Some(&text), 200).get(&k).map(|d| d.body.as_str()), Some("the kiln is cold"));
        assert!(read_map(Some(&text), 100 + DRAFT_TTL).is_empty(), "past a week");
        assert!(put(map, &k, "  ", 300).is_empty(), "blank lets it go");
        assert!(read_map(Some("not json"), 0).is_empty());
        assert!(read_map(None, 0).is_empty());
    }

    /// The comment the watch carries is the draft landing when its place
    /// and its text are the draft's — not another place, not other text.
    #[test]
    fn a_comment_is_the_draft_landing_by_place_and_text() {
        let k = key(&conv("KILN-2"));
        let mut c = comment(7, "#/ticket/KILN-2", "conversation", Some("KILN-2"));
        c.body = "the kiln is cold\n".into();
        assert!(is_landing(&k, "the kiln is cold", &c));
        assert!(!is_landing(&k, "the kiln is hot", &c));
        assert!(!is_landing(&key(&conv("KILN-3")), "the kiln is cold", &c));
        assert!(!is_landing(&k, "", &c), "no draft, nothing lands");
        // COMMENT-8: a reply lands in its thread's box, not the place's.
        c.reply_to = Some(menger_tickets_api::vocab::CommentId::new(3));
        assert!(!is_landing(&k, "the kiln is cold", &c));
        assert_eq!(key_of(&c), reply_key(3));
        assert!(is_landing(&reply_key(3), "the kiln is cold", &c));
        assert_ne!(reply_key(3), reply_key(4));
    }

    /// The outbox keeps the newest failures in order, at most OUTBOX_MAX.
    #[test]
    fn the_outbox_keeps_the_newest_in_order() {
        let f = |n: i64| Failed { anchor: conv("KILN-2"), body: format!("b{n}"), reason: "socket".into(), failed_at: n };
        let mut out = Vec::new();
        for n in 0..(OUTBOX_MAX as i64 + 3) {
            out = push(out, f(n));
        }
        assert_eq!(out.len(), OUTBOX_MAX);
        assert_eq!(out.first().map(|f| f.failed_at), Some(3));
        assert_eq!(out.last().map(|f| f.failed_at), Some(OUTBOX_MAX as i64 + 2));
        let text = serde_json::to_string(&out).unwrap();
        assert_eq!(read_outbox(Some(&text)), out);
        assert!(read_outbox(Some("[{")).is_empty());
    }

    /// The box says the comment was not kept, why, and that the draft is
    /// still there.
    #[test]
    fn the_box_says_why_and_that_the_draft_stays() {
        assert_eq!(words("not connected — nothing was kept"), "Not kept: not connected — nothing was kept — your draft is still here.");
        assert_eq!(words(&timed_out()), "Not kept: no answer from the server in 15 s (the connection may be down) — your draft is still here.");
    }
}
