//! CHAT-18: a participant's **live work** in a chat, as the chat draws it —
//! the bridge's `chats.activity`: "builder is on it" the moment it has the
//! message, each step it takes under the last turn, until the answer; then
//! the steps fold into **show work (N steps)** after that answer.
//!
//! The app keeps a chat's activity items as heard (the watch) and fetched
//! (`activity_log`, when a chat opens mid-turn), each once by `seq`
//! ([`keep`]); what is drawn is worked out from them and the thread
//! ([`shape`]) — which turns are live, which are folded and after which
//! message — and a live one with no word for a minute is stale
//! ([`stale`]). All of it pure, pinned by native tests; src/chat_pane.rs
//! draws it.

use menger_tickets_api::chats::{Activity, ActivityState};
use menger_tickets_api::Message;

/// The most items the app keeps per chat — the server keeps 200.
pub(crate) const KEPT: usize = 400;

/// A live turn's steps in view before its box scrolls.
pub(crate) const VISIBLE: usize = 8;

/// Past this with no word, a live turn is stale: the worker may be gone.
pub(crate) const STALE_SECS: i64 = 60;

/// `a` in `items` by `seq`, once; the oldest go past [`KEPT`]. Whether it
/// was new.
pub(crate) fn keep(items: &mut Vec<Activity>, a: &Activity) -> bool {
    match items.binary_search_by_key(&a.seq, |x| x.seq) {
        Ok(_) => false,
        Err(at) => {
            items.insert(at, a.clone());
            if items.len() > KEPT {
                let over = items.len() - KEPT;
                items.drain(..over);
            }
            true
        }
    }
}

/// Whether `line` is the bridge's heartbeat (`working · 42s`) — said as
/// the turn's pulse, never a step.
pub(crate) fn is_heartbeat(line: &str) -> bool {
    line.strip_prefix("working · ").and_then(|r| r.strip_suffix('s')).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// One turn of one worker's live work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Work {
    pub(crate) by: String,
    /// When it began, and its last word (unix seconds, the server's).
    pub(crate) started: i64,
    pub(crate) last: i64,
    /// What it did, in order — the heartbeats left out.
    pub(crate) steps: Vec<String>,
    /// Its last heartbeat (`working · 42s`), while it beats.
    pub(crate) beat: Option<String>,
    /// When it ended: `ended`, or the answer that came after it began.
    pub(crate) ended: Option<i64>,
}

/// What a chat draws of its activity: the turns still live (under the
/// last turn), and the folded ones, each after the message that answered it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Shape {
    pub(crate) live: Vec<Work>,
    pub(crate) folds: Vec<(u64, Work)>,
}

impl Shape {
    /// The fold after message `mid`, when there is one.
    pub(crate) fn fold_after(&self, mid: u64) -> Option<&Work> {
        self.folds.iter().find(|(m, _)| *m == mid).map(|(_, w)| w)
    }
}

/// The turns `items` (oldest first, by `seq`) make, against `messages`
/// (the thread held): `started` begins one (an unended one before it is
/// over), a line adds a step (or, without a start heard, begins one),
/// `ended` ends it. A live turn that a message by its worker came after
/// (a later second than its start) is over too — the answer arrived. An
/// ended turn folds after the first message its worker posted between its
/// start and its end that no earlier turn took; one with no steps, or no
/// such message, draws nothing.
pub(crate) fn shape(items: &[Activity], messages: &[Message]) -> Shape {
    let mut open: Vec<Work> = Vec::new();
    let mut done: Vec<Work> = Vec::new();
    for a in items {
        let at = open.iter().position(|w| w.by == a.by);
        match a.state {
            ActivityState::Started => {
                if let Some(i) = at {
                    done.push(open.remove(i));
                }
                open.push(Work { by: a.by.clone(), started: a.at, last: a.at, steps: Vec::new(), beat: None, ended: None });
            }
            ActivityState::Working => {
                let i = at.unwrap_or_else(|| {
                    open.push(Work { by: a.by.clone(), started: a.at, last: a.at, steps: Vec::new(), beat: None, ended: None });
                    open.len() - 1
                });
                let w = &mut open[i];
                w.last = a.at;
                match a.line.as_deref() {
                    Some(l) if is_heartbeat(l) => w.beat = Some(l.to_string()),
                    Some(l) => w.steps.push(l.to_string()),
                    None => {}
                }
            }
            ActivityState::Ended => {
                if let Some(i) = at {
                    let mut w = open.remove(i);
                    w.ended = Some(a.at);
                    w.last = a.at;
                    done.push(w);
                }
            }
        }
    }
    // A live turn its worker answered (a later second than its start) folds
    // after that answer.
    let mut live = Vec::new();
    let mut answered: Vec<(u64, Work)> = Vec::new();
    for mut w in open {
        match messages.iter().filter(|m| m.author == w.by && m.at > w.started).min_by_key(|m| m.id.get()) {
            Some(m) => {
                w.ended = Some(m.at);
                answered.push((m.id.get(), w));
            }
            None => live.push(w),
        }
    }
    // Each ended turn, in the order begun, folds after its worker's first
    // message in its span that no turn before it took — two turns inside
    // one second never take the same answer.
    done.sort_by_key(|w| w.started);
    let mut folds: Vec<(u64, Work)> = Vec::new();
    let mut taken: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for w in done {
        let end = w.ended.unwrap_or(w.last);
        let Some(answer) = messages
            .iter()
            .filter(|m| m.author == w.by && m.id.get() > taken.get(&w.by).copied().unwrap_or(0) && m.at >= w.started && m.at <= end && answered.iter().all(|(a, _)| *a != m.id.get()))
            .min_by_key(|m| m.id.get())
        else {
            continue;
        };
        let id = answer.id.get();
        taken.insert(w.by.clone(), id);
        if !w.steps.is_empty() {
            folds.push((id, w));
        }
    }
    folds.extend(answered.into_iter().filter(|(_, w)| !w.steps.is_empty()));
    Shape { live, folds }
}

/// A live turn's staleness at `now` (unix seconds): `no word for N min`
/// past [`STALE_SECS`] since its last word, else none.
pub(crate) fn stale(w: &Work, now: i64) -> Option<String> {
    let quiet = now - w.last;
    (quiet >= STALE_SECS).then(|| format!("no word for {} min", quiet / 60))
}

/// How many of `n` steps are above the box's view — `+N earlier`.
pub(crate) fn earlier(n: usize) -> usize {
    n.saturating_sub(VISIBLE)
}

/// The fold's label.
pub(crate) fn fold_label(n: usize) -> String {
    format!("show work ({n} step{})", if n == 1 { "" } else { "s" })
}

/// A live turn's head: `builder is on it`.
pub(crate) fn on_it(name: &str) -> String {
    format!("{name} is on it")
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::vocab::{ChatId, MessageId};

    const B: &str = "key:b1";

    fn a(seq: u64, at: i64, state: ActivityState, line: Option<&str>) -> Activity {
        Activity { seq, chat: ChatId::new(4), by: B.into(), name: None, at, state, line: line.map(str::to_string) }
    }

    fn msg(id: u64, author: &str, at: i64) -> Message {
        Message { id: MessageId::new(id), chat: ChatId::new(4), author: author.into(), name: None, via: None, at, body: "x".into(), deleted: None }
    }

    use ActivityState::{Ended, Started, Working};

    #[test]
    fn items_are_kept_once_in_seq_order_and_bounded() {
        let mut v = Vec::new();
        assert!(keep(&mut v, &a(3, 1, Working, Some("c"))));
        assert!(keep(&mut v, &a(1, 1, Started, None)));
        assert!(!keep(&mut v, &a(3, 1, Working, Some("c"))), "the log and the watch: once");
        assert_eq!(v.iter().map(|x| x.seq).collect::<Vec<_>>(), [1, 3]);
        for s in 10..10 + KEPT as u64 {
            keep(&mut v, &a(s, 1, Working, None));
        }
        assert_eq!(v.len(), KEPT);
        assert_eq!(v[0].seq, 10, "the oldest go (1 and 3)");
    }

    #[test]
    fn a_live_turn_shows_its_steps_and_its_pulse() {
        let items = [a(1, 100, Started, None), a(2, 101, Working, Some("Read a.rs")), a(3, 116, Working, Some("working · 16s")), a(4, 117, Working, Some("Bash: cargo test"))];
        let s = shape(&items, &[msg(1, "idp:owner", 99)]);
        assert!(s.folds.is_empty());
        let w = &s.live[0];
        assert_eq!((w.started, w.last), (100, 117));
        assert_eq!(w.steps, ["Read a.rs", "Bash: cargo test"]);
        assert_eq!(w.beat.as_deref(), Some("working · 16s"));
        assert!(is_heartbeat("working · 3s") && !is_heartbeat("working · s") && !is_heartbeat("working on it"));
    }

    #[test]
    fn ended_folds_it_after_the_answer() {
        let items = [a(1, 100, Started, None), a(2, 101, Working, Some("Read a.rs")), a(3, 105, Ended, None)];
        let thread = [msg(1, "idp:owner", 99), msg(2, B, 105)];
        let s = shape(&items, &thread);
        assert!(s.live.is_empty());
        assert_eq!(s.folds.len(), 1);
        assert_eq!(s.folds[0].0, 2);
        assert_eq!(s.fold_after(2).unwrap().steps, ["Read a.rs"]);
        assert_eq!(fold_label(1), "show work (1 step)");
        assert_eq!(fold_label(3), "show work (3 steps)");
    }

    #[test]
    fn the_answer_arriving_folds_it_before_ended() {
        let items = [a(1, 100, Started, None), a(2, 101, Working, Some("Edit b.rs"))];
        // Its own earlier answer, in the second it started, is not this turn's.
        let s = shape(&items, &[msg(1, B, 100)]);
        assert_eq!(s.live.len(), 1);
        let s = shape(&items, &[msg(1, B, 100), msg(2, B, 104)]);
        assert!(s.live.is_empty());
        assert_eq!(s.folds[0].0, 2);
    }

    #[test]
    fn turns_follow_one_another_and_a_dangling_one_ends_at_the_next() {
        let items = [
            a(1, 100, Started, None),
            a(2, 101, Working, Some("one")),
            a(3, 103, Ended, None),
            a(4, 103, Started, None),
            a(5, 104, Working, Some("two")),
            a(6, 105, Working, Some("three")),
        ];
        let thread = [msg(1, "idp:o", 99), msg(2, B, 103), msg(3, "idp:o", 103)];
        let s = shape(&items, &thread);
        assert_eq!(s.folds.iter().map(|(m, w)| (*m, w.steps.clone())).collect::<Vec<_>>(), [(2, vec!["one".to_string()])]);
        assert_eq!(s.live[0].steps, ["two", "three"]);
        // A line with no start heard (the log's head cut) begins a turn.
        let s = shape(&items[4..], &[]);
        assert_eq!(s.live[0].started, 104);
        // A turn with no steps folds nothing.
        let s = shape(&[a(1, 100, Started, None), a(2, 101, Ended, None)], &[msg(1, B, 101)]);
        assert!(s.folds.is_empty() && s.live.is_empty());
    }

    #[test]
    fn two_turns_in_one_second_fold_after_their_own_answers() {
        // Turn one ends in the second turn two begins and answers.
        let items = [
            a(1, 450, Started, None),
            a(2, 451, Working, Some("one")),
            a(3, 452, Ended, None),
            a(4, 452, Started, None),
            a(5, 452, Working, Some("two")),
            a(6, 452, Ended, None),
        ];
        let thread = [msg(3, "idp:o", 450), msg(4, B, 452), msg(5, "idp:o", 452), msg(6, B, 452)];
        let s = shape(&items, &thread);
        assert_eq!(s.folds.iter().map(|(m, w)| (*m, w.steps.clone())).collect::<Vec<_>>(), [(4, vec!["one".to_string()]), (6, vec!["two".to_string()])]);
    }

    #[test]
    fn a_quiet_live_turn_goes_stale_after_a_minute() {
        let s = shape(&[a(1, 100, Started, None), a(2, 110, Working, Some("x"))], &[]);
        let w = &s.live[0];
        assert_eq!(stale(w, 169), None);
        assert_eq!(stale(w, 170).as_deref(), Some("no word for 1 min"));
        assert_eq!(stale(w, 290).as_deref(), Some("no word for 3 min"));
        assert_eq!(earlier(5), 0);
        assert_eq!(earlier(VISIBLE + 4), 4);
        assert_eq!(on_it("builder"), "builder is on it");
    }
}
