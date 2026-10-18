//! COMMENT-9: comments the app could not keep — reported by the app
//! (`comments.report`) and read by whoever can see where they were being
//! left (`comments.failures`). Owner, 2026-10-18: *"you can see the failed
//! submission?"* — a comment that never reached the server left no trace,
//! so the app says so once its socket is up again.
//!
//! Never a comment and never stored: the newest
//! [`FAILURES_KEPT`](crate::comments::FAILURES_KEPT) are held here, beside
//! the activation, and each is logged (`WARN`, without its body) — a
//! restart forgets the ring, the log keeps the line.

use crate::access::VerifiedUser;
use crate::comments::{Anchor, Comment, CommentIntent, CommentState, Failure, BODY_MAX, FAILURES_KEPT};
use crate::store::{Signer, Store};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The ring, shared by every connection's activation.
#[derive(Clone, Default)]
pub struct Failures(Arc<Mutex<(u64, VecDeque<Failure>)>>);

/// What a report says, as the app sent it.
pub struct Report {
    pub anchor: Anchor,
    pub body: String,
    pub reason: String,
    pub failed_at: Option<i64>,
    pub agent: Option<String>,
}

/// `body` cut to at most `max` bytes, on a character's edge.
fn cut(body: &str, max: usize) -> String {
    let mut end = body.len().min(max);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    body[..end].to_string()
}

impl Failures {
    /// `user`'s report, kept and logged.
    pub fn report(&self, user: &VerifiedUser, r: Report) -> Failure {
        let by = Signer::of(user);
        let now = chrono::Utc::now().timestamp();
        let mut ring = self.0.lock().expect("failures lock poisoned");
        ring.0 += 1;
        let failure = Failure {
            seq: ring.0,
            at: now,
            failed_at: r.failed_at.filter(|t| *t > 0 && *t <= now).unwrap_or(now),
            by: by.principal,
            name: by.name,
            anchor: r.anchor,
            body: cut(&r.body, BODY_MAX),
            reason: r.reason,
            agent: r.agent,
        };
        tracing::warn!(
            by = %failure.by, route = %failure.anchor.route, path = %failure.anchor.path,
            ticket = ?failure.anchor.ticket, bytes = failure.body.len(), reason = %failure.reason,
            agent = ?failure.agent, "COMMENT-9: a comment the app could not keep (comments failures)"
        );
        if ring.1.len() >= FAILURES_KEPT {
            ring.1.pop_front();
        }
        ring.1.push_back(failure.clone());
        failure
    }

    /// The reports `user` sees, newest first, at most `limit`: its own, and
    /// those whose comment it would see had it been kept.
    pub fn seen(&self, store: &Store, user: &VerifiedUser, limit: usize) -> Vec<Failure> {
        let me = user.principal_text();
        let ring = self.0.lock().expect("failures lock poisoned");
        ring.1.iter().rev().filter(|f| f.by == me || store.sees_comment(user, &as_comment(f))).take(limit).cloned().collect()
    }
}

/// A report as the comment it would have been — what decides who sees it.
fn as_comment(f: &Failure) -> Comment {
    Comment {
        id: crate::vocab::CommentId::new(0),
        state: CommentState::Open,
        author: f.name.clone().unwrap_or_else(|| f.by.clone()),
        at: f.failed_at,
        anchor: f.anchor.clone(),
        body: String::new(),
        principal: Some(f.by.clone()),
        intent: CommentIntent::Comment,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{Access, Held};
    use crate::address::{OrgId, ShortcodeId};
    use crate::members::Role;
    use crate::store::{NewTicket, SYS};
    use crate::types::Kind;

    fn viewer_of(principal: &str, code: &str) -> VerifiedUser {
        let held = vec![(Held::Shortcode { org: OrgId::new(OrgId::HYPERMEMETIC).unwrap(), id: ShortcodeId::new(code) }, Role::Viewer)];
        VerifiedUser::for_tests(&Access::for_tests(), principal.parse().unwrap(), held)
    }

    fn on(ticket: &str, body: &str) -> Report {
        let anchor = Anchor { route: format!("#/ticket/{ticket}"), path: "conversation".into(), label: "Conversation".into(), excerpt: String::new(), ticket: Some(ticket.into()), version: None };
        Report { anchor, body: body.into(), reason: "socket: closed".into(), failed_at: Some(1), agent: Some("iPhone".into()) }
    }

    /// A report is kept with its reporter, the draft and the reason; the
    /// reporter reads its own, a reader of the ticket reads it, a reader of
    /// another shortcode does not; newest first; the ring keeps the newest.
    #[tokio::test]
    async fn a_report_is_read_by_its_reporter_and_whoever_sees_its_ticket() {
        let store = Store::new();
        for code in ["A", "B"] {
            let t = NewTicket { epic: code.into(), title: code.into(), kind: Kind::Scope, severity: None, blocked_by: vec![], parent: None, path: None, body: String::new() };
            store.create(SYS, t).await.unwrap();
        }
        let ring = Failures::default();
        let admin = Access::for_tests().test_admin();
        let a = viewer_of("idp:aaaaaaaa-60aa-4534-b2e3-83634591fee1", "A");
        let b = viewer_of("idp:bbbbbbbb-60aa-4534-b2e3-83634591fee1", "B");
        let first = ring.report(&a, on("A-1", "the kiln is cold"));
        assert_eq!((first.seq, first.failed_at, first.body.as_str(), first.reason.as_str()), (1, 1, "the kiln is cold", "socket: closed"));
        assert_eq!(first.by, a.principal_text());
        ring.report(&b, on("B-1", "on B"));
        ring.report(&admin, on("A-1", "the admin's"));
        let bodies = |u: &VerifiedUser| ring.seen(&store, u, 50).into_iter().map(|f| f.body).collect::<Vec<_>>();
        assert_eq!(bodies(&admin), ["the admin's", "on B", "the kiln is cold"]);
        assert_eq!(bodies(&a), ["the admin's", "the kiln is cold"]);
        assert_eq!(bodies(&b), ["on B"]);
        assert_eq!(ring.seen(&store, &admin, 1).len(), 1, "at most limit");
        for _ in 0..FAILURES_KEPT {
            ring.report(&admin, on("A-1", "more"));
        }
        let kept = ring.seen(&store, &admin, FAILURES_KEPT + 10);
        assert_eq!(kept.len(), FAILURES_KEPT);
        assert!(kept.iter().all(|f| f.body == "more"), "the oldest let go");
    }

    #[test]
    fn a_body_is_cut_on_a_characters_edge() {
        assert_eq!(cut("abc", 10), "abc");
        assert_eq!(cut("abc", 2), "ab");
        assert_eq!(cut("aé", 2), "a", "é is two bytes: not split");
    }
}
