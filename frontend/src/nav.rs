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
//! *focused* row and the *selected* row are two different things. (ACT-3:
//! the open row is now the *current* one, and selected is a set — below.)
//!
//! AGS-25: **a double-click opens or closes a row** that has children —
//! one toggle per double-click, on its text and on its `▸` alike — and
//! selects no text. AGS-29 (reversing AGS-25's "the first click still opens
//! the ticket's page"): **a double-click opens nothing.** A plain click on a
//! ticket row with children waits out the double-click window
//! (`bindings::DOUBLE_CLICK_MS`) before it opens the ticket, and a second
//! click within it makes the gesture a toggle; a leaf opens at once. The
//! rule is src/rowclick.rs's; modifiers, the `▸` and the keys are immediate,
//! as before.
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
//!   one `get` (AGS-23: the root *is* the row now, drawn from the group). A
//!   shortcode with no root — a store from before MNG-12 — lists its
//!   tickets flat under its row, by page (`list { epic }`).
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
//!
//! AGS-23: **a shortcode's top row is its first ticket; half the indent per
//! level** (owner, 2026-10-02: "it moves to the right less on each open and
//! … the highest level tags aren't the empty ones with just the scope name,
//! they are the first ticket"). Two changes to the drawing, none to where it
//! comes from:
//!
//! - **Two columns a level** ([`art`]): `│ ` / two spaces for each
//!   ancestor, `├─` / `└─` for the row — the four cases AGS-20's art told
//!   apart, at half the width.
//! - **A shortcode with a root is drawn as its root** ([`shortcode_node`]):
//!   the shortcode row whose one child was the root (MNG-14) is gone, and
//!   the root's own row stands in its place — a real ticket row (the link,
//!   the selection, the right-click menu, the kind glyph, the code, the
//!   title, the status tag or `Doc` chip) that also carries the shortcode's
//!   roll-up. Opening it shows the root's children, then the sub-shortcodes
//!   it hosts; a sub-shortcode is its own root's row the same way, under its
//!   host after the host's children. Every tree is one level shallower. A
//!   shortcode with no root (a store from before MNG-12) keeps its
//!   shortcode row and its flat list.
//! - **The top level still paints from `groups {}` alone.** The root's row
//!   needs its title, status and kind at once, so the group carries them
//!   (`Group::root_row`, from the server's cache) and `group_changed` keeps
//!   them — the server sends one when the root's title or status changes.
//!   The row draws from the group ([`face`]), not from the row cache: no
//!   `get` per shortcode, before the nav shows or after. Opening it is one
//!   page of the root's children, where opening the shortcode row used to
//!   be a `get` of the root and then that page.
//! - **The deep-link chain is one step shorter per shortcode** ([`climb`]):
//!   a root needs no shortcode row opened above it — at the top it is the
//!   top; hosted, it is a row of its host's, as the shortcode row was.
//! - **Persisted expansion moves with it** ([`migrate_expanded`]): an open
//!   shortcode's code in localStorage opened the row whose one child was
//!   the root; it now opens the root, so a tree that was open stays open.
//!
//! FILTER-14: **the nav under the lens.** The nav shows its effective filter
//! — `global ∧ nav` (FILTER-11's [`Level::Nav`]); with no group in it the
//! nav is exactly what it was, and asks exactly what it asked. With one, it
//! shows where the matches are (FILTER-3 §4, FILTER-9 decision 5) and never
//! decides membership itself — it draws what the server answers:
//!
//! - **The bar** sits at the nav's top under the brand: FILTER-11's generic
//!   bar at level `nav` — its `Filter` button, the inherited global pills,
//!   its own; the menu's counts are `facets` (FILTER-12's provider). `f`
//!   opens it while the nav has focus.
//! - **The shortcode rows** show their matches in place of their tally:
//!   `counts { filter }` (FILTER-12's [`LiveCounts`]) summed along the
//!   hosts ([`summed`]) — a shortcode's and every shortcode hosted under it.
//!   A shortcode with none is hidden; the last line says how many are
//!   ([`hidden_line`]) and toggles drawing them, dimmed.
//! - **Inside**, a node's children are the pruned tree: `tree { parent |
//!   epic, filter, after, limit }` (FILTER-13) — the rows that match or hold
//!   a match, in the nav's order, paged and windowed as before. A row that
//!   matches is drawn as today; one that only holds matches is dimmed; each
//!   with matches under it carries their number as a badge ([`face_of`]).
//!   (FILTER-23 draws no holder: see below.)
//! - **Live**: a membership watch (`watch { filter, membership: true }`)
//!   moves the counts (`entered` / `left` / `recount`), and makes every open
//!   list on the moved ticket's chain ask its page again ([`on_chain`]);
//!   `recount` and `lagged` make every open list ask again. A re-parent moves
//!   no membership, so the two parents' chains ask again on its
//!   `child_removed` / `children_changed`. A list asking again keeps the DOM
//!   of every row that is still there (the keyed `<For>`).
//! - **A deep link** to a ticket outside the filter does not draw it: the
//!   chain opens down to the deepest row the tree shows ([`drawn_steps`]),
//!   which is brought into view.
//!
//! FILTER-23: **under a filter the nav shows only matches** (owner,
//! 2026-10-08: "when filtering which excludes parents I don't want to see
//! parents") — FILTER-14's holders are gone. Moving the matches up is the
//! server's, so every list stays a page of `tree` at 10^6: `tree { …,
//! matches_only }` answers, at a parent, the matches nearest below it — a
//! ticket that does not match is no row, and the matches under it are
//! answered at its nearest matching ancestor, or at the top of its shortcode
//! — then the hosted shortcodes drawn there, each placed the same way from
//! its host up. So:
//!
//! - **A ticket row is a match**, badged with the matches below it; those
//!   nest under it as before.
//! - **A shortcode whose root does not match is a heading** — its code and
//!   its count, keyed and opened as the root's row ([`shortcode_face`]);
//!   opening it shows the top of the shortcode. Its root's word is `tree {
//!   epic, limit: 1 }` (FILTER-13's answer, which names the root either way).
//! - **A hosted shortcode is drawn in its place in the list**, where `tree`
//!   put it, rather than after its host's children (its host may not be
//!   drawn); one with no match only on the hidden line's say-so.
//! - **Live** as FILTER-14: a ticket entering or leaving makes the open lists
//!   on its chain ask again — its nearest drawn ancestor's among them, which
//!   is where it appears or disappears, and where the matches below it move.
//! - **A deep link** opens through the drawn rows only: `member` says which
//!   of its chain match, one call.
//!
//! FILTER-24: **the open ticket is always drawn** (owner, 2026-10-11: "I
//! need menger to always show the entered ticket no matter what. even if
//! it's through a filter."). Under the lens the nav asks every list with
//! `tree { …, keep: <the open ticket> }`, and the server answers as if that
//! ticket matched: it is a row where a match would be — under its nearest
//! drawn ancestor, or at the top of its shortcode — with its own matches
//! under it, and `matched: false`, which draws it marked outside the filter
//! ([`Face::outside`]). So:
//!
//! - **Its shortcode is drawn** whatever its matches, and every shortcode
//!   hosting it ([`kept_codes`]); a root that is the open ticket is its
//!   ticket row, not a heading.
//! - **The route moving** re-asks the lists on the old ticket's chain and the
//!   new one's ([`chain_moved`]) — each list asks with the ticket open when
//!   it asks ([`KEPT`]).
//! - **A deep link** opens down to the open ticket itself: it is drawn.
//!
//! FILTER-20: **a flat view beside the tree.** The header's `Tree | Flat`
//! chooses what the nav draws under its bar, and the choice is kept with the
//! nav's other state (`menger.nav.view`). Flat is every ticket the nav's
//! filter admits as one list in id order — the tree's leaf rows, no nesting,
//! windowed and paged and live (src/flat.rs). Tree is everything above,
//! unchanged: while Flat is chosen the tree is unmounted and its deep-link
//! reveal waits; while Tree is chosen the flat list does not exist.
//!
//! AGS-26: **the cursor is the open ticket** (owner, 2026-10-12: "make sure
//! that whatever ticket is open is selected in the nav and centered. that way
//! key bindings that impact the nav selection always succeed"; "moving up
//! should open the ticket the cursor is under"). So:
//!
//! - **Centred.** Every navigation to a ticket centres its row in the nav —
//!   a row already drawn where it is, otherwise once the reveal has opened its
//!   chain — and keeps it there while the lists around it settle
//!   ([`settle_centred`]); a new nav filter centres it again in the new tree.
//!   The Flat view does the same (src/flat.rs).
//! - **↑/↓ (k/j) step and open.** A step goes to the previous/next ticket's
//!   row — a shortcode's heading is passed over ([`next_stop`]) — focuses it
//!   and opens it ([`land`]). In the tree it steps from the focused row;
//!   anywhere else on a ticket page (not a text field, a dialog, a menu) from
//!   the open ticket's row ([`page_keydown`]), which is always drawn
//!   (FILTER-24). h/l's steps land the same way.
//! - **← and → are not the app's** (AGS-27, owner: "the arrow keys are
//!   janky. remove left and right"): they move no focus between the nav and
//!   the page, as AGS-26 had them, and do not open or close a row. The
//!   tree's open and close are h/l's alone.
//! - **`f` on a ticket page** makes the nav's filter `under <the ticket>` —
//!   the ticket and its descendants — and opens its row ([`filter_to`]).
//!   **↑ on the first ticket** widens the nav's first `under T` group to T's
//!   parent, a shortcode's to its root's host, and drops it at a top root
//!   ([`widen`]).
//!
//! ACT-3: **the nav selects like a file list** (src/selection.rs holds the
//! selection and its rules). A click opens the ticket as before and the
//! selection becomes its row; Shift+click selects from the anchor to the row
//! in the order the nav draws (the treeitems, top to bottom — a closed node's
//! children are not drawn, so not selected); Ctrl/Cmd+click toggles the row;
//! neither opens anything. In the tree's key map Shift+↑/↓ (Shift+j/k) extend
//! the selection from the focused row — focus moves, nothing opens, a gap is
//! crossed as AGS-26's steps cross it ([`cross_gap`]); Space toggles the
//! focused row; Ctrl/Cmd+A selects every drawn row; Esc clears (src/keys.rs).
//! A plain step of the cursor ([`land`]) is a click's: its row becomes the
//! selection. The tree is `aria-multiselectable`: `aria-selected` is the
//! selection's now, and the open ticket — still the `sel` row, AGS-26's
//! cursor — is `aria-current`. A shortcode's row stands for its root; every
//! selectable row says which ticket as `data-pick`. The foot counts the
//! selection ("3 selected · clear") and what the nav's filter hides of it
//! ([`selection_foot`]). The flat view is the same tree, so the same rows.
//!
//! ACT-7: **the tree's shape, a menu away** (owner, 2026-10-13: "a special
//! sub menu in the right click menu on the nav that is nav centric"). What is
//! open is a [`Shape`] (src/shape.rs) — a default and per-key marks, deep or
//! plain, the newer word winning along a chain — not a set of keys: each node
//! folds it over its ancestors' keys ([`Up`]), so "everything under THEME-4"
//! needs nothing under THEME-4 known. The menu (src/menu.rs) asks [`at`] where
//! it opened — a row (its key, its chain from the DOM, its shortcode's root),
//! the flat list, or the nav's empty space — and [`act`] does the item:
//! expand or collapse a subtree, a shortcode from its root, everything;
//! collapse the others; reveal the open ticket (the reveal again, then focus).
//! Expanding marks; it fetches nothing itself. A node open by a mark of its
//! own asks its first page at once, as a click's does; one open only by a
//! bulk expand asks once its list is near the viewport, holding the room its
//! subtree will take until then ([`Lazy`], [`near`]) — so Expand all on prod
//! asks 31 pages for its first screen, not 196, and the rest as the scroll
//! reaches them; a long list stays a window. Under the lens only what `tree`
//! draws opens. Canceled rows stay closed (MENGER-13). Keys: `*`, Shift+`*`,
//! `-` ([`shape_key`]).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::counts::{wire_text, LiveCounts};
use crate::filter::{Cat, FilterBar, ScTree};
use crate::flat::{self, View};
use crate::kids::key;
use crate::lens::{use_lens, Lens, Level};
use crate::session::Member;
use crate::selection::{self, Gesture, Picked};
use crate::shape::{Op, Place, Shape};
use crate::{cache, dom, keys, rowclick, session, st_class, tags, Route};
use menger_tickets_api::filter::{Field, Filter, Group as FGroup, Predicate};
use menger_tickets_api::{Group, Kind, Status, TicketSummary, TicketsEvent};

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
pub(crate) fn ensure_tree_tab_stop() {
    keys::ensure_tab_stop("#nav .tree [role=treeitem]", Some("#nav .tree .row.sel"));
}

const EXPANDED_KEY: &str = "menger.nav.expanded";
const HIDDEN_KEY: &str = "menger.nav.hidden";

/// localStorage, or None where it is unavailable (private mode, blocked
/// site data). Every persistence call degrades to "not remembered".
pub(crate) fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

/// ACT-7: the expansion is a [`Shape`] now, kept under the same key — a set
/// stored before reads as its plain marks.
fn load_expanded() -> Shape {
    Shape::load(storage().and_then(|s| s.get_item(EXPANDED_KEY).ok().flatten()).as_deref())
}

fn save_expanded(shape: &Shape) {
    if let Some(s) = storage() {
        let _ = s.set_item(EXPANDED_KEY, &shape.stored());
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
        // SDLC-11: a document — the pilcrow.
        "doc" => "¶",
        _ => "·",
    }
}

/// A shortcode row's roll-up as the nav shows it: the work's `done/total`
/// (MNG-10). SDLC-11: a doc has no completion, so docs are not in it; a row
/// of docs alone — a doc collection — shows how many docs it holds instead.
pub(crate) fn rollup_label(g: &Group) -> String {
    roll_label(g.done, g.total, g.docs)
}

/// The roll-up's words for `done` of `total` work and `docs` docs — the one
/// idiom the nav's shortcode rows and (AGS-22) the ticket page's Children
/// header both show: the work's `done/total`, whatever docs sit beside it;
/// docs alone, counted.
pub(crate) fn roll_label(done: u64, total: u64, docs: u64) -> String {
    match (total, docs) {
        (0, 1) => "1 doc".to_string(),
        (0, docs) if docs > 0 => format!("{docs} docs"),
        (total, _) => format!("{done}/{total}"),
    }
}

/// A rendered node: a ticket row, a shortcode drawn as its root's row, or a
/// rootless epic's shortcode row. Its key is the ticket id / the root's id /
/// the epic code — the string that identifies it in the persisted
/// `expanded` set and in the DOM (`data-node-key`). Ticket ids always carry
/// a `-`, epic codes never do, so the two namespaces cannot collide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Ticket(String),
    /// AGS-23: a shortcode with a root, drawn as the root's row — the root's
    /// id, then the shortcode. Keyed by the root's id, as the root's own
    /// ticket node was, so its expansion is the root's.
    Root(String, String),
    /// A shortcode with no root (a store from before MNG-12): its code and
    /// roll-up, over its tickets listed flat.
    Group(String),
}

impl Node {
    pub(crate) fn key(&self) -> &str {
        match self {
            Node::Ticket(k) | Node::Root(k, _) | Node::Group(k) => k,
        }
    }
}

/// AGS-23: the node a shortcode is drawn as — its root's row when it has a
/// root, its shortcode row (and flat list) when it has none.
pub(crate) fn shortcode_node(g: &Group) -> Node {
    match &g.root {
        Some(root) => Node::Root(root.clone(), g.epic.clone()),
        None => Node::Group(g.epic.clone()),
    }
}

/// AGS-23: a shortcode root's row as `groups {}` describes it — what the
/// row draws, without the root in the row cache: the id, and the title,
/// status and kind the group carries (`Group::root_row`). Nothing else on it
/// is drawn (a root row's children come by page, its "has children" from
/// the roll-up — [`root_has_kids`]), so nothing else is filled in. `None`
/// for a rootless group, and for a group from a server that does not dress
/// its root.
pub(crate) fn face(g: &Group) -> Option<TicketSummary> {
    let (id, row) = (g.root.as_ref()?, g.root_row.as_ref()?);
    Some(root_summary(id, &g.epic, g.host.clone(), row.title.clone(), row.status, row.kind))
}

fn root_summary(id: &str, epic: &str, host: Option<String>, title: String, status: Status, kind: Kind) -> TicketSummary {
    TicketSummary {
        labels: Vec::new(),
        id: id.to_string(),
        epic: epic.to_string(),
        seq: key(id).map_or(0, |(_, s)| s),
        title,
        status,
        kind,
        severity: None,
        parent: host,
        child_count: 0,
        blocked_by: Vec::new(),
        unlocks: Vec::new(),
        commits: Vec::new(),
        path: None,
        placement: None,
        created_unix: 0,
        updated_unix: 0,
    }
}

/// AGS-23: whether a shortcode's root row opens onto anything. Its roll-up
/// counts the root and everything drawn under the shortcode (MNG-10: its
/// tickets, the trees filed under them, the sub-shortcodes it hosts), so
/// anything beyond the root — work or docs — is under it.
pub(crate) fn root_has_kids(g: &Group) -> bool {
    g.total + g.docs > 1
}

/// AGS-20's tree art, two monospace columns a level (AGS-23; it was four):
/// for each ancestor, outermost first, `│ ` while it has siblings still to
/// come and two spaces once it was the last; then `├─` for a row with
/// siblings after it, `└─` for the last of them.
pub(crate) fn art(ancestors_last: impl IntoIterator<Item = bool>, last: bool) -> String {
    let mut s = String::new();
    for a in ancestors_last {
        s.push_str(if a { "  " } else { "│ " });
    }
    s.push_str(if last { "└─" } else { "├─" });
    s
}

/// AGS-23: one step of a deep link's chain, from the ticket `cur` up — given
/// the group of its shortcode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Climb {
    /// `cur` is a shortcode's root drawn at the top level: the chain ends.
    Top,
    /// `cur` is a sub-shortcode's root, drawn under its host after the
    /// host's children: the host opens, and the chain goes on from it.
    Host(String),
    /// `cur` is in a shortcode with no root: its shortcode row opens onto
    /// the flat list that shows it, and the chain ends.
    Flat(String),
    /// `cur` hangs under this parent, which opens onto it.
    Parent(String),
    /// None of these — a parentless ticket that is not its shortcode's root
    /// (a legacy shape the nav does not draw).
    Lost,
}

pub(crate) fn climb(g: Option<&Group>, cur: &TicketSummary) -> Climb {
    match g {
        Some(g) if g.root.as_deref() == Some(cur.id.as_str()) => match &g.host {
            Some(h) => Climb::Host(h.clone()),
            None => Climb::Top,
        },
        Some(g) if g.root.is_none() => Climb::Flat(g.epic.clone()),
        _ => match &cur.parent {
            Some(p) => Climb::Parent(p.clone()),
            None => Climb::Lost,
        },
    }
}

/// AGS-23: the persisted expansion as it reads now. Until AGS-23 a
/// shortcode's code in it opened the shortcode row, whose one child was the
/// root; that row is the root's now, so the code becomes the root's id — an
/// open tree stays open across the upgrade. A code whose shortcode has no
/// root still names its shortcode row, and one the groups do not name yet
/// is left as it is (it moves when its group arrives). `None` when nothing
/// moves. ACT-7: on the shape's marks, each keeping what it says.
pub(crate) fn migrate_expanded(expanded: &Shape, groups: &BTreeMap<String, Group>) -> Option<Shape> {
    expanded.rekeyed(|code| groups.get(code).and_then(|g| g.root.clone()))
}

// ---------------------------------------------------------------------
// FILTER-14: the nav under the lens — what decides, pure.

/// What `tree` says of a row it answers (FILTER-13's `node`): whether the
/// row itself matches, and how many matches are strictly under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(crate) matched: bool,
    pub(crate) below: u64,
}

/// How a row is drawn under the lens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Face {
    /// FILTER-23: a shortcode whose root does not match — drawn as a
    /// heading (its code and its matches), not as its root's ticket row.
    pub(crate) heading: bool,
    /// Dimmed: a shortcode with no match, drawn on the hidden line's
    /// say-so (FILTER-14: and a row that only held matches, which FILTER-23
    /// draws no more).
    pub(crate) dim: bool,
    /// The matches under it, as its badge — none when there are none.
    pub(crate) badge: Option<String>,
    /// It opens onto something the lens shows: matches under it.
    pub(crate) opens: bool,
    /// FILTER-24: the open ticket, drawn though the filter excludes it.
    pub(crate) outside: bool,
}

/// A row `tree` answered, as it is drawn: a match as today, badged with the
/// number of matches under it and opening onto them. FILTER-23: every ticket
/// row the lens draws is a match (`tree { matches_only }`); one that is not
/// would be dimmed.
pub(crate) fn face_of(m: Mark) -> Face {
    Face { heading: false, dim: !m.matched, badge: (m.below > 0).then(|| m.below.to_string()), opens: m.below > 0, outside: false }
}

/// FILTER-24: a ticket row as it is drawn when it is the open ticket — kept
/// in the tree whatever the filter, and marked outside it when it does not
/// match.
pub(crate) fn kept_face(m: Mark) -> Face {
    Face { outside: !m.matched, ..face_of(m) }
}

/// FILTER-24: where the open ticket sits for a shortcode's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Holds {
    /// Not in it.
    No,
    /// In it, or in a shortcode it hosts: the row is drawn, and opens.
    Under,
    /// It is the shortcode's root.
    Root,
}

/// A shortcode's row under the lens — `n` its matches summed along the
/// hosts, `root` what `tree` said of its root when the nav has been told
/// (a top-level root asked on its own, a hosted one in the page that places
/// it). The row's tally is `n`, so it carries no badge besides. FILTER-23:
/// a root that matches is its ticket row; one that does not is a heading —
/// and until the root's word comes, so is the row (a root is a scope or a
/// doc, which most filters do not ask for). Dimmed when nothing in it
/// matches (drawn only on the hidden line's say-so), opening when anything
/// under it does.
///
/// FILTER-24: a shortcode holding the open ticket is drawn and opens, its
/// matches or none; one whose root is the open ticket is that ticket's row
/// (marked outside the filter unless the root matches).
pub(crate) fn shortcode_face(n: usize, root: Option<Mark>, holds: Holds) -> Face {
    match (root, holds) {
        (Some(m), _) if n > 0 && m.matched => Face { heading: false, dim: false, badge: None, opens: m.below > 0, outside: false },
        (_, Holds::Root) => Face { heading: false, dim: true, badge: None, opens: n > 0, outside: true },
        (Some(m), _) if n > 0 => Face { heading: true, dim: false, badge: None, opens: m.below > 0, outside: false },
        (_, Holds::Under) => Face { heading: true, dim: n == 0, badge: None, opens: true, outside: false },
        _ => Face { heading: true, dim: n == 0, badge: None, opens: n > 0, outside: false },
    }
}

/// FILTER-24: the shortcodes whose rows lead to the open ticket `id` — its
/// own, and every one hosting it — drawn whatever their matches.
pub(crate) fn kept_codes(tree: &ScTree, id: &str) -> BTreeSet<String> {
    let Some((epic, _)) = key(id) else { return BTreeSet::new() };
    let mut out = BTreeSet::from([epic.to_string()]);
    // `hosting` names each host ticket, then its shortcode.
    out.extend(tree.hosting(epic).into_iter().skip(1).step_by(2));
    out
}

/// Each shortcode's matches summed along the hosts (MNG-14's hosting, as the
/// groups' `host` says it): its own (`held`) and those of every shortcode
/// hosted under it, at any depth — the shortcode row's number, as its roll-up
/// counts its subtree.
pub(crate) fn summed(tree: &ScTree, held: impl Fn(&str) -> usize) -> BTreeMap<String, usize> {
    tree.known().iter().map(|e| (e.clone(), tree.subtree(e).iter().map(|s| held(s)).sum())).collect()
}

/// How many shortcodes the filter hides: those with no match — FILTER-24:
/// but those leading to the open ticket (`kept`), which are drawn.
pub(crate) fn hidden_count(sums: &BTreeMap<String, usize>, kept: &BTreeSet<String>) -> usize {
    sums.iter().filter(|(e, n)| **n == 0 && !kept.contains(*e)).count()
}

/// The nav's last line under the lens.
pub(crate) fn hidden_line(n: usize) -> String {
    match n {
        1 => "1 shortcode hidden by the filter".to_string(),
        n => format!("{n} shortcodes hidden by the filter"),
    }
}

/// The shortcode a node stands for, if it does.
fn node_code(n: &Node) -> Option<&str> {
    match n {
        Node::Root(_, epic) | Node::Group(epic) => Some(epic),
        Node::Ticket(_) => None,
    }
}

/// Which shortcode rows the lens draws: those with a match — every one when
/// `all` (the hidden line toggled on: the rest dimmed). A ticket row is not
/// the counts' to hide. FILTER-24: nor a shortcode leading to the open
/// ticket (`kept`).
pub(crate) fn shown(nodes: Vec<Node>, sums: &BTreeMap<String, usize>, all: bool, kept: &BTreeSet<String>) -> Vec<Node> {
    if all {
        return nodes;
    }
    nodes.into_iter().filter(|n| node_code(n).is_none_or(|e| kept.contains(e) || sums.get(e).is_some_and(|c| *c > 0))).collect()
}

/// Whether an open list is on a ticket's chain — `chain` the ticket and its
/// ancestors: a parent's list when the parent is on it (its children's
/// faces, its count, are what moved), a flat shortcode's list when one of
/// the chain is in that shortcode.
pub(crate) fn on_chain(source: &Source, chain: &[String]) -> bool {
    match source {
        Source::Parent(p) => chain.contains(p),
        Source::Epic(e) => chain.iter().any(|id| key(id).is_some_and(|(x, _)| x == e)),
    }
}

/// FILTER-23: a deep link under the lens. `chain` is the open ticket, then
/// each node the unfiltered nav opens to show it, up to its top row (a
/// ticket's parent, a hosted root's host, a rootless shortcode's row);
/// `drawn[i]` whether the lens draws `chain[i]` — a ticket that matches, a
/// shortcode's row (its root, or its rootless row) that has matches. Only
/// those are rows, each under the nearest drawn node above it: the steps
/// are each drawn node to open and the drawn node below it to show (its
/// key), from the deepest drawn one up; that one is where the chain ends
/// (the open ticket when it matches). `None` when nothing is drawn.
pub(crate) fn drawn_steps(chain: &[Node], drawn: &[bool]) -> Option<(Vec<(Node, String)>, String)> {
    let kept: Vec<&Node> = chain.iter().zip(drawn).filter(|(_, d)| **d).map(|(n, _)| n).collect();
    let at = kept.first()?.key().to_string();
    let steps = kept.windows(2).map(|w| (w[1].clone(), w[0].key().to_string())).collect();
    Some((steps, at))
}

/// FILTER-23: the last child an open list draws under the lens — from its
/// last index back, past the hosted shortcodes it does not draw (`skip`);
/// `None` where the window has not read that far.
pub(crate) fn last_drawn(list: &List, skip: impl Fn(&str) -> bool) -> Option<String> {
    for i in (0..list.n).rev() {
        match list.id_at(i) {
            Some(id) if skip(id) => continue,
            Some(id) => return Some(id.to_string()),
            None => return None,
        }
    }
    None
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

/// ACT-7: whether a list `n` rows long, where `g` measured it, is on or
/// near the nav's viewport — its top less than two viewports below the
/// viewport's top, its bottom less than one above it. A list whose first page
/// has not come is one row long at least.
pub(crate) fn near(g: Geom, n: usize) -> bool {
    let top = -g.above;
    let bottom = top + (n.max(1) as f64) * g.pitch;
    top < 2.0 * g.viewport && bottom > -g.viewport
}

/// ACT-7: how an open node's list asks — at once when the node is open by a
/// mark of its own, otherwise once near the viewport — and how many rows of
/// room it holds until it has asked.
#[derive(Clone, Copy)]
struct Lazy {
    marked: Memo<bool>,
    reserve: usize,
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
    /// The roots of hosted sub-shortcodes, each with its shortcode: a child
    /// of their host by `parent`, drawn as their shortcode's row instead
    /// (AGS-20) — AGS-23: which is the root's own row, after the host's
    /// children. FILTER-23: under the lens, where `tree` places it.
    hosted_roots: Memo<BTreeMap<String, String>>,
}

impl Tree {
    pub(crate) fn new() -> Self {
        let groups = RwSignal::new(None::<BTreeMap<String, Group>>);
        // Each reads the groups in place, taking only what it keeps — a group
        // carries its root's title now (AGS-23), and these run on every
        // `group_changed`.
        let top = Memo::new(move |_| {
            groups.with(|m| {
                m.iter().flat_map(|m| m.values()).filter(|g| g.host.is_none()).map(|g| g.epic.clone()).collect::<Vec<_>>()
            })
        });
        let hosted = Memo::new(move |_| {
            let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
            groups.with(|m| {
                for g in m.iter().flat_map(|m| m.values()) {
                    if let Some(h) = &g.host {
                        out.entry(h.clone()).or_default().push(g.epic.clone());
                    }
                }
            });
            out
        });
        let hosted_roots = Memo::new(move |_| {
            groups.with(|m| {
                m.iter()
                    .flat_map(|m| m.values())
                    .filter(|g| g.host.is_some())
                    .filter_map(|g| g.root.clone().map(|r| (r, g.epic.clone())))
                    .collect::<BTreeMap<_, _>>()
            })
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
    /// is drawn by no one — SDLC-11: and `docs: 0`; a row of docs alone (a
    /// doc collection) is a row ([`Group::drawn`]).
    pub(crate) fn set_groups(self, all: Vec<Group>) {
        let map = all.into_iter().filter(Group::drawn).map(|g| (g.epic.clone(), g)).collect();
        self.groups.set(Some(map));
        self.epoch.update(|e| *e += 1);
    }

    /// `group_changed`: one row's roll-up as it is now — its `done/total`
    /// move in place, a changed `host` moves the row; `total: 0` (SDLC-11:
    /// with `docs: 0`) is a row no longer drawn.
    pub(crate) fn group_changed(self, g: Group) {
        let same = self.groups.with_untracked(|m| m.as_ref().and_then(|m| m.get(&g.epic)) == Some(&g));
        if same {
            return;
        }
        self.groups.update(|m| {
            if let Some(m) = m {
                if !g.drawn() {
                    m.remove(&g.epic);
                } else {
                    m.insert(g.epic.clone(), g);
                }
            }
        });
    }

    /// `f` of `epic`'s group, tracked — without cloning the group.
    fn with_group<T>(self, epic: &str, f: impl FnOnce(&Group) -> T) -> Option<T> {
        self.groups.with(|m| m.as_ref().and_then(|m| m.get(epic)).map(f))
    }

    /// WAVE-5: `f` of every group the nav holds, tracked — `None` until the
    /// session's first `groups {}`. What the wave board finds a shortcode's
    /// tree in (each group's `host`).
    pub(crate) fn with_groups<T>(self, f: impl FnOnce(Option<&BTreeMap<String, Group>>) -> T) -> T {
        self.groups.with(|m| f(m.as_ref()))
    }

    /// WAVE-5: the top-level shortcodes, in shortcode order — what `#/waves`
    /// lists.
    pub(crate) fn top(self) -> Memo<Vec<String>> {
        self.top
    }

    /// AGS-23: the nodes these shortcodes are drawn as, tracked — each its
    /// root's row, or its shortcode row with no root ([`shortcode_node`]).
    /// One list for a keyed `<For>`: a node is built once, and again only if
    /// what it is drawn as changes (a legacy shortcode gaining its first
    /// scope).
    fn nodes(self, epics: &[String]) -> Vec<Node> {
        self.groups.with(|m| {
            epics
                .iter()
                .map(|e| m.as_ref().and_then(|m| m.get(e)).map_or_else(|| Node::Group(e.clone()), shortcode_node))
                .collect()
        })
    }
}

/// One open list: what it lists, its children, the rows it holds — and
/// (FILTER-14) whether it is the pruned tree under the lens, and what moves
/// when it must ask its page again.
struct Open {
    source: Source,
    list: RwSignal<List>,
    holder: cache::Holder,
    lensed: bool,
    again: RwSignal<u32>,
}

thread_local! {
    /// Every open list, by what it lists — where `watch`'s deltas are applied.
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };

    /// The ticket the last `created` / `parent_changed` carried, and whether
    /// it was born: the `children_changed` behind it names it, and an open
    /// list takes its row from here rather than ask the server for it.
    static ARRIVED: RefCell<Option<(TicketSummary, bool)>> = const { RefCell::new(None) };

    /// The deep link being revealed; a newer navigation supersedes it.
    static REVEAL_GEN: Cell<u32> = const { Cell::new(0) };

    /// FILTER-24: the open ticket, while the nav is under the lens — what
    /// every list asks `tree` to keep.
    static KEPT: RefCell<Option<String>> = const { RefCell::new(None) };

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

/// The open lists `pick` matches, outside the registry's borrow — the
/// unfiltered ones: `watch`'s deltas are what the store did, and a list
/// under the lens shows what `tree` answers (FILTER-14).
fn lists(pick: impl Fn(&Source) -> bool) -> Vec<(RwSignal<List>, cache::Holder)> {
    OPEN.with(|o| o.borrow().iter().filter(|e| !e.lensed && pick(&e.source)).map(|e| (e.list, e.holder.clone())).collect())
}

/// FILTER-14: every open list under the lens that `pick` matches asks its
/// page again.
fn ask_again(pick: impl Fn(&Source) -> bool) {
    let hit: Vec<RwSignal<u32>> = OPEN.with(|o| o.borrow().iter().filter(|e| e.lensed && pick(&e.source)).map(|e| e.again).collect());
    for again in hit {
        again.try_update(|n| *n = n.wrapping_add(1));
    }
}

/// FILTER-14: whether any open list is under the lens.
fn any_lensed() -> bool {
    OPEN.with(|o| o.borrow().iter().any(|e| e.lensed))
}

/// A row as the app last heard of it — the row cache's, else the last
/// `watch` carried — or, failing both, the server's.
async fn row_of(id: &str) -> Option<TicketSummary> {
    match cache::get(id).map(|s| s.get_untracked()).or_else(|| session::recent(id)) {
        Some(t) => Some(t),
        None => get_summary(id).await,
    }
}

/// FILTER-14: `id` and its ancestors, nearest first — one lookup per
/// ancestor, from what the app holds where it can.
async fn chain_ids(id: &str) -> Vec<String> {
    let mut out = vec![id.to_string()];
    let mut cur = id.to_string();
    for _ in 0..64 {
        match row_of(&cur).await.and_then(|t| t.parent) {
            Some(p) if !out.contains(&p) => {
                out.push(p.clone());
                cur = p;
            }
            _ => break,
        }
    }
    out
}

/// FILTER-14: something under `id` moved in or out of the lens (or `id`
/// itself did): every open list under the lens on its chain asks again —
/// the faces and counts on the chain are what moved.
pub(crate) fn chain_moved(id: String) {
    if !any_lensed() {
        return;
    }
    spawn_local(async move {
        let chain = chain_ids(&id).await;
        ask_again(|s| on_chain(s, &chain));
    });
}

/// A `created` or `parent_changed` came in. A flat shortcode's list (no
/// root) takes a birth of its shortcode the way a parent's list takes
/// `children_changed`.
pub(crate) fn arrived(t: &TicketSummary, born: bool) {
    ARRIVED.with(|a| *a.borrow_mut() = Some((t.clone(), born)));
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
/// FILTER-14: a move (not a birth: the membership watch says whether a
/// newborn matches) changes no membership, but the pruned tree on the new
/// parent's chain: those lists ask again.
pub(crate) fn children_changed(parent: &str, added: &str, count: u64) {
    let arrived = ARRIVED.with(|a| a.borrow().as_ref().filter(|(t, _)| t.id == added).cloned());
    if arrived.as_ref().is_some_and(|(_, born)| !born) {
        chain_moved(parent.to_string());
    }
    let row = arrived.map(|(t, _)| t);
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
/// `removed` go. FILTER-14: under the lens, the old parent's chain asks again.
pub(crate) fn child_removed(parent: &str, removed: &str, count: u64) {
    chain_moved(parent.to_string());
    for (list, holder) in lists(|s| matches!(s, Source::Parent(p) if p == parent)) {
        list.try_update(|l| l.removed(removed, count as usize));
        holder.release(removed);
    }
}

/// ACT-5: `deleted`: an open list of `id`'s shortcode lets it go (its
/// parent's lists hear `child_removed`); every list under the lens asks
/// again.
pub(crate) fn departed(id: &str, epic: &str) {
    for (list, holder) in lists(|s| matches!(s, Source::Epic(e) if e == epic)) {
        list.try_update(|l| {
            let n = l.n.saturating_sub(1);
            l.removed(id, n)
        });
        holder.release(id);
    }
    ask_again(|_| true);
}

/// One page of a list, asked of the server — FILTER-14: under the lens, with
/// what `tree` said of each row.
enum Asked {
    Page(Vec<TicketSummary>, usize, Vec<(String, Mark)>),
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
    match client.list_all(epic, None, parent, after, Some(limit as u32), Some(true), None, None, None, crate::session::tz()).await {
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
                Some(r) => Asked::Page(rows, r, Vec::new()),
                None => Asked::Refused,
            }
        }
        Err(_) => Asked::Offline,
    }
}

/// FILTER-14: one page of a list under the lens — FILTER-23: `tree {
/// parent | epic, filter, after, limit, matches_only }`: the matches nearest
/// below, in the list's order — a ticket that does not match is no row, and
/// the matches under it are answered here in its place — then the hosted
/// shortcodes placed here; each with what the tree says of it.
async fn ask_tree(source: &Source, filter: &str, after: Option<String>, limit: usize) -> Asked {
    let Some(client) = session::client() else { return Asked::Offline };
    let (epic, parent) = match source {
        Source::Parent(p) => (None, Some(p.clone())),
        Source::Epic(e) => (Some(e.clone()), None),
    };
    let keep = KEPT.with(|k| k.borrow().clone());
    match client.tree_all(parent, epic, filter.to_string(), after, Some(limit as u32), Some(true), keep, crate::session::tz()).await {
        Ok(events) => {
            let mut rows = Vec::new();
            let mut marks = Vec::new();
            let mut remaining = None;
            for ev in events {
                match ev {
                    TicketsEvent::Node { summary, matched, below } => {
                        marks.push((summary.id.clone(), Mark { matched, below }));
                        rows.push(summary);
                    }
                    TicketsEvent::Page { remaining: r, .. } => remaining = Some(r as usize),
                    TicketsEvent::Error { message } => {
                        leptos::logging::log!("nav: tree: {message}");
                        return Asked::Refused;
                    }
                    _ => {}
                }
            }
            match remaining {
                Some(r) => Asked::Page(rows, r, marks),
                None => Asked::Refused,
            }
        }
        Err(_) => Asked::Offline,
    }
}

/// FILTER-14: what `tree` says of shortcode `epic`'s top row — its root —
/// under `filter`: `tree { epic, filter, limit: 1 }` (FILTER-13's answer,
/// which names the root whether or not it matches: FILTER-23 draws it as a
/// heading when it does not). `None` when the root is not drawn (nothing in
/// the shortcode matches), or no answer came.
async fn ask_root(epic: &str, root: &str, filter: &str) -> Option<Mark> {
    let client = session::client()?;
    let events = client.tree_all(None, Some(epic.to_string()), filter.to_string(), None, Some(1), None, None, crate::session::tz()).await.ok()?;
    events.into_iter().find_map(|ev| match ev {
        TicketsEvent::Node { summary, matched, below } if summary.id == root => Some(Mark { matched, below }),
        _ => None,
    })
}

/// A list's page: `list` — or under the lens, `tree`.
async fn ask_list(source: &Source, lensed: Option<&str>, after: Option<String>, limit: usize) -> Asked {
    match lensed {
        Some(filter) => ask_tree(source, filter, after, limit).await,
        None => ask_page(source, after, limit).await,
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

/// AGS-26: the open ticket's row, when it is drawn — the tree's or the flat
/// list's.
pub(crate) fn selected_row(id: &str) -> Option<web_sys::HtmlElement> {
    dom::window()
        .document()?
        .query_selector(&format!("#nav .row.sel[data-nav-id=\"{id}\"]"))
        .ok()
        .flatten()?
        .dyn_into()
        .ok()
}

/// AGS-26: how far the nav must scroll for a row to sit in its middle — the
/// row's middle less the viewport's (px; the scroller clamps it at the list's
/// ends).
pub(crate) fn centring(view_top: f64, view_height: f64, row_top: f64, row_height: f64) -> f64 {
    (row_top + row_height / 2.0) - (view_top + view_height / 2.0)
}

/// AGS-26: scroll the nav so `row` sits in its middle, as near as the list's
/// ends allow. Whether the scroll moved.
fn center_row(sc: &web_sys::HtmlElement, row: &web_sys::Element) -> bool {
    let (s, r) = (sc.get_bounding_client_rect(), row.get_bounding_client_rect());
    let delta = centring(s.top(), f64::from(sc.client_height()), r.top(), r.height()).round() as i32;
    let before = sc.scroll_top();
    if delta != 0 {
        sc.set_scroll_top(before + delta);
    }
    sc.scroll_top() != before
}

/// AGS-26: the open ticket's row centred, and kept there while the lists
/// around it settle — a page arriving above it, a list asking again under a
/// new filter, moves it after the first scroll. Polled every 50 ms until the
/// row has stood still for a quarter of a second, for at most three seconds;
/// it stops when `alive` says the navigation is over, and when the reader
/// scrolls the nav themselves (the scroll is not where it was left).
pub(crate) async fn settle_centred(id: String, alive: impl Fn() -> bool) {
    let mut left: Option<i32> = None;
    let mut still = 0;
    for _ in 0..60 {
        if !alive() {
            return;
        }
        let Some(sc) = scroller() else { return };
        if left.is_some_and(|l| sc.scroll_top() != l) {
            return; // the reader scrolled
        }
        if let Some(row) = selected_row(&id) {
            if center_row(&sc, &row) {
                still = 0;
            } else {
                still += 1;
                if still >= 5 {
                    return;
                }
            }
            left = Some(sc.scroll_top());
        }
        dom::sleep_ms(50).await;
    }
}

/// The nav's scroller.
pub(crate) fn scroller() -> Option<web_sys::HtmlElement> {
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

// ---------------------------------------------------------------------
// ACT-7: the Nav ▸ menu's ops on the tree (what each does to the shape is
// src/shape.rs's).

thread_local! {
    /// ACT-7: the mounted nav's handles — what the menu (src/menu.rs) and
    /// the keys act through.
    static HOOK: Cell<Option<Hook>> = const { Cell::new(None) };
}

#[derive(Clone, Copy)]
struct Hook {
    ctx: Ctx,
    view: RwSignal<View>,
    /// Bumped by Reveal the open ticket: the tree's and the flat list's
    /// reveal run again.
    reveal: RwSignal<u32>,
}

/// ACT-7: Reveal the open ticket's count — what the tree's and the flat
/// list's reveal effects follow (`None` until the nav is mounted).
pub(crate) fn reveal_asked() -> Option<RwSignal<u32>> {
    HOOK.with(Cell::get).map(|h| h.reveal)
}

/// ACT-7: the keys of the rows above a row of the tree, the top first — the
/// `.node`s whose `.kids` hold it (a hosted shortcode's host among them, as
/// its row is drawn there).
fn chain_of(row: &web_sys::Element) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = row.closest(".node").ok().flatten();
    while let Some(node) = at {
        let up = node.parent_element().and_then(|p| p.closest(".kids").ok().flatten()).and_then(|k| k.parent_element());
        if let Some(up) = &up {
            if let Some(k) = up.get_attribute("data-node-key") {
                out.push(k);
            }
        }
        at = up;
    }
    out.reverse();
    out
}

/// ACT-7: where in the nav the Nav ▸ menu opened, and what its items need.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct At {
    pub(crate) place: Place,
    /// A ticket is open: Reveal the open ticket is offered.
    pub(crate) reveal: bool,
    row: Option<RowAt>,
}

#[derive(Clone, Debug, PartialEq)]
struct RowAt {
    key: String,
    chain: Vec<String>,
    open: bool,
    /// The row's shortcode's root — its code for a shortcode with none.
    root: Option<String>,
}

/// ACT-7: the nav place `el` is in — a row of the tree, the flat list, or
/// the nav's empty space or header — `None` outside the nav, before it is
/// mounted, and inside a field or a popover of its own (those keep theirs).
pub(crate) fn at(el: &web_sys::Element) -> Option<At> {
    el.closest("#nav").ok().flatten()?;
    if el.closest("input, textarea, select, [contenteditable], [role=dialog], [role=menu], [role=listbox]").ok().flatten().is_some() {
        return None;
    }
    let h = HOOK.with(Cell::get)?;
    let reveal = h.ctx.route.with_untracked(|r| matches!(r, Route::Ticket(_)));
    if h.view.get_untracked() == View::Flat {
        return Some(At { place: Place::Flat, reveal, row: None });
    }
    let Some(row) = el.closest("#nav .tree [role=treeitem]").ok().flatten() else {
        return Some(At { place: Place::Space, reveal, row: None });
    };
    let node = row.closest(".node").ok().flatten()?;
    let key = node.get_attribute("data-node-key")?;
    let epic = node
        .get_attribute("data-group")
        .or_else(|| crate::kids::key(&key).map(|(e, _)| e.to_string()))
        .unwrap_or_else(|| key.clone());
    let root = h.ctx.tree.groups.with_untracked(|m| {
        m.as_ref().and_then(|m| m.get(&epic)).map(|g| g.root.clone().unwrap_or_else(|| g.epic.clone()))
    });
    let has_kids = row.has_attribute("aria-expanded");
    let place = Place::Row { label: key.clone(), has_kids, shortcode: root.as_ref().map(|_| epic) };
    let row = RowAt { chain: chain_of(&row), open: row.get_attribute("aria-expanded").as_deref() == Some("true"), key, root };
    Some(At { place, reveal, row: Some(row) })
}

/// ACT-7: one Nav ▸ item, done. The shape moves, and the tree follows it —
/// a node that opens asks its first page, and a long list stays a window
/// (SCALE-24): nothing here walks the store.
pub(crate) fn act(op: Op, at: &At) {
    let Some(h) = HOOK.with(Cell::get) else { return };
    if op == Op::Reveal {
        reveal_now(h);
        return;
    }
    let (key, chain, open) = match (&at.row, op) {
        (_, Op::ExpandAll | Op::CollapseAll) => (String::new(), Vec::new(), false),
        (Some(r), Op::ExpandShortcode | Op::CollapseShortcode) => match &r.root {
            Some(root) => (root.clone(), Vec::new(), false),
            None => return,
        },
        (Some(r), _) => (r.key.clone(), r.chain.clone(), r.open),
        (None, _) => return,
    };
    h.ctx.expanded.update(|s| s.apply(op, &key, &chain, open));
}

/// ACT-7: Reveal the open ticket — its chain opens (the tree's reveal, run
/// again whatever it did before; the flat list's, which scrolls to it), the
/// row is centred, and once it stands still it takes focus.
fn reveal_now(h: Hook) {
    let Route::Ticket(id) = h.ctx.route.get_untracked() else { return };
    h.reveal.update(|n| *n = n.wrapping_add(1));
    spawn_local(async move {
        for _ in 0..200 {
            dom::sleep_ms(25).await;
            if selected_row(&id).is_some() {
                break;
            }
        }
        settle_centred(id.clone(), || true).await;
        if let Some(row) = selected_row(&id) {
            keys::focus_and_rove(&tree_items(), &row);
        }
    });
}

/// ACT-7: the tree's own keys for its shape — `*` the focused row's
/// subtree (WAI-ARIA's tree key), Shift+`*` everything, `-` back to the
/// shortcodes. A `*` typed with Shift because the layout puts it there
/// (Shift+8) is a plain `*`: Shift counts only on the numpad's, whose `*`
/// needs none (AGS-28: the table's `*` and `Shift+Numpad*`). True when the
/// key was one of them.
fn shape_key(id: &str, current: &web_sys::Element) -> bool {
    let op = match id {
        "nav.expand-all" => Op::ExpandAll,
        "nav.expand-subtree" => Op::ExpandSubtree,
        "nav.collapse-all" => Op::CollapseAll,
        _ => return false,
    };
    if let Some(at) = at(current) {
        act(op, &at);
    }
    true
}

thread_local! {
    /// SCALE-24: a step across a gap in flight — its direction, and how many
    /// more steps were pressed while it waited. One at a time: every press
    /// counts once, and a held key does not start a crossing per repeat.
    /// ACT-3: and whether it extends the selection (Shift) rather than
    /// opening where it lands.
    static CROSSING: Cell<Option<(bool, u32, bool)>> = const { Cell::new(None) };
}

/// AGS-26: what a walkable element is to a step of the nav's cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// A ticket's row: a step lands here and opens it.
    Ticket,
    /// A row that is no ticket — a shortcode's heading (FILTER-23), a
    /// rootless shortcode's row: a step passes over it.
    Group,
    /// Rows not fetched yet — a spacer, a pending stretch: a step crosses it
    /// once they are rows ([`cross_gap`]).
    Gap,
}

/// ACT-3: an extending step (`extend`) stops on every selectable row — a
/// shortcode's heading too, which stands for its root.
fn stop_of(el: &web_sys::Element, extend: bool) -> Stop {
    if is_gap(el) {
        Stop::Gap
    } else if el.has_attribute(if extend { selection::PICK } else { "data-nav-id" }) {
        Stop::Ticket
    } else {
        Stop::Group
    }
}

/// AGS-26: where a step from `at` goes (towards the end when `down`) — the
/// next ticket's row or gap, passing over the rows that are no ticket;
/// `None` at the nav's end, which ↑ at the top widens ([`widen`]).
pub(crate) fn next_stop(stops: &[Stop], at: usize, down: bool) -> Option<usize> {
    if down {
        (at + 1..stops.len()).find(|&j| stops[j] != Stop::Group)
    } else {
        (0..at.min(stops.len())).rev().find(|&j| stops[j] != Stop::Group)
    }
}

/// AGS-26: the cursor lands on `row` — it takes focus (and the tree's tab
/// stop) and, a ticket's row, its ticket opens: the cursor and the open
/// ticket are one row.
///
/// ACT-3: a step is a click — its row becomes the selection.
fn land(row: &web_sys::HtmlElement) {
    keys::focus_and_rove(&tree_items(), row);
    if let Some(id) = selection::pick_of(row) {
        selection::set(Picked::only(&id));
    }
    if let Some(id) = row.get_attribute("data-nav-id") {
        crate::navigate_to_ticket(&id);
    }
}

/// ACT-3: the ids of the rows the nav draws, top to bottom — the tree's (or
/// the flat list's) selectable rows that are mounted and shown.
pub(crate) fn drawn_order() -> Vec<String> {
    let rows = keys::query_all(&format!("#nav .tree [{}]", selection::PICK));
    selection::drawn_order(rows.into_iter().filter(keys::is_visible).filter_map(|el| selection::pick_of(&el)))
}

/// ACT-3: an extending step lands on `row` — it takes focus, nothing opens,
/// and the selection runs from the anchor (or `from`, the row the step left)
/// to it.
fn extend_to(row: &web_sys::HtmlElement, from: &str) {
    keys::focus_and_rove(&tree_items(), row);
    if let Some(to) = selection::pick_of(row) {
        selection::set(selection::now().extended(&drawn_order(), from, &to));
    }
}

/// ACT-3: a row's landing — opening it (a step) or extending the selection
/// to it from `from`.
fn arrive(row: &web_sys::HtmlElement, extend: bool, from: &str) {
    if extend {
        extend_to(row, from);
    } else {
        land(row);
    }
}

/// SCALE-24: a step from the row `from` into the gap beside it — a spacer,
/// or rows still being fetched. Scroll the least that brings the gap's near
/// edge (and a row of it) into view, so the window mounts and fetches what
/// is there; then land on the ticket row next to `from` once it is a row
/// (AGS-26: and open it). Polled a frame at a time, as the board's j/k does
/// past its window (MFE-12), for as long as a fetch can reasonably take. A
/// press while it waits is one more step, taken when it lands
/// ([`tree_keydown`]) — the cursor lands, and a ticket opens, once.
fn cross_gap(from: String, down: bool, extend: bool) {
    if let Some((dir, extra, ext)) = CROSSING.with(Cell::get) {
        // One in flight: this press is a step more (or one less, against it).
        let extra = if dir == down { extra + 1 } else { extra.saturating_sub(1) };
        CROSSING.with(|c| c.set(Some((dir, extra, ext))));
        return;
    }
    CROSSING.with(|c| c.set(Some((down, 0, extend))));
    // ACT-3: the row an extending step left — the anchor when none is drawn.
    let origin = from.clone();
    spawn_local(async move {
        let mut from = from;
        for _ in 0..240 {
            let els = walkable();
            let Some(i) = els.iter().position(|el| el.get_attribute("data-node-key").as_deref() == Some(&from)) else {
                break; // the row we started from is gone: nothing to step from
            };
            let stops: Vec<Stop> = els.iter().map(|el| stop_of(el, extend)).collect();
            let Some(j) = next_stop(&stops, i, down) else { break };
            if stops[j] != Stop::Gap {
                // The steps pressed while this one waited, through rows
                // that are mounted, until another gap (or none are left).
                let mut extra = CROSSING.with(Cell::get).map_or(0, |(_, e, _)| e);
                let mut at = j;
                while extra > 0 {
                    match next_stop(&stops, at, down) {
                        Some(n) if stops[n] == Stop::Ticket => {
                            at = n;
                            extra -= 1;
                        }
                        _ => break,
                    }
                }
                arrive(&els[at], extend, &origin);
                if extra == 0 {
                    break;
                }
                // Another gap with steps still to take: cross it from here.
                CROSSING.with(|c| c.set(Some((down, extra - 1, extend))));
                let Some(key) = els[at].get_attribute("data-node-key") else { break };
                from = key;
                continue;
            }
            if let Some(sc) = scroller() {
                let (s, g) = (sc.get_bounding_client_rect(), els[j].get_bounding_client_rect());
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

/// FILTER-20: Home, when the nav begins in a gap — the flat list scrolled
/// away from its top (the tree's top rows are always mounted). Scroll to the
/// top and focus the first row once it is a row.
fn to_the_top() {
    spawn_local(async move {
        for _ in 0..240 {
            let els = walkable();
            match els.first() {
                Some(first) if !is_gap(first) => {
                    keys::focus_and_rove(&tree_items(), first);
                    return;
                }
                None => return,
                Some(_) => {
                    if let Some(sc) = scroller() {
                        sc.set_scroll_top(0);
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
/// Up/Down (j/k) walk the visible rows; l opens a closed node or steps into
/// an open one; h closes an open node or steps out to its parent row; Enter
/// opens the focused ticket (or toggles a group row); Home/End jump.
/// AGS-20: "node" is any row with children — an epic's shortcode row or a
/// ticket that decomposes — so the map is the same at every depth.
/// Everything else — `/`, `?`, `g h` — is left to bubble to the global
/// listener.
///
/// AGS-26: **the cursor is the open ticket.** A step (Up/Down, j/k, and h/l's
/// steps) lands on the next ticket's row — a shortcode's heading is passed
/// over — and opens it ([`land`]); Up on the first ticket widens an `under`
/// filter by one parent ([`widen`]). AGS-27: Left and Right are left to the
/// browser — they no longer open and close (h and l do) and move no focus.
///
/// SCALE-24: the visible rows are the mounted ones, and a windowed list
/// mounts only those near the viewport. A step whose next row is a gap — a
/// spacer, or a row still being fetched — scrolls it into the window and
/// lands when it is mounted ([`cross_gap`]); End does the same for the
/// end of the tree.
fn tree_keydown(ev: web_sys::KeyboardEvent, ctx: Ctx, lens: Lens) {
    if ev.default_prevented() {
        return;
    }
    // AGS-28: what the key is in the nav. `g h` still reaches the board
    // from inside the tree: the `h` that ends it is no collapse here (the
    // table leaves it to the app's map, which owns the sequence).
    let Some(id) = bindings::hit(&ev, Context::Nav) else { return };
    if select_key(id, &ev, ctx, lens) {
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
    // ACT-7: the row's chain says what it inherits; it opens or closes the
    // other way from how it is drawn.
    let toggle = |k: Option<String>| {
        if let Some(k) = k {
            let chain = chain_of(&current);
            ctx.expanded.update(|e| e.set_open(&chain, &k, !open));
        }
    };

    // ACT-7: `*`, Shift+`*`, `-`.
    if shape_key(id, &current) {
        ev.prevent_default();
        ev.stop_propagation();
        return;
    }
    let handled = match id {
        "move.next" => {
            step_from(&steps, at, true, ctx, lens, false);
            true
        }
        "move.prev" => {
            step_from(&steps, at, false, ctx, lens, false);
            true
        }
        "move.first" => {
            match steps.first() {
                // FILTER-20: the flat list away from its top.
                Some(first) if is_gap(first) => to_the_top(),
                _ => keys::focus_and_rove(&all, &items[0]),
            }
            true
        }
        "move.last" => {
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
        "nav.expand" => {
            if expandable {
                if !open {
                    toggle(node_key.clone());
                } else {
                    step_from(&steps, at, true, ctx, lens, false);
                }
            }
            true
        }
        // Close an open node in place; otherwise step out to the row it
        // hangs under (which is where a second h then closes it).
        "nav.collapse" => {
            if expandable && open {
                toggle(node_key.clone());
            } else if let Some(row) = parent_row(&current) {
                land(&row);
            }
            true
        }
        "open.focused" => {
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

/// ACT-3: the tree's selection keys, on the focused row — Shift+↑/↓ and
/// Shift+j/k extend the selection a row, Space toggles the row, Ctrl/Cmd+A
/// selects every drawn row. True when the key was one of them. (Esc, which
/// clears, is the app's: src/keys.rs.)
fn select_key(id: &str, ev: &web_sys::KeyboardEvent, ctx: Ctx, lens: Lens) -> bool {
    if id == "select.all" {
        selection::set(Picked::all(&drawn_order()));
        return true;
    }
    let Some(current) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .and_then(|t| t.closest("[role=treeitem]").ok().flatten())
    else {
        return false;
    };
    let down = match id {
        "select.toggle" => {
            if let Some(id) = selection::pick_of(&current) {
                selection::set(selection::now().toggled(&id));
            }
            return true;
        }
        "select.extend-down" => true,
        "select.extend-up" => false,
        _ => return false,
    };
    let steps = walkable();
    if let Some(at) = steps.iter().position(|el| el.is_same_node(Some(&current))) {
        step_from(&steps, at, down, ctx, lens, true);
    }
    true
}

/// AGS-26: a step of the nav's cursor from `steps[at]` — onto the next
/// ticket's row, which opens; across a gap once it is fetched; and Up with no
/// ticket above, the nav's `under` filter one parent wider.
///
/// ACT-3: `extend` (Shift) — the step moves focus and extends the selection
/// from this row to the next selectable one; it opens nothing and widens
/// nothing.
fn step_from(steps: &[web_sys::HtmlElement], at: usize, down: bool, ctx: Ctx, lens: Lens, extend: bool) {
    if CROSSING.with(Cell::get).is_some() {
        // a crossing is in flight: this press is counted there
        cross_gap(String::new(), down, extend);
        return;
    }
    let stops: Vec<Stop> = steps.iter().map(|el| stop_of(el, extend)).collect();
    let from = steps[at].get_attribute(selection::PICK).or_else(|| steps[at].get_attribute("data-node-key")).unwrap_or_default();
    match next_stop(&stops, at, down) {
        Some(j) if stops[j] == Stop::Gap => {
            if let Some(k) = steps[at].get_attribute("data-node-key") {
                cross_gap(k, down, extend);
            }
        }
        Some(j) => arrive(&steps[j], extend, &from),
        None if extend => {}
        None if !down => {
            widen(lens, ctx.tree);
        }
        None => {}
    }
}

/// AGS-26: the nav's keys where focus is not in its tree — on a ticket page,
/// anywhere but a text field, a dialog, a menu or an overlay, while the nav
/// is shown: Up/Down (k/j) step the cursor from the open ticket's row, and
/// `f` filters the nav to the open ticket and its descendants. The open
/// ticket is always drawn and selected (FILTER-24, [`reveal`]), so the keys
/// always have a row to start from.
fn page_keydown(ev: web_sys::KeyboardEvent, ctx: Ctx, lens: Lens, hidden: RwSignal<bool>) {
    if ev.default_prevented() {
        return;
    }
    let Some(Route::Ticket(id)) = ctx.route.try_get_untracked() else { return };
    if hidden.try_get_untracked() != Some(false) || !keys::no_overlay() {
        return;
    }
    let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    if target.as_ref().is_some_and(|t| keys::defer_to_focus(t) || t.closest("#nav .tree").ok().flatten().is_some()) {
        return;
    }
    let handled = match bindings::hit(&ev, Context::TicketPage) {
        Some("move.next") => {
            step_selected(id, true, ctx, lens);
            true
        }
        Some("move.prev") => {
            step_selected(id, false, ctx, lens);
            true
        }
        Some("filter.view") => {
            filter_to(&id, ctx, lens);
            true
        }
        _ => false,
    };
    if handled {
        ev.prevent_default();
    }
}

/// AGS-26: a step from the open ticket's row — at once when it is drawn,
/// otherwise once it is (a deep link still revealing it), for a while.
fn step_selected(id: String, down: bool, ctx: Ctx, lens: Lens) {
    let from_row = move || {
        let row = selected_row(&id)?;
        let steps = walkable();
        let at = steps.iter().position(|el| el.is_same_node(Some(&row)))?;
        step_from(&steps, at, down, ctx, lens, false);
        Some(())
    };
    if from_row().is_some() {
        return;
    }
    spawn_local(async move {
        for _ in 0..40 {
            dom::sleep_ms(50).await;
            if from_row().is_some() {
                return;
            }
        }
    });
}

/// AGS-26: `f` on a ticket page — the nav's filter becomes the ticket and its
/// descendants, `under <id>` (what its tag's "Filter under" makes, FILTER-22),
/// and its row opens to show them. The open ticket is centred in the new tree
/// ([`reveal`], once per filter).
fn filter_to(id: &str, ctx: Ctx, lens: Lens) {
    lens.edit(Level::Nav, |k| *k = tags::only(&tags::TagClause::Under(id.to_string()), k));
    if ctx.expanded.with_untracked(|e| !e.marked_open(id)) {
        ctx.expanded.update(|e| e.force_open(id));
    }
}

/// AGS-26: the first `under T` group of a filter — at its place, with T
/// (a ticket or a shortcode).
pub(crate) fn under_group(f: &Filter) -> Option<(usize, String)> {
    f.groups.iter().enumerate().find_map(|(i, g)| match g.preds.as_slice() {
        [p] if !p.not => match &p.field {
            Field::Under(t) => Some((i, t.clone())),
            _ => None,
        },
        _ => None,
    })
}

/// AGS-26: the filter with its group at `at` made `under up` — or, with no
/// `up` (T had no parent), taken out.
pub(crate) fn with_under(f: &Filter, at: usize, up: Option<&str>) -> Filter {
    let mut next = f.clone();
    if at < next.groups.len() {
        match up {
            Some(p) => next.groups[at] = FGroup::one(Predicate::is(Field::Under(p.to_ascii_uppercase()))),
            None => {
                next.groups.remove(at);
            }
        }
    }
    next
}

/// AGS-26: Up on the nav's first ticket — its filter one parent more
/// permissive: its first `under T` group becomes `under` T's parent (for a
/// shortcode, its root's host), and goes when T has none. Nothing without
/// such a group. The open ticket stays selected and is centred in the wider
/// tree ([`reveal`], once per filter).
fn widen(lens: Lens, tree: Tree) {
    let Some(kept) = untrack(|| lens.kept(Level::Nav)) else { return };
    let Some((at, t)) = under_group(&kept.filter) else { return };
    spawn_local(async move {
        let up = if key(&t).is_some() {
            match get_summary(&t).await {
                Some(s) => s.parent,
                None => return, // the server could not say: the filter stays
            }
        } else {
            let sc = t.to_ascii_uppercase();
            match tree.groups.with_untracked(|m| m.as_ref().map(|m| m.get(&sc).and_then(|g| g.host.clone()))) {
                Some(host) => host,
                None => return, // no groups yet
            }
        };
        lens.edit(Level::Nav, |k| {
            // Only the filter that was widened: a change since is the reader's.
            if under_group(&k.filter).is_some_and(|(a, x)| a == at && x == t) {
                k.filter = with_under(&k.filter, at, up.as_deref());
            }
        });
    });
}

/// What every node in the tree needs, in one Copy bundle (signals and
/// memos are handles).
#[derive(Clone, Copy)]
struct Ctx {
    tree: Tree,
    route: RwSignal<Route>,
    /// ACT-7: what is open — a [`Shape`], not a set of keys.
    expanded: RwSignal<Shape>,
    /// FILTER-14: the nav's lens.
    under: Under,
}

/// FILTER-14: the nav under its effective filter (`global ∧ nav`) — every
/// handle `Copy`. Off (no group in the filter), nothing here is asked and
/// every node draws as it did.
#[derive(Clone, Copy)]
struct Under {
    /// The effective filter has a group: the nav is under the lens.
    on: Memo<bool>,
    /// What the server is asked: the filter's groups as text — `None` while
    /// off, and while the filter admits nothing (nothing is asked then).
    text: Memo<Option<String>>,
    /// Each shortcode's matches, summed along the hosts — `None` while off,
    /// and until counted.
    sums: Memo<Option<BTreeMap<String, usize>>>,
    /// The hidden line toggled on: the shortcodes with no match drawn,
    /// dimmed.
    show_hidden: RwSignal<bool>,
    /// What `tree` said of each row it answered, by id — for this filter.
    marks: RwSignal<HashMap<String, Mark>>,
    /// The server did not take the filter: its words (the stream ended).
    refused: RwSignal<Option<String>>,
    /// FILTER-24: the open ticket while the nav is under the lens — kept in
    /// the tree whatever the filter says of it.
    kept: Memo<Option<String>>,
    /// FILTER-24: the shortcodes leading to it ([`kept_codes`]).
    kept_codes: Memo<BTreeSet<String>>,
}

impl Under {
    fn new(lens: Lens, route: RwSignal<Route>) -> Self {
        let eff = lens.effective(Level::Nav);
        let on = Memo::new(move |_| eff.with(|f| !f.groups.is_empty()));
        let text = Memo::new(move |_| eff.with(|f| (!f.groups.is_empty() && f.admits_nothing().is_none()).then(|| wire_text(f))));
        // `counts { filter }`, kept live by its membership watch (FILTER-12).
        let live = LiveCounts::new(text.into());
        let sctree = lens.sctree();
        let sums = Memo::new(move |_| {
            if !on.get() {
                return None;
            }
            let nothing = text.with(Option::is_none);
            let held = if nothing { None } else { Some(live.get()?) };
            sctree.with(|t| t.as_ref().map(|t| summed(t, |e| held.as_ref().map_or(0, |m| m.held(e)))))
        });
        let marks = RwSignal::new(HashMap::new());
        let refused = RwSignal::new(None);
        // A new filter: what the old one's tree said is nothing to this one.
        Effect::new(move |prev: Option<Option<String>>| {
            let now = text.get();
            if prev.is_some_and(|p| p != now) {
                marks.set(HashMap::new());
                refused.set(None);
            }
            now
        });
        let kept = Memo::new(move |_| {
            if !on.get() {
                return None;
            }
            route.with(|r| match r {
                Route::Ticket(id) => Some(id.clone()),
                _ => None,
            })
        });
        let kept_codes = Memo::new(move |_| {
            let Some(id) = kept.get() else { return BTreeSet::new() };
            let own = || key(&id).map(|(e, _)| BTreeSet::from([e.to_string()])).unwrap_or_default();
            sctree.with(|t| t.as_ref().map(|t| kept_codes(t, &id))).unwrap_or_else(own)
        });
        Under { on, text, sums, show_hidden: RwSignal::new(false), marks, refused, kept, kept_codes }
    }

    /// FILTER-24: how shortcode `epic`'s row stands to the open ticket
    /// (tracked) — `root` the row's root, when it has one.
    fn holds(self, epic: &str, root: Option<&str>) -> Holds {
        if root.is_some() && self.kept.with(|k| k.as_deref() == root) {
            return Holds::Root;
        }
        if self.kept_codes.with(|c| c.contains(epic)) {
            Holds::Under
        } else {
            Holds::No
        }
    }

    /// FILTER-24: whether `id` is the open ticket, kept (tracked).
    fn is_kept(self, id: &str) -> bool {
        self.kept.with(|k| k.as_deref() == Some(id))
    }

    /// Shortcode `epic`'s matches (tracked) — `None` while off or uncounted.
    fn count(self, epic: &str) -> Option<usize> {
        self.sums.with(|s| s.as_ref().map(|s| s.get(epic).copied().unwrap_or(0)))
    }

    /// What `tree` said of `id` (tracked).
    fn mark(self, id: &str) -> Option<Mark> {
        self.marks.with(|m| m.get(id).copied())
    }

    /// The membership watch's word, for the nav's open lists (the counts
    /// hear it on their own): a ticket moved in or out — its chain asks
    /// again; a recount or a gap — every open list does.
    fn heard(self, ev: &Member) {
        match ev {
            Member::Entered { id, .. } | Member::Left { id, .. } => chain_moved(id.clone()),
            Member::Recount { .. } | Member::Lagged => ask_again(|_| true),
            Member::Refused(message) => {
                self.refused.try_set(Some(message.clone()));
            }
            Member::Opened => {}
        }
    }
}

/// SCALE-24: the chain from a deep link's ticket up to the top level, one
/// `get` per ancestor, as the nav draws it: each step is a node to open and
/// the child it must show — a ticket's parent, the host for a sub-shortcode's
/// root. A shortcode with no root lists its tickets flat, so its row is the
/// whole chain. AGS-23: a root is its shortcode's row, so no shortcode row
/// opens above it — at the top level the chain ends there ([`climb`]).
async fn chain_to(tree: Tree, leaf: &str) -> Option<Vec<(Node, Option<TicketSummary>)>> {
    let mut steps = Vec::new();
    let mut cur = get_summary(leaf).await?;
    for _ in 0..64 {
        let g = tree.groups.with_untracked(|m| m.as_ref().and_then(|m| m.get(&cur.epic)).cloned());
        match climb(g.as_ref(), &cur) {
            Climb::Top => return Some(steps),
            Climb::Host(h) => {
                let host = get_summary(&h).await?;
                steps.push((Node::Ticket(h), None));
                cur = host;
            }
            Climb::Flat(epic) => {
                steps.push((Node::Group(epic), Some(cur)));
                return Some(steps);
            }
            Climb::Parent(p) => {
                let parent = get_summary(&p).await?;
                steps.push((Node::Ticket(p), Some(cur)));
                cur = parent;
            }
            Climb::Lost => return None,
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
    // FILTER-14: under the lens the chain opens down to the deepest row the
    // tree draws, which is what is brought into view — FILTER-23: through the
    // rows it draws only, each under the nearest drawn one above it.
    let (steps, leaf) = match ctx.under.text.get_untracked() {
        Some(filter) => {
            let Some((steps, at)) = drawn_chain(ctx, &filter, &leaf, steps).await else { return };
            if REVEAL_GEN.with(Cell::get) != gen {
                return;
            }
            (steps, at)
        }
        None if ctx.under.on.get_untracked() => return, // a filter that admits nothing draws nothing
        None => (steps, leaf),
    };
    let keys: Vec<String> = steps.iter().map(|(n, _)| n.key().to_string()).collect();
    let missing: Vec<String> = ctx.expanded.with_untracked(|e| keys.into_iter().filter(|k| !e.marked_open(k)).collect());
    if !missing.is_empty() {
        ctx.expanded.update(|e| missing.iter().for_each(|k| e.force_open(k)));
    }
    ctx.tree.reveal.set(Some(Reveal { gen, steps }));
    for _ in 0..300 {
        dom::sleep_ms(16).await;
        if REVEAL_GEN.with(Cell::get) != gen {
            return;
        }
        // AGS-26: the selected row, centred and kept there as the lists
        // around it settle.
        if selected_row(&leaf).is_some() {
            settle_centred(leaf, move || REVEAL_GEN.with(Cell::get) == gen).await;
            return;
        }
        // FILTER-14: the open ticket outside the lens — its deepest drawn
        // ancestor, centred (FILTER-23: a shortcode's heading among them).
        let row = dom::window()
            .document()
            .and_then(|d| d.query_selector(&format!("#nav .row[data-node-key=\"{leaf}\"]")).ok().flatten());
        if let Some(row) = row {
            if let Some(sc) = scroller() {
                center_row(&sc, &row);
            }
            return;
        }
    }
}

/// FILTER-14: a deep link's chain under the lens — `steps` as [`chain_to`]
/// found them (each a node to open and the child it must show) — FILTER-23:
/// as the lens draws it, and the row the chain now ends at. Only a ticket
/// that matches is a row — `member { ids, filter }` says which of the chain
/// do, one call — and a shortcode's row (its root, or a rootless one's row)
/// that has matches; each row is under the nearest drawn one above it
/// ([`drawn_steps`]). The open ticket's row when it matches, else its
/// deepest drawn ancestor's. `None` when nothing on the chain is drawn, or
/// the server cannot say.
async fn drawn_chain(ctx: Ctx, filter: &str, leaf: &str, steps: Vec<(Node, Option<TicketSummary>)>) -> Option<(Vec<(Node, Option<TicketSummary>)>, String)> {
    let mut chain: Vec<Node> = vec![Node::Ticket(leaf.to_string())];
    chain.extend(steps.iter().map(|(n, _)| n.clone()));
    let mut rows: HashMap<String, TicketSummary> = steps.into_iter().filter_map(|(_, c)| c).map(|t| (t.id.clone(), t)).collect();
    let ids: Vec<String> = chain
        .iter()
        .filter_map(|n| match n {
            Node::Ticket(id) => Some(id.clone()),
            _ => None,
        })
        .collect();
    let matched: HashSet<String> = match session::member(ids.clone(), filter.to_string()).await {
        Ok(m) => m.into_iter().filter(|(_, m)| *m).map(|(id, _)| id).collect(),
        Err(why) => {
            leptos::logging::log!("nav: member: {why}");
            return None;
        }
    };
    let roots: HashMap<String, String> = ctx
        .tree
        .groups
        .with_untracked(|m| m.iter().flat_map(|m| m.values()).filter_map(|g| g.root.clone().map(|r| (r, g.epic.clone()))).collect());
    let every = ctx.under.show_hidden.get_untracked();
    // FILTER-24: the shortcodes leading to the open ticket are drawn, and so
    // is the open ticket itself — kept whatever the filter.
    let kept = ctx.under.kept_codes.get_untracked();
    let has = |epic: &str| every || kept.contains(epic) || ctx.under.sums.with_untracked(|s| s.as_ref().and_then(|s| s.get(epic)).is_some_and(|n| *n > 0));
    let drawn: Vec<bool> = chain
        .iter()
        .map(|n| match n {
            Node::Ticket(id) => id == leaf || matched.contains(id) || roots.get(id).is_some_and(|e| has(e)),
            Node::Group(epic) | Node::Root(_, epic) => has(epic),
        })
        .collect();
    let (opens, at) = drawn_steps(&chain, &drawn)?;
    // Each child a step must show, as the windowed list asks after it: the
    // step that showed it carried it; a hosted root (its host's step carried
    // none: the root is drawn after the host's children), from what the app
    // holds.
    let mut out = Vec::with_capacity(opens.len());
    for (node, child) in opens {
        if !rows.contains_key(&child) {
            if let Some(t) = row_of(&child).await {
                rows.insert(child.clone(), t);
            }
        }
        out.push((node, rows.get(&child).cloned()));
    }
    Some((out, at))
}

/// The left panel: brand header + the tree. Always mounted; the App hides
/// it with a class so its DOM (and every row's identity) survives a
/// hide/reveal.
#[component]
pub(crate) fn Nav(tree: Tree, route: RwSignal<Route>, hidden: RwSignal<bool>) -> impl IntoView {
    let expanded = RwSignal::new(load_expanded());
    Effect::new(move |_| expanded.with(save_expanded));
    // AGS-23: an open shortcode's code, persisted before its row became its
    // root's, opens the root — as the groups that say which root arrive.
    Effect::new(move |_| {
        let moved = tree.groups.with(|m| m.as_ref().and_then(|m| expanded.with_untracked(|e| migrate_expanded(e, m))));
        if let Some(next) = moved {
            expanded.set(next);
        }
    });
    // FILTER-14: the nav's lens — its effective filter, the shortcodes'
    // matches, what `tree` says of each row; and the membership watch that
    // keeps the open lists live (shared with the counts' own: one stream).
    let lens = use_lens();
    let under = Under::new(lens, route);
    // FILTER-24: the open ticket is kept in every list's answer; when it
    // moves, the lists on the old one's chain and the new one's ask again —
    // but for a ticket the tree already said matches (keeping it moved
    // nothing).
    Effect::new(move |prev: Option<Option<String>>| {
        let now = under.kept.get();
        KEPT.with(|k| *k.borrow_mut() = now.clone());
        let matched = |id: &str| under.marks.with_untracked(|m| m.get(id).is_some_and(|m| m.matched));
        if let Some(Some(old)) = prev.filter(|p| *p != now) {
            if !matched(&old) {
                chain_moved(old);
            }
        }
        if let Some(new) = now.clone().filter(|n| !matched(n)) {
            chain_moved(new);
        }
        now
    });
    session::follow_membership(move || under.text.get(), move |ev| under.heard(ev));
    // FILTER-21: menger's own documents are no toggle here — the nav's
    // filter leaves them out by default (`not menger doc`, src/lens.rs).
    let ctx = Ctx { tree, route, expanded, under };
    // ACT-7: Reveal the open ticket's count — the reveals below follow it.
    let reveal_ask = RwSignal::new(0u32);
    // FILTER-20: Tree or Flat, the choice kept with the nav's other state. A
    // switch starts the nav at its top (the open ticket is brought back into
    // view by the view it switched to).
    let nav_view = RwSignal::new(flat::load_view());
    HOOK.with(|h| h.set(Some(Hook { ctx, view: nav_view, reveal: reveal_ask })));
    Effect::new(move |prev: Option<View>| {
        let now = nav_view.get();
        if prev.is_some_and(|p| p != now) {
            flat::save_view(now);
            if let Some(sc) = scroller() {
                sc.set_scroll_top(0);
            }
        }
        now
    });
    // FILTER-20: what the flat list shows — the nav's filter, or every ticket
    // — and the line it has to say under itself.
    let flat_filter = Memo::new(move |_| flat::list_filter(under.on.get(), under.text.get()));
    let flat_note = RwSignal::new(None::<flat::Note>);
    let top = tree.top;
    // AGS-23: the top-level rows, each shortcode as the node it is drawn as;
    // the last of them by its key. FILTER-14: under the lens, those with a
    // match — or all, the rest dimmed, when the hidden line says so — and
    // none until they are counted.
    let top_nodes = Memo::new(move |_| {
        let all = top.with(|t| tree.nodes(t));
        if !under.on.get() {
            return all;
        }
        let every = under.show_hidden.get();
        let kept = under.kept_codes.get();
        under.sums.with(|s| s.as_ref().map(|s| shown(all, s, every, &kept)).unwrap_or_default())
    });
    let top_last = Memo::new(move |_| top_nodes.with(|n| n.last().map(|n| n.key().to_string())));
    // FILTER-14: the last line under the lens — how many shortcodes the
    // filter hides, toggling them in (dimmed).
    let hidden_n = Memo::new(move |_| {
        let kept = under.kept_codes.get();
        under.sums.with(|s| s.as_ref().map(|s| hidden_count(s, &kept)))
    });
    let show_hidden = under.show_hidden;
    let lens_line = move || {
        if !under.on.get() {
            return None;
        }
        if let Some(why) = under.refused.get() {
            return Some(view! { <div class="nlens-note" data-nlens="refused">"The server did not take the filter: "{why}</div> }.into_any());
        }
        let line = match hidden_n.get() {
            None => view! { <div class="nlens-note" data-nlens="counting">"counting…"</div> }.into_any(),
            Some(0) => return None,
            Some(n) => view! {
                <button
                    class="nhidden"
                    data-nhidden=n
                    aria-pressed=move || show_hidden.get().to_string()
                    title=move || if show_hidden.get() { "hide the shortcodes with no match" } else { "show the shortcodes with no match, dimmed" }
                    on:click=move |_| show_hidden.update(|s| *s = !*s)
                >
                    {hidden_line(n)}
                    {move || if show_hidden.get() { " · shown dimmed" } else { "" }}
                </button>
            }
            .into_any(),
        };
        Some(line)
    };

    // Selection follows the route. The chain holding the open ticket
    // auto-expands so the selected row is visible — once per navigation
    // (`revealed` remembers which id was handled), so a later live insert
    // does not fight a node the user then chose to close. SCALE-24: the
    // chain comes from the server (`get` per ancestor), so it waits for the
    // first groups — not for any walk.
    // AGS-26: and once per filter — a new nav filter draws a new tree, and the
    // open ticket is centred in it again. A row already drawn (a step of the
    // cursor, a click) is centred where it is, with no chain to fetch.
    let revealed = StoredValue::new(None::<(String, Option<String>)>);
    // ACT-7: Reveal the open ticket asks again: what was revealed is forgotten.
    let asked = StoredValue::new(0u32);
    Effect::new(move |_| {
        let ask = reveal_ask.get();
        if asked.get_value() != ask {
            asked.set_value(ask);
            revealed.set_value(None);
        }
        let Route::Ticket(id) = route.get() else { return };
        let filter = under.text.get();
        if tree.epoch.get() == 0 {
            return; // no groups yet: this effect re-runs when there are
        }
        // FILTER-20: the flat list brings the open ticket into view its own
        // way; back in the tree, its chain is revealed again.
        if nav_view.get() != View::Tree {
            revealed.set_value(None);
            return;
        }
        let was = revealed.get_value();
        if was.as_ref().is_some_and(|(i, f)| *i == id && *f == filter) {
            return;
        }
        let same_tree = was.is_some_and(|(_, f)| f == filter);
        revealed.set_value(Some((id.clone(), filter)));
        let gen = REVEAL_GEN.with(|g| {
            g.set(g.get().wrapping_add(1));
            g.get()
        });
        spawn_local(async move {
            dom::sleep_ms(0).await;
            if same_tree && selected_row(&id).is_some() {
                settle_centred(id, move || REVEAL_GEN.with(Cell::get) == gen).await;
            } else {
                reveal(ctx, id, gen).await;
            }
        });
    });

    // AGS-18: keep exactly one row as the tree's tab stop as the tree
    // changes shape (rows arrive, a node opens, the route moves). The
    // work is a DOM read plus at most two attribute writes, a tick after
    // the render that changed things. SCALE-24: and every windowed list
    // measures again — opening a node moves everything below it.
    Effect::new(move |_| {
        top_nodes.track();
        expanded.track();
        let _ = route.get();
        spawn_local(async move {
            dom::sleep_ms(0).await;
            ensure_tree_tab_stop();
            tree.viewport.notify();
        });
    });
    let _ = window_event_listener(leptos::ev::resize, move |_| tree.viewport.notify());
    // AGS-26: the nav's keys from anywhere on a ticket page.
    let _ = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| page_keydown(ev, ctx, lens, hidden));
    let tree_ref = NodeRef::<leptos::html::Div>::new();
    tree_ref.on_load(|el: web_sys::HtmlDivElement| keep_focus(&el));

    view! {
        <aside
            id="nav"
            class="nav"
            // FILTER-22: a tag in the nav builds the nav's filter.
            data-tag-level="nav"
            class:lensed=move || under.on.get()
            // FILTER-14: `f` opens the nav's filter while the nav has focus —
            // AGS-26: on a ticket page, filters it to the open ticket's tree.
            on:keydown=move |ev| nav_f(ev, ctx, lens)
        >
            <div class="brand">
                "menger"
                <span class="muted">"· tickets"</span>
                // FILTER-20: the nav's view — the tree, or every ticket its
                // filter admits as one list.
                <span class="nview" role="group" aria-label="the nav's view">
                    {View::ALL
                        .map(|v| {
                            view! {
                                <button
                                    class="nview-b"
                                    data-view=v.name()
                                    aria-pressed=move || (nav_view.get() == v).to_string()
                                    title=match v {
                                        View::Tree => "the nav as a tree: shortcodes, their tickets, their children",
                                        View::Flat => "the nav as one list: every ticket its filter admits, in id order",
                                    }
                                    on:click=move |_| nav_view.set(v)
                                >
                                    {v.label()}
                                </button>
                            }
                        })
                        .to_vec()}
                </span>
                <button
                    class="nav-hide"
                    title="hide the nav"
                    on:click=move |_| hidden.set(true)
                >
                    "‹"
                </button>
            </div>
            // FILTER-14: the nav's bar — FILTER-11's generic bar at level
            // nav: the server-backed categories, the inherited global pills,
            // the menu's counts from `facets`.
            <div class="nbar">
                <FilterBar level=Level::Nav cats=Cat::SERVER.to_vec() counts=crate::counts::provider(Level::Nav) hint="Filter the nav (f — on a ticket page, f filters it to that ticket and its children)" />
            </div>
            <div class="tree-scroll" on:scroll=move |_| tree.viewport.notify()>
                <div
                    class="tree"
                    role="tree"
                    aria-label="tickets"
                    // ACT-3: rows are selected many at a time.
                    aria-multiselectable="true"
                    data-view=move || nav_view.get().name()
                    node_ref=tree_ref
                    on:keydown=move |ev| tree_keydown(ev, ctx, lens)
                    on:click=tree_click
                    on:mousedown=|ev: web_sys::MouseEvent| {
                        // ACT-3: a Shift+click selects rows, not text.
                        if bindings::hit_mouse(&ev, Context::Nav) == Some("select.range") {
                            ev.prevent_default();
                        }
                    }
                >
                    {move || match nav_view.get() {
                        View::Tree => view! {
                            <For
                                each=move || top_nodes.get()
                                key=|n: &Node| n.key().to_string()
                                children=move |n: Node| node_view(n, ctx, Arc::new(Vec::new()), top_last, None)
                            />
                        }
                        .into_any(),
                        // FILTER-20: every ticket the nav's filter admits, as one list.
                        View::Flat => flat::flat_list(flat_filter, route, tree.viewport, flat_note),
                    }}
                </div>
                {move || match nav_view.get() {
                    View::Tree => lens_line(),
                    View::Flat => flat_note.get().map(|n| flat::note_view(n, under.on.get())),
                }}
            </div>
            // ACT-3: what is selected, at the nav's foot.
            {selection_foot(under)}
        </aside>
    }
}

/// ACT-3: a click on a row of the nav — the tree's or the flat list's, one
/// listener on the tree. Plain, the row's own behaviour runs (a ticket opens,
/// a shortcode's heading opens or closes) and the selection becomes the row;
/// Shift selects the range from the anchor, Ctrl/Cmd toggles the row — and
/// neither follows the link: the row takes focus instead.
fn tree_click(ev: web_sys::MouseEvent) {
    let Some(clicked) = bindings::hit_mouse(&ev, Context::Nav) else { return };
    let Some(row) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .and_then(|t| t.closest("[role=treeitem]").ok().flatten())
    else {
        return;
    };
    let Some(id) = selection::pick_of(&row) else { return };
    let gesture = Gesture::of(clicked);
    if gesture != Gesture::Plain {
        ev.prevent_default();
        if let Ok(row) = row.dyn_into::<web_sys::HtmlElement>() {
            keys::focus_and_rove(&tree_items(), &row);
        }
    }
    selection::set(gesture.apply(&selection::now(), &drawn_order(), &id));
}

/// ACT-3: the nav's foot while something is selected — "3 selected · clear",
/// and how many of them the nav's filter hides ("· 2 hidden"): `member {
/// ids, filter }` says which it admits, and a ticket drawn anyway (FILTER-24's
/// open ticket) is not hidden. A filter `member` does not take (`under`,
/// `text`) counts what is not drawn. Asked again when the selection or the
/// filter changes, and when the filter's membership moves a selected ticket.
fn selection_foot(under: Under) -> AnyView {
    let sel = selection::signal();
    let n = Memo::new(move |_| sel.with(Picked::len));
    let hidden = RwSignal::new(0usize);
    let again = Trigger::new();
    session::follow_membership(
        move || under.text.get(),
        move |ev| match ev {
            Member::Entered { id, .. } | Member::Left { id, .. } => {
                if sel.with_untracked(|p| p.contains(id)) {
                    again.notify();
                }
            }
            Member::Recount { .. } | Member::Lagged | Member::Opened => again.notify(),
            Member::Refused(_) => {}
        },
    );
    let gen = StoredValue::new(0u32);
    Effect::new(move |_| {
        again.track();
        let ids = sel.with(|p| p.ids().to_vec());
        let on = under.on.get();
        let filter = under.text.get();
        let kept = under.kept.get();
        let g = gen.get_value().wrapping_add(1);
        gen.set_value(g);
        if ids.is_empty() || !on {
            hidden.set(0);
            return;
        }
        spawn_local(async move {
            // the tree redraws under a new filter first
            dom::sleep_ms(60).await;
            if gen.try_get_value() != Some(g) {
                return;
            }
            let admitted: HashSet<String> = match filter {
                // a filter that admits nothing
                None => HashSet::new(),
                Some(f) => match session::member(ids.clone(), f).await {
                    Ok(v) => v.into_iter().filter(|(_, m)| *m).map(|(id, _)| id).collect(),
                    Err(_) => HashSet::new(),
                },
            };
            if gen.try_get_value() != Some(g) {
                return;
            }
            let mut drawn: HashSet<String> = drawn_order().into_iter().collect();
            drawn.extend(kept);
            hidden.try_set(selection::hidden_of(&ids, Some(&admitted), &drawn));
        });
    });
    view! {
        {move || {
            selection::foot_line(n.get(), hidden.get()).map(|line| {
                view! {
                    <div class="nsel" data-nsel=move || n.get() data-nsel-hidden=move || hidden.get()>
                        <span class="nsel-n" aria-live="polite">{line}</span>
                        " · "
                        <button class="nsel-clear" title="clear the selection (Esc)" on:click=move |_| {
                            selection::clear();
                        }>"clear"</button>
                    </div>
                }
            })
        }}
    }
    .into_any()
}

/// FILTER-14: `f` in the nav opens the nav's filter menu — unless typing,
/// or in a dialog or menu (the filter's own menu among them). AGS-26: on a
/// ticket page it filters the nav to the open ticket and its descendants, as
/// it does anywhere on the page ([`filter_to`]).
fn nav_f(ev: web_sys::KeyboardEvent, ctx: Ctx, lens: Lens) {
    if ev.default_prevented() || !bindings::is(&ev, Context::Nav, "filter.view") {
        return;
    }
    let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
    if target.as_ref().is_some_and(keys::defer_to_focus) {
        return;
    }
    ev.prevent_default();
    if let Some(Route::Ticket(id)) = ctx.route.try_get_untracked() {
        filter_to(&id, ctx, lens);
        return;
    }
    if !lens.pop(Level::Nav).is_open() {
        lens.open_menu(Level::Nav, None, None);
    }
}

/// The `is last child` flags of a node's ancestors, outermost first —
/// what turns a depth into the tree's connector art ([`art`]: `│ ` for an
/// ancestor with siblings still to come, two spaces for one that was
/// last). Each flag is a memo, so an insert repaints only the rows whose
/// answer actually changed. ACT-7: and each ancestor's key — the chain a
/// row folds the nav's [`Shape`] over to know whether it is open.
type Ancestry = Arc<Vec<Up>>;

/// One ancestor of a row: whether it is its list's last, and its key.
#[derive(Clone)]
struct Up {
    last: Memo<bool>,
    key: String,
}

fn connectors(anc: &Ancestry, last: bool) -> String {
    art(anc.iter().map(|a| a.last.get()), last)
}

/// A child's place in a list: the list, and whether it is windowed (then
/// the row says its position among all of its siblings).
#[derive(Clone, Copy)]
struct Pos {
    list: RwSignal<List>,
    windowed: Memo<bool>,
}

/// One node of the tree — a ticket row, a shortcode drawn as its root's row
/// (AGS-23), or a rootless shortcode's row — and, beneath it, its children,
/// recursively.
///
/// MFE-12: the children exist only while the node is open — collapse
/// unmounts them rather than hiding them, so a closed node costs its own
/// row and an empty container, whatever it holds. SCALE-24: and a closed
/// node asks the server nothing; what it shows of its children is its
/// row's `child_count` (a ticket) or its roll-up (a shortcode).
/// MENGER-13: whether `ancestor` is on the open ticket `open`'s chain of
/// parents, as the row cache holds it — what keeps a canceled row open so
/// the cursor (AGS-26) can stand on a ticket below it.
fn open_is_below(open: &str, ancestor: &str) -> bool {
    let mut cur = cache::find(open).and_then(|t| t.with(|t| t.parent.clone()));
    for _ in 0..64 {
        match cur {
            Some(p) if p == ancestor => return true,
            Some(p) => cur = cache::find(&p).and_then(|t| t.with(|t| t.parent.clone())),
            None => return false,
        }
    }
    false
}

fn node_view(node: Node, ctx: Ctx, ancestry: Ancestry, last: Memo<Option<String>>, pos: Option<Pos>) -> AnyView {
    let key = node.key().to_string();
    let tree = ctx.tree;
    // The shortcode this node stands for, if it does: a root's row or a
    // rootless shortcode's.
    let shortcode = match &node {
        Node::Root(_, epic) | Node::Group(epic) => Some(epic.clone()),
        Node::Ticket(_) => None,
    };
    let is_group = matches!(node, Node::Group(_));

    // A ticket's row, from the row cache — held by the list that put it
    // here; a handle in this node's owner. AGS-23: a root's row, from its
    // group ([`face`]) — `groups {}` carries what it shows and
    // `group_changed` keeps it, so it needs no row of the cache.
    let ticket: Option<Signal<TicketSummary>> = match &node {
        Node::Ticket(id) => match cache::get(id) {
            Some(sig) => Some(Signal::from(sig)),
            None => return ().into_any(),
        },
        Node::Root(id, epic) => {
            let (id, epic) = (id.clone(), epic.clone());
            // The group as it is now; once it is gone (its row is leaving),
            // the last it said.
            Some(Signal::from(Memo::new(move |prev: Option<&TicketSummary>| {
                tree.with_group(&epic, face)
                    .flatten()
                    .filter(|t| t.id == id)
                    .or_else(|| prev.cloned())
                    .unwrap_or_else(|| root_summary(&id, &epic, None, String::new(), Status::Pending, Kind::Scope))
            })))
        }
        Node::Group(_) => None,
    };

    let mine_last = {
        let k = key.clone();
        Memo::new(move |_| last.with(|l| l.as_deref() == Some(k.as_str())))
    };
    // MENGER-13: a canceled ticket's descendants are not shown under it — the
    // row stays closed, its count beside it, unless the reader opens it here
    // (`peek`, this view's alone, never kept) or the open ticket is below it
    // (the reveal's chain opens it, so the cursor can stand on its row).
    let canceled = Memo::new(move |_| ticket.is_some_and(|t| t.with(|t| t.status == Status::Canceled)));
    let peek = RwSignal::new(false);
    // ACT-7: the keys above this row — what it inherits from the shape.
    let chain: Arc<Vec<String>> = Arc::new(ancestry.iter().map(|a| a.key.clone()).collect());
    let open = {
        let (k, chain) = (key.clone(), chain.clone());
        let expanded = ctx.expanded;
        let route = ctx.route;
        Memo::new(move |_| {
            expanded.with(|e| e.is_open(&chain, &k))
                && (!canceled.get()
                    || peek.get()
                    || route.with(|r| matches!(r, Route::Ticket(open) if open_is_below(open, &k))))
        })
    };
    // MENGER-13: what a closed canceled row says it holds.
    let folded = Memo::new(move |_| {
        (canceled.get() && !open.get()).then(|| ticket.map_or(0, |t| t.with(|t| t.child_count))).filter(|n| *n > 0)
    });
    // FILTER-14: how the row is drawn under the lens — `None` off it (as
    // today). A ticket row as `tree` answered it; a shortcode's row by its
    // matches (and its root's word, when its host's page said it).
    let under = ctx.under;
    let face: Memo<Option<Face>> = {
        let node = node.clone();
        Memo::new(move |_| {
            if !under.on.get() {
                return None;
            }
            Some(match &node {
                // FILTER-24: the open ticket, marked outside the filter when
                // it does not match.
                Node::Ticket(id) if under.is_kept(id) => under.mark(id).map(kept_face).unwrap_or_default(),
                Node::Ticket(id) => under.mark(id).map(face_of).unwrap_or_default(),
                Node::Root(id, epic) => shortcode_face(under.count(epic).unwrap_or(0), under.mark(id), under.holds(epic, Some(id))),
                Node::Group(epic) => shortcode_face(under.count(epic).unwrap_or(0), None, under.holds(epic, None)),
            })
        })
    };
    // FILTER-14: a shortcode's row is its root's (AGS-23), and whether the
    // root itself matches is `tree { epic }`'s word — asked while the
    // shortcode has matches, and again whenever their number moves (the root
    // entering or leaving moves it) or the connection does.
    if let Node::Root(id, epic) = &node {
        let (id, epic) = (id.clone(), epic.clone());
        let asked = StoredValue::new(0u64);
        Effect::new(move |_| {
            let Some(filter) = under.text.get() else { return };
            let n = under.count(&epic).unwrap_or(0);
            tree.epoch.track();
            if n == 0 {
                return;
            }
            let ask = asked.get_value().wrapping_add(1);
            asked.set_value(ask);
            let (id, epic) = (id.clone(), epic.clone());
            spawn_local(async move {
                let mark = ask_root(&epic, &id, &filter).await;
                if asked.try_get_value() != Some(ask) {
                    return; // a newer ask, or the row is gone
                }
                if let Some(m) = mark {
                    if under.marks.try_with_untracked(|ms| ms.get(&id) != Some(&m)) == Some(true) {
                        under.marks.try_update(|ms| ms.insert(id, m));
                    }
                }
            });
        });
    }
    let has_kids = {
        let (k, node) = (key.clone(), node.clone());
        Memo::new(move |_| match &node {
            // FILTER-14: under the lens a row opens onto the matches under it.
            _ if face.with(Option::is_some) => face.with(|f| f.as_ref().is_some_and(|f| f.opens)),
            Node::Ticket(_) => {
                ticket.is_some_and(|t| t.with(|t| t.child_count > 0)) || tree.hosted.with(|h| h.contains_key(&k))
            }
            // AGS-23: from the roll-up — the root's row is not the cache's.
            Node::Root(_, epic) => {
                tree.with_group(epic, root_has_kids).unwrap_or(false) || tree.hosted.with(|h| h.contains_key(&k))
            }
            Node::Group(epic) => tree.with_group(epic, Group::drawn).unwrap_or(false),
        })
    };
    let child_ancestry: Ancestry = {
        let mut v = (*ancestry).clone();
        v.push(Up { last: mine_last, key: key.clone() });
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
            // ACT-7: the other way from how it is drawn, under its chain.
            let now = !open.get_untracked();
            // MENGER-13: a canceled row opens for this view only.
            if canceled.get_untracked() {
                peek.set(now);
            }
            expanded.update(|e| e.set_open(&chain, &k, now));
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
    // Where the node is in its list — only a node in one has a place.
    let index = pos.map(|p| {
        let k = key.clone();
        Memo::new(move |_| p.list.with(|l| l.index_of(&k)))
    });
    // A shortcode's roll-up, on whichever row stands for it (AGS-23: a
    // root's row carries it beside the root's own status). FILTER-14: under
    // the lens, its matches in place of it.
    let rollup = shortcode.clone().map(|epic| {
        Memo::new(move |_| match under.count(&epic) {
            Some(n) => n.to_string(),
            None => tree.with_group(&epic, rollup_label).unwrap_or_default(),
        })
    });

    // Only one of these runs, so each may take the row's closures by
    // value; the two row shapes are otherwise unrelated.
    let row = match (ticket, &node) {
        (None, _) => group_row(key.clone(), key.clone(), rollup.expect("a shortcode row"), face, prefix, toggle_glyph, toggle, expanded_attr, owns_attr),
        // FILTER-23: a shortcode's root that does not match is no ticket row
        // under the lens: the shortcode is a heading — its code and its
        // matches — keyed, opened and closed as the root's row is.
        (Some(t), Node::Root(_, epic)) => {
            let heading = Memo::new(move |_| face.with(|f| f.as_ref().is_some_and(|f| f.heading)));
            let (key, epic, shortcode) = (key.clone(), epic.clone(), shortcode.clone());
            let rollup = rollup.expect("a shortcode row");
            view! {
                {move || {
                    if heading.get() {
                        group_row(key.clone(), epic.clone(), rollup, face, prefix.clone(), toggle_glyph, toggle.clone(), expanded_attr, owns_attr.clone())
                    } else {
                        leaf_row(
                            key.clone(),
                            t,
                            Some(rollup),
                            face,
                            folded,
                            shortcode.clone(),
                            ctx,
                            pos,
                            index,
                            prefix.clone(),
                            toggle_glyph,
                            toggle.clone(),
                            expanded_attr,
                            owns_attr.clone(),
                        )
                    }
                }}
            }
            .into_any()
        }
        (Some(t), _) => leaf_row(
            key.clone(),
            t,
            rollup,
            face,
            folded,
            shortcode.clone(),
            ctx,
            pos,
            index,
            prefix,
            toggle_glyph,
            toggle,
            expanded_attr,
            owns_attr,
        ),
    };

    let kids_ref = NodeRef::<leptos::html::Div>::new();
    // ACT-7: opened by a mark of its own (a click, h/l, the reveal) — or only
    // by a bulk expand, when its list waits to be near the viewport to ask.
    let marked = {
        let k = key.clone();
        Memo::new(move |_| ctx.expanded.with(|e| e.marked_open(&k)))
    };
    view! {
        <div
            class="node"
            class:group=is_group
            class:shortcode=shortcode.is_some()
            role="none"
            data-node-key=key.clone()
            data-group=shortcode
            data-index=index.map(|ix| move || ix.get())
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
                                    // FILTER-14: a new filter is a new list
                                    // (a position in one pruned tree is
                                    // nothing in another); one that admits
                                    // nothing lists nothing.
                                    let lensed = match (under.on.get(), under.text.get()) {
                                        (true, None) => return None,
                                        (on, text) => text.filter(|_| on),
                                    };
                                    open.get().then(|| children_of(node.clone(), ctx, child_ancestry.clone(), kids_ref, ticket, lensed, marked))
                                }}
                            </div>
                        }
                    })
            }}
        </div>
    }
    .into_any()
}

/// What an open node shows beneath it. A ticket — AGS-23: a shortcode's
/// root's row too — its children by page, then the sub-shortcodes it hosts.
/// A shortcode with no root: its tickets by page, flat.
fn children_of(
    node: Node,
    ctx: Ctx,
    ancestry: Ancestry,
    kids_ref: NodeRef<leptos::html::Div>,
    ticket: Option<Signal<TicketSummary>>,
    lensed: Option<String>,
    marked: Memo<bool>,
) -> AnyView {
    let tree = ctx.tree;
    let hosted = |id: &str| {
        let id = id.to_string();
        Memo::new(move |_| tree.hosted.with(|h| h.get(&id).cloned().unwrap_or_default()))
    };
    // ACT-7: a lazy list not asked yet keeps the room its subtree will take
    // open, so the lists below it are not all near at once — a shortcode's
    // root's, its roll-up (everything under it); under the lens, its matches,
    // and a ticket's the matches below it (a ticket's list off the lens has
    // its `child_count` of pending rows already).
    let under = ctx.under;
    let reserve = |epic: &str| match lensed {
        Some(_) => untrack(|| under.count(epic)).unwrap_or(0),
        None => tree.with_group(epic, |g| (g.total + g.docs).saturating_sub(1) as usize).unwrap_or(0),
    };
    match node {
        Node::Group(epic) => {
            let lazy = Lazy { marked, reserve: reserve(&epic) };
            list_view(Source::Epic(epic), None, ctx, ancestry, kids_ref, None, lensed, lazy)
        }
        Node::Ticket(id) => {
            // FILTER-14: under the lens a row's `child_count` is not its
            // list's: the pruned tree is sized by its first page.
            let count = ticket.filter(|_| lensed.is_none()).map(|t| Memo::new(move |_| t.with(|t| t.child_count as usize)));
            let hosted = hosted(&id);
            let below = if lensed.is_some() { untrack(|| under.mark(&id)).map_or(0, |m| m.below as usize) } else { 0 };
            list_view(Source::Parent(id), count, ctx, ancestry, kids_ref, Some(hosted), lensed, Lazy { marked, reserve: below })
        }
        // AGS-23: the root's row is the group's, which carries no count of
        // its children: its list is sized by its first page, and moved by
        // `children_changed` / `child_removed`, which carry the count.
        Node::Root(id, epic) => {
            let hosted = hosted(&id);
            let lazy = Lazy { marked, reserve: reserve(&epic) };
            list_view(Source::Parent(id), None, ctx, ancestry, kids_ref, Some(hosted), lensed, lazy)
        }
    }
}

/// One slot of a mounted chunk: a known child, or a row still being fetched
/// — FILTER-23: or, under the lens, a hosted shortcode's row where `tree`
/// placed it (its root, its shortcode).
#[derive(Clone, Debug, PartialEq)]
enum Slot {
    Child(String),
    Shortcode(String, String),
    Pending(usize),
}

/// A node's children, by page — and, past one page, a window over them
/// ([`span`]). `count` is the node's `child_count` (a ticket's list),
/// followed live; `hosted` the sub-shortcodes drawn after the children.
/// FILTER-14: `lensed`, the nav's filter: the children are the pruned tree
/// `tree` answers for it, and `watch`'s deltas are not applied — the list
/// asks again when the membership watch says its chain moved.
///
/// ACT-7: [`Lazy`] — while the node is open only by a bulk expand (Expand
/// all, a subtree, a shortcode), not by a mark of its own, the list asks its
/// first page only once it has been measured near the nav's viewport
/// ([`near`]): Expand all on 650 tickets opens every node, and only those on
/// or near the screen ask; the rest ask as the scroll reaches them. Until its
/// page comes, it holds the room of its reserve's rows (a gap, as a spacer
/// is), so the lists below it are measured where they will be. A mark (a
/// click, the reveal's chain) makes it ask at once.
#[allow(clippy::too_many_arguments)]
fn list_view(
    source: Source,
    count: Option<Memo<usize>>,
    ctx: Ctx,
    ancestry: Ancestry,
    kids_ref: NodeRef<leptos::html::Div>,
    hosted: Option<Memo<Vec<String>>>,
    lensed: Option<String>,
    lazy: Lazy,
) -> AnyView {
    let tree = ctx.tree;
    let under = ctx.under;
    let list = RwSignal::new(List::of(count.map_or(0, |c| c.get_untracked())));
    let holder = cache::Holder::new();
    let again = RwSignal::new(0u32);
    OPEN.with(|o| {
        o.borrow_mut().push(Open { source: source.clone(), list, holder: holder.clone(), lensed: lensed.is_some(), again })
    });
    on_cleanup(move || OPEN.with(|o| o.borrow_mut().retain(|e| e.list != list)));
    let lensed: Option<Arc<str>> = lensed.map(Arc::from);

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
    // ACT-7: whether a lazy list may ask — once measured, near the viewport.
    let placed = RwSignal::new(false);
    let Lazy { marked, reserve } = lazy;
    let may_ask = Memo::new(move |_| marked.get() || (placed.get() && near(geom.get(), list.with(|l| l.n).max(reserve))));
    let reserved = Memo::new(move |_| (reserve > 0 && !marked.get() && list.with(|l| !l.loaded)).then_some(reserve));
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
        if placed.try_get_untracked() == Some(false) {
            placed.try_set(true);
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
    // The epoch (and, FILTER-14, the times this list was told to ask
    // again) when it last asked.
    let seen = StoredValue::new((0u32, 0u32));
    // An answer that changed nothing re-runs nothing — except when the
    // connection moved on while it was in flight (a reconnect, a `lagged`),
    // or (FILTER-14) the list was told to ask again: then it asks again.
    let retry = Trigger::new();
    // Asks in a row that told the list nothing, for this window: a list
    // whose known ids disagree with the server cannot spin on one gap.
    let strikes = StoredValue::new(((0usize, 0usize), 0u8));
    {
        let (source, holder, lensed) = (source.clone(), holder.clone(), lensed.clone());
        Effect::new(move |_| {
            let epoch = tree.epoch.get();
            let told = again.get();
            let (lo, hi) = range.get();
            list.track();
            retry.track();
            // ACT-7: a lazy list off the screen asks nothing yet.
            if !may_ask.get() && list.with_untracked(|l| !l.loaded) {
                return;
            }
            if epoch == 0 || busy.get_value() {
                return;
            }
            let (seen_epoch, seen_told) = seen.get_value();
            // A reconnect or a `lagged` since this list last asked: what it
            // shows is asked for again, from its first row, in one page.
            // FILTER-14: under the lens — or told to ask again — from the
            // top: the pruned tree's count is the first page's, and what
            // moved may be anywhere before the window.
            let stale = seen_epoch != 0 && (seen_epoch != epoch || seen_told != told);
            if !stale && strikes.with_value(|(r, n)| *r == (lo, hi) && *n >= 3) {
                return; // until the window, or the connection, moves
            }
            let ask = list.with_untracked(|l| {
                if stale {
                    let a = if lo == 0 || lensed.is_some() { Ask::top() } else { l.ask_for(lo) };
                    let limit = if lensed.is_some() { PAGE } else { (hi - lo).max(PAGE) };
                    return Some((a, limit, true));
                }
                if !l.loaded {
                    return Some((Ask::top(), PAGE, false));
                }
                let missing = (lo..hi).find(|&i| l.id_at(i).is_none_or(|id| !holder.holds(id)))?;
                Some((l.ask_for(missing), PAGE, false))
            });
            seen.set_value((epoch, told));
            let Some((ask, limit, reset)) = ask else {
                // Everything shown is in hand: let go of what is far away.
                let keep = (lo.saturating_sub(KEEP_CHUNKS * CHUNK), hi + KEEP_CHUNKS * CHUNK);
                list.with_untracked(|l| {
                    holder.retain(|id| l.index_of(id).is_some_and(|i| i >= keep.0 && i < keep.1));
                });
                return;
            };
            busy.set_value(true);
            let (source, holder, lensed) = (source.clone(), holder.clone(), lensed.clone());
            spawn_local(async move {
                let answer = ask_list(&source, lensed.as_deref(), ask.after.clone(), limit).await;
                busy.try_set_value(false);
                if tree.epoch.try_get_untracked().is_some_and(|e| e != epoch) || again.try_get_untracked().is_some_and(|t| t != told) {
                    retry.notify(); // a no-op once the list is gone
                }
                match answer {
                    Asked::Page(rows, remaining, marks) => {
                        let ids: Vec<String> = rows.iter().map(|t| t.id.clone()).collect();
                        // Rows it did not hold are news too: a scroll back
                        // over children it had let go of.
                        let newly = ids.iter().any(|id| !holder.holds(id));
                        // FILTER-14: what the tree says of each row, before
                        // the row is drawn.
                        if !marks.is_empty() {
                            let changed = under.marks.try_with_untracked(|m| marks.iter().any(|(id, k)| m.get(id) != Some(k)));
                            if changed == Some(true) {
                                under.marks.try_update(|m| m.extend(marks));
                            }
                        }
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
        let (source, holder, lensed) = (source.clone(), holder.clone(), lensed.clone());
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
            let (source, holder, lensed) = (source.clone(), holder.clone(), lensed.clone());
            // `try_` throughout: the list can be gone (its node closed or
            // rebuilt) by the time the page comes back.
            spawn_local(async move {
                let Some(known) = list.try_with_untracked(|l| l.index_of(&child.id)) else { return };
                let i = match known {
                    Some(i) => i,
                    None => {
                        // FILTER-14: under the lens the child is one the
                        // tree draws (the reveal opened no further), and
                        // the page after it is the tree's.
                        let Asked::Page(rows, remaining, marks) = ask_list(&source, lensed.as_deref(), Some(child.id.clone()), PAGE).await else {
                            return;
                        };
                        if !marks.is_empty() {
                            under.marks.try_update(|m| m.extend(marks));
                        }
                        let ids: Vec<String> = rows.iter().map(|t| t.id.clone()).collect();
                        let Some(n) = list.try_with_untracked(|l| l.n) else { return };
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
    // root is drawn as its shortcode's row, after the children (AGS-23: the
    // root's own row, which carries the shortcode's roll-up). FILTER-23:
    // under the lens `tree` answers it where it is drawn — after the
    // matches, here or at the nearest drawn row above its host — so it is
    // drawn in its place in the list, if it has matches (or the hidden line
    // says to draw every shortcode).
    let only = lensed.is_some();
    let drawn_here = move |epic: &str| under.show_hidden.get() || under.holds(epic, None) != Holds::No || under.count(epic).is_some_and(|n| n > 0);
    let slots = Memo::new(move |_| {
        let (lo, hi) = range.get();
        let hosted = tree.hosted_roots.get();
        list.with(|l| {
            (lo..hi)
                .filter_map(|i| match l.id_at(i) {
                    Some(id) => match hosted.get(id) {
                        Some(epic) if only => drawn_here(epic).then(|| Slot::Shortcode(id.to_string(), epic.clone())),
                        Some(_) => None,
                        None if holder.holds(id) => Some(Slot::Child(id.to_string())),
                        None => Some(Slot::Pending(i)),
                    },
                    None => Some(Slot::Pending(i)),
                })
                .collect::<Vec<_>>()
        })
    });
    // The hosted sub-shortcodes, each as the node it is drawn as (AGS-23:
    // its root's row); the last row under the node by its key — the last
    // of them, else the last child.
    // FILTER-14: under the lens, those with a match — or every one, the
    // rest dimmed, when the hidden line says so.
    let hosted = match hosted {
        // FILTER-23: under the lens they are in the list, where `tree` placed them.
        Some(_) if only => Memo::new(|_| Vec::new()),
        Some(h) => Memo::new(move |_| {
            let nodes = h.with(|h| tree.nodes(h));
            if !under.on.get() {
                return nodes;
            }
            let every = under.show_hidden.get();
            let kept = under.kept_codes.get();
            under.sums.with(|s| s.as_ref().map(|s| shown(nodes, s, every, &kept)).unwrap_or_default())
        }),
        None => Memo::new(|_| Vec::new()),
    };
    let last_kid = Memo::new(move |_| {
        if only {
            // FILTER-23: the last child drawn — past the shortcodes not drawn.
            let hosted = tree.hosted_roots.get();
            return list.with(|l| last_drawn(l, |id| hosted.get(id).is_some_and(|e| !drawn_here(e))));
        }
        hosted
            .with(|h| h.last().map(|n| n.key().to_string()))
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
        {move || reserved.get().map(|n| view! {
            <div class="spacer gap" role="none" aria-hidden="true" data-reserve=n style:height=move || format!("{}px", n as f64 * tree.pitch.get())></div>
        })}
        {spacer(|s| s.top_px)}
        <For
            each=move || slots.get()
            key=|s: &Slot| match s {
                Slot::Child(id) | Slot::Shortcode(id, _) => id.clone(),
                Slot::Pending(i) => format!("?{i}"),
            }
            children=move |s: Slot| match s {
                Slot::Child(id) => node_view(Node::Ticket(id), ctx, child_ancestry.clone(), last_kid, Some(pos)),
                Slot::Shortcode(root, epic) => node_view(Node::Root(root, epic), ctx, child_ancestry.clone(), last_kid, Some(pos)),
                Slot::Pending(i) => pending_view(i, pending_prefix.clone()),
            }
        />
        {spacer(|s| s.bottom_px)}
        <For
            each=move || hosted.get()
            key=|n: &Node| n.key().to_string()
            children=move |n: Node| node_view(n, ctx, hosted_ancestry.clone(), last_kid, None)
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
/// `group_changed` — nothing here counts. AGS-23: only a shortcode with no
/// root is drawn this way; one with a root is its root's row — FILTER-23:
/// unless, under the lens, its root does not match: then it is this row,
/// keyed by the root (`key`) and showing the shortcode (`code`).
#[allow(clippy::too_many_arguments)]
fn group_row(
    key: String,
    code: String,
    rollup: Memo<String>,
    face: Memo<Option<Face>>,
    prefix: impl Fn() -> String + Send + Sync + 'static,
    toggle_glyph: impl Fn() -> &'static str + Send + Sync + 'static,
    toggle: impl Fn() + Send + Sync + 'static,
    expanded_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
    owns_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> AnyView {
    let tag = crate::tags::Tag::shortcode(&code).attr();
    // ACT-3: a heading stands for its root (keyed by it); a shortcode with no
    // root stands for no ticket and is not selectable.
    let pick = (key != code).then(|| key.clone());
    let heading_code = pick.as_ref().map(|_| code.clone());
    let picked = pick.clone().map(selection::is_picked);
    view! {
        <div
            class="row grouprow"
            class:dim=move || face.with(|f| f.as_ref().is_some_and(|f| f.dim))
            // FILTER-23: under the lens a shortcode's row is a heading.
            data-lens=move || face.with(|f| f.as_ref().map(|f| if f.heading { "heading" } else { "match" }))
            role="treeitem"
            tabindex="-1"
            data-node-key=key
            data-pick=pick.clone()
            // ACT-4: a heading is its root's row — the root's menu, with the
            // shortcode's items.
            data-ticket-id=pick
            data-shortcode=heading_code
            class:picked=move || picked.is_some_and(|p| p.get())
            aria-selected=move || picked.map(|p| p.get().to_string())
            aria-expanded=expanded_attr
            aria-owns=owns_attr
            // AGS-25: a click toggles, and a double-click is one toggle too —
            // its second click is not another, and it selects no text.
            // ACT-3: a Shift or Ctrl/Cmd click selects instead.
            on:click=move |ev: web_sys::MouseEvent| {
                if ev.detail() <= 1 && Gesture::of_mouse(&ev, Context::Nav) == Gesture::Plain {
                    toggle();
                }
            }
            on:mousedown=no_select_on_multi_click
        >
            <span class="prefix">{prefix}</span>
            <span class="toggle clickable">{toggle_glyph}</span>
            <span class="toggle mark" aria-hidden="true">" "</span>
            // FILTER-22: the shortcode is a tag.
            <span class="code" data-tag=tag>{code}</span>
            {roll_tag(rollup, face)}
        </div>
    }
    .into_any()
}

/// A shortcode's tally — its roll-up; FILTER-14: under the lens, its
/// matches (summed along the hosts), marked so.
fn roll_tag(rollup: Memo<String>, face: Memo<Option<Face>>) -> AnyView {
    let lensed = Memo::new(move |_| face.with(Option::is_some));
    let title = move || lensed.get().then(|| format!("{} matching the nav's filter, with the shortcodes it hosts", rollup.get()));
    view! { <span class="tag roll" class:lensed=move || lensed.get() title=title>{move || rollup.get()}</span> }.into_any()
}

/// AGS-25: a double-click toggles a nav row; its second press must not also
/// select a word of the row's text.
fn no_select_on_multi_click(ev: web_sys::MouseEvent) {
    if ev.detail() > 1 {
        ev.prevent_default();
    }
}

/// One ticket row: a real link (middle-click / copy-link friendly) whose
/// title and status tag render through the row's inner signal. Selected
/// iff the route is this ticket's page. Carries `data-ticket-id`, the
/// AGS-16 menu's attachment (a right-click anywhere on the row opens it).
/// A ticket with children also carries the collapse toggle — clicking it
/// opens the row instead of following the link. SCALE-24: in a windowed
/// list it says where it is among all of its siblings. AGS-23: a shortcode
/// drawn as its root's row is one of these, drawn from its group and
/// carrying the shortcode's roll-up (`rollup`) beside the root's status —
/// WAVE-5: and the shortcode (`data-shortcode`), which the ticket menu reads
/// to offer the shortcode's wave board.
#[allow(clippy::too_many_arguments)]
fn leaf_row(
    id: String,
    ticket: Signal<TicketSummary>,
    rollup: Option<Memo<String>>,
    face: Memo<Option<Face>>,
    folded: Memo<Option<u64>>,
    shortcode: Option<String>,
    ctx: Ctx,
    pos: Option<Pos>,
    index: Option<Memo<Option<usize>>>,
    prefix: impl Fn() -> String + Send + Sync + 'static,
    toggle_glyph: impl Fn() -> &'static str + Send + Sync + 'static,
    toggle: impl Fn() + Send + Sync + 'static,
    expanded_attr: impl Fn() -> Option<String> + Copy + Send + Sync + 'static,
    owns_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> AnyView {
    let route = ctx.route;
    let sel = {
        let id = id.clone();
        Memo::new(move |_| route.with(|rt| matches!(rt, Route::Ticket(x) if *x == id)))
    };
    // ACT-3: whether the row is in the selection — `aria-selected`.
    let picked = selection::is_picked(id.clone());
    let href = format!("#/ticket/{id}");
    // SDLC-13: a kind changes (`set_kind`), so the row reads it as it reads
    // the status — one memo, which the glyph, its title and the row's
    // `data-kind` (its tint) follow.
    let kind = Memo::new(move |_| ticket.with(|t| t.kind.as_str()));
    // A row's place among its siblings, said only in a list that is a window
    // — and asked only of a row in a list (AGS-23: a top-level root's row is
    // in none).
    let setsize = pos.map(|p| move || p.windowed.get().then(|| p.list.with(|l| l.n)));
    let posinset = pos.zip(index).map(|(p, ix)| move || p.windowed.get().then(|| ix.get().map(|i| i + 1)).flatten());
    // AGS-25: the `▸`'s click and the row's double-click toggle the same
    // node, so they share the one closure.
    let toggle = std::sync::Arc::new(toggle);
    let on_toggle = toggle.clone();
    let (click_id, dbl_id, leave_id) = (id.clone(), id.clone(), id.clone());
    let click_toggle = toggle.clone();

    view! {
        <a
            class="row leaf"
            class:sel=move || sel.get()
            // FILTER-14: a row that only holds matches, dimmed.
            class:dim=move || face.with(|f| f.as_ref().is_some_and(|f| f.dim))
            // FILTER-24: the open ticket, drawn though the filter excludes it.
            class:outside=move || face.with(|f| f.as_ref().is_some_and(|f| f.outside))
            data-lens=move || face.with(|f| f.as_ref().map(|f| if f.outside { "outside" } else if f.dim { "holder" } else { "match" }))
            role="treeitem"
            tabindex="-1"
            // ACT-3: selected is the selection's; the open ticket is current.
            class:picked=move || picked.get()
            aria-selected=move || picked.get().to_string()
            aria-current=move || sel.get().then_some("page")
            data-pick=id.clone()
            aria-expanded=expanded_attr
            aria-owns=owns_attr
            aria-setsize=setsize
            aria-posinset=posinset
            data-node-key=id.clone()
            data-nav-id=id.clone()
            data-ticket-id=id.clone()
            // WAVE-5: a shortcode's row says which shortcode it stands for —
            // what the ticket menu offers its wave board by.
            data-shortcode=shortcode
            data-kind=move || kind.get()
            href=href
            // AGS-29: a plain click on a row with children is held for the
            // double-click window, then opens the ticket; a leaf follows its
            // link at once. Any other click drops a held one (ACT-3's
            // selection runs in `tree_click`, as before).
            on:click=move |ev: web_sys::MouseEvent| {
                if bindings::hit_mouse(&ev, Context::Nav) != Some("select.only") {
                    rowclick::feed(rowclick::Ev::Other);
                    return;
                }
                let (at, parent) = (ev.time_stamp(), expanded_attr().is_some());
                match rowclick::feed(rowclick::Ev::Click { row: &click_id, at, count: ev.detail(), parent }) {
                    rowclick::Act::Hold => {
                        ev.prevent_default();
                        spawn_local(async move {
                            dom::sleep_ms(bindings::DOUBLE_CLICK_MS as i32).await;
                            if let rowclick::Act::Open(id) = rowclick::feed(rowclick::Ev::Lapse { at }) {
                                crate::navigate_to_ticket(&id);
                            }
                        });
                    }
                    rowclick::Act::Swallow => ev.prevent_default(),
                    // AGS-30: an even click of a quick run opens or closes it.
                    rowclick::Act::Toggle(_) => {
                        ev.prevent_default();
                        click_toggle();
                    }
                    _ => {}
                }
            }
            // AGS-25, AGS-29, AGS-30: a double-click on a row with children
            // opens or closes it (from the click handler's even click), and
            // opens nothing; it selects no text.
            on:dblclick=move |ev: web_sys::MouseEvent| {
                if bindings::hit_mouse(&ev, Context::Nav) != Some("nav.toggle") {
                    return;
                }
                // AGS-30: the toggle was the run's even click; this only keeps
                // the browser's default (a word selected) away.
                let parent = expanded_attr().is_some();
                rowclick::feed(rowclick::Ev::DoubleClick { row: &dbl_id, parent });
                ev.prevent_default();
            }
            // AGS-29: the pointer leaving the row drops its held click.
            on:mouseleave=move |_| {
                rowclick::feed(rowclick::Ev::Leave { row: &leave_id });
            }
            on:mousedown=no_select_on_multi_click
        >
            <span class="prefix">{prefix}</span>
            <span
                class="toggle clickable"
                // A double-click here is one toggle: every odd click of a run
                // toggles (AGS-30), and the clicks stop here rather than
                // reaching the row's.
                on:click=move |ev: web_sys::MouseEvent| {
                    ev.prevent_default();
                    ev.stop_propagation();
                    // AGS-29: the latest click wins — a held one is dropped.
                    rowclick::feed(rowclick::Ev::Other);
                    if rowclick::glyph_toggles(ev.detail()) {
                        on_toggle();
                    }
                }
                on:dblclick=move |ev: web_sys::MouseEvent| {
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            >
                {toggle_glyph}
            </span>
            // FILTER-22: the kind, the id and the status are tags; a
            // right-click anywhere else on the row is the ticket menu's.
            <span class="toggle kind" title=move || kind.get() data-tag=move || crate::tags::Tag::kind(ticket.with(|t| t.kind)).attr()>
                {move || kind_glyph(kind.get())}
            </span>
            <span class="code" data-tag=crate::tags::Tag::ticket(&id).attr()>{id.clone()}</span>
            <span class="label">{move || ticket.with(|t| t.title.clone())}</span>
            // FILTER-24: why a ticket the filter excludes is here.
            {move || {
                face.with(|f| f.as_ref().is_some_and(|f| f.outside)).then(|| {
                    view! { <span class="outside-mark" title="outside the nav's filter — shown because it is the open ticket">"outside"</span> }
                })
            }}
            // MENGER-12: its labels.
            {crate::labels::row_chips(move || ticket.with(|t| t.labels.clone()))}
            // FILTER-14: the matches under the row, as its badge (a
            // shortcode's row has its tally instead).
            {move || {
                face.with(|f| f.as_ref().and_then(|f| f.badge.clone())).map(|b| {
                    let (title, at) = (format!("{b} matching below"), b.clone());
                    view! { <span class="below" data-below=at title=title>{b}</span> }
                })
            }}
            {rollup.map(|r| roll_tag(r, face))}
            // MENGER-13: a canceled row, closed: what it holds, not shown.
            {move || folded.get().map(|n| {
                let title = format!("canceled: its {n} child ticket{} not shown — open it to look", if n == 1 { "" } else { "s" });
                view! { <span class="folded" title=title>{format!("{n} hidden")}</span> }
            })}
            // The status tag, its paint and its word one effect: they move
            // together.
            {move || {
                let st = ticket.with(|t| t.status);
                view! { <span class=format!("tag {}", st_class(st.as_str())) data-tag=crate::tags::Tag::status(st).attr()>{st.as_str()}</span> }
            }}
        </a>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::{
        art, chunks, climb, drawn_steps, face, face_of, hidden_count, kept_codes, kept_face, Holds, hidden_line, kind_glyph, last_drawn, migrate_expanded, offset_of,
        on_chain, rollup_label, root_has_kids, shortcode_face, shortcode_node, shown, span, summed, Ask, Climb, Face, Geom, List, Mark,
        Node, Source, CHUNK, PAGE, WHOLE, centring, next_stop, under_group, with_under, Stop, near,
    };

    /// ACT-7: a lazy list asks once it is on the screen, or within two
    /// screens below it, or a screen above.
    #[test]
    fn a_lazy_list_asks_when_near() {
        let at = |top: f64| Geom { above: -top, viewport: 900.0, pitch: 19.0 };
        assert!(near(at(0.0), 0), "at the top");
        assert!(near(at(850.0), 3), "on the screen");
        assert!(near(at(1700.0), 3), "within two screens below");
        assert!(!near(at(1800.0), 3), "further down");
        assert!(near(at(-950.0), 10), "its end within a screen above");
        assert!(!near(at(-2000.0), 10), "far above");
        assert!(near(at(-2000.0), 100), "a long list reaching the screen");
    }
    use crate::board::Matches;
    use crate::filter::ScTree;
    use menger_tickets_api::{Group, Kind, RootRow, Status, TicketSummary};
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    /// A group as `groups {}` sends it (AGS-23: with its root's row).
    fn group(epic: &str, total: u64, done: u64, docs: u64, root: Option<(&str, &str, Status, Kind)>, host: Option<&str>) -> Group {
        Group {
            epic: epic.into(),
            total,
            done,
            docs,
            root: root.map(|r| r.0.into()),
            root_row: root.map(|(_, title, status, kind)| RootRow { title: title.into(), status, kind }),
            host: host.map(Into::into),
        }
    }

    /// The demo store AGS-23 was checked against in the browser: SUP, whose
    /// SUP-4 hosts the sub-shortcode SUB; DOCS, a doc collection; ALPHA, a
    /// plain one; and PRE, a store from before MNG-12 — no root.
    fn demo() -> BTreeMap<String, Group> {
        [
            group("ALPHA", 3, 0, 0, Some(("ALPHA-1", "Alpha: the first scope", Status::Ready, Kind::Scope)), None),
            group("DOCS", 0, 0, 4, Some(("DOCS-1", "Reference shelf", Status::Doc, Kind::Doc)), None),
            group("PRE", 5, 0, 0, None, None),
            group("SUB", 4, 1, 0, Some(("SUB-1", "Codegen", Status::Pending, Kind::Scope)), Some("SUP-4")),
            group("SUP", 11, 2, 0, Some(("SUP-1", "Superproject", Status::Coding, Kind::Scope)), None),
        ]
        .into_iter()
        .map(|g| (g.epic.clone(), g))
        .collect()
    }

    fn row(id: &str, parent: Option<&str>) -> TicketSummary {
        let (epic, seq) = id.rsplit_once('-').unwrap();
        TicketSummary {
            labels: Vec::new(),
            id: id.into(),
            epic: epic.into(),
            seq: seq.parse().unwrap(),
            title: id.into(),
            status: Status::Pending,
            kind: Kind::Build,
            severity: None,
            parent: parent.map(Into::into),
            child_count: 0,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            commits: Vec::new(),
            path: None,
            placement: None,
            created_unix: 0,
            updated_unix: 0,
        }
    }

    /// FILTER-21: the header's "menger docs" toggle (WAVE-10) is gone, and
    /// nothing reads the setting it stored — menger's own documents are the
    /// nav filter's, left out by its default (`lens::nav_default`). Neither
    /// the toggle nor its key is named by the nav, its flat list, the lens or
    /// the page's styles.
    #[test]
    fn the_menger_docs_toggle_and_its_stored_setting_are_gone() {
        let key = ["menger.nav.", "mengerDocs"].concat();
        let toggle = ["menger", "-docs"].concat();
        let off = ["menger", "-off"].concat();
        for (name, src) in [
            ("nav.rs", include_str!("nav.rs")),
            ("flat.rs", include_str!("flat.rs")),
            ("lens.rs", include_str!("lens.rs")),
            ("index.html", include_str!("../index.html")),
        ] {
            for gone in [&key, &toggle, &off] {
                assert!(!src.contains(gone.as_str()), "{name} still names {gone}");
            }
        }
    }

    /// AGS-23, the indent: each level of the tree adds two monospace columns
    /// of connector art, not four — and the art still tells apart the four
    /// cases it always did: an ancestor with siblings to come (`│`), one that
    /// was last (blank), a row with siblings after it (`├`), the last (`└`).
    #[test]
    fn each_level_moves_right_two_columns_and_the_art_stays_unambiguous() {
        assert_eq!(art([], false), "├─");
        assert_eq!(art([], true), "└─");
        assert_eq!(art([false], false), "│ ├─");
        assert_eq!(art([true], true), "  └─");
        assert_eq!(art([false, true, false], true), "│   │ └─");
        for depth in 0..12 {
            for last in [false, true] {
                let ancestors: Vec<bool> = (0..depth).map(|d| d % 3 == 0).collect();
                let s = art(ancestors.iter().copied(), last);
                assert_eq!(s.chars().count(), 2 * (depth + 1), "depth {depth}: {s:?}");
                // The row's own connector starts two columns further in per
                // level, and each ancestor's column is its line or a blank.
                let cols: Vec<char> = s.chars().collect();
                assert_eq!(cols[2 * depth], if last { '└' } else { '├' });
                for (d, was_last) in ancestors.iter().enumerate() {
                    assert_eq!(cols[2 * d], if *was_last { ' ' } else { '│' }, "depth {depth}, ancestor {d}");
                    assert_eq!(cols[2 * d + 1], ' ');
                }
            }
        }
    }

    /// AGS-23, the shape: a shortcode with a root is drawn as the root's
    /// row — keyed by the root's id, its face the group's `root_row` (the
    /// kind glyph, the title, the status; a doc collection's root its `Doc`
    /// chip), its roll-up the shortcode's — whether it is at the top level
    /// or a sub-shortcode under its host. A shortcode with no root keeps its
    /// own row.
    #[test]
    fn a_shortcode_with_a_root_is_its_roots_row_and_one_without_keeps_its_own() {
        let g = demo();
        let sup = &g["SUP"];
        assert_eq!(shortcode_node(sup), Node::Root("SUP-1".into(), "SUP".into()));
        assert_eq!(shortcode_node(sup).key(), "SUP-1", "the root's key: its expansion is the root's");
        let t = face(sup).unwrap();
        assert_eq!((t.id.as_str(), t.title.as_str(), t.status, t.kind), ("SUP-1", "Superproject", Status::Coding, Kind::Scope));
        assert_eq!((t.epic.as_str(), t.seq, t.parent), ("SUP", 1, None), "top level: no host");
        assert_eq!(kind_glyph(t.kind.as_str()), "◆");
        assert_eq!(rollup_label(sup), "2/11", "the shortcode's roll-up moves onto the root's row");
        assert!(root_has_kids(sup));

        // A sub-shortcode: its root's row too, under its host.
        let sub = &g["SUB"];
        assert_eq!(shortcode_node(sub), Node::Root("SUB-1".into(), "SUB".into()));
        assert_eq!(face(sub).unwrap().parent.as_deref(), Some("SUP-4"));
        assert_eq!(rollup_label(sub), "1/4");

        // A doc collection: its root is a doc, in `Doc`.
        let docs = &g["DOCS"];
        let d = face(docs).unwrap();
        assert_eq!((d.status, d.kind, kind_glyph(d.kind.as_str())), (Status::Doc, Kind::Doc, "¶"));
        assert_eq!(rollup_label(docs), "4 docs");
        assert!(root_has_kids(docs));

        // A root alone opens onto nothing.
        let lone = group("LONE", 1, 0, 0, Some(("LONE-1", "alone", Status::Pending, Kind::Scope)), None);
        assert!(!root_has_kids(&lone));
        let lone_doc = group("NOTE", 0, 0, 1, Some(("NOTE-1", "a note", Status::Doc, Kind::Doc)), None);
        assert!(!root_has_kids(&lone_doc));

        // No root (a store from before MNG-12): the shortcode row, keyed by
        // its code, and no face to draw a ticket row with.
        let pre = &g["PRE"];
        assert_eq!(shortcode_node(pre), Node::Group("PRE".into()));
        assert_eq!(shortcode_node(pre).key(), "PRE");
        assert_eq!(face(pre), None);
        assert_eq!(rollup_label(pre), "0/5");
    }

    /// AGS-23, the chain a deep link opens: no shortcode row above a root —
    /// a top-level root is the top, a sub-shortcode's root steps to its host
    /// (which opens), and a rootless shortcode's row is the whole chain. So
    /// SUB-4 — under SUB-2, under SUB's root, hosted under SUP-4, under SUP's
    /// root — opens four nodes, every one a ticket, and sits at depth four:
    /// ten columns of art, where AGS-20's tree put it at depth six, 28.
    #[test]
    fn a_deep_link_opens_one_level_fewer_per_shortcode() {
        let g = demo();
        let rows: HashMap<String, TicketSummary> = [
            row("SUP-1", None),
            row("SUP-4", Some("SUP-1")),
            row("SUB-1", Some("SUP-4")),
            row("SUB-2", Some("SUB-1")),
            row("SUB-4", Some("SUB-2")),
            row("PRE-2", Some("PRE-1")),
            row("LEGACY-3", None),
        ]
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();
        let chain = |leaf: &str| -> Option<Vec<Node>> {
            let mut out = Vec::new();
            let mut cur = rows[leaf].clone();
            loop {
                match climb(g.get(&cur.epic), &cur) {
                    Climb::Top => return Some(out),
                    Climb::Host(h) | Climb::Parent(h) => {
                        out.push(Node::Ticket(h.clone()));
                        cur = rows[h.as_str()].clone();
                    }
                    Climb::Flat(e) => {
                        out.push(Node::Group(e));
                        return Some(out);
                    }
                    Climb::Lost => return None,
                }
            }
        };
        let keys = |leaf: &str| chain(leaf).map(|c| c.iter().map(|n| n.key().to_string()).collect::<Vec<_>>());
        assert_eq!(keys("SUB-4").unwrap(), ["SUB-2", "SUB-1", "SUP-4", "SUP-1"]);
        assert_eq!(climb(g.get("SUB"), &rows["SUB-1"]), Climb::Host("SUP-4".into()), "a hosted root: its host opens");
        assert_eq!(keys("SUP-1").unwrap(), Vec::<String>::new(), "a top-level root is the top row: nothing opens");
        assert_eq!(keys("PRE-2").unwrap(), ["PRE"], "no root: the shortcode row, over its flat list");
        assert_eq!(keys("LEGACY-3"), None, "a parentless non-root is drawn nowhere");
        let depth = chain("SUB-4").unwrap().len();
        assert_eq!((depth, art(vec![false; depth], true).chars().count()), (4, 10));
    }

    /// AGS-23: what an earlier nav persisted — an open shortcode's code —
    /// opens that shortcode's root now (the row whose one child the root
    /// was is the root's), so an open tree stays open. A rootless
    /// shortcode's code, a ticket's id and a code no group names stay as
    /// they were; a second pass moves nothing.
    #[test]
    fn a_persisted_shortcode_key_opens_its_root() {
        let g = demo();
        // ACT-7: the set as it was stored, read as a shape's plain marks.
        let set = |v: &[&str]| crate::shape::Shape::load(Some(&serde_json::to_string(v).unwrap()));
        let keys = |s: &crate::shape::Shape| s.marks.keys().cloned().collect::<BTreeSet<String>>();
        let was = set(&["SUP", "SUP-1", "SUP-4", "SUB", "DOCS", "PRE", "PRE-1", "GONE"]);
        let now = migrate_expanded(&was, &g).expect("the codes with a root move");
        assert_eq!(keys(&now), keys(&set(&["SUP-1", "SUP-4", "SUB-1", "DOCS-1", "PRE", "PRE-1", "GONE"])));
        assert_eq!(migrate_expanded(&now, &g), None, "once");
        assert_eq!(migrate_expanded(&set(&[]), &g), None);
        assert_eq!(migrate_expanded(&set(&["SUP-1", "PRE"]), &g), None);
    }

    /// The six kinds (SDLC-11: the doc's `¶` too) each get their own glyph;
    /// the neutral dot is reserved for whatever the table does not know.
    #[test]
    fn every_kind_has_a_distinct_glyph_and_unknown_is_the_neutral_dot() {
        let kinds = ["scope", "execution", "build", "gap", "spike", "doc"];
        let glyphs: std::collections::BTreeSet<&str> = kinds.iter().map(|k| kind_glyph(k)).collect();
        assert_eq!(glyphs.len(), kinds.len(), "distinct: {glyphs:?}");
        assert!(!glyphs.contains("·"));
        assert_eq!(kind_glyph("bug"), "·");
        assert_eq!(kind_glyph(""), "·");
        // One character each: the marker slot is 1ch wide.
        for k in kinds {
            assert_eq!(kind_glyph(k).chars().count(), 1, "{k}");
        }
        // …and the list is the type's, every kind of it.
        assert_eq!(kinds.to_vec(), Kind::ALL.map(Kind::as_str).to_vec());
    }

    /// SDLC-11: a row's roll-up is the work's `done/total`, whatever docs
    /// it holds; a row of docs alone counts its docs.
    #[test]
    fn a_rollup_shows_the_works_done_over_total_and_a_doc_collection_its_docs() {
        let g = |total, done, docs| Group { epic: "G".into(), total, done, docs, root: None, root_row: None, host: None };
        assert_eq!(rollup_label(&g(5, 2, 0)), "2/5");
        assert_eq!(rollup_label(&g(4, 4, 3)), "4/4", "docs do not hold a row open");
        assert_eq!(rollup_label(&g(0, 0, 1)), "1 doc");
        assert_eq!(rollup_label(&g(0, 0, 7)), "7 docs");
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

    // -----------------------------------------------------------------
    // FILTER-14: the nav under the lens

    /// The demo store's tree of shortcodes, and counts over it: SUB is hosted
    /// under SUP's SUP-4.
    fn sums_of(counts: &[(&str, u64)]) -> (ScTree, Matches) {
        let t = ScTree::of(&demo());
        (t, Matches::new(counts.iter().map(|(e, n)| (e.to_string(), *n))))
    }

    /// FILTER-14 acceptance 1, counts summed along the hosts: a shortcode
    /// row's number is its own matches and those of every shortcode hosted
    /// under it, at any depth — SUP's is SUP's and SUB's; SUB's (drawn under
    /// its host) its own; a shortcode the counts do not name has none.
    #[test]
    fn a_shortcode_counts_its_matches_and_those_of_the_shortcodes_it_hosts() {
        let (t, m) = sums_of(&[("SUP", 2), ("SUB", 3), ("ALPHA", 1)]);
        let s = summed(&t, |e| m.held(e));
        assert_eq!(s["SUP"], 5, "SUP's two and SUB's three");
        assert_eq!(s["SUB"], 3);
        assert_eq!(s["ALPHA"], 1);
        assert_eq!((s["DOCS"], s["PRE"]), (0, 0));
        assert_eq!(s.len(), 5, "every shortcode the groups know");
        // A hosted shortcode's matches alone hold its host's row open.
        let (t, m) = sums_of(&[("SUB", 1)]);
        let s = summed(&t, |e| m.held(e));
        assert_eq!((s["SUP"], s["SUB"]), (1, 1));
        // Deeper: a shortcode hosted under a hosted one counts at every level.
        let mut g = demo();
        g.insert("LEAF".into(), group("LEAF", 2, 0, 0, Some(("LEAF-1", "leaf", Status::Pending, Kind::Scope)), Some("SUB-2")));
        let t = ScTree::of(&g);
        let m = Matches::new([("LEAF".to_string(), 4u64), ("SUP".to_string(), 1)]);
        let s = summed(&t, |e| m.held(e));
        assert_eq!((s["SUP"], s["SUB"], s["LEAF"]), (5, 4, 4));
    }

    /// FILTER-14 acceptance 1, a hidden shortcode: one with no match is not
    /// drawn, the last line counts it, and toggled on every shortcode is
    /// drawn — the rest dimmed (its face). A ticket row is never the counts'
    /// to hide.
    #[test]
    fn a_shortcode_with_no_match_is_hidden_and_counted_on_the_last_line() {
        let (t, m) = sums_of(&[("SUB", 1), ("ALPHA", 2)]);
        let s = summed(&t, |e| m.held(e));
        let g = demo();
        let nodes: Vec<Node> = ["ALPHA", "DOCS", "PRE", "SUP"].iter().map(|e| shortcode_node(&g[*e])).collect();
        let keys = |v: Vec<Node>| v.iter().map(|n| n.key().to_string()).collect::<Vec<_>>();
        assert_eq!(keys(shown(nodes.clone(), &s, false, &BTreeSet::new())), ["ALPHA-1", "SUP-1"], "DOCS and PRE hidden; SUP holds SUB's match");
        assert_eq!(keys(shown(nodes.clone(), &s, true, &BTreeSet::new())), ["ALPHA-1", "DOCS-1", "PRE", "SUP-1"], "toggled: every one");
        assert_eq!(hidden_count(&s, &BTreeSet::new()), 2, "DOCS, PRE");
        assert_eq!(hidden_line(2), "2 shortcodes hidden by the filter");
        assert_eq!(hidden_line(1), "1 shortcode hidden by the filter");
        assert_eq!(hidden_line(0), "0 shortcodes hidden by the filter");
        // A hidden one drawn on the line's say-so is a dimmed heading and
        // opens onto nothing.
        assert_eq!(shortcode_face(s["DOCS"], None, Holds::No), Face { heading: true, dim: true, badge: None, opens: false, outside: false });
        assert_eq!(shortcode_face(s["PRE"], None, Holds::No), Face { heading: true, dim: true, badge: None, opens: false, outside: false });
        // One with matches is drawn and opens.
        assert_eq!(shortcode_face(s["SUP"], None, Holds::No), Face { heading: true, dim: false, badge: None, opens: true, outside: false });
        // A ticket node is not a shortcode's: never hidden by the counts.
        assert_eq!(keys(shown(vec![Node::Ticket("X-2".into())], &s, false, &BTreeSet::new())), ["X-2"]);
    }

    /// FILTER-23: no holder is drawn — every ticket row the lens draws is a
    /// match, as today, badged with the matches below it and opening onto
    /// them; a match with nothing below is a leaf. A shortcode whose root
    /// does not match is a heading — its code and its count, no ticket row —
    /// and so is one whose root's word has not come; a root that matches is
    /// its ticket row. Either carries its tally, not a badge.
    #[test]
    fn a_root_that_does_not_match_is_a_heading_and_every_ticket_row_a_match() {
        let both = face_of(Mark { matched: true, below: 12 });
        assert_eq!(both, Face { heading: false, dim: false, badge: Some("12".into()), opens: true, outside: false });
        let leaf = face_of(Mark { matched: true, below: 0 });
        assert_eq!(leaf, Face { heading: false, dim: false, badge: None, opens: false, outside: false });
        // The ticket's first example: FILTER-1 is a scope, the filter `kind
        // build` — FILTER is a heading with its builds under it.
        assert_eq!(shortcode_face(2, Some(Mark { matched: false, below: 2 }), Holds::No), Face { heading: true, dim: false, badge: None, opens: true, outside: false });
        // The second: RETURNS-1 matches `status Pending` — its own row, its
        // matches nested under it.
        assert_eq!(shortcode_face(2, Some(Mark { matched: true, below: 1 }), Holds::No), Face { heading: false, dim: false, badge: None, opens: true, outside: false });
        // A root that is the shortcode's only match: its row, a leaf.
        assert_eq!(shortcode_face(1, Some(Mark { matched: true, below: 0 }), Holds::No), Face { heading: false, dim: false, badge: None, opens: false, outside: false });
        // Without its word, a heading that opens onto the matches.
        assert_eq!(shortcode_face(4, None, Holds::No), Face { heading: true, dim: false, badge: None, opens: true, outside: false });
    }

    /// FILTER-24: the open ticket is drawn whatever the filter. The
    /// shortcodes leading to it — its own and those hosting it — are drawn
    /// with no match and open, and are not counted on the hidden line; its
    /// row is marked outside the filter when it does not match (as a match it
    /// is a match's row); a root that is the open ticket is its ticket row,
    /// not a heading.
    #[test]
    fn the_open_ticket_and_the_shortcodes_leading_to_it_are_drawn() {
        let t = ScTree::of(&demo());
        assert_eq!(kept_codes(&t, "SUB-3"), BTreeSet::from(["SUB".to_string(), "SUP".to_string()]), "SUB is hosted at SUP-4");
        assert_eq!(kept_codes(&t, "PRE-2"), BTreeSet::from(["PRE".to_string()]));
        assert_eq!(kept_codes(&t, "not an id"), BTreeSet::new());
        let (t, m) = sums_of(&[("ALPHA", 2)]);
        let s = summed(&t, |e| m.held(e));
        let g = demo();
        let nodes: Vec<Node> = ["ALPHA", "DOCS", "PRE", "SUP"].iter().map(|e| shortcode_node(&g[*e])).collect();
        let keys = |v: Vec<Node>| v.iter().map(|n| n.key().to_string()).collect::<Vec<_>>();
        let kept = kept_codes(&t, "SUB-3");
        assert_eq!(keys(shown(nodes.clone(), &s, false, &kept)), ["ALPHA-1", "SUP-1"], "SUP leads to SUB-3");
        assert_eq!(hidden_count(&s, &kept), 2, "DOCS, PRE — not SUP or SUB");
        assert_eq!(hidden_count(&s, &BTreeSet::new()), 4);
        // Leading to it with no match: a dimmed heading that opens.
        assert_eq!(shortcode_face(0, None, Holds::Under), Face { heading: true, dim: true, badge: None, opens: true, outside: false });
        // With matches, as without it.
        assert_eq!(shortcode_face(2, Some(Mark { matched: false, below: 2 }), Holds::Under), shortcode_face(2, Some(Mark { matched: false, below: 2 }), Holds::No));
        // Its root is the open ticket: its row, marked — unless it matches.
        assert_eq!(shortcode_face(0, None, Holds::Root), Face { heading: false, dim: true, badge: None, opens: false, outside: true });
        assert_eq!(shortcode_face(3, Some(Mark { matched: false, below: 3 }), Holds::Root), Face { heading: false, dim: true, badge: None, opens: true, outside: true });
        assert_eq!(shortcode_face(3, Some(Mark { matched: true, below: 2 }), Holds::Root), shortcode_face(3, Some(Mark { matched: true, below: 2 }), Holds::No));
        // The open ticket's row: marked outside when it does not match.
        assert_eq!(kept_face(Mark { matched: false, below: 1 }), Face { heading: false, dim: true, badge: Some("1".into()), opens: true, outside: true });
        assert_eq!(kept_face(Mark { matched: true, below: 0 }), face_of(Mark { matched: true, below: 0 }));
    }

    /// FILTER-23: an open list under the lens draws a hosted shortcode in its
    /// place in the answer — so the list's last row is the last one it
    /// draws: past the shortcodes without a match at its end; where the
    /// window has not read that far, unknown.
    #[test]
    fn the_last_row_drawn_skips_the_shortcodes_not_drawn() {
        let mut l = List::of(5);
        l.place(0, ["A-3", "A-5", "A-9", "B-1", "C-1"].map(String::from).to_vec());
        let none = |_: &str| false;
        assert_eq!(last_drawn(&l, none).as_deref(), Some("C-1"));
        assert_eq!(last_drawn(&l, |id| id == "C-1").as_deref(), Some("B-1"));
        assert_eq!(last_drawn(&l, |id| id == "C-1" || id == "B-1").as_deref(), Some("A-9"));
        assert_eq!(last_drawn(&List::of(0), none), None);
        let mut far = List::of(400);
        far.place(0, vec!["A-1".into()]);
        assert_eq!(last_drawn(&far, none), None, "the end is not read yet");
    }

    /// FILTER-14 acceptance 1, applying `entered` / `left`: the counts move
    /// (a shortcode whose last match leaves is hidden; one a match enters is
    /// drawn, and so is its host), and the open lists that ask again are
    /// those on the moved ticket's chain — a parent's when the parent is on
    /// it, a flat shortcode's when the ticket is in it — and no other.
    #[test]
    fn entered_and_left_move_the_counts_and_the_open_pages_on_the_chain() {
        let t = ScTree::of(&demo());
        let mut m = Matches::new([("SUP".to_string(), 1u64), ("ALPHA".to_string(), 1)]);
        let g = demo();
        let nodes: Vec<Node> = ["ALPHA", "PRE", "SUP"].iter().map(|e| shortcode_node(&g[*e])).collect();
        let keys = |m: &Matches| {
            let s = summed(&t, |e| m.held(e));
            shown(nodes.clone(), &s, false, &BTreeSet::new()).iter().map(|n| n.key().to_string()).collect::<Vec<_>>()
        };
        assert_eq!(keys(&m), ["ALPHA-1", "SUP-1"]);
        m.left("ALPHA");
        assert_eq!(keys(&m), ["SUP-1"], "ALPHA's last match left: hidden");
        m.entered("SUB");
        let s = summed(&t, |e| m.held(e));
        assert_eq!((s["SUP"], s["SUB"]), (2, 1), "a match entered SUB: SUB's and its host's counts move");
        m.entered("PRE");
        assert_eq!(keys(&m), ["PRE", "SUP-1"], "PRE drawn");
        m.left("SUP");
        m.left("SUB");
        assert_eq!(keys(&m), ["PRE"]);

        // The open lists on SUB-4's chain (SUB-4 < SUB-2 < SUB-1 < SUP-4 < SUP-1).
        let chain: Vec<String> = ["SUB-4", "SUB-2", "SUB-1", "SUP-4", "SUP-1"].map(String::from).to_vec();
        let open = [
            Source::Parent("SUP-1".into()),
            Source::Parent("SUP-4".into()),
            Source::Parent("SUB-2".into()),
            Source::Parent("SUB-4".into()),
            Source::Parent("SUP-7".into()),
            Source::Parent("ALPHA-1".into()),
            Source::Epic("PRE".into()),
            Source::Epic("SUB".into()),
        ];
        let asked: Vec<bool> = open.iter().map(|s| on_chain(s, &chain)).collect();
        assert_eq!(asked, [true, true, true, true, false, false, false, true]);
    }

    /// FILTER-23: a deep link under the lens opens through the rows the
    /// lens draws only — each under the nearest drawn one above it — and
    /// ends at the deepest drawn: the open ticket when it matches, else its
    /// nearest drawn ancestor; nothing when nothing is drawn.
    #[test]
    fn a_deep_link_under_the_lens_opens_through_the_drawn_rows_only() {
        let t = |id: &str| Node::Ticket(id.to_string());
        // RETURNS-4 (a build) under RETURNS-3 (an execution) under RETURNS-1
        // (the root, a scope), under `kind build`: the heading RETURNS opens
        // onto RETURNS-4 directly.
        let chain = [t("RETURNS-4"), t("RETURNS-3"), t("RETURNS-1")];
        let (steps, at) = drawn_steps(&chain, &[true, false, true]).unwrap();
        assert_eq!(steps, [(t("RETURNS-1"), "RETURNS-4".to_string())]);
        assert_eq!(at, "RETURNS-4");
        // Every one drawn: the chain as the unfiltered nav opens it, nearest
        // first (as `chain_to`'s steps).
        let (steps, at) = drawn_steps(&chain, &[true, true, true]).unwrap();
        assert_eq!(steps, [(t("RETURNS-3"), "RETURNS-4".to_string()), (t("RETURNS-1"), "RETURNS-3".to_string())]);
        assert_eq!(at, "RETURNS-4");
        // The open ticket does not match: the chain ends at its nearest
        // drawn ancestor — here the shortcode's heading, which opens nothing.
        let (steps, at) = drawn_steps(&chain, &[false, false, true]).unwrap();
        assert_eq!((steps, at.as_str()), (Vec::new(), "RETURNS-1"));
        // A hosted shortcode's row moves up with its host: FILTER-19 under
        // FILTER-1 (FILTER's root, hosted at MENGER-2, an area that does not
        // match) under MENGER-1 (the root): MENGER's heading opens onto
        // FILTER's, FILTER's onto FILTER-19.
        let chain = [t("FILTER-19"), t("FILTER-1"), t("MENGER-2"), t("MENGER-1")];
        let (steps, at) = drawn_steps(&chain, &[true, true, false, true]).unwrap();
        assert_eq!(steps, [(t("FILTER-1"), "FILTER-19".to_string()), (t("MENGER-1"), "FILTER-1".to_string())]);
        assert_eq!(at, "FILTER-19");
        // A rootless shortcode's row is the chain's top.
        let chain = [t("PRE-3"), t("PRE-2"), Node::Group("PRE".into())];
        let (steps, _) = drawn_steps(&chain, &[true, false, true]).unwrap();
        assert_eq!(steps, [(Node::Group("PRE".into()), "PRE-3".to_string())]);
        assert_eq!(drawn_steps(&chain, &[false, false, false]), None, "nothing drawn: nothing opens");
        assert_eq!(drawn_steps(&[], &[]), None);
    }

    /// AGS-26: a step of the cursor lands on the next ticket's row or gap,
    /// passing over the rows that are no ticket (a shortcode's heading); at
    /// the nav's end there is nowhere to go — which is where ↑ widens.
    #[test]
    fn a_step_lands_on_the_next_ticket_passing_over_headings() {
        use Stop::{Gap, Group as G, Ticket as T};
        // MENGER's heading, FILTER's heading, FILTER-9, its builds, a gap.
        let stops = [G, G, T, T, T, Gap, T];
        assert_eq!(next_stop(&stops, 2, true), Some(3));
        assert_eq!(next_stop(&stops, 4, true), Some(5), "a gap is a stop: it is crossed when fetched");
        assert_eq!(next_stop(&stops, 6, true), None, "the end");
        assert_eq!(next_stop(&stops, 3, false), Some(2));
        assert_eq!(next_stop(&stops, 2, false), None, "the first ticket: only headings above it");
        assert_eq!(next_stop(&stops, 1, true), Some(2), "from a heading, onto the ticket under it");
        assert_eq!(next_stop(&[T, G, G, T], 0, true), Some(3), "headings between two tickets are passed over");
        assert_eq!(next_stop(&[T, G, G, T], 3, false), Some(0));
        assert_eq!(next_stop(&[], 0, true), None);
        assert_eq!(next_stop(&[T], 5, false), Some(0), "an index past the end steps back into the list");
    }

    /// AGS-26: the open ticket's row is centred — the scroll that puts its
    /// middle on the nav's middle.
    #[test]
    fn a_row_is_centred_on_the_navs_middle() {
        // the nav 100 px from the top, 600 tall: its middle at 400
        assert_eq!(centring(100.0, 600.0, 390.0, 20.0), 0.0, "already in the middle");
        assert_eq!(centring(100.0, 600.0, 690.0, 20.0), 300.0, "at the bottom: scroll down 300");
        assert_eq!(centring(100.0, 600.0, 90.0, 20.0), -300.0, "above the nav: scroll up");
    }

    /// AGS-26: `f`'s filter one parent more permissive — its first `under T`
    /// group, made `under` T's parent, in its place; the group goes when T
    /// has none; nothing else moves.
    #[test]
    fn widening_moves_the_under_group_one_parent_up() {
        use menger_tickets_api::filter::Filter;
        let f = |t: &str| Filter::parse(t).unwrap();
        assert_eq!(under_group(&f("under FILTER-9")), Some((0, "FILTER-9".to_string())));
        assert_eq!(under_group(&f("status Ready and under FILTER-9")), Some((1, "FILTER-9".to_string())));
        assert_eq!(under_group(&f("under FILTER")), Some((0, "FILTER".to_string())), "a shortcode");
        assert_eq!(under_group(&f("not under FILTER-9")), None, "a negated group is no subtree");
        assert_eq!(under_group(&f("under in (FILTER-9, FILTER-2)")), None, "nor is a choice of two");
        assert_eq!(under_group(&f("status Ready")), None);
        assert_eq!(under_group(&Filter::default()), None);

        assert_eq!(with_under(&f("under FILTER-9"), 0, Some("FILTER-1")), f("under FILTER-1"));
        assert_eq!(with_under(&f("under FILTER-1"), 0, Some("menger-2")), f("under MENGER-2"), "upper case");
        assert_eq!(
            with_under(&f("status Ready and under FILTER-9 and kind build"), 1, Some("FILTER-1")),
            f("status Ready and under FILTER-1 and kind build"),
            "the other groups stay, in their places"
        );
        assert_eq!(with_under(&f("under MENGER-1"), 0, None), Filter::default(), "a top root: the group goes");
        assert_eq!(with_under(&f("status Ready and under MENGER-1"), 1, None), f("status Ready"));
        assert_eq!(with_under(&f("under FILTER-9"), 3, Some("X-1")), f("under FILTER-9"), "no group there: unchanged");
    }
}
