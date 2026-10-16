//! AGS-18: the keyboard layer — the whole app without a mouse.
//!
//! Three pieces live here: the **global key map** (a single `window`
//! keydown listener: `/` and Ctrl/⌘+K open the jump palette, `?` the
//! cheat sheet, `Escape` closes an overlay, `g h` goes to the board, and
//! j/k/Enter move and open board rows), the **two overlays** (jump
//! palette and cheat sheet — real `role="dialog"` boxes that trap Tab and
//! hand focus back where they found it), and the small **roving-tabindex
//! helpers** the nav tree (src/nav.rs) shares with the board.
//!
//! Two rules shape the design:
//!
//! - **Focus is real focus.** Nothing here paints a pretend cursor: the
//!   focused row is the document's `activeElement`, carries the sole
//!   `tabindex="0"` of its widget (WAI-ARIA roving tabindex), and wears a
//!   token-painted ring that is visibly distinct from the *selected* row
//!   (which is the route's ticket, tinted with an inset accent bar).
//!   That is also why the tab stop is moved imperatively rather than
//!   through the reactive graph: which row has focus is a property of the
//!   document, not of the ticket store, and keeping it out of the signals
//!   means a focus move patches two attributes instead of re-running an
//!   attribute closure per row.
//! - **Keys never fire under someone else's cursor.** The global listener
//!   stands down inside an `<input>`/`<textarea>`/`<select>`/
//!   contenteditable, inside a `[role=dialog]` (the overlays own their
//!   keys) and inside a `[role=menu]` (AGS-16's ticket menu owns its
//!   own arrows/Enter/Escape).
//!
//! "Menu key / Shift+F10 opens the context menu on the focused ticket" —
//! AGS-18's one-line follow-up — landed with AGS-20 where it belongs, in
//! AGS-16's menu module (src/menu.rs); the focus this module owns is what
//! it opens on, and [`remember_focus`] / [`restore_focus`] are what hand
//! it back.
//!
//! MFE-12: the board mounts a window, so "the rows" are no longer the DOM.
//! j/k walk the row store's order — the board's order by construction —
//! and focus the next row where it is mounted; where it is not (`k` from
//! nowhere is the last ticket, ten thousand rows down), the board scrolls
//! it in and it takes focus once the window has drawn it
//! ([`focus_board_row`]). A focused row that the window leaves behind (a
//! wheel scroll, a scrollbar drag) takes the focus with it, so the board
//! remembers which row that was, and j/k resume there rather than at the
//! top. The palette was always the store's, not the DOM's.
//!
//! SCALE-25: **the palette asks the server.** It filtered every row the
//! client held on every keystroke — right only once the whole store had
//! been walked, and the last reader of "every row" besides the board. Now it
//! asks `tickets.find` (SCALE-6), the palette's own rule answered by the
//! server ([`palette_match`] stays here, unchanged, as the rule's statement
//! and the server's test oracle): the head of the store when it opens
//! (`find { query: "" }`), then each query once the typing pauses
//! ([`DEBOUNCE_MS`]), the 50-row cap as `find`'s `limit`, and the count as
//! the answer's rows plus its `remaining`. Every question is numbered and
//! only the answer to the newest is shown ([`Found`]), so an answer that
//! comes back late never replaces a newer one. While a question is out the
//! list shows what the last answer has that the new query still matches —
//! exactly the head of the new answer when the query only grew ([`narrows`]),
//! nothing otherwise — and Enter on an empty list waits for the answer and
//! opens its first row. With no connection it says so and shows no matches.
//! Its rows are its own: they are not put in the row cache, and the ticket
//! opened from one is fetched by its page, as any.
//!
//! SCALE-26: **the board's order is arithmetic, and its rows may be on their
//! way.** j/k walked the board's row list — every ticket the walk had found.
//! There is no such list now: the board holds a window (src/board.rs), and a
//! position is a ticket by `extents` alone. So a key steps a *position* — the
//! focused row's, by its id — and focuses the ticket at the next one, which
//! the window may not hold yet: the board scrolls it in, asks for it, and the
//! row takes focus when it is drawn ([`focus_board_row`], polled for up to
//! three seconds). Presses made meanwhile step on from that row, not from the
//! one still focused, so a held-down j crosses the window's edge without
//! losing a press. Home and End, once a board row has focus (or the window
//! left it behind), go to the board's first and last rows the same way —
//! before, they were the page's own scroll, which carried the focused row out
//! of the window and dropped the focus.
//!
//! FILTER-16: **the palette searches inside the global filter.** While it is
//! non-empty every question is `find { query, filter: global }`, and the
//! list ends in one more row, "Search everything": chosen (Enter, a click),
//! the palette asks again without the filter and marks each answer's rows
//! the filter does not admit `outside the filter` (`member { ids, filter }`
//! over the rows shown); the row then reads "Search inside the filter" and
//! turns it back. The rows, the 50-row budget, the keys and the numbering
//! of questions are SCALE-25's; with the global filter empty the palette is
//! exactly what it was ([`pass`]).
//!
//! FILTER-22: **the palette filters by a shortcode.** `filter <shortcode>`
//! ([`filter_command`]) offers, above the tickets, a row per shortcode it
//! names — the one typed, then each the groups know that it begins
//! ([`filter_offers`]); chosen, the global filter becomes that shortcode's
//! clause, the one a right-click on its tag's Filter makes (src/tags.rs),
//! and the palette closes where it was.
//!
//! FILTER-26: `promote filter` (or `make global`) offers, while the nav
//! holds a clause the reader added, the nav's Make global
//! (src/promote.rs).
//!
//! SEARCH-3: **the palette searches inside tickets.** Every question is
//! asked of `search` too (SEARCH-2), under the same number and the same pass:
//! below the jump rows, an "In bodies" section holds its best hits — the
//! ones no jump row already shows — each with its title and a passage of
//! its body, the matched words marked ([`Found::shown_hits`]). When the
//! server corrected a word, the heading says what it searched for
//! (`In bodies · placement`, [`Found::hits_heading`]); the count line adds
//! how many tickets match inside. Hits are rows like the others — ↑↓, Enter,
//! a click — and "Search everything" asks both without the filter and marks
//! the hits outside it as it marks the jump rows.
//!
//! AGS-26: **the nav's cursor is the open ticket** (src/nav.rs). On a ticket
//! page ↑/↓ (k/j) open the previous/next ticket in the nav from wherever focus
//! is (a text field, a dialog, a menu aside), and `f` filters the nav to the
//! open ticket and its descendants; in the nav l/h open and close. AGS-27: ←
//! and → are not the app's — they neither open and close nor move focus
//! between the nav and the page. The board's j/k are as they were.
//!
//! COMMENT-3: **`a` (or `A`) is annotation mode** (src/annotate.rs) — on, and
//! off again; Esc closes what the mode has open (a pin's list, a comment
//! box) and then leaves it. A body's frame that has focus posts the same
//! keys to the app (`menger:key`, src/frame.rs).
//!
//! MENGER-15: **`z` is full screen** for the ticket page's framed body
//! (src/present.rs) — on, and off again; from inside a focused body too (the
//! SDK and the page shim post it). Esc leaves it, after annotation mode's
//! own Esc.
//!
//! ACT-3: **the selection's keys** (src/selection.rs). On a focused board row
//! Shift+j/k (Shift+↓/↑) move a row and extend the selection to it, Space
//! toggles the row, Ctrl/Cmd+A selects every row drawn — the nav tree's own
//! (src/nav.rs). Esc, once nothing else it closes is open, clears the
//! selection.
//!
//! ACT-7: **the nav's shape keys** (src/nav.rs, src/shape.rs) — `*` expands
//! the focused row's subtree, Shift+`*` everything (the numpad's `*`: where
//! the layout puts `*` on Shift, Shift is how it is typed), `-` collapses
//! everything. None of them is the selection's.
//!
//! AGS-28: **no key is spelled here.** Every key and gesture above is a
//! binding in src/bindings.rs's table — an id, where it applies, its chords,
//! what it does — and the handlers here (the global map, the board's keys,
//! the overlays') match what [`bindings::hit`] answers by id. The `g …`
//! sequences and their timeout are the table's; ⌘K is `palette.toggle`
//! wherever the platform says ⌘. The cheat sheet draws the table
//! ([`KeyTable`]) — grouped, each row its chords as this platform spells
//! them, what it does and where — and so does Settings → Keyboard, with a
//! search over it.

use std::cell::{Cell, RefCell};

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context, Group};
use crate::filter::Cat;
use crate::{annotate, board, counts, dom, lens, navigate_to_ticket, present, selection, session, st_class, tags};
use menger_tickets_api::{Marked, SearchWord, TicketSummary};
use std::collections::{BTreeSet, HashSet};

/// How many matches the jump palette renders at once. The list is
/// rebuilt per keystroke, so this is a rendering budget, not a filter
/// limit anyone can observe on a store of menger's size. SCALE-25: and the
/// `limit` it asks `find` for.
const PALETTE_MAX: usize = 50;

/// SEARCH-3: how many hits in bodies the palette asks `search` for — below
/// the jump rows, a handful: the best of them.
const PALETTE_HITS: u32 = 8;

/// SCALE-25: how long the typing must pause before the palette asks `find`
/// — short enough to feel immediate, long enough that a word typed at speed
/// is one question, not one per letter (a non-empty `find` counts its
/// matches across the store: 52–155 ms at 10^6, SCALE-21).
pub(crate) const DEBOUNCE_MS: i32 = 80;

/// Which overlay is up. Exactly one at a time — the signal *is* the
/// mutual exclusion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Overlay {
    None,
    Palette,
    Cheats,
}

// ---------------------------------------------------------------------
// Matching (pure — the part that is worth a native test)

/// The jump palette's filter: a case-insensitive substring of the code
/// or the title (v1, as specified). Case folding is ASCII — ticket codes
/// are ASCII by construction and the Unicode case tables cost the wasm
/// ~7 KiB (AGS-15 removed the last one). An empty query matches
/// everything, so the palette opens on the whole store.
pub(crate) fn palette_match(code: &str, title: &str, query: &str) -> bool {
    let q = query.trim();
    q.is_empty() || contains_ignore_ascii_case(code, q) || contains_ignore_ascii_case(title, q)
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    // UTF-8 is self-synchronizing, so a byte-window scan cannot match
    // across a character boundary by accident.
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// SCALE-25: whether every ticket `query` matches is one `before` matched —
/// true when the query only grew (the trimmed `before` is inside the trimmed
/// `query`). Then the rows of `before`'s answer that `query` matches are
/// the head of `query`'s own answer, in order: a match of `query` that sorts
/// before the last row `before` answered with was a match of `before`, so it
/// is among those rows.
pub(crate) fn narrows(before: &str, query: &str) -> bool {
    // An empty `before` (the palette's opening question) matched everything;
    // the scan itself is never handed an empty needle.
    let before = before.trim();
    before.is_empty() || contains_ignore_ascii_case(query.trim(), before)
}

/// FILTER-22: the palette's `filter <shortcode>` — the shortcode, upper
/// case, when what is typed is the word `filter` (any case) and one
/// shortcode; `None` for anything else.
pub(crate) fn filter_command(query: &str) -> Option<String> {
    let mut words = query.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("filter") {
        return None;
    }
    let code = words.next()?;
    if words.next().is_some() {
        return None;
    }
    Cat::Shortcode.token(code)
}

/// MENGER-12: `filter <text>` where the text is a filter in the language
/// (`filter label needs-you`, `filter asking`, `filter ready and in MNG`) —
/// its canonical text, offered as one row: the global filter becomes it. A
/// bare shortcode is no filter text (the language reads it as nothing), so
/// `filter DEMAND` stays FILTER-22's.
pub(crate) fn filter_text_command(query: &str) -> Option<String> {
    let rest = query.trim_start();
    let (word, rest) = rest.split_at(rest.find(char::is_whitespace)?);
    if !word.eq_ignore_ascii_case("filter") {
        return None;
    }
    let f = menger_tickets_api::filter::Filter::parse(rest.trim()).ok()?;
    (!f.groups.is_empty()).then(|| f.format())
}

/// DEPLOY-13: the palette's `deployed …` — typed `depl…`, `deployed pr`,
/// `not depl…` or `waiting` (after `filter ` or not), the `deployed` cells
/// it may mean, each a filter's canonical text: `deployed <env>` for every
/// environment `choices` holds that begins with what follows, `not deployed
/// <env>` after `not`, and `waiting` the Complete work not yet on the last
/// one — at most [`FILTER_OFFERS`].
pub(crate) fn deployed_offers(query: &str, choices: &[String]) -> Vec<String> {
    let q = query.trim().to_ascii_lowercase();
    let q = q.strip_prefix("filter ").map_or(q.as_str(), str::trim_start);
    let words: Vec<&str> = q.split_whitespace().collect();
    let begins = |w: &str, of: &str, min: usize| w.len() >= min && of.starts_with(w);
    let canon = |text: String| menger_tickets_api::filter::Filter::parse(&text).ok().map(|f| f.format());
    let (not, rest) = match words.as_slice() {
        ["not", w, rest @ ..] if begins(w, "deployed", 3) => (true, rest),
        [w, rest @ ..] if begins(w, "deployed", 4) => (false, rest),
        [w] if begins(w, "waiting", 4) => {
            let last = choices.iter().filter(|c| !c.contains('/')).max_by_key(|c| (menger_tickets_api::deployments::rank(c), c.to_string()));
            return last.and_then(|l| canon(format!("status Complete and not deployed {l}"))).into_iter().collect();
        }
        _ => return Vec::new(),
    };
    let typed = match rest {
        [] => "",
        [one] => one,
        _ => return Vec::new(),
    };
    choices
        .iter()
        .filter(|c| c.starts_with(typed))
        .filter_map(|c| canon(format!("{}deployed {c}", if not { "not " } else { "" })))
        .take(FILTER_OFFERS)
        .collect()
}

/// CHAT-4: the palette's chats cells — typed `chat…` (`chats`, `filter
/// chatting`) or `unr…`, `not` before either: `chatting` (the reader is in
/// an open chat on the ticket) and `unread` (it has unread there), each a
/// filter's canonical text.
pub(crate) fn chat_offers(query: &str) -> Vec<String> {
    let q = query.trim().to_ascii_lowercase();
    let q = q.strip_prefix("filter ").map_or(q.as_str(), str::trim_start);
    let words: Vec<&str> = q.split_whitespace().collect();
    let (not, w) = match words.as_slice() {
        ["not", w] => (true, *w),
        [w] => (false, *w),
        _ => return Vec::new(),
    };
    let cells: &[&str] = if w.len() >= 4 && (w.starts_with("chat") || "chatting".starts_with(w)) {
        if "chatting".starts_with(w) && w.len() > 4 { &["chatting"] } else { &["chatting", "unread"] }
    } else if w.len() >= 3 && "unread".starts_with(w) {
        &["unread"]
    } else {
        return Vec::new();
    };
    cells
        .iter()
        .filter_map(|c| menger_tickets_api::filter::Filter::parse(&format!("{}{c}", if not { "not " } else { "" })).ok().map(|f| f.format()))
        .collect()
}

/// A command row the palette offers above the tickets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PalCmd {
    /// FILTER-22: the global filter becomes this shortcode's tree.
    Code(String),
    /// MENGER-12: the global filter becomes this text.
    Text(String),
    /// THEME-7: the reader's site theme becomes this one (`None`: the
    /// site's own again).
    Theme(Option<String>),
    /// FILTER-26: `promote filter` — the nav's filter made global.
    Promote,
}

/// THEME-7: the palette's `theme <name>` (or `theme: <name>`) — what is
/// typed after the word, trimmed and lower case (empty: every theme);
/// `None` for anything else.
pub(crate) fn theme_command(query: &str) -> Option<String> {
    let rest = query.trim_start();
    let word_end = rest.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(rest.len());
    if !rest[..word_end].eq_ignore_ascii_case("theme") {
        return None;
    }
    let typed = rest[word_end..].trim_start_matches(':').trim();
    (!typed.contains(char::is_whitespace)).then(|| typed.to_ascii_lowercase())
}

/// THEME-7: the themes `theme <typed>` offers, in `names`' order (`default`
/// first): each beginning with `typed`, then "the site's" (`None`) when
/// `typed` is empty or begins `site` — at most [`FILTER_OFFERS`].
pub(crate) fn theme_offers(typed: &str, names: &[String]) -> Vec<Option<String>> {
    names
        .iter()
        .filter(|n| n.starts_with(typed))
        .cloned()
        .map(Some)
        .chain(("site's".starts_with(typed) || typed.is_empty()).then_some(None))
        .take(FILTER_OFFERS)
        .collect()
}

/// How many shortcodes `filter <shortcode>` offers at most.
pub(crate) const FILTER_OFFERS: usize = 5;

/// FILTER-22: the shortcodes `filter <typed>` offers: `typed` when the
/// groups know it, then each they know that it begins, in order — at most
/// [`FILTER_OFFERS`]; while the groups are not known, `typed` alone.
pub(crate) fn filter_offers(typed: &str, known: Option<&BTreeSet<String>>) -> Vec<String> {
    let Some(known) = known else { return vec![typed.to_string()] };
    let exact = known.contains(typed).then(|| typed.to_string());
    exact
        .into_iter()
        .chain(known.iter().filter(|c| c.as_str() != typed && c.starts_with(typed)).cloned())
        .take(FILTER_OFFERS)
        .collect()
}

/// FILTER-16: what one palette question asks `find` with, and what its
/// rows are marked against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pass {
    /// `find`'s `filter` — the global filter's text; none with the global
    /// filter empty, or for "Search everything".
    pub(crate) filter: Option<String>,
    /// "Search everything" under a global filter: the filter its rows are
    /// asked of (`member`), to mark those outside it.
    pub(crate) mark: Option<String>,
}

/// FILTER-16: the palette's two passes — inside the global filter (`global`,
/// its wire text), or everything with the rows outside it marked; with no
/// global filter, SCALE-25's one pass.
pub(crate) fn pass(global: Option<&str>, everything: bool) -> Pass {
    match (global, everything) {
        (Some(g), false) => Pass { filter: Some(g.to_string()), mark: None },
        (Some(g), true) => Pass { filter: None, mark: Some(g.to_string()) },
        (None, _) => Pass { filter: None, mark: None },
    }
}

/// FILTER-16: the palette's count line — SCALE-25's words, and under a
/// global filter which pass they count.
pub(crate) fn count_words(f: &Found, under: bool) -> String {
    if f.offline {
        return "offline".to_string();
    }
    if f.pending() {
        return "… matches".to_string();
    }
    if f.refused.is_some() {
        return "the global filter was not taken".to_string();
    }
    let (shown, all) = (f.rows.len() as u64, f.total);
    let words = if all > shown { format!("{shown} of {all} matches") } else { format!("{all} matches") };
    let words = match (under, f.everything) {
        (false, _) => words,
        (true, false) => format!("{words} inside the global filter"),
        (true, true) => format!("{words} in everything · {} outside the filter", f.outside.len()),
    };
    // SEARCH-3: and how many match inside tickets, once that is the answer
    // to what is typed.
    match (f.hits_matched, f.hits_took == f.asked) {
        (0, _) | (_, false) => words,
        (n, true) => format!("{words} · {n} in bodies"),
    }
}

/// SCALE-25: what the palette shows — the newest answer it took, and
/// whether a newer question is out. Questions are numbered as they are
/// asked; an answer is taken only if it is to the newest, so one that comes
/// back after a newer question was asked is dropped, whatever order the
/// server answers in.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Found {
    /// The query the rows answer — `None` before any answer.
    pub(crate) query: Option<String>,
    pub(crate) rows: Vec<TicketSummary>,
    /// The answer's rows plus its `remaining`: every match.
    pub(crate) total: u64,
    /// The newest question's number, and the shown answer's.
    pub(crate) asked: u32,
    pub(crate) took: u32,
    /// The newest question found no connection: nothing is shown as current.
    pub(crate) offline: bool,
    /// FILTER-16: the shown answer searched everything ("Search
    /// everything"), and so does the newest question.
    pub(crate) everything: bool,
    pub(crate) wants_everything: bool,
    /// FILTER-16: the shown answer's rows the global filter does not admit.
    pub(crate) outside: HashSet<String>,
    /// FILTER-16: the server did not take the newest question's filter.
    pub(crate) refused: Option<String>,
    /// SEARCH-3: the newest `search` answer taken — its hits, as the server
    /// ranked them; how many tickets match; the words as it read them; the
    /// question and query it answers, and its pass; the hits outside the
    /// global filter ("Search everything").
    pub(crate) hits: Vec<session::Hit>,
    pub(crate) hits_matched: u64,
    pub(crate) words: Vec<SearchWord>,
    pub(crate) hits_took: u32,
    pub(crate) hits_query: Option<String>,
    pub(crate) hits_everything: bool,
    pub(crate) hits_outside: HashSet<String>,
}

impl Found {
    /// A new question; its number.
    pub(crate) fn ask(&mut self) -> u32 {
        self.asked = self.asked.wrapping_add(1);
        self.asked
    }

    /// FILTER-16: a new question of the other pass (`everything`); its
    /// number.
    pub(crate) fn ask_pass(&mut self, everything: bool) -> u32 {
        self.wants_everything = everything;
        self.ask()
    }

    /// Whether an answer to the newest question is still to come.
    pub(crate) fn pending(&self) -> bool {
        self.took != self.asked && !self.offline
    }

    /// The answer to question `n`: taken if `n` is still the newest.
    pub(crate) fn take(&mut self, n: u32, query: String, rows: Vec<TicketSummary>, remaining: u64) -> bool {
        self.take_marked(n, query, rows, remaining, HashSet::new())
    }

    /// FILTER-16: [`Found::take`], with the rows outside the global filter.
    pub(crate) fn take_marked(&mut self, n: u32, query: String, rows: Vec<TicketSummary>, remaining: u64, outside: HashSet<String>) -> bool {
        if n != self.asked {
            return false;
        }
        self.total = rows.len() as u64 + remaining;
        self.query = Some(query);
        self.rows = rows;
        self.took = n;
        self.offline = false;
        self.everything = self.wants_everything;
        self.outside = outside;
        self.refused = None;
        true
    }

    /// FILTER-16: question `n`'s filter was refused — its words, no rows.
    pub(crate) fn refuse(&mut self, n: u32, query: String, why: String) -> bool {
        if !self.take(n, query, Vec::new(), 0) {
            return false;
        }
        self.refused = Some(why);
        true
    }

    /// Question `n` found no connection.
    pub(crate) fn lost(&mut self, n: u32) {
        if n == self.asked {
            self.offline = true;
        }
    }

    /// SEARCH-3: the `search` answer to question `n` — taken if `n` is still
    /// the newest, as [`Found::take`] takes `find`'s.
    pub(crate) fn take_hits(
        &mut self,
        n: u32,
        query: String,
        hits: Vec<session::Hit>,
        matched: u64,
        words: Vec<SearchWord>,
        outside: HashSet<String>,
    ) -> bool {
        if n != self.asked {
            return false;
        }
        self.hits = hits;
        self.hits_matched = matched;
        self.words = words;
        self.hits_took = n;
        self.hits_query = Some(query);
        self.hits_everything = self.wants_everything;
        self.hits_outside = outside;
        true
    }

    /// SEARCH-3: the hits to show for what is typed now, below `rows` (the
    /// jump rows shown): the answer's, once it is the newest question's;
    /// while one is out, the last answer's if the query only grew from its
    /// query, in the same pass (fuzzy hits cannot be narrowed here, so they
    /// stand until the answer comes); none for a blank query or offline —
    /// and never a ticket a jump row shows already.
    pub(crate) fn shown_hits(&self, typed: &str, rows: &[TicketSummary]) -> Vec<session::Hit> {
        if self.offline || typed.trim().is_empty() {
            return Vec::new();
        }
        let current = self.hits_took == self.asked;
        let grew = self.hits_query.as_deref().is_some_and(|q| narrows(q, typed)) && self.hits_everything == self.wants_everything;
        if !current && !grew {
            return Vec::new();
        }
        let jump: HashSet<&str> = rows.iter().map(|t| t.id.as_str()).collect();
        self.hits.iter().filter(|h| !jump.contains(h.summary.id.as_str())).cloned().collect()
    }

    /// SEARCH-3: the section's heading — and, when the server corrected a
    /// word, what it searched for: each word as its nearest correction.
    pub(crate) fn hits_heading(&self) -> String {
        if self.words.iter().all(|w| w.also.is_empty()) {
            return "In bodies".to_string();
        }
        let read: Vec<&str> = self.words.iter().map(|w| w.also.first().unwrap_or(&w.word).as_str()).collect();
        format!("In bodies · {}", read.join(" "))
    }

    /// The rows to show for what is typed now: the answer, once it is the
    /// newest question's; while one is out, the rows of the last answer the
    /// typed query still matches if it only grew from that answer's query
    /// (the head of the coming answer), else none; offline, none.
    pub(crate) fn shown(&self, typed: &str) -> Vec<TicketSummary> {
        if self.offline {
            return Vec::new();
        }
        if !self.pending() {
            return self.rows.clone();
        }
        match &self.query {
            // FILTER-16: of the same pass only — the other's rows are not
            // the head of what is coming.
            Some(q) if narrows(q, typed) && self.everything == self.wants_everything => self
                .rows
                .iter()
                .filter(|t| palette_match(&t.id, &t.title, typed))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------
// Focus plumbing shared with the nav tree and the board

/// Every element matching `selector`, in document order.
pub(crate) fn query_all(selector: &str) -> Vec<web_sys::HtmlElement> {
    let Some(doc) = dom::window().document() else {
        return Vec::new();
    };
    let Ok(list) = doc.query_selector_all(selector) else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|i| list.get(i).and_then(|n| n.dyn_into().ok()))
        .collect()
}

/// Rendered (not inside a collapsed group, not behind `#app.nonav`).
pub(crate) fn is_visible(el: &web_sys::HtmlElement) -> bool {
    el.offset_parent().is_some()
}

/// Focus a row and bring it into view — `nearest`, so a row that is
/// already on screen never jumps.
pub(crate) fn focus_row(el: &web_sys::HtmlElement) {
    let _ = el.focus();
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
    el.scroll_into_view_with_scroll_into_view_options(&opts);
}

/// Roving tabindex: `el` becomes the widget's single tab stop (all of
/// `all` are the widget's items), then takes focus.
pub(crate) fn focus_and_rove(all: &[web_sys::HtmlElement], el: &web_sys::HtmlElement) {
    rove(all, el);
    focus_row(el);
}

/// The tab stop only — no focus move (used to seed a freshly rendered
/// widget so Tab can reach it).
pub(crate) fn rove(all: &[web_sys::HtmlElement], el: &web_sys::HtmlElement) {
    for other in all {
        let stop = if other.is_same_node(Some(el)) { "0" } else { "-1" };
        let _ = other.set_attribute("tabindex", stop);
    }
}

/// Give a widget a tab stop if it has none *visible* — the selected row
/// when it is on screen, else the first row. Called a tick after the
/// rows change, so Tab always lands somewhere sensible, and a tab stop
/// that scrolled out of existence (its group collapsed, its row gone) is
/// replaced rather than stranded.
pub(crate) fn ensure_tab_stop(item_selector: &str, preferred: Option<&str>) {
    let all = query_all(item_selector);
    let visible: Vec<web_sys::HtmlElement> = all.iter().filter(|el| is_visible(el)).cloned().collect();
    let Some(first) = visible.first() else { return };
    if visible
        .iter()
        .any(|el| el.get_attribute("tabindex").as_deref() == Some("0"))
    {
        return;
    }
    let pick = preferred
        .map(query_all)
        .and_then(|els| els.into_iter().find(is_visible))
        .unwrap_or_else(|| first.clone());
    rove(&all, &pick);
}

// ---------------------------------------------------------------------
// Overlay state: opening remembers where focus was, closing hands it back

thread_local! {
    /// MENGER-6: the overlay signal, for the chord a frame forwards.
    static PALETTE_OVERLAY: Cell<Option<RwSignal<Overlay>>> = const { Cell::new(None) };
    /// Where focus was when the overlay opened.
    static FOCUS_BEFORE_OVERLAY: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
    /// MFE-12: the board row that last had focus, until focus lands
    /// anywhere else — what j/k resume from when the window has unmounted
    /// that row (and focus fell to `<body>` with it).
    static BOARD_FOCUS: RefCell<Option<String>> = const { RefCell::new(None) };

    /// SCALE-26: the board row a step is on its way to (not drawn yet), and
    /// when the step was taken — where the next press steps from.
    static BOARD_TARGET: RefCell<Option<(String, f64)>> = const { RefCell::new(None) };

    /// SCALE-26: the focused board row's element, and the one observer that
    /// gives it focus back when the window's `<For>` moves it ([`keep_board_focus`]).
    static BOARD_FOCUS_EL: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
    static BOARD_OBSERVER: RefCell<Option<web_sys::MutationObserver>> = const { RefCell::new(None) };
    /// SCALE-26: the observer is handing focus back — not a new focus, so a
    /// step on its way stays on its way.
    static REFOCUS: Cell<bool> = const { Cell::new(false) };
    /// FILTER-12: moves with every step the keys take on the board — a wait
    /// for a row a filtered board has not fetched yet ends at the next.
    static BOARD_STEPS: Cell<u64> = const { Cell::new(0) };
}

/// How long a step waits for its row to be drawn: the scroll's frame, then
/// the window's page (a few ms at 10^6, more over a slow link).
const BOARD_STEP_WAIT_MS: f64 = 3000.0;

/// MFE-12: a board row took focus (the board's `focusin`). SCALE-26: a step
/// on its way ends there — it landed, or a click went somewhere else — and
/// the row's element is what [`keep_board_focus`] gives focus back to.
pub(crate) fn note_board_focus(id: String, row: Option<web_sys::HtmlElement>) {
    if !REFOCUS.with(Cell::get) {
        BOARD_TARGET.with(|t| *t.borrow_mut() = None);
    }
    if row.is_some() {
        BOARD_FOCUS_EL.with(|f| *f.borrow_mut() = row);
    }
    BOARD_FOCUS.with(|c| *c.borrow_mut() = Some(id));
}

/// SCALE-26: focus stays on the board's row while its window moves — the
/// nav's `keep_focus` (SCALE-24), for the board. When the window grows at its
/// top (a step up across its edge, a scroll back) the keyed `<For>` puts the
/// rows that stay back behind the new ones, and an element moved in the DOM
/// loses focus: the next j/k would start from nowhere. So the board's
/// `<tbody>` is watched, and a focused row that a mutation batch moved while
/// focus fell to `<body>` gets it back, without scrolling, in the observer's
/// microtask — before the next keystroke. A row blurred any other way (a
/// click elsewhere, `blur()`) is not moved, and stays blurred. One observer
/// for the app's lifetime, pointed at each `<tbody>` the board mounts.
pub(crate) fn keep_board_focus(tbody: &web_sys::Element) {
    let obs = BOARD_OBSERVER.with(|o| {
        o.borrow_mut()
            .get_or_insert_with(|| {
                let cb = Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new(
                    |records: js_sys::Array, _: web_sys::MutationObserver| {
                        let Some(el) = BOARD_FOCUS_EL.with(|f| f.borrow().clone()) else { return };
                        let Some(doc) = dom::window().document() else { return };
                        let resting = doc.active_element().is_none_or(|a| a.tag_name() == "BODY");
                        if !resting || !el.is_connected() {
                            return;
                        }
                        // Taken out and put back — in one batch, or in two
                        // (the second only adds it).
                        let moved = records.iter().any(|r| {
                            let Ok(r) = r.dyn_into::<web_sys::MutationRecord>() else { return false };
                            [r.removed_nodes(), r.added_nodes()]
                                .iter()
                                .any(|nodes| (0..nodes.length()).filter_map(|i| nodes.get(i)).any(|n| n.is_same_node(Some(&el))))
                        });
                        if moved {
                            let opts = web_sys::FocusOptions::new();
                            opts.set_prevent_scroll(true);
                            REFOCUS.with(|r| r.set(true));
                            let _ = el.focus_with_options(&opts);
                            REFOCUS.with(|r| r.set(false));
                        }
                    },
                );
                let obs = web_sys::MutationObserver::new(cb.as_ref().unchecked_ref()).expect("MutationObserver");
                cb.forget(); // lives as long as the app
                obs
            })
            .clone()
    });
    let init = web_sys::MutationObserverInit::new();
    init.set_child_list(true);
    let _ = obs.observe_with_options(tbody, &init);
}

/// SCALE-26: the row a step is still on its way to, if one is.
fn board_target() -> Option<String> {
    let now = js_sys::Date::now();
    BOARD_TARGET.with(|t| t.borrow().as_ref().filter(|(_, at)| now - at < BOARD_STEP_WAIT_MS).map(|(id, _)| id.clone()))
}

/// Remember where focus is, so [`restore_focus`] can hand it back when
/// whatever took it goes away. Shared by the overlays and (AGS-20) the
/// keyboard-opened ticket menu: the two are mutually exclusive by
/// construction — the global map stands down inside `[role=menu]`, and
/// the menu's own opener stands down inside `[role=dialog]` — so one slot
/// is enough.
pub(crate) fn remember_focus() {
    let el = dom::window()
        .document()
        .and_then(|d| d.active_element())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    FOCUS_BEFORE_OVERLAY.with(|c| *c.borrow_mut() = el);
}

/// WAVE-12: let go of what [`remember_focus`] kept — the board's filter
/// closed by a click, which put focus where it wanted it.
pub(crate) fn forget_focus() {
    FOCUS_BEFORE_OVERLAY.with(|c| *c.borrow_mut() = None);
}

/// Hand focus back, a tick later — the dialog (or menu) has to leave the
/// DOM first, or the browser's own "focus fell out of a removed subtree"
/// fixup lands on `<body>` after us. A no-op when nothing was remembered.
pub(crate) fn restore_focus() {
    let el = FOCUS_BEFORE_OVERLAY.with(|c| c.borrow_mut().take());
    spawn_local(async move {
        dom::sleep_ms(0).await;
        let Some(el) = el else { return };
        if el.is_connected() {
            focus_row(&el);
        } else if let Some(id) = el
            .get_attribute("data-ticket-id")
            .filter(|_| el.tag_name() == "TR")
        {
            // MFE-12: a board row the window unmounted while the overlay
            // (or the menu) was up. The same ticket's row, if the window
            // has it again; otherwise j/k resume from it.
            note_board_focus(id.clone(), None);
            if let Some(row) = mounted_board_row(&query_all("#board tr[data-ticket-id]"), &id) {
                focus_row(row);
            }
        }
    });
}

pub(crate) fn open_overlay(overlay: RwSignal<Overlay>, which: Overlay) {
    if overlay.get_untracked() == Overlay::None {
        remember_focus();
    }
    overlay.set(which);
}

pub(crate) fn close_overlay(overlay: RwSignal<Overlay>) {
    if overlay.get_untracked() == Overlay::None {
        return;
    }
    overlay.set(Overlay::None);
    restore_focus();
}

// ---------------------------------------------------------------------
// The global listener

/// One `window` keydown listener for the app's lifetime — the same
/// shape as the router's `hashchange`. MFE-12: it holds the row store,
/// which is what the board's j/k walk now. SCALE-26: the board's model —
/// positions by `extents`, not a list of rows.
pub(crate) fn install(overlay: RwSignal<Overlay>, board: board::Board) {
    // MENGER-6: ⌘K (Ctrl+K off a Mac) is the palette's from anywhere — a
    // text field, an open dialog — so it is taken in the capture phase,
    // before any field or dialog sees it, and the browser's own chord (its
    // search box) never runs. A framed body forwards it ([`palette_from_frame`]).
    PALETTE_OVERLAY.with(|o| o.set(Some(overlay)));
    let chord = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        if bindings::is(&ev, Context::Anywhere, "palette.toggle") {
            ev.prevent_default();
            toggle_palette(overlay);
        }
    });
    dom::window()
        .add_event_listener_with_callback_and_bool("keydown", chord.as_ref().unchecked_ref(), true)
        .expect("keydown chord listener");
    chord.forget(); // lives as long as the app

    let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        global_keydown(&ev, overlay, board);
    });
    dom::window()
        .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
        .expect("keydown listener");
    cb.forget(); // lives as long as the app

    // MFE-12: focus landing anywhere but on a board row ends the board's
    // claim to it (`BOARD_FOCUS`); a row of the board renews it itself.
    let on_focus = Closure::<dyn FnMut(web_sys::Event)>::new(|ev: web_sys::Event| {
        let on_board = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("#board tr[data-ticket-id]").ok().flatten())
            .is_some();
        if !on_board {
            BOARD_FOCUS.with(|c| *c.borrow_mut() = None);
            BOARD_TARGET.with(|t| *t.borrow_mut() = None);
            BOARD_FOCUS_EL.with(|f| *f.borrow_mut() = None);
        }
    });
    if let Some(doc) = dom::window().document() {
        let _ = doc.add_event_listener_with_callback("focusin", on_focus.as_ref().unchecked_ref());
    }
    on_focus.forget(); // lives as long as the app
}

/// The palette, opened — or closed, when it is what is open.
fn toggle_palette(overlay: RwSignal<Overlay>) {
    if overlay.get_untracked() == Overlay::Palette {
        close_overlay(overlay);
    } else {
        open_overlay(overlay, Overlay::Palette);
    }
}

/// MENGER-6: the chord pressed inside a ticket body's frame, which the
/// frame's SDK posts as `menger:key { key: "palette" }`.
pub(crate) fn palette_from_frame() {
    if let Some(overlay) = PALETTE_OVERLAY.with(|o| o.get()) {
        toggle_palette(overlay);
    }
}

/// COMMENT-3: no overlay (the palette, the cheat sheet) is up — a key a
/// frame posts is the app's then.
pub(crate) fn no_overlay() -> bool {
    dom::window().document().and_then(|d| d.query_selector(".overlay").ok().flatten()).is_none()
}

/// True where a keystroke belongs to whatever already has focus: a text
/// field, a `<select>`, a dialog (the overlays run their own key maps),
/// a menu (AGS-16), or a slider — APP-33's mockup handle owns its arrows.
pub(crate) fn defer_to_focus(el: &web_sys::Element) -> bool {
    matches!(
        el.tag_name().as_str(),
        "INPUT" | "TEXTAREA" | "SELECT" | "OPTION"
    ) || el
        .closest("[role=dialog], [role=menu], [role=slider], [contenteditable='true']")
        .ok()
        .flatten()
        .is_some()
}

fn global_keydown(ev: &web_sys::KeyboardEvent, overlay: RwSignal<Overlay>, board: board::Board) {
    if ev.default_prevented() || ev.is_composing() {
        return;
    }
    let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());

    // Escape reaches here only when focus escaped the dialog (a click on
    // the scrim, a removed row); the dialog's own handler is the normal
    // path. Cheap safety net, so Escape always closes an overlay.
    if overlay.get_untracked() != Overlay::None && bindings::is(ev, Context::Global, "app.escape") {
        ev.prevent_default();
        close_overlay(overlay);
        return;
    }
    if let Some(el) = &target {
        if defer_to_focus(el) {
            return;
        }
    }
    if overlay.get_untracked() != Overlay::None {
        return;
    }
    let used = match bindings::hit(ev, Context::Global) {
        // COMMENT-3: Esc closes what annotation mode has open, then leaves
        // it; MENGER-15: then full screen (the overlay; the browser's own
        // full screen takes its Esc itself); ACT-3: then the selection.
        Some("app.escape") => annotate::escape() || present::escape() || selection::clear(),
        // The first key of a `g …` sequence: swallowed, the table armed.
        Some(bindings::PREFIX) => true,
        Some("go.board") => {
            let _ = dom::window().location().set_hash("/");
            true
        }
        // MENGER-43: `g n` — the nav under Needs you (again: its default).
        Some("go.needs") => {
            crate::needs::go();
            true
        }
        // Prevented: Firefox's quick-find would eat the `/`.
        Some("palette.open") => {
            open_overlay(overlay, Overlay::Palette);
            true
        }
        Some("help.keys") => {
            open_overlay(overlay, Overlay::Cheats);
            true
        }
        // COMMENT-3: annotation mode, on and off.
        Some("annotate.toggle") => {
            annotate::toggle();
            true
        }
        _ => false,
    };
    if used {
        ev.prevent_default();
        return;
    }
    // MENGER-15: the ticket page's framed body, full screen and back.
    if bindings::is(ev, Context::TicketPage, "view.full-screen") {
        if present::toggle_page() {
            ev.prevent_default();
        }
        return;
    }
    // LOOK-3: the section dock — `}` `{` its sections, `i` its next input.
    for (id, k) in [("dock.next", crate::sections::Key::Next), ("dock.prev", crate::sections::Key::Prev), ("dock.input", crate::sections::Key::Input)] {
        if bindings::is(ev, Context::TicketPage, id) {
            if crate::sections::key(k) {
                ev.prevent_default();
            }
            return;
        }
    }
    // APP-44: `c` with focus in a mockup's box — that mockup in or out of
    // the page's comparison (the box is a region of its own: not the
    // Conversation's `c`).
    if let Some(key) = target.as_ref().and_then(crate::compare::box_key) {
        if bindings::is(ev, Context::MockBox, "compare.toggle") && crate::compare::toggle_key(&key) {
            ev.prevent_default();
        }
        return;
    }
    // APP-22: `c` — the ticket page's Conversation box.
    if bindings::is(ev, Context::TicketPage, "comment.focus") {
        if crate::conversation::focus_box() {
            ev.prevent_default();
        }
        return;
    }
    if board_key(ev, target.as_ref(), board) {
        ev.prevent_default();
    }
}

/// ACT-3: the board's selection keys, on its focused row — the nav's
/// (src/nav.rs): Shift+j/k and Shift+↑/↓ move focus a row and extend the
/// selection to it, Space toggles the row, Ctrl/Cmd+A selects every row the
/// board draws. True when `id` was one of them and a row has focus.
fn board_select_key(id: &str, target: Option<&web_sys::Element>, board: board::Board) -> bool {
    let Some(row) = target.and_then(|t| t.closest("#board tr[data-ticket-id]").ok().flatten()) else {
        return false;
    };
    let Some(focused) = row.get_attribute("data-ticket-id") else { return false };
    let down = match id {
        "select.all" => {
            let drawn = query_all("#board tr[data-ticket-id]").into_iter().filter_map(|r| r.get_attribute("data-ticket-id"));
            selection::set(selection::Picked::all(&selection::drawn_order(drawn)));
            return true;
        }
        "select.toggle" => {
            selection::set(selection::now().toggled(&focused));
            return true;
        }
        "select.extend-down" => true,
        "select.extend-up" => false,
        _ => return false,
    };
    let Some(at) = board.pos_of_untracked(&focused) else { return true };
    let last = board.n_untracked().saturating_sub(1);
    let next = if down { (at + 1).min(last) } else { at.saturating_sub(1) };
    if let Some(to) = board.id_at(next) {
        selection::board_extend(board, &focused, &to);
    }
    focus_board_row(&query_all("#board tr[data-ticket-id]"), next, board);
    true
}

/// The mounted board row for ticket `id`, if the window holds it.
fn mounted_board_row<'a>(mounted: &'a [web_sys::HtmlElement], id: &str) -> Option<&'a web_sys::HtmlElement> {
    mounted
        .iter()
        .find(|r| r.get_attribute("data-ticket-id").as_deref() == Some(id))
}

/// Board rows: j/k (and, once a row has focus, ↑/↓) move; Enter opens.
/// Returns true when the key was used. MFE-12: the order walked is the
/// row store's — the board's order by construction, so nothing is
/// mirrored into a second structure — not the DOM's, which holds only the
/// window; the row a key lands on may not be mounted yet, and
/// [`focus_board_row`] brings it in. SCALE-26: the order is the extents'
/// (a row's position is its id's), the row may not be fetched yet either, a
/// step still on its way is where the next one starts, and Home/End are the
/// board's ends once a row has focus.
fn board_key(ev: &web_sys::KeyboardEvent, target: Option<&web_sys::Element>, board: board::Board) -> bool {
    let Some(id) = bindings::hit(ev, Context::Board) else { return false };
    if id.starts_with("select.") {
        return board_select_key(id, target, board);
    }
    // A letter with no row focused starts at the near end; an arrow with
    // no row focused is the page's own scroll, left alone.
    let letter = bindings::typed(ev);
    let down = id == "move.next";
    // SCALE-26: whether the board is on screen is its `<tbody>`, not its
    // mounted rows — a window run ahead of its pages (k held down past what
    // has been fetched) mounts none for a moment, and the presses made then
    // are steps like any other.
    if dom::window().document().and_then(|d| d.get_element_by_id("board")).is_none() {
        return false; // no board on screen
    }
    let mounted = query_all("#board tr[data-ticket-id]");
    let focused = target
        .and_then(|t| t.closest("#board tr[data-ticket-id]").ok().flatten())
        .and_then(|el| el.get_attribute("data-ticket-id"));
    // Focus on nothing (`<body>`) because the window unmounted the
    // focused row: the letters carry on from that row. A row still
    // mounted that lost focus to a click elsewhere is the plain "no row
    // focused" case, as before.
    let resting = target.is_none_or(|t| matches!(t.tag_name().as_str(), "BODY" | "HTML"));
    let parked = BOARD_FOCUS
        .with(|c| c.borrow().clone())
        .filter(|id| resting && mounted_board_row(&mounted, id).is_none());
    // A step on its way (a click into a row, or focus anywhere else, ends it).
    let pending = board_target();
    let n = board.n_untracked();
    let at = |id: &Option<String>| id.as_deref().and_then(|id| board.pos_of_untracked(id));
    let cur = at(&pending).or_else(|| at(&focused));
    let resume = cur.or_else(|| at(&parked));
    if n == 0 {
        return false;
    }
    // FILTER-12: under a filter the board knows where the rows it holds sit;
    // a parked row it gave back (scrolled more than its margins away) is
    // placed by the server — a deep link, `list { filter, after, limit: 0 }`
    // — and the step taken from there.
    if resume.is_none() && board.filtered_untracked() && letter && matches!(id, "move.next" | "move.prev") {
        if let Some(id) = parked {
            spawn_local(async move {
                let next = match board.locate(&id).await {
                    Some(p) if down => (p + 1).min(board.n_untracked().saturating_sub(1)),
                    Some(p) => p.saturating_sub(1),
                    None if down => 0,
                    None => board.n_untracked().saturating_sub(1),
                };
                focus_board_row(&query_all("#board tr[data-ticket-id]"), next, board);
            });
            return true;
        }
    }
    let last = n - 1;
    let next = match id {
        "move.next" if letter => Some(resume.map_or(0, |i| (i + 1).min(last))),
        "move.prev" if letter => Some(resume.map_or(last, |i| i.saturating_sub(1))),
        "move.next" => cur.map(|i| (i + 1).min(last)),
        "move.prev" => cur.map(|i| i.saturating_sub(1)),
        // SCALE-26: the board's ends, once a row has focus — with none, the
        // page's own Home/End, as before.
        "move.first" => resume.map(|_| 0),
        "move.last" => resume.map(|_| last),
        "open.focused" => {
            let Some(id) = focused else { return false };
            navigate_to_ticket(&id);
            return true;
        }
        _ => None,
    };
    match next {
        Some(i) => {
            focus_board_row(&mounted, i, board);
            true
        }
        None => false,
    }
}

/// Focus store row `index` on the board (MFE-12): at once where the window
/// holds it; otherwise scroll it in and focus it when the window has drawn
/// it — polled a frame at a time, because a scroll's event lands on the
/// next frame and the window moves on that event. SCALE-26: and the row may
/// still have to be fetched, so the wait is longer, the row it waits for is
/// remembered (the next press steps on from it), and a newer step ends it.
fn focus_board_row(mounted: &[web_sys::HtmlElement], index: usize, board: board::Board) {
    BOARD_STEPS.with(|s| s.set(s.get().wrapping_add(1)));
    let Some(id) = board.id_at(index) else {
        // FILTER-12: under a filter, which match sits at a position the
        // board does not hold only the server knows — scroll there, and step
        // onto it once the window has it.
        if board.filtered_untracked() {
            reach_board_row(index, board);
        }
        return;
    };
    if let Some(el) = mounted_board_row(mounted, &id) {
        BOARD_TARGET.with(|t| *t.borrow_mut() = None);
        focus_board_el(mounted, el, index, board);
        return;
    }
    let since = js_sys::Date::now();
    BOARD_TARGET.with(|t| *t.borrow_mut() = Some((id.clone(), since)));
    crate::scroll_board_to(index, board.n_untracked(), board.pitch());
    spawn_local(async move {
        while js_sys::Date::now() - since < BOARD_STEP_WAIT_MS {
            dom::sleep_ms(16).await;
            let mine = BOARD_TARGET.with(|t| t.borrow().as_ref().is_some_and(|(want, at)| *want == id && *at == since));
            if !mine {
                return; // a newer step, or a focus elsewhere
            }
            let all = query_all("#board tr[data-ticket-id]");
            if let Some(el) = mounted_board_row(&all, &id) {
                BOARD_TARGET.with(|t| *t.borrow_mut() = None);
                focus_board_el(&all, el, index, board);
                return;
            }
        }
        BOARD_TARGET.with(|t| {
            let mut t = t.borrow_mut();
            if t.as_ref().is_some_and(|(want, at)| *want == id && *at == since) {
                *t = None;
            }
        });
    });
}

/// FILTER-12: step onto store row `index` of a filtered board that does not
/// hold it yet — scrolled in, then focused once its page has come; a newer
/// step ends the wait.
fn reach_board_row(index: usize, board: board::Board) {
    let step = BOARD_STEPS.with(Cell::get);
    let since = js_sys::Date::now();
    crate::scroll_board_to(index, board.n_untracked(), board.pitch());
    spawn_local(async move {
        while js_sys::Date::now() - since < BOARD_STEP_WAIT_MS {
            dom::sleep_ms(16).await;
            if BOARD_STEPS.with(Cell::get) != step {
                return;
            }
            let Some(id) = board.id_at(index) else { continue };
            let all = query_all("#board tr[data-ticket-id]");
            if let Some(el) = mounted_board_row(&all, &id) {
                focus_board_el(&all, el, index, board);
                return;
            }
        }
    });
}

/// SCALE-26: a board row takes focus without the browser's own scroll into
/// view, which assumes the rows stay where they are when it scrolls — and on
/// a board laid out above 15 M px they do not (the scroll range is mapped
/// onto the store, MFE-12: a scroll of one pitch moves the rows by ~2.5 at
/// 10^6), so the row it scrolled to was somewhere else, often unmounted, and
/// focus fell to `<body>` (j stopped dead 25 rows down at 10^6, before this
/// build too). The board scrolls itself to the row by its own mapping
/// (`scroll_board_to`, the least that shows it — a no-op when it is on
/// screen), and the row is focused where it is.
fn focus_board_el(all: &[web_sys::HtmlElement], el: &web_sys::HtmlElement, index: usize, board: board::Board) {
    rove(all, el);
    crate::scroll_board_to(index, board.n_untracked(), board.pitch());
    let opts = web_sys::FocusOptions::new();
    opts.set_prevent_scroll(true);
    let _ = el.focus_with_options(&opts);
}

/// Seed the board's tab stop after a render (see [`ensure_tab_stop`]).
/// The board has no selected row: the way in is the row focus last left,
/// while the window holds it, else the first row on screen. MFE-12: not
/// the window's first row — that one sits a margin above the viewport, and
/// Tab would scroll the board out from under the reader to reach it.
pub(crate) fn ensure_board_tab_stop() {
    let all = query_all("#board tr[data-ticket-id]");
    if all.is_empty()
        || all
            .iter()
            .any(|el| el.get_attribute("tabindex").as_deref() == Some("0"))
    {
        return;
    }
    let remembered = BOARD_FOCUS.with(|c| c.borrow().clone());
    let on_screen = || {
        let top = all[0]
            .closest(".content")
            .ok()
            .flatten()
            .map_or(0.0, |s| s.get_bounding_client_rect().top());
        all.iter().find(|el| el.get_bounding_client_rect().bottom() > top + 1.0)
    };
    let pick = remembered
        .and_then(|id| mounted_board_row(&all, &id))
        .or_else(on_screen)
        .unwrap_or(&all[0]);
    rove(&all, pick);
}

// ---------------------------------------------------------------------
// The overlays

/// Keep Tab inside the open dialog (both overlays are modal). Focus
/// cycles through the dialog's own focusable elements and never reaches
/// the page behind it.
pub(crate) fn trap_tab(back: bool) {
    let Some(doc) = dom::window().document() else { return };
    let Ok(Some(dialog)) = doc.query_selector("[role=dialog]") else {
        return;
    };
    let Ok(list) = dialog.query_selector_all(
        "a[href], button:not([disabled]), input:not([disabled]), select, textarea, [tabindex]:not([tabindex='-1'])",
    ) else {
        return;
    };
    let items: Vec<web_sys::HtmlElement> = (0..list.length())
        .filter_map(|i| list.get(i).and_then(|n| n.dyn_into().ok()))
        .collect();
    if items.is_empty() {
        return;
    }
    let active = doc.active_element();
    let cur = active
        .and_then(|a| items.iter().position(|el| el.is_same_node(Some(&a))))
        .unwrap_or(0);
    let n = items.len();
    let next = if back { (cur + n - 1) % n } else { (cur + 1) % n };
    let _ = items[next].focus();
}

/// Whichever overlay is up, mounted once beside the app's columns.
#[component]
pub(crate) fn Overlays(overlay: RwSignal<Overlay>) -> impl IntoView {
    view! {
        {move || match overlay.get() {
            Overlay::None => ().into_any(),
            Overlay::Palette => view! { <Palette overlay /> }.into_any(),
            Overlay::Cheats => view! { <Cheats overlay /> }.into_any(),
        }}
    }
}

/// SCALE-25: ask `find` question `n` (`query`), and take the answer if it is
/// still the newest; with no connection, say so. `then` runs when an answer
/// is taken (Enter, pressed while it was on its way).
///
/// FILTER-16: under the global filter, `find { query, filter }` — or, for
/// "Search everything", `find { query }` and then `member` over its rows, to
/// mark those outside the filter ([`pass`]).
fn ask_find(found: RwSignal<Found>, n: u32, query: String, pass: Pass, then: impl FnOnce() + 'static) {
    spawn_local(async move {
        if session::client().is_none() {
            found.try_update(|f| f.lost(n));
            return;
        }
        match session::find_under(query.clone(), PALETTE_MAX as u32, pass.filter).await {
            Ok(Ok((rows, remaining))) => {
                let mut outside = HashSet::new();
                if let (Some(mark), false) = (pass.mark, rows.is_empty()) {
                    match session::member(rows.iter().map(|t| t.id.to_string()).collect(), mark).await {
                        Ok(answer) => outside.extend(answer.into_iter().filter(|(_, m)| !m).map(|(id, _)| id)),
                        Err(why) => leptos::logging::log!("palette: member: {why}"),
                    }
                }
                if found.try_update(|f| f.take_marked(n, query, rows, remaining, outside)) == Some(true) {
                    then();
                }
            }
            // The server did not take the filter: its words.
            Ok(Err(why)) => {
                found.try_update(|f| f.refuse(n, query, why));
            }
            Err(_) => {
                found.try_update(|f| f.lost(n));
            }
        }
    });
}

/// SEARCH-3: question `n` asked of both — `find`, the jump rows, and
/// `search`, the hits in bodies — in the same pass.
fn ask(found: RwSignal<Found>, n: u32, query: String, pass: Pass, then: impl FnOnce() + 'static) {
    ask_search(found, n, query.clone(), pass.clone());
    ask_find(found, n, query, pass, then);
}

/// SEARCH-3: ask `search` question `n` (`query`), and take the answer if it
/// is still the newest. A blank query asks nothing: it has no hits. A filter
/// the server did not take, or a call that failed, is no hits — `find`'s
/// answer says why. "Search everything" marks the hits outside the filter
/// (`member`), as [`ask_find`] marks its rows.
fn ask_search(found: RwSignal<Found>, n: u32, query: String, pass: Pass) {
    if query.trim().is_empty() {
        found.try_update(|f| f.take_hits(n, query, Vec::new(), 0, Vec::new(), HashSet::new()));
        return;
    }
    spawn_local(async move {
        if session::client().is_none() {
            return;
        }
        let (hits, matched, words) = match session::search_under(query.clone(), PALETTE_HITS, pass.filter).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) | Err(_) => (Vec::new(), 0, Vec::new()),
        };
        let mut outside = HashSet::new();
        if let (Some(mark), false) = (pass.mark, hits.is_empty()) {
            match session::member(hits.iter().map(|h| h.summary.id.to_string()).collect(), mark).await {
                Ok(answer) => outside.extend(answer.into_iter().filter(|(_, m)| !m).map(|(id, _)| id)),
                Err(why) => leptos::logging::log!("palette: member: {why}"),
            }
        }
        found.try_update(|f| f.take_hits(n, query, hits, matched, words, outside));
    });
}

/// SEARCH-3: marked stretches as text, the matched words in `<mark>`.
fn marked(runs: Vec<Marked>) -> impl IntoView {
    runs.into_iter()
        .map(|r| if r.hit { view! { <mark>{r.text}</mark> }.into_any() } else { view! { {r.text} }.into_any() })
        .collect_view()
}

/// The jump palette: an input over a listbox of matches. The input keeps
/// focus (ARIA combobox pattern — the highlighted row is named by
/// `aria-activedescendant`, never focused), so typing and moving are the
/// same gesture.
#[component]
fn Palette(overlay: RwSignal<Overlay>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let hi = RwSignal::new(0usize);
    // FILTER-16: the global filter (read as the palette opens: it is modal),
    // and which pass is asked — inside it, or everything.
    let lens = lens::use_lens();
    let global = lens.filter(lens::Level::Global).with_untracked(|g| (!g.groups.is_empty()).then(|| counts::wire_text(g)));
    let under = global.is_some();
    let global = StoredValue::new(global);
    let everything = RwSignal::new(false);
    let pass_now = move || pass(global.try_with_value(|g| g.clone()).flatten().as_deref(), everything.try_get_untracked().unwrap_or(false));
    // SCALE-25: the server's answers, numbered; what is shown is derived
    // from the newest one and what is typed now.
    let found = RwSignal::new(Found::default());
    let results = Memo::new(move |_| {
        let q = query.get();
        found.with(|f| f.shown(&q))
    });
    // SEARCH-3: the hits in bodies, below the jump rows — none a jump row
    // already shows.
    let hits = Memo::new(move |_| {
        let q = query.get();
        results.with(|rows| found.with(|f| f.shown_hits(&q, rows)))
    });
    // FILTER-22: `filter <shortcode>` — the shortcodes it offers, above the
    // tickets.
    let sctree = lens.sctree();
    // DEPLOY-13: the environments, for `deployed …`.
    crate::deploys::want();
    let deploy_choices = crate::deploys::held();
    let offers = Memo::new(move |_| {
        let q = query.get();
        let typed = filter_command(&q);
        let mut out: Vec<PalCmd> = typed
            .map(|t| sctree.with(|tree| filter_offers(&t, tree.as_ref().map(|t| t.known()))))
            .unwrap_or_default()
            .into_iter()
            .map(PalCmd::Code)
            .collect();
        // MENGER-12: and the text, when it is a filter.
        out.extend(filter_text_command(&q).map(PalCmd::Text));
        // DEPLOY-13: `deployed …` — the environments the app holds.
        let choices = deploy_choices.with(|h| h.as_ref().map(crate::deploys::Held::choices).unwrap_or_default());
        for text in deployed_offers(&q, &choices) {
            if !out.contains(&PalCmd::Text(text.clone())) {
                out.push(PalCmd::Text(text));
            }
        }
        // CHAT-4: `chat…` / `unread` — the reader's chats' cells.
        for text in chat_offers(&q) {
            if !out.contains(&PalCmd::Text(text.clone())) {
                out.push(PalCmd::Text(text));
            }
        }
        // THEME-7: `theme <name>` — the themes the app holds.
        if let Some(typed) = theme_command(&q) {
            let names = crate::scopes::held().with(crate::scopes::Held::names);
            out.extend(theme_offers(&typed, &names).into_iter().map(PalCmd::Theme));
        }
        // FILTER-26: `promote filter`, while the nav has a clause to move.
        if crate::promote::promote_command(&q) && lens.can_promote() {
            out.push(PalCmd::Promote);
        }
        out
    });
    // Enter pressed while the answer was on its way, with nothing to open.
    let enter_waits = StoredValue::new(false);
    // FILTER-16: the last row under a global filter — the other pass, asked
    // now with what is typed.
    let widen = move || {
        let e = !everything.get_untracked();
        everything.set(e);
        hi.set(0);
        let n = found.try_update(|f| f.ask_pass(e)).unwrap_or(0);
        ask(found, n, query.get_untracked(), pass_now(), || {});
    };
    // Row `i` of every row: the shortcodes offered, the tickets, then —
    // under a global filter — the other pass.
    let open_at = move |i: usize| {
        let cmds = offers.with_untracked(Vec::len);
        if let Some(cmd) = offers.with_untracked(|o| o.get(i).cloned()) {
            close_overlay(overlay);
            match cmd {
                PalCmd::Code(code) => tags::filter_globally(lens, &code),
                PalCmd::Text(text) => {
                    if let Ok(f) = menger_tickets_api::filter::Filter::parse(&text) {
                        lens.apply(lens::Level::Global, Some(crate::saved::chosen(f)));
                    }
                }
                PalCmd::Theme(name) => crate::scopes::pick(name),
                PalCmd::Promote => crate::promote::make_global(lens),
            }
            return;
        }
        let i = i - cmds;
        let len = results.with_untracked(Vec::len);
        // SEARCH-3: the jump rows, then the hits in bodies.
        let row = results.with_untracked(|r| r.get(i).map(|t| t.id.clone())).or_else(|| {
            let j = i.checked_sub(len)?;
            hits.with_untracked(|h| h.get(j).map(|h| h.summary.id.clone()))
        });
        if let Some(id) = row {
            // DESIGN-39: the jump moves the id from the palette's row.
            crate::motion::note_origin(&id);
            close_overlay(overlay);
            navigate_to_ticket(&id);
        } else if under && i == len + hits.with_untracked(Vec::len) {
            widen();
        }
    };
    let answered = move || {
        if enter_waits.try_get_value() == Some(true) {
            enter_waits.set_value(false);
            let cmds = offers.with_untracked(Vec::len);
            let at = hi.get_untracked().saturating_sub(cmds);
            let len = results.with_untracked(Vec::len);
            if len > 0 {
                open_at(cmds + at.min(len - 1));
            }
        }
    };
    // Opening is the head of the store, asked at once.
    let first = found.try_update(Found::ask).unwrap_or(0);
    ask(found, first, String::new(), pass_now(), answered);
    // The connection: lost, the palette says so and shows nothing as
    // current; back, it asks the newest question again.
    let link = session::link();
    Effect::new(move |was: Option<bool>| {
        let up = link.get();
        if !up {
            found.update(|f| {
                let n = f.ask();
                f.lost(n);
            });
        } else if was == Some(false) {
            let n = found.try_update(Found::ask).unwrap_or(0);
            ask(found, n, query.get_untracked(), pass_now(), answered);
        }
        up
    });

    let input_ref = NodeRef::<leptos::html::Input>::new();
    input_ref.on_load(|el: web_sys::HtmlInputElement| {
        let _ = el.focus();
    });

    let on_key = move |ev: web_sys::KeyboardEvent| {
        // SEARCH-3: the jump rows and the hits in bodies, as one list.
        let len = results.with_untracked(Vec::len) + hits.with_untracked(Vec::len);
        // FILTER-22: the shortcodes offered come first.
        let cmds = offers.with_untracked(Vec::len);
        // FILTER-16: under a global filter the last row ("Search everything")
        // is a row like the others.
        let items = cmds + len + usize::from(under);
        let cur = hi.get_untracked().min(items.saturating_sub(1));
        match bindings::hit(&ev, Context::Palette) {
            Some("dialog.close") => {
                ev.prevent_default();
                close_overlay(overlay);
            }
            Some("list.choose") => {
                ev.prevent_default();
                if cmds == 0 && len == 0 && found.with_untracked(Found::pending) {
                    // SCALE-25: the answer is on its way — open its first.
                    enter_waits.set_value(true);
                } else if items > 0 {
                    open_at(cur);
                }
            }
            Some("list.next") => {
                ev.prevent_default();
                if items > 0 {
                    hi.set((cur + 1) % items);
                }
            }
            Some("list.prev") => {
                ev.prevent_default();
                if items > 0 {
                    hi.set((cur + items - 1) % items);
                }
            }
            Some("list.first") => {
                ev.prevent_default();
                hi.set(0);
            }
            Some("list.last") => {
                ev.prevent_default();
                hi.set(items.saturating_sub(1));
            }
            Some(id @ ("dialog.next-field" | "dialog.prev-field")) => {
                ev.prevent_default();
                trap_tab(id == "dialog.prev-field");
            }
            _ => {}
        }
        ev.stop_propagation();
    };

    view! {
        <div class="overlay" on:click=move |_| close_overlay(overlay)>
            <div
                id="palette"
                class="dialog"
                role="dialog"
                aria-modal="true"
                aria-label="Jump to a ticket"
                on:click=|ev| ev.stop_propagation()
                on:keydown=on_key
            >
                <input
                    id="palette-input"
                    node_ref=input_ref
                    type="text"
                    role="combobox"
                    aria-expanded="true"
                    aria-controls="palette-list"
                    aria-autocomplete="list"
                    aria-label="Jump to a ticket"
                    aria-activedescendant=move || format!("pal-{}", hi.get())
                    autocomplete="off"
                    spellcheck="false"
                    placeholder="jump to a ticket — code, title, or words inside"
                    prop:value=move || query.get()
                    on:input=move |ev| {
                        query.set(event_target_value(&ev));
                        hi.set(0);
                        // SCALE-25: a question per pause in the typing — and
                        // every answer to an earlier one is no longer wanted.
                        let n = found.try_update(Found::ask).unwrap_or(0);
                        spawn_local(async move {
                            dom::sleep_ms(DEBOUNCE_MS).await;
                            if found.try_with_untracked(|f| f.asked) == Some(n) {
                                if let Some(q) = query.try_get_untracked() {
                                    ask(found, n, q, pass_now(), answered);
                                }
                            }
                        });
                    }
                />
                <ul
                    id="palette-list"
                    role="listbox"
                    aria-label="matching tickets"
                    aria-busy=move || found.with(Found::pending).to_string()
                >
                    {move || {
                        let shown = results.get();
                        let len = shown.len();
                        // SEARCH-3: the hits in bodies, after the jump rows.
                        let inside = hits.get();
                        let n_hits = inside.len();
                        // FILTER-22: the shortcodes `filter …` offers, first.
                        let cmds = offers.get();
                        let base = cmds.len();
                        let cur = hi.get().min((base + len + n_hits + usize::from(under)).saturating_sub(1));
                        let cmd_rows = cmds
                            .into_iter()
                            .enumerate()
                            .map(|(i, cmd)| {
                                let (verb, code, hint) = match cmd {
                                    PalCmd::Code(code) => {
                                        let hint = format!("the global filter becomes Shortcode is {code}");
                                        ("Filter", code, hint)
                                    }
                                    PalCmd::Text(text) => ("Filter", text, "the global filter becomes this".to_string()),
                                    PalCmd::Theme(Some(name)) => ("Theme", name, "your site theme, at once (settings keeps it)".to_string()),
                                    PalCmd::Theme(None) => ("Theme", "the site's".to_string(), "wear the site's theme again".to_string()),
                                    PalCmd::Promote => (
                                        "Promote filter",
                                        lens.text(lens::Level::Nav),
                                        "make the nav's filter global; the nav goes back to its default".to_string(),
                                    ),
                                };
                                let at = code.clone();
                                view! {
                                    <li
                                        id=format!("pal-{i}")
                                        role="option"
                                        class="pal-cmd"
                                        data-pal-filter=at
                                        aria-selected=if i == cur { "true" } else { "false" }
                                        class:hi=i == cur
                                        on:click=move |_| open_at(i)
                                    >
                                        <span class="pal-do">{verb}</span>
                                        <span class="code">{code}</span>
                                        <span class="label pal-hint">{hint}</span>
                                    </li>
                                }
                            })
                            .collect_view();
                        // FILTER-16: the last row under a global filter — the
                        // other pass.
                        let wide = under.then(|| {
                            let e = everything.get();
                            let at = base + len + n_hits;
                            view! {
                                <li
                                    id=format!("pal-{at}")
                                    role="option"
                                    class="pal-wide"
                                    data-pal-wide=if e { "inside" } else { "everything" }
                                    aria-selected=if cur == at { "true" } else { "false" }
                                    class:hi=cur == at
                                    on:click=move |_| open_at(at)
                                >
                                    <span class="label">{if e { "Search inside the filter" } else { "Search everything" }}</span>
                                    <span class="pal-hint">{if e { "only what the global filter admits" } else { "without the global filter" }}</span>
                                </li>
                            }
                        });
                        // SEARCH-3: a heading, then a row per hit — its code,
                        // its title and status, and its passage beneath, the
                        // matched words marked.
                        let section = (n_hits > 0).then(|| {
                            let head = found.with(Found::hits_heading);
                            let outside = found.with(|f| f.hits_outside.clone());
                            let rows = inside
                                .into_iter()
                                .enumerate()
                                .map(|(k, h)| {
                                    let i = base + len + k;
                                    let id = h.summary.id.to_string();
                                    let status = h.summary.status;
                                    let out = outside.contains(id.as_str());
                                    view! {
                                        <li
                                            id=format!("pal-{i}")
                                            role="option"
                                            class="pal-hit"
                                            aria-selected=if i == cur { "true" } else { "false" }
                                            class:hi=i == cur
                                            class:outside=out
                                            data-pal-hit=id.clone()
                                            on:click=move |_| open_at(i)
                                        >
                                            <span class="pal-line">
                                                <span class="code">{id.to_string()}</span>
                                                <span class="label">{marked(h.title)}</span>
                                                {out.then(|| view! { <span class="pal-out">"outside the filter"</span> })}
                                                <span class=format!("tag {}", st_class(status.as_str()))>{status.to_string()}</span>
                                            </span>
                                            <span class="pal-snip">{marked(h.snippet)}</span>
                                        </li>
                                    }
                                })
                                .collect_view();
                            view! { <li class="pal-head" role="presentation">{head}</li> {rows} }
                        });
                        if shown.is_empty() && section.is_some() {
                            let word = if under && !found.with(|f| f.everything) { "no code or title inside the global filter matches" } else { "no code or title matches" };
                            return view! { {cmd_rows} <li class="empty">{word}</li> {section} {wide} }.into_any();
                        }
                        if shown.is_empty() {
                            let word = found.with(|f| {
                                if f.offline {
                                    "offline — reconnecting…".to_string()
                                } else if f.pending() {
                                    "searching…".to_string()
                                } else if let Some(why) = &f.refused {
                                    format!("the server did not take the global filter: {why}")
                                } else if under && !f.everything {
                                    "no ticket inside the global filter matches".to_string()
                                } else {
                                    "no ticket matches".to_string()
                                }
                            });
                            return view! { {cmd_rows} <li class="empty">{word}</li> {wide} }.into_any();
                        }
                        let outside = found.with(|f| f.outside.clone());
                        let rows = shown
                            .into_iter()
                            .enumerate()
                            .map(|(i, t)| {
                                let id = t.id.to_string();
                                let (title, status) = (t.title.clone(), t.status);
                                let out = outside.contains(id.as_str());
                                let i = base + i;
                                view! {
                                    <li
                                        id=format!("pal-{i}")
                                        role="option"
                                        aria-selected=if i == cur { "true" } else { "false" }
                                        class:hi=i == cur
                                        class:outside=out
                                        data-pal-id=id.clone()
                                        on:click=move |_| open_at(i)
                                    >
                                        <span class="code">{id.to_string()}</span>
                                        <span class="label">{title}</span>
                                        {out.then(|| view! { <span class="pal-out">"outside the filter"</span> })}
                                        <span class=format!("tag {}", st_class(status.as_str()))>{status.to_string()}</span>
                                    </li>
                                }
                            })
                            .collect_view();
                        view! { {cmd_rows} {rows} {section} {wide} }.into_any()
                    }}
                </ul>
                <div class="hint">
                    <span id="palette-count">
                        {move || {
                            // SCALE-25: the server's count — the answer's rows
                            // plus its `remaining` — once it is the answer to
                            // what is typed. FILTER-16: and which pass.
                            found.with(|f| count_words(f, under))
                        }}
                    </span>
                    " · ↑↓ move · Enter opens · Esc closes · filter SC"
                </div>
            </div>
        </div>
    }
}

/// The `?` cheat sheet: the key map ([`KeyTable`]), nothing else — the rows
/// Settings → Keyboard draws, unsearched. Its close button is the focus
/// trap's one stop, and it takes focus on open so Escape and Enter both work
/// with no aim.
#[component]
fn Cheats(overlay: RwSignal<Overlay>) -> impl IntoView {
    let close_ref = NodeRef::<leptos::html::Button>::new();
    close_ref.on_load(|el: web_sys::HtmlButtonElement| {
        let _ = el.focus();
    });
    let on_key = move |ev: web_sys::KeyboardEvent| {
        match bindings::hit(&ev, Context::Dialog) {
            Some("dialog.close") => {
                ev.prevent_default();
                close_overlay(overlay);
            }
            Some(id @ ("dialog.next-field" | "dialog.prev-field")) => {
                ev.prevent_default();
                trap_tab(id == "dialog.prev-field");
            }
            _ => {}
        }
        ev.stop_propagation();
    };

    view! {
        <div class="overlay" on:click=move |_| close_overlay(overlay)>
            <div
                id="cheats"
                class="dialog"
                role="dialog"
                aria-modal="true"
                aria-label="Keyboard shortcuts"
                on:click=|ev| ev.stop_propagation()
                on:keydown=on_key
            >
                <h2>"keyboard" <span class="cheats-more">"· Settings › Keyboard searches it"</span></h2>
                <div class="cheats-scroll">
                    <KeyTable query=Signal::derive(String::new) />
                </div>
                <button class="close" node_ref=close_ref on:click=move |_| close_overlay(overlay)>
                    "close (Esc)"
                </button>
            </div>
        </div>
    }
}

/// AGS-28: the key map as a table — every binding [`bindings::search`]
/// keeps for `query`, grouped ([`Group::ALL`]'s order), each row its chords
/// as this platform spells them, what it does and where it applies. The `?`
/// sheet draws it whole; Settings → Keyboard with its search. A row is
/// `tr[data-binding=<id>]`.
#[component]
pub(crate) fn KeyTable(#[prop(into)] query: Signal<String>) -> impl IntoView {
    let mac = bindings::is_mac();
    let table = StoredValue::new(bindings::table());
    move || {
        let q = query.get();
        table.with_value(|t| {
            let rows = bindings::search(t, &q, mac);
            if rows.is_empty() {
                return view! { <p class="keys-none">"no key or action matches"</p> }.into_any();
            }
            let groups = Group::ALL
                .into_iter()
                .filter_map(|g| {
                    let mine: Vec<&bindings::Entry> = rows.iter().copied().filter(|e| e.binding.group == g).collect();
                    (!mine.is_empty()).then_some((g, mine))
                })
                .map(|(g, mine)| {
                    let n = mine.len();
                    let trs = mine
                        .into_iter()
                        .enumerate()
                        .map(|(i, e)| {
                            let b = e.binding;
                            let head = (i == 0).then(|| view! { <th scope="rowgroup" rowspan=n.to_string()>{g.label()}</th> });
                            let keys = e
                                .chords
                                .iter()
                                .map(|c| view! { <kbd>{c.show(mac)}</kbd> })
                                .collect_view();
                            view! {
                                <tr data-binding=b.id data-native=b.native.then_some("browser")>
                                    {head}
                                    <td class="k">{keys}</td>
                                    <td class="does">{b.does}</td>
                                    <td class="where">{bindings::where_text(b)}</td>
                                </tr>
                            }
                        })
                        .collect_view();
                    view! { <tbody data-group=g.label()>{trs}</tbody> }
                })
                .collect_view();
            view! { <table class="keymap">{groups}</table> }.into_any()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{chat_offers, count_words, deployed_offers, filter_command, filter_offers, filter_text_command, narrows, palette_match, pass, theme_command, theme_offers, Found, Pass, FILTER_OFFERS};

    /// THEME-7: `theme <name>` and `theme: <name>` in the palette — the
    /// themes held that begin with what is typed, and the site's own.
    #[test]
    fn the_palette_picks_a_theme() {
        assert_eq!(theme_command("theme pantry").as_deref(), Some("pantry"));
        assert_eq!(theme_command("Theme: Pan").as_deref(), Some("pan"));
        assert_eq!(theme_command("theme").as_deref(), Some(""));
        assert_eq!(theme_command("  theme:").as_deref(), Some(""));
        assert_eq!(theme_command("themes"), None);
        assert_eq!(theme_command("theme a b"), None);
        assert_eq!(theme_command("filter MNG"), None);
        let names: Vec<String> = ["default", "garish", "ocean", "pantry"].map(String::from).to_vec();
        assert_eq!(theme_offers("pa", &names), vec![Some("pantry".to_string())]);
        assert_eq!(theme_offers("", &names).len(), FILTER_OFFERS);
        assert_eq!(theme_offers("", &names)[0].as_deref(), Some("default"));
        assert_eq!(theme_offers("site", &names), vec![None], "the site's own");
        assert!(theme_offers("zz", &names).is_empty());
    }

    use crate::filter::tests::{platform, strs};
    use crate::lens::Kept;
    use crate::tags::{self, Act, Tag};
    use menger_tickets_api::{Kind, Status, TicketSummary};

    /// The palette's v1 filter: a case-insensitive substring of either
    /// the code or the title, and an empty query keeps everything.
    #[test]
    fn palette_matches_code_or_title_case_insensitively() {
        // code, exactly and by prefix — "AGS-1" narrows to AGS-1, AGS-10…
        assert!(palette_match("AGS-1", "Keyboard navigation", "AGS-1"));
        assert!(palette_match("AGS-18", "Keyboard navigation", "AGS-1"));
        assert!(!palette_match("AGS-2", "Parent hierarchy", "AGS-1"));
        assert!(!palette_match("MFE-3", "The live board", "AGS-1"));
        // case folds both ways
        assert!(palette_match("AGS-18", "Keyboard navigation", "ags-1"));
        assert!(palette_match("ags-18", "keyboard", "AGS"));
        assert!(palette_match("MFE-7", "The Ticket Page", "ticket page"));
        // title substrings, anywhere in the string
        assert!(palette_match("AGS-18", "Keyboard navigation", "navig"));
        assert!(palette_match("AGS-18", "Keyboard navigation", "board"));
        assert!(!palette_match("AGS-18", "Keyboard navigation", "mouse"));
        // an empty (or blank) query is "everything"
        assert!(palette_match("AGS-18", "Keyboard navigation", ""));
        assert!(palette_match("AGS-18", "Keyboard navigation", "   "));
        // a query longer than both fields matches nothing, no panic
        assert!(!palette_match("A-1", "hi", "a much longer query"));
        // the query is trimmed, the fields are not re-tokenized: a
        // substring is a substring, spaces included
        assert!(palette_match("AGS-18", "Keyboard navigation", " keyboard nav "));
        assert!(!palette_match("AGS-18", "Keyboard navigation", "keyboardnav"));
        // non-ASCII text is carried through the scan without splitting a
        // character (ASCII case folding only, by design)
        assert!(palette_match("AGS-9", "manifest's palette — tokens", "—"));
        assert!(palette_match("AGS-9", "manifest's palette — tokens", "PALETTE"));
    }

    /// SCALE-25: a query narrows another when it only grew — then every
    /// ticket it matches, the other matched.
    #[test]
    fn a_query_narrows_only_when_it_grew() {
        assert!(narrows("", "map"));
        assert!(narrows("map", "map-12"));
        assert!(narrows("MAP", " map-1 "));
        assert!(narrows("ap-1", "map-12"));
        assert!(!narrows("map-12", "map-1"), "it shrank");
        assert!(!narrows("map", "mop"), "it changed");
    }

    fn hit(id: &str, title: &str) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        TicketSummary {
            hidden_blocked_by: 0,
            hidden_unlocks: 0,
            parent_hidden: false,
            archived: None,
            labels: Vec::new(),
            id: menger_tickets_api::TicketId::new(id),
            epic: menger_tickets_api::Code::stored(epic), shortcode: menger_tickets_api::ShortcodeId::new(epic), address: String::new(),
            seq: seq.parse().expect("seq"),
            title: title.to_string(),
            status: Status::Pending,
            kind: Kind::Build,
            severity: None,
            parent: None,
            child_count: 0,
            commits: Vec::new(),
            path: None,
            placement: None,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn ids(rows: Vec<TicketSummary>) -> Vec<String> {
        rows.into_iter().map(|t| t.id.into_string()).collect()
    }

    /// SEARCH-3: a hit in bodies — its passage one marked word.
    fn inside(id: &str, word: &str) -> crate::session::Hit {
        crate::session::Hit {
            summary: hit(id, "a title"),
            title: vec![menger_tickets_api::Marked { text: "a title".into(), hit: false }],
            snippet: vec![menger_tickets_api::Marked { text: word.into(), hit: true }],
            corrected: false,
        }
    }

    fn hit_ids(hits: Vec<crate::session::Hit>) -> Vec<String> {
        hits.into_iter().map(|h| h.summary.id.into_string()).collect()
    }

    /// SEARCH-3: the hits in bodies are numbered as `find`'s answers are —
    /// one to an older question is dropped — and shown below the jump rows
    /// without the tickets those already show; while a question is out, the
    /// last hits stand if the query only grew, in the same pass; a blank
    /// query and no connection show none.
    #[test]
    fn the_newest_hits_show_below_the_jump_rows() {
        use std::collections::HashSet;
        let words = |w: &str, also: &[&str]| menger_tickets_api::SearchWord { word: w.into(), also: also.iter().map(|a| a.to_string()).collect() };
        let mut f = Found::default();
        let old = f.ask();
        let new = f.ask();
        assert!(!f.take_hits(old, "plc".into(), vec![inside("AGS-9", "plc")], 1, Vec::new(), HashSet::new()), "an older answer");
        let all = vec![inside("AGS-1", "placement"), inside("AGS-2", "placement"), inside("AGS-3", "placement")];
        assert!(f.take_hits(new, "plcement wav".into(), all, 3, vec![words("plcement", &["placement", "placements"]), words("wav", &[])], HashSet::new()));
        assert_eq!(hit_ids(f.shown_hits("plcement wav", &[])), ["AGS-1", "AGS-2", "AGS-3"]);
        assert_eq!(hit_ids(f.shown_hits("plcement wav", &[hit("AGS-2", "a jump row")])), ["AGS-1", "AGS-3"], "a jump row shows AGS-2");
        assert!(f.shown_hits("   ", &[]).is_empty(), "a blank query has no hits");
        assert_eq!(f.hits_heading(), "In bodies · placement wav", "each word as its nearest correction");

        let typed = f.ask();
        assert_eq!(hit_ids(f.shown_hits("plcement wave", &[])).len(), 3, "the query grew: the hits stand");
        assert!(f.shown_hits("other", &[]).is_empty(), "the query changed");
        let wide = f.ask_pass(true);
        assert!(f.shown_hits("plcement wave", &[]).is_empty(), "the other pass's hits are not these");
        assert!(f.take_hits(wide, "plcement wave".into(), vec![inside("AGS-4", "wave")], 1, vec![words("plcement", &[]), words("wave", &[])], ["AGS-4".to_string()].into()));
        assert!(f.hits_everything && f.hits_outside.contains("AGS-4"));
        assert_eq!(f.hits_heading(), "In bodies", "nothing corrected");
        assert!(!f.take_hits(typed, "late".into(), Vec::new(), 0, Vec::new(), HashSet::new()));
        let gone = f.ask();
        f.lost(gone);
        assert!(f.shown_hits("plcement wave", &[]).is_empty(), "offline: none");
    }

    /// SEARCH-3: the count line adds how many tickets match inside, once
    /// that answers what is typed — and says nothing of it otherwise.
    #[test]
    fn the_count_line_says_how_many_match_inside() {
        use std::collections::HashSet;
        let mut f = Found::default();
        let n = f.ask();
        assert!(f.take(n, "cache".into(), vec![hit("AGS-1", "the cache")], 0));
        assert_eq!(count_words(&f, false), "1 matches", "no answer from search yet");
        assert!(f.take_hits(n, "cache".into(), Vec::new(), 12, Vec::new(), HashSet::new()));
        assert_eq!(count_words(&f, false), "1 matches · 12 in bodies");
        assert_eq!(count_words(&f, true), "1 matches inside the global filter · 12 in bodies");
        let m = f.ask();
        assert!(f.take(m, "cachez".into(), Vec::new(), 0));
        assert_eq!(count_words(&f, false), "0 matches", "the hits answer an older question");
        assert!(f.take_hits(m, "cachez".into(), Vec::new(), 0, Vec::new(), HashSet::new()));
        assert_eq!(count_words(&f, false), "0 matches");
    }

    /// SCALE-25: only the newest question's answer is shown — one that comes
    /// back after a newer question went out is dropped, whatever the order —
    /// and the count is the answer's rows plus its `remaining`. While a
    /// question is out, a query that grew shows what the last answer had
    /// that it matches (the head of what is coming); one that changed shows
    /// nothing; and with no connection nothing is shown as current.
    #[test]
    fn only_the_newest_answer_is_shown() {
        let mut f = Found::default();
        let open = f.ask();
        assert!(f.pending() && f.shown("").is_empty());
        assert!(f.take(open, String::new(), vec![hit("AAA-1", "x"), hit("MAP-1", "a map"), hit("MAP-12", "y")], 997));
        assert_eq!((f.total, f.pending()), (1_000, false));
        assert_eq!(ids(f.shown("")), ["AAA-1", "MAP-1", "MAP-12"]);

        let map = f.ask();
        let map12 = f.ask();
        assert!(f.pending());
        assert_eq!(ids(f.shown("map-12")), ["MAP-12"], "the head of what is coming");
        assert_eq!(ids(f.shown("map")), ["MAP-1", "MAP-12"], "titles count, as the rule says");
        assert!(!f.take(map, "map".into(), vec![hit("MAP-1", "a map")], 40), "an older question's answer");
        assert!(f.pending(), "still waiting for the newest");
        assert!(f.take(map12, "map-12".into(), vec![hit("MAP-12", "y"), hit("MAP-120", "z")], 5));
        assert_eq!((f.total, f.pending()), (7, false));
        assert!(!f.take(map, "map".into(), Vec::new(), 0), "late, and older still");
        assert_eq!(ids(f.shown("map-12")), ["MAP-12", "MAP-120"]);

        let shorter = f.ask();
        assert!(f.shown("map-1").is_empty(), "it shrank: nothing until its own answer");
        f.lost(shorter);
        assert!(f.offline && !f.pending() && f.shown("map-1").is_empty());
        let again = f.ask();
        assert!(f.take(again, "map-1".into(), vec![hit("MAP-1", "a map")], 0));
        assert!(!f.offline);
    }

    /// FILTER-16: the palette's two passes — with a global filter, `find
    /// { filter }`, and "Search everything" asks with none and marks against
    /// it; with none, SCALE-25's one pass.
    #[test]
    fn the_palette_asks_inside_the_filter_then_everything() {
        assert_eq!(pass(None, false), Pass { filter: None, mark: None }, "no global filter: as it was");
        assert_eq!(pass(None, true), Pass { filter: None, mark: None });
        assert_eq!(pass(Some("kind build"), false), Pass { filter: Some("kind build".into()), mark: None });
        assert_eq!(pass(Some("kind build"), true), Pass { filter: None, mark: Some("kind build".into()) });

        // Inside the filter.
        let mut f = Found::default();
        let open = f.ask();
        assert!(f.take(open, String::new(), vec![hit("AGS-3", "a build"), hit("AGS-5", "another")], 16));
        assert_eq!(count_words(&f, true), "2 of 18 matches inside the global filter");
        assert_eq!(count_words(&f, false), "2 of 18 matches", "SCALE-25's words with no global filter");
        assert!(!f.everything && f.outside.is_empty());

        // "Search everything": nothing of the other pass shows while it is
        // out; its answer is marked.
        let wide = f.ask_pass(true);
        assert!(f.pending());
        assert!(f.shown("").is_empty(), "the filtered rows are not the head of everything");
        let out: std::collections::HashSet<String> = ["AGS-1".to_string()].into();
        assert!(f.take_marked(wide, String::new(), vec![hit("AGS-1", "a scope"), hit("AGS-3", "a build")], 280, out));
        assert!(f.everything);
        assert_eq!(ids(f.shown("")), ["AGS-1", "AGS-3"]);
        assert!(f.outside.contains("AGS-1") && !f.outside.contains("AGS-3"));
        assert_eq!(count_words(&f, true), "2 of 282 matches in everything · 1 outside the filter");

        // Typing keeps the pass; a query that grew shows the head meanwhile.
        let typed = f.ask();
        assert!(f.wants_everything);
        assert_eq!(ids(f.shown("ags-3")), ["AGS-3"]);
        assert!(f.take(typed, "ags-3".into(), vec![hit("AGS-3", "a build")], 0));
        assert!(f.everything && f.outside.is_empty(), "a new answer's own marks");

        // Back inside; a refused filter says so.
        let back = f.ask_pass(false);
        assert!(f.refuse(back, String::new(), "under comes with FILTER-13".into()));
        assert!(!f.everything && f.rows.is_empty());
        assert_eq!(count_words(&f, true), "the global filter was not taken");
    }

    /// FILTER-22: the palette reads `filter <shortcode>` — the word in any
    /// case, one shortcode after it, upper case; anything else is a search.
    #[test]
    fn the_palette_reads_filter_and_a_shortcode() {
        assert_eq!(filter_command("filter demand"), Some("DEMAND".into()));
        // MENGER-12: a filter in the language is offered as itself; a bare
        // shortcode is not one.
        assert_eq!(filter_text_command("filter label Needs-You"), Some("label needs-you".into()));
        assert_eq!(filter_text_command("filter asking or label needs-you"), Some("(asking or label needs-you)".into()));
        assert_eq!(filter_text_command("Filter asking"), Some("asking".into()));
        assert_eq!(filter_text_command("filter demand"), None);
        assert_eq!(filter_text_command("filter"), None);
        assert_eq!(filter_text_command("asking"), None);
        // MENGER-13: the history predicates, as the language reads them.
        assert_eq!(filter_text_command("filter entered Coding within 1h").as_deref(), Some("entered Coding within 1h"));
        assert_eq!(
            filter_text_command("filter status \"In Review\" and entered in review within 12 hours").as_deref(),
            Some("status \"In Review\" and entered \"In Review\" within 12h")
        );
        assert_eq!(filter_text_command("filter changed today by system").as_deref(), Some("changed today by system"));
        assert_eq!(filter_command("  Filter   DEMAND "), Some("DEMAND".into()));
        assert_eq!(filter_command("FILTER plexus_2"), Some("PLEXUS_2".into()));
        for not in ["", "filter", "filter ", "filters demand", "filter demand now", "filter de$mand", "demand", "find demand"] {
            assert_eq!(filter_command(not), None, "{not:?}");
        }
    }

    /// CHAT-4: the palette offers the chats' cells — `chat…` both,
    /// `chatting` / `unr…` one, `not` turning either around.
    #[test]
    fn the_palette_offers_the_chats_cells() {
        assert_eq!(chat_offers("chat"), ["chatting", "unread"]);
        assert_eq!(chat_offers("filter chats"), ["chatting", "unread"]);
        assert_eq!(chat_offers("chatti"), ["chatting"]);
        assert_eq!(chat_offers("unr"), ["unread"]);
        assert_eq!(chat_offers("not unread"), ["not unread"]);
        assert!(chat_offers("ch").is_empty() && chat_offers("unicorn").is_empty() && chat_offers("chat with ada").is_empty());
        assert_eq!(filter_text_command("filter chatting and unread"), Some("chatting and unread".into()));
    }

    /// DEPLOY-13: `deployed …` offers the environments held — `not` turns
    /// them around, a partial name narrows them, `waiting` is the Complete
    /// work not on the last one; anything else offers nothing.
    #[test]
    fn the_palette_offers_the_deployed_cells() {
        let held: Vec<String> = ["test", "prod", "menger/test", "menger/prod"].map(String::from).to_vec();
        assert_eq!(deployed_offers("depl", &held), ["deployed test", "deployed prod", "deployed menger/test", "deployed menger/prod"]);
        assert_eq!(deployed_offers("filter deployed pr", &held), ["deployed prod"]);
        assert_eq!(deployed_offers("not dep m", &held), ["not deployed menger/test", "not deployed menger/prod"]);
        assert_eq!(deployed_offers("waiting", &held), ["status Complete and not deployed prod"]);
        for none in ["", "dep", "deploy a b", "notes", "waste", "deployment x y"] {
            assert!(deployed_offers(none, &held).is_empty(), "{none:?}");
        }
        assert!(deployed_offers("deployed", &[]).is_empty(), "no environment held: nothing to offer");
    }

    /// FILTER-22: what `filter <typed>` offers — the shortcode typed when the
    /// groups know it, then the ones it begins, at most five; none it names;
    /// before the groups, what was typed.
    #[test]
    fn the_palette_offers_the_shortcodes_typed() {
        let t = platform();
        let known = Some(t.known());
        assert_eq!(filter_offers("MACRO", known), strs(&["MACRO"]));
        assert_eq!(filter_offers("C", known), strs(&["CODEGEN", "CONTEXT"]));
        assert_eq!(filter_offers("GEN", known), strs(&["GEN"]), "the shortcodes it begins, not every one holding it");
        assert_eq!(filter_offers("NOPE", known), Vec::<String>::new());
        assert_eq!(filter_offers("", known).len(), FILTER_OFFERS);
        assert_eq!(filter_offers("NOPE", None), strs(&["NOPE"]));
    }

    /// FILTER-22: `filter demand` makes the global filter what the Filter
    /// item of DEMAND's tag makes: Shortcode is DEMAND's tree, nothing else.
    #[test]
    fn the_palettes_filter_is_the_tags() {
        let t = platform();
        let code = filter_command("filter macro").unwrap();
        let first = tags::offers(&Tag::shortcode(&code), Some(&t)).remove(0);
        let item = &tags::items(&Tag::shortcode(&code), Some(&t))[0];
        assert_eq!((item.act, &item.clause), (Act::Filter, &first.clause));
        let mut was = Kept::default();
        was.filter = menger_tickets_api::filter::Filter::parse("kind build and status Ready").unwrap();
        assert_eq!(tags::apply(Act::Filter, &first.clause, &was).filter.format(), "in (CODEGEN, GEN, MACRO)");
    }
}
