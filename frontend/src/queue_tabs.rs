//! CHAT-17: **the queue's sheet has two tabs** — *queue N* (MENGER-39's
//! sheet, unchanged) and *chats N* — so one place on the bottom edge (the
//! tray's control; at a phone's width the phone bar's last place) holds both
//! what is asking and who is talking (owner, 2026-10-18: *"a view … from the
//! alerts section that shows all active chats … a tab within the queue"*).
//!
//! - **chats N** is how many open chats hold unread for the reader; its
//!   badge is green (CHAT-10's). The tab lists every open chat the reader is
//!   in, on tickets and outside them, in `#/chats`' order and words
//!   ([`crate::chats_page::rows`]): unread first, then by last activity —
//!   its ticket's chip, *direct* or *group*, who or its title, its last line
//!   and its unread ([`lines`]).
//! - **A row tapped** opens it where it lives ([`crate::chats::href`]): a
//!   ticket's chat in that ticket's dock (`#/ticket/X?chat=ID`), else
//!   `#/chats/ID`; the sheet lowers.
//! - **The control shows** while something is queued or a chat has unread
//!   ([`shows`]); with nothing queued it says the chats' count and opens on
//!   Chats ([`opening`]). Nothing of either: no control, as before.
//! - **The tab last picked** is kept per browser ([`TAB_KEY`]).
//!
//! What decides is pure and pinned by native tests; the rest is the DOM, in
//! src/queue.rs's tray.

use std::cell::Cell;

use leptos::prelude::*;

use crate::chats::{self, Held};
use crate::chats_page;
use crate::dom;

/// The sheet's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    /// MENGER-39's queue.
    #[default]
    Queue,
    /// CHAT-17's open chats.
    Chats,
}

impl Tab {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Tab::Queue => "queue",
            Tab::Chats => "chats",
        }
    }

    /// A kept word as a tab; anything else is none.
    pub(crate) fn parse(s: &str) -> Option<Tab> {
        match s.trim() {
            "queue" => Some(Tab::Queue),
            "chats" => Some(Tab::Chats),
            _ => None,
        }
    }
}

/// Where the tab last picked is kept (localStorage).
pub(crate) const TAB_KEY: &str = "menger.queue.tab";

fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

/// The tab last picked in this browser (the queue's when none was).
pub(crate) fn kept() -> Tab {
    storage().and_then(|s| s.get_item(TAB_KEY).ok().flatten()).and_then(|v| Tab::parse(&v)).unwrap_or_default()
}

/// `t` picked: kept for the next time the sheet opens.
pub(crate) fn keep(t: Tab) {
    if let Some(s) = storage() {
        let _ = s.set_item(TAB_KEY, t.as_str());
    }
}

/// One row of the Chats tab: `#/chats`' row, and where a tap goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) row: chats_page::Row,
    pub(crate) href: String,
}

/// The Chats tab: every open chat the reader is in, in the inbox's order
/// (unread first, then by last activity), each with its place.
pub(crate) fn lines(held: &Held, me: &str, name: &impl Fn(&str) -> String) -> Vec<Line> {
    chats_page::rows(held, me, name)
        .into_iter()
        .zip(held.inbox())
        .filter(|(row, _)| !row.closed)
        .map(|(row, c)| Line { href: chats::href(&c), row })
        .collect()
}

/// **chats N**: how many open chats hold unread.
pub(crate) fn waiting(lines: &[Line]) -> usize {
    lines.iter().filter(|l| l.row.unread > 0).count()
}

/// Whether the control (and the phone bar's queue place) shows: something
/// queued, or a chat with unread.
pub(crate) fn shows(queued: usize, chats: usize) -> bool {
    queued > 0 || chats > 0
}

/// The tab the sheet opens on: Chats when nothing is queued, else the one
/// last picked.
pub(crate) fn opening(queued: usize, kept: Tab) -> Tab {
    if queued == 0 {
        Tab::Chats
    } else {
        kept
    }
}

/// The control's classes with nothing queued and `chats` unread — its
/// count, `chats`, green — `open` while the sheet is up.
pub(crate) fn control_class(open: bool) -> String {
    let mut c = "qc chats many".to_string();
    if open {
        c.push_str(" open");
    }
    c
}

/// What a screen reader hears of the chats: `2 chats unread`.
pub(crate) fn words(chats: usize) -> String {
    format!("{chats} chat{} unread", if chats == 1 { "" } else { "s" })
}

/// A row's classes: `un` while it holds unread, `sel` while `j` / `k` is on it.
pub(crate) fn row_class(l: &Line, sel: bool) -> String {
    let mut c = "qcr".to_string();
    for (on, word) in [(l.row.unread > 0, "un"), (sel, "sel")] {
        if on {
            c.push(' ');
            c.push_str(word);
        }
    }
    c
}

thread_local! {
    /// The names the server knows principals by, asked once.
    static NAMED: Cell<bool> = const { Cell::new(false) };
}

/// The Chats tab shown: the names the server knows (`candidates`), once, so
/// every row says who rather than a key.
pub(crate) fn learn_names() {
    if !NAMED.with(|n| n.replace(true)) {
        wasm_bindgen_futures::spawn_local(async {
            let _ = chats::candidates(None).await;
        });
    }
}

/// The Chats tab's list: `lines`, `sel` the row `j` / `k` is on; a tap
/// calls `opened` (the sheet lowers) and the link goes.
pub(crate) fn list_view(held: Signal<bool>, lines: Memo<Vec<Line>>, sel: RwSignal<Option<u64>>, opened: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    move || {
        if !held.get() {
            return view! { <p class="qempty">"loading chats…"</p> }.into_any();
        }
        let list = lines.get();
        if list.is_empty() {
            return view! { <p class="qempty">"No open chats."</p> }.into_any();
        }
        let rows = list
            .into_iter()
            .map(|l| {
                let id = l.row.id;
                let class = {
                    let l = l.clone();
                    move || row_class(&l, sel.get() == Some(id))
                };
                let mark = if l.row.ticket {
                    view! { <span class="tchip">{l.row.mark.clone()}</span> }.into_any()
                } else {
                    view! { <span class="kchip">{l.row.mark.clone()}</span> }.into_any()
                };
                view! {
                    <a class=class role="listitem" data-chat=id.to_string() href=l.href.clone() on:click=move |_| opened()>
                        <span class="qcm">{mark}<span class="qcl-t">{l.row.label.clone()}</span></span>
                        {(l.row.unread > 0).then(|| view! { <span class="count" aria-label=format!("{} unread", l.row.unread)>{l.row.unread}</span> })}
                        <small>{l.row.last.clone()}</small>
                    </a>
                }
            })
            .collect_view();
        view! { <div class="qcl" role="list" aria-label="open chats">{rows}</div> }.into_any()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chats::tests::{chat, msg};
    use menger_tickets_api::{ChatKind, ChatState, ChatsEvent};

    fn held() -> Held {
        let mut dm = chat(5, "", &["ada", "key:54a9"]);
        dm.last_at = 500;
        dm.unread = 1;
        let mut t = chat(6, "KILN-7", &["ada", "rui"]);
        t.last_at = 900;
        let mut g = chat(7, "", &["ada", "rui", "sam"]);
        g.kind = ChatKind::Group;
        g.title = Some("the kiln crew".into());
        g.unread = 3;
        g.last_at = 100;
        let mut quiet = chat(9, "KILN-4", &["ada", "sam"]);
        quiet.last_at = 950;
        let mut old = chat(8, "", &["ada", "sam"]);
        old.state = ChatState::Closed;
        old.last_at = 999;
        old.unread = 4;
        let mut h = Held::from_events([dm, t, g, quiet, old].map(|chat| ChatsEvent::Chat { chat }));
        h.learn([("key:54a9".to_string(), "builder".to_string())]);
        h.loaded(5, vec![msg(1, 5, "key:54a9", "On it.\nmore")]);
        h
    }

    /// The Chats tab: every open chat, unread first, then by last activity
    /// (newest first) — the closed left out — in `#/chats`' words, each to
    /// its place: a ticket's chat in its dock, else `#/chats/ID`.
    #[test]
    fn the_chats_tab_lists_the_open_chats_unread_first() {
        let h = held();
        let name = |p: &str| h.name_of(p).map_or_else(|| p.to_string(), str::to_string);
        let got = lines(&h, "ada", &name);
        let seen: Vec<(u64, &str, &str, u32)> = got.iter().map(|l| (l.row.id, l.row.mark.as_str(), l.row.label.as_str(), l.row.unread)).collect();
        // Unread: 5 (at 500) before 7 (at 100); then the read, 9 (950) before 6 (900); 8 is closed.
        assert_eq!(seen, [(5, "direct", "builder", 1), (7, "group", "the kiln crew", 3), (9, "KILN-4", "sam", 0), (6, "KILN-7", "rui", 0)]);
        assert_eq!(got[0].row.last, "builder: On it.");
        let hrefs: Vec<&str> = got.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, ["#/chats/5", "#/chats/7", "#/ticket/KILN-4?chat=9", "#/ticket/KILN-7?chat=6"]);
        assert_eq!(waiting(&got), 2, "the closed chat's unread is not counted");
        assert_eq!(row_class(&got[0], false), "qcr un");
        assert_eq!(row_class(&got[2], true), "qcr sel");
        assert_eq!(waiting(&[]), 0);
    }

    /// The control: shown for something queued, or for a chat with unread
    /// alone (then opening on Chats); neither, none. Otherwise the sheet
    /// opens on the tab last picked.
    #[test]
    fn the_control_shows_for_chats_alone_and_opens_on_them() {
        assert!(shows(2, 0) && shows(0, 1) && shows(3, 4));
        assert!(!shows(0, 0));
        assert_eq!(opening(0, Tab::Queue), Tab::Chats);
        assert_eq!(opening(0, Tab::Chats), Tab::Chats);
        assert_eq!(opening(2, Tab::Queue), Tab::Queue);
        assert_eq!(opening(2, Tab::Chats), Tab::Chats);
        assert_eq!(control_class(false), "qc chats many");
        assert_eq!(control_class(true), "qc chats many open");
        assert_eq!((words(1), words(2)), ("1 chat unread".to_string(), "2 chats unread".to_string()));
    }

    /// The tab kept per browser reads back as itself; a word it doesn't
    /// know is none (the queue's then).
    #[test]
    fn the_tab_kept_reads_back() {
        for t in [Tab::Queue, Tab::Chats] {
            assert_eq!(Tab::parse(t.as_str()), Some(t));
        }
        assert_eq!(Tab::parse("inbox"), None);
        assert_eq!(Tab::default(), Tab::Queue);
        assert_eq!(TAB_KEY, "menger.queue.tab");
    }
}
