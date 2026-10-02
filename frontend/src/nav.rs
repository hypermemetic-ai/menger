//! AGS-12: the persistent left nav — manifest's sidebar pattern, tickets
//! only. A brand header over a scrolling tree; epics as parent rows,
//! tickets as children ordered (epic, seq); each ticket row = code +
//! title + status tag and is a real link to `#/ticket/{id}` (the MFE-1
//! deep-link contract), so the row for the open ticket is marked
//! selected simply by comparing against the route.
//!
//! The nav renders THROUGH the board's id-keyed row store — it opens no
//! data path of its own — so the watch stream keeps it live for free and
//! the house reactivity bar holds: a `status_changed` updates one row's
//! tag text in place (the row list never fires), a `created` inserts one
//! node into a keyed `<For>`, and every untouched row keeps its DOM.
//!
//! Groups open **collapsed** (owner refinement, 2026-09-05: the nav opens
//! showing just the epic shortcodes; clicking one drops its tickets down).
//! The *expanded* set is what persists in localStorage — so a newly seen
//! epic defaults closed and the user's openings survive reload — and the
//! chain holding the currently open ticket auto-expands once per
//! navigation so the selected row is visible. The hidden/shown state
//! persists alongside.
//!
//! AGS-13: each leaf's marker is its kind's glyph — see [`kind_glyph`] —
//! so the tree stays one column of monospace text while every row shows
//! what shape of work it is; the row carries `data-kind` and a `title`
//! for the reader who wants the word.
//!
//! AGS-18: the tree is a real WAI-ARIA tree — `tree` / `treeitem` /
//! `group` with `aria-expanded` / `aria-selected` (levels come from the
//! `aria-owns` structure: leptos 0.8 has no `aria-level` attribute), a
//! roving tabindex (exactly one row is the tab stop), and [`tree_keydown`]
//! for the pattern's key map: Up/Down (j/k) move, Right/Left (l/h) open a
//! node or step between a node and its children, Enter opens the focused
//! ticket, Home/End jump. Selection still belongs to the route — the
//! *focused* row and the *selected* row are two different things.
//!
//! AGS-20: **children nest.** The forest is no longer epic → tickets but
//! a recursive tree, derived in one pure pass (`plan()`, until SCALE-24)
//! from the flat row store's `parent` fields:
//!
//! - a ticket with no parent is a row of its epic's group; a ticket with
//!   a parent nests under that parent's row, at any depth and **across
//!   epics** (the group is where the parent lives; the child keeps its own
//!   code chip, so the epic stays visible);
//! - an epic whose **scope root** (its lowest-seq `scope` ticket) has a
//!   parent in another epic is a **sub-shortcode**: its shortcode row
//!   nests under that parent — with its own rollup and its own tickets
//!   beneath it — and it never appears at top level (`PLEXUS` /
//!   `TRANSPORT`, owner 2026-09-06);
//! - a group with no parentless tickets never appears at top level;
//! - a rollup counts every ticket in its subtree, at every depth.
//!
//! Every node — group row or ticket row — with children carries the
//! collapse toggle, one `expanded` key (an epic code for a group, a ticket
//! id for a ticket: the two can never collide), and the tree's key map at
//! any depth.
//!
//! MFE-12: **children mount when their node opens.** Collapse used to be a
//! class — every leaf of every group was in the DOM from the first paint,
//! 80,325 nodes at 10^4 behind `display: none` — and is now an unmount: a
//! closed node holds an empty `group` container (so its `aria-owns` still
//! names an element) and nothing else, and opening it mounts its children
//! then. What that trades away is AGS-20's "expanding hands back the very
//! same row nodes": a reopened node's children are new elements (their
//! ticket signals are the same, so nothing they show is stale). The forest
//! itself (`plan()`) was untouched and still covered every ticket — it was
//! what the rollups counted, what the auto-expand walked, and why the
//! tree's key map, which only ever walked the open rows, needed nothing new.
//!
//! MFE-17: **a parent can change.** MNG-12's `set_parent` files a ticket
//! under another, or re-hosts a sub-shortcode's root under another
//! shortcode's ticket; the forest was built on the stated assumption that
//! it could not. The plan still read `parent` untracked — a status change
//! must never re-plan — and the session notified the row list on
//! `parent_changed`, so the plan re-ran on the move like on an insert: the
//! leaf leaves its old parent's `<For>` and joins its new parent's, a
//! re-hosted shortcode's group row moves under its new host, and every
//! other node — its `expanded` key, its rows' DOM — stays as it was.
//!
//! SCALE-24: **the nav from roll-ups, children by page.** `plan()` is gone,
//! and with it the nav's reading of the board's list. It needed every row
//! walked before it was right (50.8 s to the shortcodes at 10^6), rebuilt
//! the whole forest on every insert (2.03 s for a created ticket to show)
//! and mounted every child of an opened node (163,531 leaves: the wasm
//! aborted). The drawing is AGS-20's still; where it comes from is not:
//!
//! - **The shortcode rows are the server's** — `groups {}` (MNG-10), asked
//!   for by the session before its walk and kept by `group_changed`
//!   ([`Tree`]). `done/total` are the server's roll-ups, which MNG-17 held
//!   equal to `plan()`'s; a sub-shortcode nests under its `host`, after the
//!   host's own children, as before.
//! - **A shortcode's tree hangs from its group's `root`** (MNG-14: no other
//!   ticket of a shortcode is parentless): opening the row shows the root,
//!   one `get`. A shortcode with no root — a store from before MNG-12 —
//!   lists its tickets flat under its row, by page (`list { epic }`).
//! - **A node's children come by page** when it opens — `list { parent,
//!   after, limit: 200, summary }` (SCALE-4) — into a [`List`]: how many
//!   there are (`child_count`), and which ids are known at which index. The
//!   index comes with the answer: a page after a known child starts right
//!   after it, a page from the top at 0, and a page after any other cursor
//!   ends `remaining` from the end.
//! - **A list of more than a page is a window**, as the board is (MFE-12):
//!   its children are mounted in chunks of [`CHUNK`] — the chunks in or near
//!   the nav's viewport — between two spacers that stand for the rest at
//!   their measured height, so the scrollbar spans every child and the last
//!   of 163,530 is a scroll away. A chunk the window reaches before its
//!   children are known draws rows marked pending and asks for them: after
//!   the child before them when that is known, otherwise after a position
//!   worked out from the ids around them ([`List::ask_for`]: a shortcode's
//!   sequence is dense, MNG-11, and its children share it, MNG-14 — so the
//!   cursor for the child k places on is `E-(seq + k)`), corrected by what
//!   comes back. A chunk far from the window gives its rows back to the row
//!   cache; the ids stay, so scrolling back asks from an exact cursor.
//! - **`watch` moves an open list in place**: `children_changed` /
//!   `child_removed` put the child at its place among the known ids, or
//!   shift those after an unknown stretch, and set the count. A closed node
//!   only changes its count — its row's `child_count`, in the row cache — and
//!   a closed shortcode only its `done/total`, from `group_changed`. A
//!   re-hosted sub-shortcode moves with its `group_changed`.
//! - **A deep link fetches its chain** — one `get` per ancestor — opens it,
//!   and in each windowed list on the way asks for the page *after* the
//!   child it must show, whose `remaining` says where the child is; the
//!   window goes there, fetching the page around it and nothing before.
//! - **The keyboard (AGS-18) walks what is mounted and fetches on the way.**
//!   Spacers and pending rows are marked as gaps; a step from a row into a
//!   gap scrolls the nav the least that brings the rows behind it into the
//!   window and focuses the next row once it is there ([`tree_keydown`]);
//!   End goes to the end of the tree the same way. A windowed list's rows
//!   carry `aria-setsize` / `aria-posinset`, so assistive technology hears
//!   where a row is among all of its siblings, not among the mounted ones.
//!   Nothing reachable before is unreachable now: a row that was mounted is
//!   mounted or a scroll away, and every key that reached it reaches it.
//! - **AGS-12's persisted expansion** may name nodes that are not fetched
//!   yet at load; a node is open when it is mounted and named there, and it
//!   asks for its children then.
//!
//! Every row is a signal of the row cache (src/cache.rs), the one the board
//! and the ticket page draw the same ticket through.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::kids::key;
use crate::{cache, dom, keys, session, st_class, Route};
use menger_tickets_api::{Group, TicketSummary, TicketsEvent};

/// Every treeitem in the nav, in document order (group rows and leaves).
fn tree_items() -> Vec<web_sys::HtmlElement> {
    keys::query_all("#nav .tree [role=treeitem]")
}

/// The rows a keystroke can reach: the open rows of the tree. MFE-12: the
/// children of a collapsed node are not mounted at all, so this is every
/// mounted treeitem — the filter is for the nav hidden whole
/// (`#app.nonav`).
fn visible_items() -> Vec<web_sys::HtmlElement> {
    tree_items().into_iter().filter(keys::is_visible).collect()
}

/// SCALE-24: the mounted rows *and the gaps between them* — a windowed
/// list's spacers and its rows still being fetched — in document order: the
/// tree as far as a keystroke can see it.
fn walkable() -> Vec<web_sys::HtmlElement> {
    keys::query_all("#nav .tree [role=treeitem], #nav .tree .gap")
        .into_iter()
        .filter(keys::is_visible)
        .collect()
}

fn is_gap(el: &web_sys::Element) -> bool {
    el.class_list().contains("gap")
}

/// Seed the tree's tab stop after a render: the selected row when it is
/// on screen, else the first row — so Tab from a cold load lands in the
/// nav at the ticket the page is showing.
fn ensure_tree_tab_stop() {
    keys::ensure_tab_stop("#nav .tree [role=treeitem]", Some("#nav .tree .row.sel"));
}

const EXPANDED_KEY: &str = "menger.nav.expanded";
const HIDDEN_KEY: &str = "menger.nav.hidden";

/// localStorage, or None where it is unavailable (private mode, blocked
/// site data). Every persistence call degrades to "not remembered".
fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

fn load_expanded() -> BTreeSet<String> {
    storage()
        .and_then(|s| s.get_item(EXPANDED_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn save_expanded(set: &BTreeSet<String>) {
    if let Some(s) = storage() {
        let raw = serde_json::to_string(&set.iter().collect::<Vec<_>>()).unwrap_or_default();
        let _ = s.set_item(EXPANDED_KEY, &raw);
    }
}

pub(crate) fn load_hidden() -> bool {
    storage()
        .and_then(|s| s.get_item(HIDDEN_KEY).ok().flatten())
        .as_deref()
        == Some("1")
}

pub(crate) fn save_hidden(hidden: bool) {
    if let Some(s) = storage() {
        let _ = if hidden {
            s.set_item(HIDDEN_KEY, "1")
        } else {
            s.remove_item(HIDDEN_KEY)
        };
    }
}

/// One glyph per kind (AGS-13), for the leaf's marker slot. Anything
/// outside the five — the server's vocabulary may grow before this
/// table does — degrades to the neutral dot the slot showed before, never
/// to a blank.
pub(crate) fn kind_glyph(kind: &str) -> &'static str {
    match kind {
        "scope" => "◆",
        "execution" => "▶",
        "build" => "■",
        "gap" => "○",
        "spike" => "△",
        _ => "·",
    }
}

/// A rendered node: a ticket row, or an epic's shortcode row. Its key is
/// the ticket id / the epic code — the string that identifies it in the
/// persisted `expanded` set and in the DOM (`data-node-key`). Ticket ids
/// always carry a `-`, epic codes never do, so the two namespaces cannot
/// collide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Ticket(String),
    Group(String),
}

impl Node {
    pub(crate) fn key(&self) -> &str {
        match self {
            Node::Ticket(k) | Node::Group(k) => k,
        }
    }
}

// ---------------------------------------------------------------------
// A node's children, by page (SCALE-24) — pure, so a native test can pin
// what a page, a birth, a removal and a guess do to it.

/// How many children one `list { parent }` asks for: SCALE-4's page, the
/// ticket page's too — 2.4 ms at 10^6, first page or deep.
pub(crate) const PAGE: usize = 200;

/// Children are mounted, measured and let go of in runs of this many — the
/// window's step, as `BOARD_STEP` is the board's. About two and a half
/// screens of the nav.
pub(crate) const CHUNK: usize = 50;

/// A list of at most this many children is mounted whole, as every open
/// node's was (MFE-12): one page, no spacers.
pub(crate) const WHOLE: usize = PAGE;

/// Chunks a list keeps its rows held for beyond the window, each side: a
/// scroll back over them is a re-render, not a re-fetch.
pub(crate) const KEEP_CHUNKS: usize = 4;

/// The nav's row pitch (px) until a row has been measured: 12.5 px type at
/// a 1.55 line height (index.html).
const PITCH_GUESS: f64 = 19.375;

/// What an open node lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// A ticket's children: `list { parent }`.
    Parent(String),
    /// A shortcode with no root — a store from before MNG-12 — lists its
    /// tickets flat: `list { epic }`.
    Epic(String),
}

/// Where to ask for the page that starts at a child: the cursor, and where
/// the answer starts when the cursor says so — `None` when it must be
/// worked out from the answer's `remaining`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ask {
    pub(crate) after: Option<String>,
    pub(crate) start: Option<usize>,
}

impl Ask {
    fn top() -> Self {
        Self { after: None, start: Some(0) }
    }
}

/// One open node's children: how many it has, and which are known where.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct List {
    /// How many children the node has — its `child_count`; for a flat
    /// shortcode, what the first page counted.
    pub(crate) n: usize,
    /// The known children: index → id, and back. Store order, so the ids
    /// read in index order are in `(epic, seq)` order.
    at: BTreeMap<usize, String>,
    of: HashMap<String, usize>,
    /// A page has answered: `n` is the server's, not a guess.
    pub(crate) loaded: bool,
    /// Moves every time a known index does (a birth or a removal before it,
    /// a reset) — what the measured chunk heights are good for.
    pub(crate) moved: u32,
    /// The last guess brought nothing new: ask from the child before the
    /// gap, which always does, until a page does.
    stalled: bool,
}

impl List {
    pub(crate) fn of(n: usize) -> Self {
        Self { n, ..Self::default() }
    }

    pub(crate) fn id_at(&self, i: usize) -> Option<&str> {
        self.at.get(&i).map(String::as_str)
    }

    pub(crate) fn index_of(&self, id: &str) -> Option<usize> {
        self.of.get(id).copied()
    }

    /// The unknown children in `lo..hi`, as runs.
    #[cfg(test)]
    pub(crate) fn gaps(&self, lo: usize, hi: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut from = lo;
        for (&i, _) in self.at.range(lo..hi) {
            if i > from {
                out.push((from, i));
            }
            from = i + 1;
        }
        if from < hi {
            out.push((from, hi));
        }
        out
    }

    /// The count moved without a page saying where (the parent's row says
    /// so): what is known past the end is let go of.
    pub(crate) fn resize(&mut self, n: usize) {
        if n == self.n {
            return;
        }
        self.n = n;
        let past: Vec<usize> = self.at.range(n..).map(|(i, _)| *i).collect();
        for i in past {
            if let Some(id) = self.at.remove(&i) {
                self.of.remove(&id);
            }
        }
    }

    /// Every known index from `from` on moves by `by`.
    fn shift(&mut self, from: usize, by: isize) {
        let moved = self.at.split_off(&from);
        for (i, id) in moved {
            let j = i.saturating_add_signed(by);
            self.of.insert(id.clone(), j);
            self.at.insert(j, id);
        }
        self.moved = self.moved.wrapping_add(1);
    }

    /// The smallest known index whose id sorts after `k` — scanned from the
    /// end, where births land.
    fn first_after(&self, k: (&str, u32)) -> Option<usize> {
        let mut found = None;
        for (i, id) in self.at.iter().rev() {
            match key(id) {
                Some(x) if x > k => found = Some(*i),
                _ => break,
            }
        }
        found
    }

    /// A page placed by arithmetic (`remaining`): `ids`, the children from
    /// about index `start` on. An id already known pins the page to where
    /// that one is — two pages placed either side of a birth that raced them
    /// would otherwise disagree by one.
    pub(crate) fn place(&mut self, start: usize, ids: Vec<String>) {
        let start = ids
            .iter()
            .enumerate()
            .find_map(|(j, id)| self.of.get(id).and_then(|&k| k.checked_sub(j)))
            .unwrap_or(start);
        self.place_at(start, ids);
    }

    /// A page placed exactly — after a known child, or from the top: `ids`
    /// are the children from index `start` on. What it says replaces what
    /// was known over its range, and its ids anywhere else; and if one of
    /// them was known somewhere else, what was known after the page
    /// disagrees with the server and is forgotten (a page will bring it).
    pub(crate) fn place_at(&mut self, start: usize, ids: Vec<String>) {
        let end = start + ids.len();
        let elsewhere = ids.iter().any(|id| self.of.get(id).is_some_and(|&i| i < start || i >= end));
        if elsewhere {
            for (_, id) in self.at.split_off(&end) {
                self.of.remove(&id);
            }
            self.moved = self.moved.wrapping_add(1);
        }
        let stale: Vec<usize> = self.at.range(start..end).map(|(i, _)| *i).collect();
        for i in stale {
            if let Some(id) = self.at.remove(&i) {
                self.of.remove(&id);
            }
        }
        for id in &ids {
            if let Some(i) = self.of.remove(id) {
                self.at.remove(&i);
            }
        }
        for (j, id) in ids.into_iter().enumerate() {
            self.of.insert(id.clone(), start + j);
            self.at.insert(start + j, id);
        }
        self.n = self.n.max(end);
        self.loaded = true;
    }

    /// The answer to `ask`: `ids` and the server's `remaining` after them.
    /// `reset` forgets what was known first (a reconnect, a `lagged`).
    /// Returns whether it told the list anything new.
    pub(crate) fn answered(&mut self, ask: &Ask, ids: Vec<String>, remaining: usize, reset: bool) -> bool {
        let len = ids.len();
        if ask.after.is_none() {
            self.n = len + remaining; // from the top: the exact count
        }
        if reset {
            self.at.clear();
            self.of.clear();
            self.moved = self.moved.wrapping_add(1);
        }
        let known = self.at.len();
        let fresh = ids.iter().any(|id| !self.of.contains_key(id));
        if len == 0 {
            if let (Some(_), Some(s)) = (&ask.after, ask.start) {
                // nothing after a known child: the list ends there
                self.resize(s);
            }
            self.loaded = true;
        } else {
            match ask.start {
                Some(s) => self.place_at(s, ids),
                None => {
                    let s = self.n.saturating_sub(remaining + len);
                    self.place(s, ids)
                }
            }
        }
        let progress = fresh || reset || self.at.len() != known;
        // A guess that brought nothing new would be asked again; the next
        // ask is from the child before the gap, which always moves.
        self.stalled = ask.start.is_none() && !progress;
        progress
    }

    /// A guessed cursor named no ticket (a hole in a hand-edited store, or
    /// past a shortcode's end): ask from what is known instead.
    pub(crate) fn stall(&mut self) {
        self.stalled = true;
    }

    /// `children_changed`: `id` joined — born, or moved in — and the node now
    /// has `n`. Placed if its neighbours are known; otherwise the known
    /// children after it move down one, and a page will bring it.
    pub(crate) fn added(&mut self, id: &str, n: usize) -> bool {
        if self.of.contains_key(id) {
            self.n = n.max(self.at.keys().next_back().map_or(0, |i| i + 1));
            return false;
        }
        let was = self.n;
        self.n = n;
        let Some(k) = key(id) else { return false };
        match self.first_after(k) {
            Some(b) => {
                self.shift(b, 1);
                if b == 0 || self.at.contains_key(&(b - 1)) {
                    self.of.insert(id.to_string(), b);
                    self.at.insert(b, id.to_string());
                }
            }
            // After every known child: the new last, if the list was known
            // to its end.
            None if was > 0 && self.at.contains_key(&(was - 1)) => {
                self.of.insert(id.to_string(), was);
                self.at.insert(was, id.to_string());
            }
            None if was == 0 && self.loaded => {
                self.of.insert(id.to_string(), 0);
                self.at.insert(0, id.to_string());
            }
            None => {}
        }
        true
    }

    /// MFE-17's `child_removed`: `id` left — moved under another — and the
    /// node now has `n`.
    pub(crate) fn removed(&mut self, id: &str, n: usize) -> bool {
        self.n = n;
        match self.of.remove(id) {
            Some(i) => {
                self.at.remove(&i);
                self.shift(i + 1, -1);
                true
            }
            None => {
                if let Some(b) = key(id).and_then(|k| self.first_after(k)) {
                    self.shift(b, -1);
                }
                false
            }
        }
    }

    /// Where to ask for the page that starts at child `i`.
    ///
    /// After the child before it when that is known — the answer starts at
    /// `i`, exactly — or when the nearest known child before it is within
    /// half a page (one page brings both). Otherwise a position worked out
    /// from the known ids around `i`: a shortcode's sequence is dense
    /// (MNG-11), and a node's children share their shortcode (MNG-14), so
    /// between two known children in one shortcode the child `i - 1` is
    /// interpolated from their sequences, and beyond the last (or before
    /// the first) it is that child's sequence plus (minus) the distance. The
    /// cursor need not be a child — `after` is a position (MNG-9) — and the
    /// answer's `remaining` says where it landed. A guess that lands
    /// nowhere new stalls the list, and it asks from the known child before
    /// the gap until a page moves it on.
    pub(crate) fn ask_for(&self, i: usize) -> Ask {
        if i == 0 {
            return Ask::top();
        }
        let before = self.at.range(..i).next_back().map(|(a, id)| (*a, id.as_str()));
        if let Some((a, id)) = before {
            if a + 1 == i || self.stalled || i - a <= PAGE / 2 {
                return Ask { after: Some(id.to_string()), start: Some(a + 1) };
            }
        }
        if self.stalled {
            return Ask::top();
        }
        let after = self.at.range(i..).next().map(|(b, id)| (*b, id.as_str()));
        let guess = match (before, after) {
            (Some((a, x)), Some((b, y))) => key(x).map(|(e, s)| {
                let steps = (i - 1 - a) as u64;
                match key(y) {
                    Some((ye, ys)) if ye == e && ys > s => {
                        (e, s + (steps * u64::from(ys - s) / (b - a) as u64) as u32)
                    }
                    _ => (e, s.saturating_add(steps as u32)),
                }
            }),
            (Some((a, x)), None) => key(x).map(|(e, s)| (e, s.saturating_add((i - 1 - a) as u32))),
            (None, Some((b, y))) => key(y)
                .and_then(|(e, s)| s.checked_sub((b - (i - 1)) as u32).filter(|s| *s >= 1).map(|s| (e, s))),
            (None, None) => None,
        };
        match guess {
            Some((e, s)) => Ask { after: Some(format!("{e}-{s}")), start: None },
            None => Ask::top(),
        }
    }
}

/// How many chunks a list of `n` children is.
pub(crate) fn chunks(n: usize) -> usize {
    n.div_ceil(CHUNK)
}

fn rows_in(k: usize, n: usize) -> usize {
    n.saturating_sub(k * CHUNK).min(CHUNK)
}

/// Where a windowed list sits against the nav's viewport (px): how far its
/// top has scrolled above the viewport's top edge (negative: the list
/// starts below it), the viewport's height, and the row pitch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geom {
    pub(crate) above: f64,
    pub(crate) viewport: f64,
    pub(crate) pitch: f64,
}

/// What a list mounts: chunks `start..end`, and the heights of the two
/// spacers that stand for the chunks either side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) top_px: f64,
    pub(crate) bottom_px: f64,
}

/// The window over a list of `n` children: the chunks within a chunk's
/// height of the viewport, either side. A chunk's height is what it measured
/// when it was last mounted (`measured`), else its rows at the pitch — every
/// row is one line, so that is exact until a child is opened. A list wholly
/// above the viewport mounts its last chunk and one wholly below its first,
/// so a step into it from outside finds a row. A list of up to [`WHOLE`]
/// children is mounted whole.
pub(crate) fn span(n: usize, g: Geom, measured: &BTreeMap<usize, f64>) -> Span {
    let total = chunks(n);
    if n <= WHOLE {
        return Span { start: 0, end: total, top_px: 0.0, bottom_px: 0.0 };
    }
    let h = |k: usize| measured.get(&k).copied().unwrap_or(rows_in(k, n) as f64 * g.pitch);
    let margin = CHUNK as f64 * g.pitch;
    let (lo, hi) = (g.above - margin, g.above + g.viewport + margin);
    let (mut start, mut end) = (None, total);
    let mut y = 0.0;
    for k in 0..total {
        let next = y + h(k);
        if start.is_none() && next > lo {
            start = Some(k);
        }
        if y >= hi {
            end = k;
            break;
        }
        y = next;
    }
    let start = start.unwrap_or(total - 1).min(total - 1);
    let end = end.max(start + 1).min(total);
    let top_px: f64 = (0..start).map(h).sum();
    let bottom_px: f64 = (end..total).map(h).sum();
    Span { start, end, top_px, bottom_px }
}

/// Where child `i` starts, from the top of its list (px).
pub(crate) fn offset_of(i: usize, n: usize, pitch: f64, measured: &BTreeMap<usize, f64>) -> f64 {
    let k = i / CHUNK;
    let above: f64 = (0..k).map(|c| measured.get(&c).copied().unwrap_or(rows_in(c, n) as f64 * pitch)).sum();
    above + (i - k * CHUNK) as f64 * pitch
}

/// A list's measured chunk heights, and the `moved` stamp of the list they
/// were measured against.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Heights {
    moved: u32,
    by_chunk: BTreeMap<usize, f64>,
}

impl Heights {
    /// What still describes the list as it is (`moved` now): nothing, once
    /// its known indices have moved since.
    fn of(&self, moved: u32) -> &BTreeMap<usize, f64> {
        static NONE: BTreeMap<usize, f64> = BTreeMap::new();
        if self.moved == moved {
            &self.by_chunk
        } else {
            &NONE
        }
    }

    fn differs(&self, moved: u32, got: &[(usize, f64)]) -> bool {
        let m = self.of(moved);
        got.iter().any(|(k, h)| m.get(k).is_none_or(|old| (old - h).abs() > 0.5))
    }

    fn take(&mut self, moved: u32, got: Vec<(usize, f64)>) {
        if self.moved != moved {
            self.moved = moved;
            self.by_chunk.clear();
        }
        self.by_chunk.extend(got);
    }
}

// ---------------------------------------------------------------------
// The nav's model of the store (SCALE-24)

/// What a deep link opens: every node on the way down, and the child each
/// must show — the page after that child says where it is in a windowed
/// list. A host's step has none: the sub-shortcode it shows is a row drawn
/// after its children, always mounted.
#[derive(Clone, Debug, PartialEq)]
struct Reveal {
    gen: u32,
    steps: Vec<(Node, Option<TicketSummary>)>,
}

/// What the nav knows of the store: the server's roll-ups, one per
/// shortcode row, and the signals its open lists follow. Made once by the
/// App; the session writes the groups and moves the epoch, the nav reads.
#[derive(Clone, Copy)]
pub(crate) struct Tree {
    groups: RwSignal<Option<BTreeMap<String, Group>>>,
    /// Moves each time the session has the groups in hand — on every
    /// connect, and after a `lagged`. An open list asks for its window again
    /// when it moves (what it shows may be stale); nothing asks before it is
    /// first set.
    epoch: RwSignal<u32>,
    /// The nav scrolled or resized, or something in it changed height: each
    /// windowed list measures where it is.
    viewport: Trigger,
    pitch: RwSignal<f64>,
    reveal: RwSignal<Option<Reveal>>,
    /// The top-level shortcode rows, in shortcode order.
    top: Memo<Vec<String>>,
    /// Ticket → the sub-shortcodes hosted under it, in shortcode order.
    hosted: Memo<BTreeMap<String, Vec<String>>>,
    /// The roots of hosted sub-shortcodes: a child of their host by
    /// `parent`, drawn as their shortcode's row instead (AGS-20).
    hosted_roots: Memo<BTreeSet<String>>,
}

impl Tree {
    pub(crate) fn new() -> Self {
        let groups = RwSignal::new(None::<BTreeMap<String, Group>>);
        let all = move || groups.with(|g| g.as_ref().map(|m| m.values().cloned().collect::<Vec<_>>()).unwrap_or_default());
        let top = Memo::new(move |_| {
            all().into_iter().filter(|g| g.host.is_none()).map(|g| g.epic).collect::<Vec<_>>()
        });
        let hosted = Memo::new(move |_| {
            let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for g in all() {
                if let Some(h) = g.host {
                    out.entry(h).or_default().push(g.epic);
                }
            }
            out
        });
        let hosted_roots = Memo::new(move |_| {
            all().into_iter().filter(|g| g.host.is_some()).filter_map(|g| g.root).collect::<BTreeSet<_>>()
        });
        Self {
            groups,
            epoch: RwSignal::new(0),
            viewport: Trigger::new(),
            pitch: RwSignal::new(PITCH_GUESS),
            reveal: RwSignal::new(None),
            top,
            hosted,
            hosted_roots,
        }
    }

    /// `groups {}` answered: every row the nav draws. A row with `total: 0`
    /// is drawn by no one.
    pub(crate) fn set_groups(self, all: Vec<Group>) {
        let map = all.into_iter().filter(|g| g.total > 0).map(|g| (g.epic.clone(), g)).collect();
        self.groups.set(Some(map));
        self.epoch.update(|e| *e += 1);
    }

    /// `group_changed`: one row's roll-up as it is now — its `done/total`
    /// move in place, a changed `host` moves the row; `total: 0` is a row
    /// no longer drawn.
    pub(crate) fn group_changed(self, g: Group) {
        let same = self.groups.with_untracked(|m| m.as_ref().and_then(|m| m.get(&g.epic)) == Some(&g));
        if same {
            return;
        }
        self.groups.update(|m| {
            if let Some(m) = m {
                if g.total == 0 {
                    m.remove(&g.epic);
                } else {
                    m.insert(g.epic.clone(), g);
                }
            }
        });
    }

    fn group(self, epic: &str) -> Option<Group> {
        self.groups.with(|m| m.as_ref().and_then(|m| m.get(epic)).cloned())
    }
}

thread_local! {
    /// Every open list, by what it lists — where `watch`'s deltas are applied.
    static OPEN: RefCell<Vec<(Source, RwSignal<List>, cache::Holder)>> = const { RefCell::new(Vec::new()) };

    /// The ticket the last `created` / `parent_changed` carried: the
    /// `children_changed` behind it names it, and an open list takes its
    /// row from here rather than ask the server for it.
    static ARRIVED: RefCell<Option<TicketSummary>> = const { RefCell::new(None) };

    /// The deep link being revealed; a newer navigation supersedes it.
    static REVEAL_GEN: Cell<u32> = const { Cell::new(0) };

    /// The tree's row that has focus, while one does — what [`keep_focus`]
    /// gives focus back to.
    static TREE_FOCUS: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
}

/// SCALE-24: focus stays in the tree while its windows move.
///
/// A windowed list's keyed `<For>` moves rows in the DOM when its window
/// grows at the top — tachys re-inserts the survivors behind a prepend — and
/// an element moved in the DOM loses focus: the keystroke after it would
/// land on `<body>`, and the board's j/k would take it. So the tree watches
/// its own subtree, and when a mutation batch removed the focused row (or a
/// node holding it) while focus fell to `<body>`, it gives focus back — to
/// the row itself if the move put it back, else (its window let it go: a
/// wheel scroll, a collapse) to the first row in view — without scrolling,
/// in the observer's microtask, before any next keystroke is dispatched.
fn keep_focus(tree: &web_sys::Element) {
    let Some(doc) = dom::window().document() else { return };
    let on_focus = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::FocusEvent)>::new(|ev: web_sys::FocusEvent| {
        let row = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("#nav .tree [role=treeitem]").ok().flatten())
            .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok());
        TREE_FOCUS.with(|f| *f.borrow_mut() = row);
    });
    let _ = doc.add_event_listener_with_callback("focusin", on_focus.as_ref().unchecked_ref());
    on_focus.forget(); // lives as long as the app

    let on_mutate = wasm_bindgen::closure::Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new(
        |records: js_sys::Array, _: web_sys::MutationObserver| {
            let Some(el) = TREE_FOCUS.with(|f| f.borrow().clone()) else { return };
            let Some(doc) = dom::window().document() else { return };
            let resting = doc.active_element().is_none_or(|a| a.tag_name() == "BODY");
            if !resting {
                return;
            }
            let lost = records.iter().any(|r| {
                let Ok(r) = r.dyn_into::<web_sys::MutationRecord>() else { return false };
                let gone = r.removed_nodes();
                (0..gone.length()).filter_map(|i| gone.get(i)).any(|n| n.is_same_node(Some(&el)) || n.contains(Some(&el)))
            });
            if !lost {
                return;
            }
            let opts = web_sys::FocusOptions::new();
            opts.set_prevent_scroll(true);
            if el.is_connected() {
                let _ = el.focus_with_options(&opts);
                return;
            }
            // Gone for good: the first row in view takes the focus (and the
            // tab stop), so the next keystroke is still the tree's.
            let Some(sc) = scroller() else { return };
            let top = sc.get_bounding_client_rect().top();
            let all = tree_items();
            if let Some(row) = all.iter().find(|r| keys::is_visible(r) && r.get_bounding_client_rect().top() >= top - 1.0) {
                keys::rove(&all, row);
                let _ = row.focus_with_options(&opts);
            }
        },
    );
    if let Ok(obs) = web_sys::MutationObserver::new(on_mutate.as_ref().unchecked_ref()) {
        let init = web_sys::MutationObserverInit::new();
        init.set_child_list(true);
        init.set_subtree(true);
        let _ = obs.observe_with_options(tree, &init);
    }
    on_mutate.forget(); // lives as long as the app
}

/// The open lists `pick` matches, outside the registry's borrow.
fn lists(pick: impl Fn(&Source) -> bool) -> Vec<(RwSignal<List>, cache::Holder)> {
    OPEN.with(|o| o.borrow().iter().filter(|(s, _, _)| pick(s)).map(|(_, l, h)| (*l, h.clone())).collect())
}

/// A `created` or `parent_changed` came in. A flat shortcode's list (no
/// root) takes a birth of its shortcode the way a parent's list takes
/// `children_changed`.
pub(crate) fn arrived(t: &TicketSummary, born: bool) {
    ARRIVED.with(|a| *a.borrow_mut() = Some(t.clone()));
    if !born {
        return;
    }
    for (list, holder) in lists(|s| matches!(s, Source::Epic(e) if *e == t.epic)) {
        holder.hold(t.clone());
        list.try_update(|l| {
            let n = l.n + 1;
            l.added(&t.id, n)
        });
    }
}

/// `children_changed`: an open list of `parent`'s children takes `added`.
pub(crate) fn children_changed(parent: &str, added: &str, count: u64) {
    let row = ARRIVED.with(|a| a.borrow().as_ref().filter(|t| t.id == added).cloned());
    for (list, holder) in lists(|s| matches!(s, Source::Parent(p) if p == parent)) {
        match &row {
            Some(t) => {
                holder.hold(t.clone());
            }
            None => {
                holder.hold_id(added);
            }
        }
        list.try_update(|l| l.added(added, count as usize));
    }
}

/// MFE-17's `child_removed`: an open list of `parent`'s children lets
/// `removed` go.
pub(crate) fn child_removed(parent: &str, removed: &str, count: u64) {
    for (list, holder) in lists(|s| matches!(s, Source::Parent(p) if p == parent)) {
        list.try_update(|l| l.removed(removed, count as usize));
        holder.release(removed);
    }
}

/// One page of a list, asked of the server.
enum Asked {
    Page(Vec<TicketSummary>, usize),
    /// An in-band refusal — a guessed cursor that names no ticket (a hole
    /// in a hand-edited store), or worse.
    Refused,
    /// No socket, or it died on the way: asked again when it is back.
    Offline,
}

async fn ask_page(source: &Source, after: Option<String>, limit: usize) -> Asked {
    let Some(client) = session::client() else { return Asked::Offline };
    let (epic, parent) = match source {
        Source::Parent(p) => (None, Some(p.clone())),
        Source::Epic(e) => (Some(e.clone()), None),
    };
    match client.list_all(epic, None, parent, after, Some(limit as u32), Some(true)).await {
        Ok(events) => {
            let mut rows = Vec::new();
            let mut remaining = None;
            for ev in events {
                match ev {
                    TicketsEvent::Summary { summary } => rows.push(summary),
                    TicketsEvent::Page { remaining: r, .. } => remaining = Some(r as usize),
                    TicketsEvent::Error { .. } => return Asked::Refused,
                    _ => {}
                }
            }
            match remaining {
                Some(r) => Asked::Page(rows, r),
                None => Asked::Refused,
            }
        }
        Err(_) => Asked::Offline,
    }
}

/// One ticket's summary, by `get` — a shortcode's root, an ancestor on a
/// deep link's chain.
async fn get_summary(id: &str) -> Option<TicketSummary> {
    let client = session::client()?;
    client.get_all(id.to_string()).await.ok()?.into_iter().find_map(|ev| match ev {
        TicketsEvent::Ticket { ticket } => Some(TicketSummary::from(ticket)),
        _ => None,
    })
}

/// Bring the selected row into view (block: nearest — no jump if it is
/// already visible). Called a tick after a route change so the `.sel`
/// class has been applied.
fn scroll_selected_into_view() -> bool {
    let Some(doc) = dom::window().document() else { return false };
    if let Ok(Some(el)) = doc.query_selector("#nav .row.sel") {
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
        return true;
    }
    false
}

/// The nav's scroller.
fn scroller() -> Option<web_sys::HtmlElement> {
    dom::window()
        .document()?
        .query_selector("#nav .tree-scroll")
        .ok()
        .flatten()?
        .dyn_into()
        .ok()
}

/// The treeitem one level up: the row of the `.node` wrapper whose
/// `.kids` container holds this row. Works at any depth, and answers
/// None for a top-level row (there is nothing to step out to).
fn parent_row(current: &web_sys::Element) -> Option<web_sys::HtmlElement> {
    let kids = current.closest(".kids").ok().flatten()?;
    let node = kids.parent_element()?;
    node.query_selector(":scope > .row")
        .ok()
        .flatten()
        .and_then(|r| r.dyn_into::<web_sys::HtmlElement>().ok())
}

thread_local! {
    /// SCALE-24: a step across a gap in flight — its direction, and how many
    /// more steps were pressed while it waited. One at a time: every press
    /// counts once, and a held key does not start a crossing per repeat.
    static CROSSING: Cell<Option<(bool, u32)>> = const { Cell::new(None) };
}

/// SCALE-24: a step from the row `from` into the gap beside it — a spacer,
/// or rows still being fetched. Scroll the least that brings the gap's near
/// edge (and a row of it) into view, so the window mounts and fetches what
/// is there; then focus the row that is next to `from` once it is a row.
/// Polled a frame at a time, as the board's j/k does past its window
/// (MFE-12), for as long as a fetch can reasonably take. A press while it
/// waits is one more step, taken when it lands ([`tree_keydown`]).
fn cross_gap(from: String, down: bool) {
    if let Some((dir, extra)) = CROSSING.with(Cell::get) {
        // One in flight: this press is a step more (or one less, against it).
        let extra = if dir == down { extra + 1 } else { extra.saturating_sub(1) };
        CROSSING.with(|c| c.set(Some((dir, extra))));
        return;
    }
    CROSSING.with(|c| c.set(Some((down, 0))));
    spawn_local(async move {
        let mut from = from;
        for _ in 0..240 {
            let els = walkable();
            let Some(i) = els.iter().position(|el| el.get_attribute("data-node-key").as_deref() == Some(&from)) else {
                break; // the row we started from is gone: nothing to step from
            };
            let next = if down { els.get(i + 1) } else { i.checked_sub(1).and_then(|j| els.get(j)) };
            let Some(next) = next else { break };
            if !is_gap(next) {
                keys::focus_and_rove(&tree_items(), next);
                // The steps pressed while this one waited, through rows
                // that are mounted, until another gap (or none are left).
                let mut extra = CROSSING.with(Cell::get).map_or(0, |(_, e)| e);
                let mut at = next.clone();
                while extra > 0 {
                    let els = walkable();
                    let Some(i) = els.iter().position(|el| el.is_same_node(Some(&at))) else { break };
                    let n = if down { els.get(i + 1) } else { i.checked_sub(1).and_then(|j| els.get(j)) };
                    match n {
                        Some(n) if !is_gap(n) => {
                            keys::focus_and_rove(&tree_items(), n);
                            at = n.clone();
                            extra -= 1;
                        }
                        _ => break,
                    }
                }
                if extra == 0 {
                    break;
                }
                // Another gap with steps still to take: cross it from here.
                CROSSING.with(|c| c.set(Some((down, extra - 1))));
                let Some(key) = at.get_attribute("data-node-key") else { break };
                from = key;
                continue;
            }
            if let Some(sc) = scroller() {
                let (s, g) = (sc.get_bounding_client_rect(), next.get_bounding_client_rect());
                let pitch = PITCH_GUESS;
                let delta = if down { g.top() + pitch - s.bottom() } else { g.bottom() - pitch - s.top() };
                if (down && delta > 0.5) || (!down && delta < -0.5) {
                    sc.set_scroll_top(sc.scroll_top() + delta.round() as i32);
                }
            }
            dom::sleep_ms(16).await;
        }
        CROSSING.with(|c| c.set(None));
    });
}

/// SCALE-24: End, when the tree ends in a gap — a windowed list whose last
/// chunk is not mounted. Scroll to the bottom and focus the last row once
/// it is a row.
fn to_the_end() {
    spawn_local(async move {
        for _ in 0..240 {
            let els = walkable();
            match els.last() {
                Some(last) if !is_gap(last) => {
                    keys::focus_and_rove(&tree_items(), last);
                    return;
                }
                None => return,
                Some(_) => {
                    if let Some(sc) = scroller() {
                        sc.set_scroll_top(sc.scroll_height());
                    }
                }
            }
            dom::sleep_ms(16).await;
        }
    });
}

/// The WAI-ARIA tree key map (AGS-18), one handler on the tree itself —
/// keystrokes arrive here having bubbled from whichever row has focus,
/// so there is no per-row listener and no second notion of "current".
///
/// Up/Down (j/k) walk the visible rows; Right (l) opens a closed node or
/// steps into an open one; Left (h) closes an open node or steps out to
/// its parent row; Enter opens the focused ticket (or toggles a group
/// row); Home/End jump. AGS-20: "node" is any row with children — an
/// epic's shortcode row or a ticket that decomposes — so the map is the
/// same at every depth. Everything else — `/`, `?`, `g h` — is left to
/// bubble to the global listener.
///
/// SCALE-24: the visible rows are the mounted ones, and a windowed list
/// mounts only those near the viewport. A step whose next row is a gap — a
/// spacer, or a row still being fetched — scrolls it into the window and
/// focuses it when it is mounted ([`cross_gap`]); End does the same for the
/// end of the tree.
fn tree_keydown(ev: web_sys::KeyboardEvent, expanded: RwSignal<BTreeSet<String>>) {
    if ev.default_prevented() || ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
        return;
    }
    let key = ev.key();
    // `g h` first, so the sequence still reaches the board from inside
    // the tree, where `h` otherwise means "collapse".
    if keys::consume_go_prefix(&key) {
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }
    let Some(target) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
        return;
    };
    let Ok(Some(current)) = target.closest("[role=treeitem]") else {
        return;
    };
    // Rove over every treeitem (a row hidden by a collapse must not keep
    // the tab stop), but walk only the visible ones — and the gaps between.
    let all = tree_items();
    let items = visible_items();
    if !items.iter().any(|el| el.is_same_node(Some(&current))) {
        return;
    }
    let steps = walkable();
    let Some(at) = steps.iter().position(|el| el.is_same_node(Some(&current))) else {
        return;
    };
    // A leaf is a ticket row (it carries the id); a row with children
    // carries `aria-expanded` — a ticket may be both.
    let leaf_id = current.get_attribute("data-nav-id");
    let node_key = current.get_attribute("data-node-key");
    let expandable = current.has_attribute("aria-expanded");
    let open = current.get_attribute("aria-expanded").as_deref() == Some("true");
    let toggle = |k: Option<String>| {
        if let Some(k) = k {
            expanded.update(|e| {
                if !e.remove(&k) {
                    e.insert(k);
                }
            });
        }
    };
    let step = |down: bool| {
        if CROSSING.with(Cell::get).is_some() {
            // a crossing is in flight: this press is counted there
            cross_gap(String::new(), down);
            return;
        }
        let next = if down { steps.get(at + 1) } else { at.checked_sub(1).and_then(|j| steps.get(j)) };
        match next {
            Some(el) if is_gap(el) => {
                if let Some(k) = node_key.clone() {
                    cross_gap(k, down);
                }
            }
            Some(el) => keys::focus_and_rove(&all, el),
            None => {}
        }
    };

    let handled = match key.as_str() {
        "ArrowDown" | "j" => {
            step(true);
            true
        }
        "ArrowUp" | "k" => {
            step(false);
            true
        }
        "Home" => {
            keys::focus_and_rove(&all, &items[0]);
            true
        }
        "End" => {
            match steps.last() {
                Some(last) if is_gap(last) => to_the_end(),
                Some(last) => keys::focus_and_rove(&all, last),
                None => {}
            }
            true
        }
        // Open a closed node in place; step into an open one. On a row
        // with no children there is nowhere further in — the pattern says
        // do nothing.
        "ArrowRight" | "l" => {
            if expandable {
                if !open {
                    toggle(node_key.clone());
                } else {
                    step(true);
                }
            }
            true
        }
        // Close an open node in place; otherwise step out to the row it
        // hangs under (which is where a second Left then closes it).
        "ArrowLeft" | "h" => {
            if expandable && open {
                toggle(node_key.clone());
            } else if let Some(row) = parent_row(&current) {
                keys::focus_and_rove(&all, &row);
            }
            true
        }
        "Enter" => {
            match &leaf_id {
                // The row is a real <a href>, so the browser would
                // navigate anyway; doing it here keeps one code path (and
                // one history entry) for every way a ticket is opened.
                Some(id) => crate::navigate_to_ticket(id),
                None => toggle(node_key.clone()),
            }
            true
        }
        _ => false,
    };
    if handled {
        ev.prevent_default();
        ev.stop_propagation();
    }
}

/// What every node in the tree needs, in one Copy bundle (signals and
/// memos are handles).
#[derive(Clone, Copy)]
struct Ctx {
    tree: Tree,
    route: RwSignal<Route>,
    expanded: RwSignal<BTreeSet<String>>,
}

/// SCALE-24: the chain from a deep link's ticket up to the top level, one
/// `get` per ancestor, as the nav draws it: each step is a node to open and
/// the child it must show — a ticket's parent, a shortcode's row for its
/// root, the host for a sub-shortcode's row. A shortcode with no root lists
/// its tickets flat, so it is the whole chain.
async fn chain_to(tree: Tree, leaf: &str) -> Option<Vec<(Node, Option<TicketSummary>)>> {
    let mut steps = Vec::new();
    let mut cur = get_summary(leaf).await?;
    for _ in 0..64 {
        let g = tree.groups.with_untracked(|m| m.as_ref().and_then(|m| m.get(&cur.epic)).cloned());
        match g {
            Some(g) if g.root.as_deref() == Some(cur.id.as_str()) => {
                steps.push((Node::Group(g.epic.clone()), Some(cur)));
                match g.host {
                    Some(h) => {
                        let host = get_summary(&h).await?;
                        steps.push((Node::Ticket(h), None));
                        cur = host;
                    }
                    None => return Some(steps),
                }
            }
            Some(g) if g.root.is_none() => {
                steps.push((Node::Group(g.epic.clone()), Some(cur)));
                return Some(steps);
            }
            _ => {
                let p = cur.parent.clone()?;
                let parent = get_summary(&p).await?;
                steps.push((Node::Ticket(p), Some(cur)));
                cur = parent;
            }
        }
    }
    None
}

/// AGS-20's auto-expand, from the server (SCALE-24): open every node on the
/// way to `leaf`, show `leaf` in each windowed list on the way, and bring
/// the selected row into view once it is mounted.
async fn reveal(ctx: Ctx, leaf: String, gen: u32) {
    let Some(steps) = chain_to(ctx.tree, &leaf).await else { return };
    if REVEAL_GEN.with(Cell::get) != gen {
        return;
    }
    let keys: Vec<String> = steps.iter().map(|(n, _)| n.key().to_string()).collect();
    let missing: Vec<String> = ctx.expanded.with_untracked(|e| keys.into_iter().filter(|k| !e.contains(k)).collect());
    if !missing.is_empty() {
        ctx.expanded.update(|e| e.extend(missing));
    }
    ctx.tree.reveal.set(Some(Reveal { gen, steps }));
    for _ in 0..300 {
        dom::sleep_ms(16).await;
        if REVEAL_GEN.with(Cell::get) != gen {
            return;
        }
        let sel = dom::window()
            .document()
            .and_then(|d| d.query_selector(&format!("#nav .row.sel[data-nav-id=\"{leaf}\"]")).ok().flatten());
        if sel.is_some() {
            scroll_selected_into_view();
            return;
        }
    }
}

/// The left panel: brand header + the tree. Always mounted; the App hides
/// it with a class so its DOM (and every row's identity) survives a
/// hide/reveal.
#[component]
pub(crate) fn Nav(tree: Tree, route: RwSignal<Route>, hidden: RwSignal<bool>) -> impl IntoView {
    let expanded = RwSignal::new(load_expanded());
    Effect::new(move |_| expanded.with(save_expanded));
    let ctx = Ctx { tree, route, expanded };
    let top = tree.top;
    let top_last = Memo::new(move |_| top.with(|t| t.last().cloned()));

    // Selection follows the route. The chain holding the open ticket
    // auto-expands so the selected row is visible — once per navigation
    // (`revealed` remembers which id was handled), so a later live insert
    // does not fight a node the user then chose to close. SCALE-24: the
    // chain comes from the server (`get` per ancestor), so it waits for the
    // first groups — not for any walk.
    let revealed = StoredValue::new(None::<String>);
    Effect::new(move |_| {
        let Route::Ticket(id) = route.get() else { return };
        if tree.epoch.get() == 0 {
            return; // no groups yet: this effect re-runs when there are
        }
        if revealed.get_value().as_deref() == Some(id.as_str()) {
            return;
        }
        revealed.set_value(Some(id.clone()));
        let gen = REVEAL_GEN.with(|g| {
            g.set(g.get().wrapping_add(1));
            g.get()
        });
        spawn_local(reveal(ctx, id, gen));
    });

    // AGS-18: keep exactly one row as the tree's tab stop as the tree
    // changes shape (rows arrive, a node opens, the route moves). The
    // work is a DOM read plus at most two attribute writes, a tick after
    // the render that changed things. SCALE-24: and every windowed list
    // measures again — opening a node moves everything below it.
    Effect::new(move |_| {
        top.track();
        expanded.with(|e| e.len());
        let _ = route.get();
        spawn_local(async move {
            dom::sleep_ms(0).await;
            ensure_tree_tab_stop();
            tree.viewport.notify();
        });
    });
    let _ = window_event_listener(leptos::ev::resize, move |_| tree.viewport.notify());
    let tree_ref = NodeRef::<leptos::html::Div>::new();
    tree_ref.on_load(|el: web_sys::HtmlDivElement| keep_focus(&el));

    view! {
        <aside id="nav" class="nav">
            <div class="brand">
                "menger"
                <span class="muted">"· tickets"</span>
                <button
                    class="nav-hide"
                    title="hide the nav"
                    on:click=move |_| hidden.set(true)
                >
                    "‹"
                </button>
            </div>
            <div class="tree-scroll" on:scroll=move |_| tree.viewport.notify()>
                <div
                    class="tree"
                    role="tree"
                    aria-label="tickets"
                    node_ref=tree_ref
                    on:keydown=move |ev| tree_keydown(ev, expanded)
                >
                    <For
                        each=move || top.get()
                        key=|g| g.clone()
                        children=move |g: String| {
                            node_view(Node::Group(g), ctx, Arc::new(Vec::new()), top_last, None)
                        }
                    />
                </div>
            </div>
        </aside>
    }
}

/// The `is last child` flags of a node's ancestors, outermost first —
/// what turns a depth into the tree's connector art (`│   ` for an
/// ancestor with siblings still to come, four spaces for one that was
/// last). Each flag is a memo, so an insert repaints only the rows whose
/// answer actually changed.
type Ancestry = Arc<Vec<Memo<bool>>>;

fn connectors(anc: &Ancestry, last: bool) -> String {
    let mut s = String::new();
    for a in anc.iter() {
        s.push_str(if a.get() { "    " } else { "│   " });
    }
    s.push_str(if last { "└── " } else { "├── " });
    s
}

/// A child's place in a list: the list, and whether it is windowed (then
/// the row says its position among all of its siblings).
#[derive(Clone, Copy)]
struct Pos {
    list: RwSignal<List>,
    windowed: Memo<bool>,
}

/// One node of the tree — an epic's shortcode row or a ticket row — and,
/// beneath it, its children, recursively.
///
/// MFE-12: the children exist only while the node is open — collapse
/// unmounts them rather than hiding them, so a closed node costs its own
/// row and an empty container, whatever it holds. SCALE-24: and a closed
/// node asks the server nothing; what it shows of its children is its
/// row's `child_count` (a ticket) or its roll-up (a shortcode).
fn node_view(node: Node, ctx: Ctx, ancestry: Ancestry, last: Memo<Option<String>>, pos: Option<Pos>) -> AnyView {
    let key = node.key().to_string();
    let is_group = matches!(node, Node::Group(_));
    let tree = ctx.tree;

    // A ticket's row, from the row cache — held by the list (or the
    // shortcode row) that put it here. A handle in this node's owner.
    let ticket = match &node {
        Node::Ticket(id) => match cache::get(id) {
            Some(sig) => Some(RwSignal::from(sig)),
            None => return ().into_any(),
        },
        Node::Group(_) => None,
    };

    let mine_last = {
        let k = key.clone();
        Memo::new(move |_| last.with(|l| l.as_deref() == Some(k.as_str())))
    };
    let open = {
        let k = key.clone();
        let expanded = ctx.expanded;
        Memo::new(move |_| expanded.with(|e| e.contains(&k)))
    };
    let has_kids = {
        let k = key.clone();
        Memo::new(move |_| match ticket {
            Some(t) => t.with(|t| t.child_count > 0) || tree.hosted.with(|h| h.contains_key(&k)),
            None => tree.groups.with(|m| m.as_ref().and_then(|m| m.get(&k)).is_some_and(|g| g.total > 0)),
        })
    };
    let child_ancestry: Ancestry = {
        let mut v = (*ancestry).clone();
        v.push(mine_last);
        Arc::new(v)
    };

    let prefix = {
        let anc = ancestry.clone();
        move || connectors(&anc, mine_last.get())
    };
    let toggle_glyph = move || {
        if !has_kids.get() {
            " "
        } else if open.get() {
            "▾"
        } else {
            "▸"
        }
    };
    let toggle = {
        let k = key.clone();
        let expanded = ctx.expanded;
        move || {
            expanded.update(|e| {
                if !e.remove(&k) {
                    e.insert(k.clone());
                }
            })
        }
    };
    // A row with no children is a leaf of the a11y tree: no
    // `aria-expanded`, nothing owned.
    let expanded_attr = move || has_kids.get().then(|| open.get().to_string());
    let kids_id = format!("nav-kids-{key}");
    let owns_attr = {
        let id = kids_id.clone();
        move || has_kids.get().then(|| id.clone())
    };
    let index = {
        let k = key.clone();
        Memo::new(move |_| pos.and_then(|p| p.list.with(|l| l.index_of(&k))))
    };

    // Only one of these runs, so each may take the row's closures by
    // value; the two row shapes are otherwise unrelated.
    let row = match ticket {
        None => group_row(key.clone(), ctx, prefix, toggle_glyph, toggle, expanded_attr, owns_attr),
        Some(t) => leaf_row(key.clone(), t, ctx, pos, index, prefix, toggle_glyph, toggle, expanded_attr, owns_attr),
    };

    let kids_ref = NodeRef::<leptos::html::Div>::new();
    view! {
        <div
            class="node"
            class:group=is_group
            role="none"
            data-node-key=key.clone()
            data-group=is_group.then(|| key.clone())
            data-index=move || index.get()
            class:collapsed=move || !open.get()
        >
            {row}
            {move || {
                has_kids
                    .get()
                    .then(|| {
                        let child_ancestry = child_ancestry.clone();
                        let node = node.clone();
                        view! {
                            <div class="kids" role="group" id=kids_id.clone() node_ref=kids_ref>
                                {move || {
                                    open.get().then(|| children_of(node.clone(), ctx, child_ancestry.clone(), kids_ref, ticket))
                                }}
                            </div>
                        }
                    })
            }}
        </div>
    }
    .into_any()
}

/// What an open node shows beneath it. A shortcode: its root (or, with no
/// root, its tickets by page). A ticket: its children by page, then the
/// sub-shortcodes it hosts.
fn children_of(
    node: Node,
    ctx: Ctx,
    ancestry: Ancestry,
    kids_ref: NodeRef<leptos::html::Div>,
    ticket: Option<RwSignal<TicketSummary>>,
) -> AnyView {
    let tree = ctx.tree;
    match node {
        Node::Group(epic) => {
            let root = {
                let epic = epic.clone();
                Memo::new(move |_| tree.group(&epic).and_then(|g| g.root))
            };
            (move || match root.get() {
                Some(r) => root_view(r, ctx, ancestry.clone()),
                None => list_view(Source::Epic(epic.clone()), None, ctx, ancestry.clone(), kids_ref, None),
            })
            .into_any()
        }
        Node::Ticket(id) => {
            let count = ticket.map(|t| Memo::new(move |_| t.with(|t| t.child_count as usize)));
            let hosted = {
                let id = id.clone();
                Memo::new(move |_| tree.hosted.with(|h| h.get(&id).cloned().unwrap_or_default()))
            };
            list_view(Source::Parent(id), count, ctx, ancestry, kids_ref, Some(hosted))
        }
    }
}

/// A shortcode row's one child, its root: from the row cache if some view
/// holds it, else one `get` — held while the row is open, and asked again
/// on every reconnect so what it shows is not stale.
fn root_view(root: String, ctx: Ctx, ancestry: Ancestry) -> AnyView {
    let tree = ctx.tree;
    let holder = cache::Holder::new();
    let ready = RwSignal::new(holder.hold_id(&root));
    let asked = StoredValue::new(0u32);
    {
        let (root, holder) = (root.clone(), holder.clone());
        Effect::new(move |_| {
            let epoch = tree.epoch.get();
            if epoch == 0 || asked.get_value() == epoch {
                return;
            }
            asked.set_value(epoch);
            let (root, holder) = (root.clone(), holder.clone());
            spawn_local(async move {
                if let Some(t) = get_summary(&root).await {
                    if holder.hold(t).is_some() {
                        ready.try_set(true);
                    }
                }
            });
        });
    }
    let last = {
        let r = root.clone();
        Memo::new(move |_| Some(r.clone()))
    };
    (move || ready.get().then(|| node_view(Node::Ticket(root.clone()), ctx, ancestry.clone(), last, None))).into_any()
}

/// One slot of a mounted chunk: a known child, or a row still being fetched.
#[derive(Clone, Debug, PartialEq)]
enum Slot {
    Child(String),
    Pending(usize),
}

/// A node's children, by page — and, past one page, a window over them
/// ([`span`]). `count` is the node's `child_count` (a ticket's list),
/// followed live; `hosted` the sub-shortcodes drawn after the children.
fn list_view(
    source: Source,
    count: Option<Memo<usize>>,
    ctx: Ctx,
    ancestry: Ancestry,
    kids_ref: NodeRef<leptos::html::Div>,
    hosted: Option<Memo<Vec<String>>>,
) -> AnyView {
    let tree = ctx.tree;
    let list = RwSignal::new(List::of(count.map_or(0, |c| c.get_untracked())));
    let holder = cache::Holder::new();
    OPEN.with(|o| o.borrow_mut().push((source.clone(), list, holder.clone())));
    on_cleanup(move || OPEN.with(|o| o.borrow_mut().retain(|(_, l, _)| *l != list)));

    // The node's own count, as `watch` moves it on its row.
    if let Some(count) = count {
        Effect::new(move |_| {
            let c = count.get();
            if list.with_untracked(|l| l.n != c) {
                list.update(|l| l.resize(c));
            }
        });
    }

    let windowed = Memo::new(move |_| list.with(|l| l.n > WHOLE));
    let geom = RwSignal::new(Geom { above: 0.0, viewport: 900.0, pitch: PITCH_GUESS });
    // The mounted chunks' measured heights, stamped with the list's `moved`
    // when they were taken: once known indices move, a chunk is not the
    // children it was measured with, and its height is the estimate again.
    let measured = RwSignal::new(Heights::default());
    let window = Memo::new(move |_| {
        let (n, moved) = list.with(|l| (l.n, l.moved));
        measured.with(|h| span(n, geom.get(), h.of(moved)))
    });
    let range = Memo::new(move |_| {
        let s = window.get();
        let n = list.with(|l| l.n);
        ((s.start * CHUNK).min(n), (s.end * CHUNK).min(n))
    });

    // Where the list is, against the nav's viewport — on every scroll, and
    // whenever anything in the nav may have moved.
    let measure = move || {
        let Some(Some(el)) = kids_ref.try_get_untracked() else { return };
        let Some(sc) = scroller() else { return };
        if let Some(pitch) = measured_pitch(&el) {
            if tree.pitch.get_untracked() != pitch {
                tree.pitch.set(pitch);
            }
        }
        let (s, k) = (sc.get_bounding_client_rect(), el.get_bounding_client_rect());
        let g = Geom { above: s.top() - k.top(), viewport: f64::from(sc.client_height()), pitch: tree.pitch.get_untracked() };
        if geom.try_get_untracked() != Some(g) {
            geom.try_set(g);
        }
    };
    Effect::new(move |_| {
        tree.viewport.track();
        measure();
    });
    // After each render: the mounted chunks' heights, and a word to the
    // other lists — this one may have changed height.
    Effect::new(move |_| {
        window.track();
        list.track();
        spawn_local(async move {
            dom::sleep_ms(0).await;
            if windowed.try_get_untracked() == Some(true) {
                if let (Some(Some(el)), Some(s), Some(moved)) =
                    (kids_ref.try_get_untracked(), window.try_get_untracked(), list.try_with_untracked(|l| l.moved))
                {
                    let got = chunk_heights(&el, s);
                    let changed = measured.try_with_untracked(|h| h.differs(moved, &got)).unwrap_or(false);
                    if changed {
                        measured.try_update(|h| h.take(moved, got));
                    }
                }
            }
            tree.viewport.notify();
        });
    });

    // What the window shows that the list does not have — unknown ids, or
    // rows it no longer holds — asked for a page at a time, one page in
    // flight; and what is far from it, given back.
    let busy = StoredValue::new(false);
    let seen = StoredValue::new(0u32);
    // An answer that changed nothing re-runs nothing — except when the
    // connection moved on while it was in flight (a reconnect, a `lagged`):
    // then the list asks again, on the new one.
    let retry = Trigger::new();
    // Asks in a row that told the list nothing, for this window: a list
    // whose known ids disagree with the server cannot spin on one gap.
    let strikes = StoredValue::new(((0usize, 0usize), 0u8));
    {
        let (source, holder) = (source.clone(), holder.clone());
        Effect::new(move |_| {
            let epoch = tree.epoch.get();
            let (lo, hi) = range.get();
            list.track();
            retry.track();
            if epoch == 0 || busy.get_value() {
                return;
            }
            if strikes.with_value(|(r, n)| *r == (lo, hi) && *n >= 3) {
                return; // until the window, or the connection, moves
            }
            // A reconnect or a `lagged` since this list last asked: what it
            // shows is asked for again, from its first row, in one page.
            let stale = seen.get_value() != 0 && seen.get_value() != epoch;
            let ask = list.with_untracked(|l| {
                if stale {
                    let a = if lo == 0 { Ask::top() } else { l.ask_for(lo) };
                    return Some((a, (hi - lo).max(PAGE), true));
                }
                if !l.loaded {
                    return Some((Ask::top(), PAGE, false));
                }
                let missing = (lo..hi).find(|&i| l.id_at(i).is_none_or(|id| !holder.holds(id)))?;
                Some((l.ask_for(missing), PAGE, false))
            });
            seen.set_value(epoch);
            let Some((ask, limit, reset)) = ask else {
                // Everything shown is in hand: let go of what is far away.
                let keep = (lo.saturating_sub(KEEP_CHUNKS * CHUNK), hi + KEEP_CHUNKS * CHUNK);
                list.with_untracked(|l| {
                    holder.retain(|id| l.index_of(id).is_some_and(|i| i >= keep.0 && i < keep.1));
                });
                return;
            };
            busy.set_value(true);
            let (source, holder) = (source.clone(), holder.clone());
            spawn_local(async move {
                let answer = ask_page(&source, ask.after.clone(), limit).await;
                busy.try_set_value(false);
                if tree.epoch.try_get_untracked().is_some_and(|e| e != epoch) {
                    retry.notify(); // a no-op once the list is gone
                }
                match answer {
                    Asked::Page(rows, remaining) => {
                        let ids: Vec<String> = rows.iter().map(|t| t.id.clone()).collect();
                        // Rows it did not hold are news too: a scroll back
                        // over children it had let go of.
                        let newly = ids.iter().any(|id| !holder.holds(id));
                        if holder.hold_page(rows) {
                            let moved = list
                                .try_update(|l| l.answered(&ask, ids, remaining, reset))
                                .unwrap_or(false);
                            strikes.try_update_value(|(r, n)| {
                                if *r != (lo, hi) || moved || newly {
                                    *r = (lo, hi);
                                    *n = 0;
                                } else {
                                    *n += 1;
                                }
                            });
                        }
                    }
                    // A guess that named no ticket: ask from what is known.
                    Asked::Refused => {
                        list.try_update(List::stall);
                        strikes.try_update_value(|(r, n)| {
                            if *r != (lo, hi) {
                                *r = (lo, hi);
                                *n = 0;
                            }
                            *n += 1;
                        });
                    }
                    Asked::Offline => {}
                }
            });
        });
    }

    // A deep link through this list: ask for the page after the child it
    // must show — its `remaining` says where the child is — and scroll there.
    {
        let (source, holder) = (source.clone(), holder.clone());
        let me = match &source {
            Source::Parent(p) => Node::Ticket(p.clone()),
            Source::Epic(e) => Node::Group(e.clone()),
        };
        let done = StoredValue::new(0u32);
        Effect::new(move |_| {
            let Some(r) = tree.reveal.get() else { return };
            let Some(child) = r.steps.iter().find(|(c, _)| *c == me).and_then(|(_, child)| child.clone()) else { return };
            if done.get_value() == r.gen || !windowed.get() {
                return;
            }
            done.set_value(r.gen);
            holder.hold(child.clone());
            let (source, holder) = (source.clone(), holder.clone());
            spawn_local(async move {
                let known = list.with_untracked(|l| l.index_of(&child.id));
                let i = match known {
                    Some(i) => i,
                    None => {
                        let Asked::Page(rows, remaining) = ask_page(&source, Some(child.id.clone()), PAGE).await else { return };
                        let ids: Vec<String> = rows.iter().map(|t| t.id.clone()).collect();
                        let n = list.with_untracked(|l| l.n);
                        let i = n.saturating_sub(remaining + ids.len() + 1);
                        if !holder.hold_page(rows) {
                            return;
                        }
                        list.try_update(|l| {
                            l.place(i, vec![child.id.clone()]);
                            l.place(i + 1, ids);
                        });
                        i
                    }
                };
                dom::sleep_ms(0).await;
                center(kids_ref, list, measured, tree, i);
            });
        });
    }

    // The children the window mounts, in order; a hosted sub-shortcode's
    // root is drawn as its shortcode's row, after the children.
    let slots = Memo::new(move |_| {
        let (lo, hi) = range.get();
        let hidden = tree.hosted_roots.get();
        list.with(|l| {
            (lo..hi)
                .filter_map(|i| match l.id_at(i) {
                    Some(id) if hidden.contains(id) => None,
                    Some(id) if holder.holds(id) => Some(Slot::Child(id.to_string())),
                    _ => Some(Slot::Pending(i)),
                })
                .collect::<Vec<_>>()
        })
    });
    let hosted = hosted.unwrap_or_else(|| Memo::new(|_| Vec::new()));
    let last_kid = Memo::new(move |_| {
        hosted
            .with(|h| h.last().cloned())
            .or_else(|| list.with(|l| l.n.checked_sub(1).and_then(|i| l.id_at(i)).map(str::to_string)))
    });
    let pos = Pos { list, windowed };
    let spacer = move |px: fn(&Span) -> f64| {
        let present = Memo::new(move |_| window.with(|s| px(s) > 0.0));
        move || {
            present.get().then(|| {
                view! {
                    <div class="spacer gap" role="none" aria-hidden="true" style:height=move || format!("{}px", window.with(px))></div>
                }
            })
        }
    };
    let pending_prefix = ancestry.clone();
    let child_ancestry = ancestry.clone();
    let hosted_ancestry = ancestry;
    view! {
        {spacer(|s| s.top_px)}
        <For
            each=move || slots.get()
            key=|s: &Slot| match s {
                Slot::Child(id) => id.clone(),
                Slot::Pending(i) => format!("?{i}"),
            }
            children=move |s: Slot| match s {
                Slot::Child(id) => node_view(Node::Ticket(id), ctx, child_ancestry.clone(), last_kid, Some(pos)),
                Slot::Pending(i) => pending_view(i, pending_prefix.clone()),
            }
        />
        {spacer(|s| s.bottom_px)}
        <For
            each=move || hosted.get()
            key=|g| g.clone()
            children=move |g: String| node_view(Node::Group(g), ctx, hosted_ancestry.clone(), last_kid, None)
        />
    }
    .into_any()
}

/// A child the window has reached before its row has come: a line of the
/// row's height, marked as a gap for the keyboard, named nothing for
/// assistive technology.
fn pending_view(i: usize, ancestry: Ancestry) -> AnyView {
    view! {
        <div class="node pending gap" role="none" data-index=i>
            <div class="row pending" aria-hidden="true">
                <span class="prefix">{move || connectors(&ancestry, false)}</span>
                <span class="toggle">" "</span>
                <span class="toggle kind">"·"</span>
                <span class="label">"…"</span>
            </div>
        </div>
    }
    .into_any()
}

/// Scroll the nav so child `i` of a windowed list sits mid-viewport — a deep
/// link's ticket, among however many siblings. The window follows the
/// scroll and mounts it.
fn center(kids_ref: NodeRef<leptos::html::Div>, list: RwSignal<List>, measured: RwSignal<Heights>, tree: Tree, i: usize) {
    let Some(Some(el)) = kids_ref.try_get_untracked() else { return };
    let Some(sc) = scroller() else { return };
    let Some((n, moved)) = list.try_with_untracked(|l| (l.n, l.moved)) else { return };
    let pitch = tree.pitch.get_untracked();
    let y = measured.try_with_untracked(|h| offset_of(i, n, pitch, h.of(moved))).unwrap_or(0.0);
    let (s, k) = (sc.get_bounding_client_rect(), el.get_bounding_client_rect());
    let delta = k.top() + y - s.top() - f64::from(sc.client_height()) / 2.0;
    sc.set_scroll_top(sc.scroll_top() + delta.round() as i32);
    tree.viewport.notify();
}

/// A nav row's height (px), read as the board reads its own (MFE-12): the
/// computed height of a mounted row, through the page's own parseFloat.
fn measured_pitch(within: &web_sys::Element) -> Option<f64> {
    let row = within.query_selector(".row").ok().flatten()?;
    let height = dom::window().get_computed_style(&row).ok()??.get_property_value("height").ok()?;
    let pitch = js_sys::parse_float(&height);
    (pitch > 1.0).then_some(pitch)
}

/// The mounted chunks' heights (px), from where each chunk's first child
/// starts to where the next one does — so a chunk holding an opened child
/// keeps its real height when it is unmounted.
fn chunk_heights(kids: &web_sys::Element, s: Span) -> Vec<(usize, f64)> {
    let Ok(nodes) = kids.query_selector_all(":scope > [data-index]") else { return Vec::new() };
    let mut tops: BTreeMap<usize, f64> = BTreeMap::new();
    let mut end = None;
    for j in 0..nodes.length() {
        let Some(el) = nodes.get(j).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        let Some(i) = el.get_attribute("data-index").and_then(|v| v.parse::<usize>().ok()) else { continue };
        let r = el.get_bounding_client_rect();
        tops.entry(i / CHUNK).or_insert(r.top());
        end = Some(r.bottom());
    }
    let Some(end) = end else { return Vec::new() };
    let ks: Vec<usize> = tops.keys().copied().filter(|k| *k >= s.start && *k < s.end).collect();
    ks.iter()
        .enumerate()
        .map(|(j, k)| {
            let next = ks.get(j + 1).map_or(end, |n| tops[n]);
            (*k, next - tops[k])
        })
        .collect()
}

/// An epic's shortcode row: the code chip and a `done/total` rollup over
/// its whole subtree (AGS-20 — descendants included, at every depth and
/// across epics). SCALE-24: the server's roll-up (MNG-10), moved in place by
/// `group_changed` — nothing here counts.
fn group_row(
    key: String,
    ctx: Ctx,
    prefix: impl Fn() -> String + Send + Sync + 'static,
    toggle_glyph: impl Fn() -> &'static str + Send + Sync + 'static,
    toggle: impl Fn() + Send + Sync + 'static,
    expanded_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
    owns_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> AnyView {
    let rollup = {
        let k = key.clone();
        let tree = ctx.tree;
        Memo::new(move |_| tree.group(&k).map(|g| format!("{}/{}", g.done, g.total)).unwrap_or_default())
    };
    let code = key.clone();
    view! {
        <div
            class="row grouprow"
            role="treeitem"
            tabindex="-1"
            data-node-key=key
            aria-expanded=expanded_attr
            aria-owns=owns_attr
            on:click=move |_| toggle()
        >
            <span class="prefix">{prefix}</span>
            <span class="toggle clickable">{toggle_glyph}</span>
            <span class="toggle mark" aria-hidden="true">" "</span>
            <span class="code">{code}</span>
            <span class="tag roll">{move || rollup.get()}</span>
        </div>
    }
    .into_any()
}

/// One ticket row: a real link (middle-click / copy-link friendly) whose
/// title and status tag render through the row's inner signal. Selected
/// iff the route is this ticket's page. Carries `data-ticket-id`, the
/// AGS-16 menu's attachment (a right-click anywhere on the row opens it).
/// A ticket with children also carries the collapse toggle — clicking it
/// opens the row instead of following the link. SCALE-24: in a windowed
/// list it says where it is among all of its siblings.
#[allow(clippy::too_many_arguments)]
fn leaf_row(
    id: String,
    ticket: RwSignal<TicketSummary>,
    ctx: Ctx,
    pos: Option<Pos>,
    index: Memo<Option<usize>>,
    prefix: impl Fn() -> String + Send + Sync + 'static,
    toggle_glyph: impl Fn() -> &'static str + Send + Sync + 'static,
    toggle: impl Fn() + Send + Sync + 'static,
    expanded_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
    owns_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> AnyView {
    let route = ctx.route;
    let sel = {
        let id = id.clone();
        Memo::new(move |_| route.with(|rt| matches!(rt, Route::Ticket(x) if *x == id)))
    };
    let href = format!("#/ticket/{id}");
    let tag_class = move || format!("tag {}", ticket.with(|t| st_class(t.status.as_str())));
    let kind = move || ticket.with(|t| t.kind.to_string());
    let windowed = move || pos.is_some_and(|p| p.windowed.get());
    let setsize = move || windowed().then(|| pos.map_or(0, |p| p.list.with(|l| l.n)));
    let posinset = move || windowed().then(|| index.get().map(|i| i + 1)).flatten();

    view! {
        <a
            class="row leaf"
            class:sel=move || sel.get()
            role="treeitem"
            tabindex="-1"
            aria-selected=move || sel.get().to_string()
            aria-expanded=expanded_attr
            aria-owns=owns_attr
            aria-setsize=setsize
            aria-posinset=posinset
            data-node-key=id.clone()
            data-nav-id=id.clone()
            data-ticket-id=id.clone()
            data-kind=kind
            href=href
        >
            <span class="prefix">{prefix}</span>
            <span
                class="toggle clickable"
                on:click=move |ev: web_sys::MouseEvent| {
                    ev.prevent_default();
                    ev.stop_propagation();
                    toggle();
                }
            >
                {toggle_glyph}
            </span>
            <span class="toggle kind" title=kind>{move || kind_glyph(&kind())}</span>
            <span class="code">{id.clone()}</span>
            <span class="label">{move || ticket.with(|t| t.title.clone())}</span>
            <span class=tag_class>{move || ticket.with(|t| t.status.to_string())}</span>
        </a>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::{chunks, kind_glyph, offset_of, span, Ask, Geom, List, CHUNK, PAGE, WHOLE};
    use std::collections::BTreeMap;

    /// The five kinds each get their own glyph; the neutral dot is
    /// reserved for whatever the table does not know.
    #[test]
    fn every_kind_has_a_distinct_glyph_and_unknown_is_the_neutral_dot() {
        let kinds = ["scope", "execution", "build", "gap", "spike"];
        let glyphs: std::collections::BTreeSet<&str> = kinds.iter().map(|k| kind_glyph(k)).collect();
        assert_eq!(glyphs.len(), kinds.len(), "distinct: {glyphs:?}");
        assert!(!glyphs.contains("·"));
        assert_eq!(kind_glyph("bug"), "·");
        assert_eq!(kind_glyph(""), "·");
        // One character each: the marker slot is 1ch wide.
        for k in kinds {
            assert_eq!(kind_glyph(k).chars().count(), 1, "{k}");
        }
    }

    fn ids(epic: &str, seqs: std::ops::Range<u32>) -> Vec<String> {
        seqs.map(|s| format!("{epic}-{s}")).collect()
    }

    /// The synthetic store's shape (MNG-14): a root `MAP-1` whose children
    /// are `MAP-2` … — child `i` is `MAP-(i + 2)`.
    fn map(i: usize) -> String {
        format!("MAP-{}", i + 2)
    }

    /// A page from the top places at 0 and says how many there are; the
    /// next is asked after its last child and lands right behind it.
    #[test]
    fn pages_from_the_top_and_after_a_known_child_are_placed_exactly() {
        let mut l = List::of(0);
        assert_eq!(l.ask_for(0), Ask { after: None, start: Some(0) });
        let ask = Ask { after: None, start: Some(0) };
        assert!(l.answered(&ask, ids("MAP", 2..202), 163_330, false));
        assert_eq!(l.n, 163_530, "the first page counts the list");
        assert_eq!(l.id_at(0), Some("MAP-2"));
        assert_eq!(l.index_of("MAP-201"), Some(199));
        let next = l.ask_for(200);
        assert_eq!(next, Ask { after: Some("MAP-201".into()), start: Some(200) });
        assert!(l.answered(&next, ids("MAP", 202..402), 163_130, false));
        assert_eq!(l.id_at(399), Some("MAP-401"));
        assert!(l.gaps(0, 400).is_empty());
        assert_eq!(l.gaps(350, 500), vec![(400, 500)]);
    }

    /// Far from anything known, the cursor is worked out from the ids: the
    /// tail by stepping on from the last known sequence, a stretch between
    /// two known children by interpolating, the head by stepping back — and
    /// the answer, placed by `remaining`, lands exactly where it was wanted
    /// in a dense shortcode.
    #[test]
    fn a_far_page_is_asked_from_a_position_worked_out_from_the_ids() {
        let n = 163_530;
        let mut l = List::of(n);
        l.answered(&Ask::top(), (0..PAGE).map(map).collect(), n - PAGE, false);
        // the end of the list: the child before index n - 80 is MAP-(n - 81 + 2)
        let tail = l.ask_for(n - 80);
        assert_eq!(tail, Ask { after: Some(map(n - 81)), start: None });
        assert!(l.answered(&tail, (n - 80..n).map(map).collect(), 0, false));
        assert_eq!(l.id_at(n - 1), Some(map(n - 1).as_str()), "the last child, where it belongs");
        // between the two: interpolated
        let mid = l.ask_for(80_000);
        assert_eq!(mid, Ask { after: Some(map(79_999)), start: None });
        assert!(l.answered(&mid, (80_000..80_200).map(map).collect(), n - 80_200, false));
        assert_eq!(l.index_of(&map(80_000)), Some(80_000));
        // before a known child, with nothing known nearer: stepped back from it
        let mut d = List::of(n);
        d.place(81_763, vec![map(81_763)]);
        let back = d.ask_for(81_700);
        assert_eq!(back, Ask { after: Some(map(81_699)), start: None });
        // a guess that would fall before the shortcode's first is the top
        let mut e = List::of(n);
        e.place(150, vec![map(150)]);
        assert_eq!(e.ask_for(120), Ask { after: Some(map(119)), start: None });
        assert_eq!(e.ask_for(1), Ask { after: Some(map(0)), start: None });
    }

    /// A guess that brings nothing new stalls the list, and the next ask is
    /// after the known child before the gap — which always brings something.
    #[test]
    fn a_guess_that_lands_nowhere_new_falls_back_to_paging_on() {
        let n = 1_000;
        let mut l = List::of(n);
        l.answered(&Ask::top(), ids("SPR", 2..202), n - 200, false);
        l.place(900, ids("SPR", 5_000..5_100));
        let guess = l.ask_for(500);
        assert_eq!(guess.start, None, "a position, not a child");
        // the server answers with children already known (nothing between)
        assert!(!l.answered(&guess, ids("SPR", 5_000..5_100), 0, false));
        assert_eq!(l.ask_for(500), Ask { after: Some("SPR-201".into()), start: Some(200) });
        // a refused guess stalls the same way
        let mut r = List::of(n);
        r.answered(&Ask::top(), ids("SPR", 2..202), n - 200, false);
        r.stalled = true;
        assert_eq!(r.ask_for(700).after.as_deref(), Some("SPR-201"));
    }

    /// `children_changed` at the end of a list known to its end appends; in
    /// the middle of known children, inserts and moves the rest down; inside
    /// an unknown stretch, only moves what is known after it. A child moved
    /// away leaves, and what was after it moves up.
    #[test]
    fn births_and_removals_move_what_is_known() {
        let mut l = List::of(0);
        l.answered(&Ask::top(), ids("PAR", 2..6), 0, false); // PAR-2..PAR-5, whole
        assert!(l.added("PAR-6", 5));
        assert_eq!((l.n, l.index_of("PAR-6")), (5, Some(4)), "appended");
        assert!(l.added("AAA-1", 6));
        assert_eq!(l.index_of("AAA-1"), Some(0), "sorts first: inserted at the head");
        assert_eq!(l.index_of("PAR-2"), Some(1));
        assert!(l.removed("PAR-3", 5));
        assert_eq!(l.index_of("PAR-4"), Some(2), "moved up");
        assert_eq!(l.n, 5);
        assert!(!l.added("PAR-4", 5), "known already");

        // known [0, 200) and [900, 950) of 1,000; a birth between them
        let mut w = List::of(1_000);
        w.answered(&Ask::top(), ids("MAP", 2..202), 800, false);
        w.place(900, ids("MAP", 1_000..1_050));
        let moved = w.moved;
        assert!(w.added("MAP-500", 1_001));
        assert_eq!(w.index_of("MAP-500"), None, "inside an unknown stretch: a page brings it");
        assert_eq!(w.index_of("MAP-1000"), Some(901), "what is known after it moved down");
        assert_eq!(w.index_of("MAP-201"), Some(199), "before it did not");
        assert_ne!(w.moved, moved);
        // a birth past the known tail of a list not known to its end
        assert!(w.added("MAP-2000", 1_002));
        assert_eq!(w.index_of("MAP-2000"), None);
        // …and one known to its end takes it as its last
        let mut whole = List::of(0);
        whole.answered(&Ask::top(), ids("MAP", 2..302), 0, false);
        assert!(whole.added("MAP-302", 301));
        assert_eq!(whole.index_of("MAP-302"), Some(300));
        // a removal inside the unknown stretch moves what is after it up
        assert!(!w.removed("MAP-600", 1_001));
        assert_eq!(w.index_of("MAP-1000"), Some(900));
    }

    /// A page that overlaps known children is pinned to where they are,
    /// replaces what was known over its range, and takes its ids from
    /// wherever else they were known; a reset forgets everything else.
    #[test]
    fn a_page_replaces_its_range_and_a_reset_forgets_the_rest() {
        let mut l = List::of(1_000);
        l.place(0, ids("MAP", 2..202));
        // arithmetic said 101, but MAP-102 is known at 100: the page goes there
        l.place(101, ids("MAP", 102..302));
        assert_eq!(l.index_of("MAP-301"), Some(299));
        assert_eq!(l.id_at(300), None);
        let mut r = l.clone();
        let ask = Ask { after: Some("MAP-150".into()), start: None };
        r.answered(&ask, ids("MAP", 151..161), 1_000 - 159, true);
        assert_eq!(r.index_of("MAP-151"), Some(149));
        assert_eq!(r.index_of("MAP-2"), None, "a reset forgets what it did not bring");
        // nothing after a known child: the list ends there
        let mut e = List::of(10);
        e.place(0, ids("END", 2..5));
        let ask = e.ask_for(3);
        e.answered(&ask, Vec::new(), 0, false);
        assert_eq!(e.n, 3);
    }

    /// The window: a whole list under a page; past it, the chunks within a
    /// chunk's height of the viewport, spacers that add up to the rest, the
    /// last chunk at the end of the scroll, and a list out of view mounting
    /// the chunk nearest it.
    #[test]
    fn the_window_is_the_viewport_plus_a_chunk_either_side() {
        let none = BTreeMap::new();
        let g = |above: f64| Geom { above, viewport: 900.0, pitch: 20.0 };
        let small = span(WHOLE, g(5_000.0), &none);
        assert_eq!((small.start, small.end, small.top_px, small.bottom_px), (0, chunks(WHOLE), 0.0, 0.0));

        let n = 163_530;
        let total = chunks(n) as f64;
        let check = |s: super::Span| {
            let height = s.top_px + s.bottom_px + (s.start..s.end).map(|k| super::rows_in(k, n) as f64 * 20.0).sum::<f64>();
            assert!((height - n as f64 * 20.0).abs() < 1e-6, "{s:?}: {height}");
        };
        let top = span(n, g(0.0), &none);
        assert_eq!(top.start, 0);
        assert!(top.end >= 2 && top.end <= 3, "{top:?}");
        check(top);
        let mid = span(n, g(1_000_000.0), &none);
        let at = 1_000_000.0 / 20.0 / CHUNK as f64;
        assert!(mid.start as f64 <= at && mid.end as f64 >= at + 900.0 / 20.0 / CHUNK as f64, "{mid:?}");
        assert!(mid.end - mid.start <= 5, "bounded: {mid:?}");
        check(mid);
        let end = span(n, g(n as f64 * 20.0 - 900.0), &none);
        assert_eq!(end.end as f64, total);
        assert_eq!(end.bottom_px, 0.0);
        check(end);
        let below = span(n, g(-50_000.0), &none);
        assert_eq!((below.start, below.end), (0, 1), "a list below the viewport mounts its first chunk");
        let above = span(n, g(n as f64 * 20.0 + 50_000.0), &none);
        assert_eq!(above.end as f64, total, "a list above it mounts its last");
        // a measured chunk (one child opened) keeps its height in the spacer
        let mut m = BTreeMap::new();
        m.insert(0usize, CHUNK as f64 * 20.0 + 400.0);
        let s = span(n, g(1_000_000.0), &m);
        assert_eq!(s.top_px, (1..s.start).map(|k| super::rows_in(k, n) as f64 * 20.0).sum::<f64>() + CHUNK as f64 * 20.0 + 400.0);
        assert_eq!(offset_of(CHUNK + 3, n, 20.0, &m), CHUNK as f64 * 20.0 + 400.0 + 60.0);
    }
}
