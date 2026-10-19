//! CHAT-44: **deleted messages** in a chat (owner, 2026-10-19: *"it should
//! collapse all deleted messages in sequence a single line which can open
//! them inside an inline box, clicking again should close. all animated"*).
//!
//! The server keeps a deleted message and marks it (`deleted { at, by }`);
//! the turns fold every run of consecutive deleted messages in the window
//! held into **one line** — `▸ 96 deleted messages · 14:02–14:20` — drawn
//! in the run's first row ([`Folds`]: its *head*; the rest of the run are
//! rows of no height, so the virtual list keeps one row per message).
//! Tapping the line opens an **inline box** under it with the run's
//! messages, dimmed, each its time and author (and *restore* for its author
//! or an admin); tapping again closes it. The box opens and closes by its
//! rows' height and opacity (`.tdbox`'s grid), a live delete collapses the
//! turn the same way (`.tkeep`, src/chat_pane.rs), and Motion none (or
//! reduced motion) makes both instant.
//!
//! Which rows fold, and the line's words, are pure and pinned by native
//! tests; [`Run`] is the DOM.

use leptos::prelude::*;
use menger_tickets_api::Message;
use std::collections::HashMap;
use wasm_bindgen_futures::spawn_local;

use crate::chats;

/// How long the collapse and the box's open take — `.tkeep` and `.tdbox`'s
/// transitions in index.html; what a live delete waits before it takes the
/// collapsed turn out.
pub(crate) const MOTION_MS: i32 = 260;

/// What a message's row draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Its turn.
    Kept,
    /// The first of a run of deleted messages: the run's line (and box).
    Head,
    /// Deleted, after the head of its run: nothing.
    Member,
}

/// The runs of deleted messages in a window, by their heads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Folds {
    role: HashMap<u64, Role>,
    runs: HashMap<u64, Vec<Message>>,
}

impl Folds {
    /// The runs of consecutive deleted messages in `list` (oldest first).
    pub(crate) fn of(list: &[Message]) -> Self {
        let mut f = Folds::default();
        let mut head: Option<u64> = None;
        for m in list {
            let id = m.id.get();
            match (m.deleted.is_some(), head) {
                (false, _) => head = None,
                (true, Some(h)) => {
                    f.role.insert(id, Role::Member);
                    f.runs.entry(h).or_default().push(m.clone());
                }
                (true, None) => {
                    head = Some(id);
                    f.role.insert(id, Role::Head);
                    f.runs.insert(id, vec![m.clone()]);
                }
            }
        }
        f
    }

    /// What the row of message `id` draws.
    pub(crate) fn role(&self, id: u64) -> Role {
        self.role.get(&id).copied().unwrap_or(Role::Kept)
    }

    /// The run `head` begins, oldest first.
    pub(crate) fn run(&self, head: u64) -> &[Message] {
        self.runs.get(&head).map_or(&[], Vec::as_slice)
    }
}

/// The line's words: how many, and the run's span of time (`clock` says a
/// moment) — one time when they are all one minute.
pub(crate) fn label(run: &[Message], clock: impl Fn(i64) -> String) -> String {
    let n = run.len();
    let what = format!("{n} deleted message{}", if n == 1 { "" } else { "s" });
    let (Some(first), Some(last)) = (run.first(), run.last()) else { return what };
    let (a, b) = (clock(first.at), clock(last.at));
    if a == b {
        format!("{what} · {a}")
    } else {
        format!("{what} · {a}–{b}")
    }
}

/// A deleted message's text as the box shows it: its first lines, an image
/// said as one.
pub(crate) fn excerpt(body: &str) -> String {
    let text: Vec<&str> = body.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).take(6).collect();
    let text = text.join("\n");
    if text.trim_start().starts_with("![") {
        "an image".into()
    } else {
        text
    }
}

/// The run of deleted messages `head` begins, in chat `chat`: its line, and
/// the box it opens. `fresh`: it appeared by a live delete — it slides in.
/// `may`: whether the reader may restore a message by this author.
#[component]
pub(crate) fn Run(head: u64, folds: Memo<Folds>, chat: u64, fresh: bool, may: Callback<String, bool>, clock: fn(i64) -> String) -> impl IntoView {
    let open = RwSignal::new(false);
    // Drawn the first time it opens, then kept, so it closes animated.
    let drawn = RwSignal::new(false);
    let run = Memo::new(move |_| folds.with(|f| f.run(head).to_vec()));
    let words = Memo::new(move |_| run.with(|r| label(r, clock)));
    let toggle = move |_| {
        drawn.set(true);
        open.update(|o| *o = !*o);
    };
    let err = RwSignal::new(None::<String>);
    let name = crate::chat_pane::names();
    let items = move || {
        let name = name.clone();
        drawn.get().then(|| {
            view! {
                <For each=move || run.get() key=|m| m.id.get() children=move |m: Message| {
                    let who = m.name.clone().unwrap_or_else(|| name(&m.author));
                    let by = m.deleted.as_ref().map(|d| d.by.clone()).unwrap_or_default();
                    let by = if by == m.author { String::new() } else { format!(" · deleted by {}", name(&by)) };
                    let mid = m.id.get();
                    let restore = may.run(m.author.clone()).then(|| view! {
                        <button type="button" class="tdrest" on:click=move |_| spawn_local(async move {
                            if let Err(e) = chats::delete(chat, &[mid], true).await {
                                err.try_set(Some(e));
                            }
                        })>"restore"</button>
                    });
                    view! {
                        <li class="tditem" data-message-id=mid.to_string()>
                            <div class="tdmeta"><span>{clock(m.at)}</span><b>{who}</b><span>{by}</span>{restore}</div>
                            <div class="tdbody">{excerpt(&m.body)}</div>
                        </li>
                    }
                } />
            }
        })
    };
    view! {
        <div class="tdel" class:open=move || open.get() class:fresh=fresh>
            <button type="button" class="tdline" aria-expanded=move || open.get().to_string() on:click=toggle>
                <span class="tdcaret" aria-hidden="true">"▸"</span>
                <span>{move || words.get()}</span>
            </button>
            <div class="tdbox" aria-hidden=move || (!open.get()).to_string() inert=move || !open.get()>
                <div class="tdclip">
                    <ol class="tdlist">{items}</ol>
                    {move || err.get().map(|e| view! { <p class="perr" role="alert">{e}</p> })}
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::chats::Deleted;

    fn msg(id: u64, gone: bool) -> Message {
        let mut m = crate::chats::tests::msg(id, 4, "key:b", "the report");
        m.deleted = gone.then(|| Deleted { at: 1, by: "key:b".into() });
        m
    }

    #[test]
    fn a_run_of_deleted_messages_is_one_head_and_its_members() {
        let list: Vec<Message> = [(1, false), (2, true), (3, true), (4, true), (5, false), (6, true), (7, false)].map(|(i, g)| msg(i, g)).into();
        let f = Folds::of(&list);
        assert_eq!([1, 2, 3, 4, 5, 6, 7].map(|i| f.role(i)), [Role::Kept, Role::Head, Role::Member, Role::Member, Role::Kept, Role::Head, Role::Kept]);
        assert_eq!(f.run(2).iter().map(|m| m.id.get()).collect::<Vec<_>>(), [2, 3, 4]);
        assert_eq!(f.run(6).len(), 1);
        assert!(f.run(3).is_empty(), "a member heads nothing");
        // A run of 96, as chat 6 holds.
        let many: Vec<Message> = (1..=97).map(|i| msg(i, i > 1)).collect();
        let f = Folds::of(&many);
        assert_eq!((f.role(2), f.run(2).len(), f.role(97)), (Role::Head, 96, Role::Member));
        assert_eq!(Folds::of(&[]), Folds::default());
    }

    #[test]
    fn the_line_says_how_many_and_the_span() {
        let clock = |at: i64| format!("14:{at:02}");
        let mut run: Vec<Message> = (1..=96).map(|i| msg(i, true)).collect();
        run[0].at = 2;
        run[95].at = 20;
        assert_eq!(label(&run, clock), "96 deleted messages · 14:02–14:20");
        assert_eq!(label(&run[..1], clock), "1 deleted message · 14:02");
        run[1].at = 2;
        assert_eq!(label(&run[..2], clock), "2 deleted messages · 14:02");
    }

    #[test]
    fn an_excerpt_is_the_first_lines() {
        assert_eq!(excerpt("one\n\n two \nthree"), "one\n two\nthree");
        assert_eq!(excerpt(&(1..=9).map(|i| i.to_string()).collect::<Vec<_>>().join("\n")), "1\n2\n3\n4\n5\n6");
        assert_eq!(excerpt("![shot](media:ab)"), "an image");
    }
}
