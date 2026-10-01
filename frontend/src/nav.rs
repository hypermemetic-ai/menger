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
//! a recursive tree, derived in one pure pass ([`plan`]) from the flat row
//! store's `parent` fields:
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
//! itself ([`plan`]) is untouched and still covers every ticket — it is
//! what the rollups count, what the auto-expand walks, and why the tree's
//! key map, which only ever walked the open rows, needs nothing new.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{dom, keys, row_index, st_class, Route, Row};
use menger_tickets_api::Status;

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

// ---------------------------------------------------------------------
// The forest (AGS-20) — pure, so it is the part a native test can pin.
//
// Everything the shape depends on is immutable for a ticket's lifetime
// (`id`, `epic`, `seq`, `kind`, `parent` — the model has no re-parent and
// no re-kind), which is why the plan can be a memo over the row *list*
// and read those fields untracked.

/// The facts one ticket contributes to the forest's shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) id: String,
    pub(crate) epic: String,
    pub(crate) seq: u32,
    pub(crate) kind: String,
    pub(crate) parent: Option<String>,
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
    fn is_group(&self) -> bool {
        matches!(self, Node::Group(_))
    }
}

/// The whole forest, computed once per change of the row list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Top-level group rows, in row order.
    pub(crate) top: Vec<String>,
    /// Node key → its children, tickets first (in (epic, seq) order),
    /// then any sub-shortcode groups it hosts.
    pub(crate) kids: BTreeMap<String, Vec<Node>>,
    /// Node key → the key of the node it hangs under (the auto-expand
    /// walk).
    pub(crate) up: BTreeMap<String, String>,
    /// Group key → every ticket id in its subtree, at every depth — what
    /// a rollup counts.
    pub(crate) subtree: BTreeMap<String, Vec<String>>,
    /// Sub-shortcode epic → the ticket its shortcode row hangs under.
    pub(crate) sub_of: BTreeMap<String, String>,
}

/// Where a ticket is rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place<'a> {
    /// Beneath another ticket's row.
    Under(&'a str),
    /// Directly beneath its epic's shortcode row.
    Root(&'a str),
}

/// A parent's id, if it names a ticket in the store *and* does not make
/// this ticket its own ancestor. The server refuses cycles at create; a
/// hand-edited database must still not make the nav recurse forever, so a
/// ticket on a parent cycle is rendered as a root of its epic (every node
/// of the cycle is, which is what breaks it). A loop *above* this ticket
/// is already broken there, so its own edge is kept.
fn resolved_parent<'a>(
    index: &BTreeMap<&'a str, &'a Facts>,
    f: &'a Facts,
) -> Option<&'a str> {
    let p = f.parent.as_deref()?;
    let mut cur = *index.get(p)?;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    loop {
        if cur.id == f.id {
            return None;
        }
        if !seen.insert(cur.id.as_str()) {
            return Some(p);
        }
        match cur.parent.as_deref().and_then(|n| index.get(n)) {
            Some(next) => cur = *next,
            None => return Some(p),
        }
    }
}

fn place<'a>(
    f: &'a Facts,
    resolved: &BTreeMap<&'a str, Option<&'a str>>,
    sub_of: &BTreeMap<String, String>,
) -> Place<'a> {
    let parent = resolved.get(f.id.as_str()).copied().flatten();
    match sub_of.get(&f.epic) {
        // Inside a sub-shortcode, the epic's own group row is the roof:
        // its scope root (and anything else hanging off the host, or off
        // nothing) is a row of the group; the rest nests normally.
        Some(host) => match parent {
            None => Place::Root(&f.epic),
            Some(p) if p == host => Place::Root(&f.epic),
            Some(p) => Place::Under(p),
        },
        None => match parent {
            Some(p) => Place::Under(p),
            None => Place::Root(&f.epic),
        },
    }
}

/// Would hanging group `epic` under ticket `host` put the group inside
/// itself? Walks the rendered tree upward from `host`; hitting a row of
/// `epic` on the way is the cycle (`PLEXUS-1` under `TRANSPORT-2` while
/// `TRANSPORT` hangs under `PLEXUS-1`).
fn nests_inside<'a>(
    index: &BTreeMap<&'a str, &'a Facts>,
    resolved: &BTreeMap<&'a str, Option<&'a str>>,
    sub_of: &BTreeMap<String, String>,
    host: &str,
    epic: &str,
) -> bool {
    let mut cur = host.to_string();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    loop {
        if !seen.insert(cur.clone()) {
            return true; // a loop we cannot render: refuse the nesting
        }
        let Some(f) = index.get(cur.as_str()) else {
            return false;
        };
        match place(f, resolved, sub_of) {
            Place::Under(p) => cur = p.to_string(),
            Place::Root(g) => {
                if g == epic {
                    return true;
                }
                match sub_of.get(g) {
                    Some(h) => cur = h.clone(),
                    None => return false,
                }
            }
        }
    }
}

/// The forest, from the flat row store's facts (given in (epic, seq)
/// order — the order the store keeps).
pub(crate) fn plan(facts: &[Facts]) -> Plan {
    let index: BTreeMap<&str, &Facts> = facts.iter().map(|f| (f.id.as_str(), f)).collect();
    let resolved: BTreeMap<&str, Option<&str>> = facts
        .iter()
        .map(|f| (f.id.as_str(), resolved_parent(&index, f)))
        .collect();

    // Sub-shortcodes: an epic whose scope root (lowest-seq `scope`
    // ticket) has a parent in another epic. Candidates are accepted in a
    // deterministic order, each checked against the ones already accepted
    // so no group can end up nested inside itself.
    let mut scope_root: BTreeMap<&str, &Facts> = BTreeMap::new();
    for f in facts.iter().filter(|f| f.kind == "scope") {
        scope_root
            .entry(f.epic.as_str())
            .and_modify(|cur| {
                if f.seq < cur.seq {
                    *cur = f;
                }
            })
            .or_insert(f);
    }
    let mut sub_of: BTreeMap<String, String> = BTreeMap::new();
    for (epic, root) in &scope_root {
        let Some(host) = resolved.get(root.id.as_str()).copied().flatten() else {
            continue;
        };
        let Some(hf) = index.get(host) else { continue };
        if hf.epic == **epic {
            continue; // a parent inside the epic is ordinary nesting
        }
        sub_of.insert((*epic).to_string(), host.to_string());
        if nests_inside(&index, &resolved, &sub_of, host, epic) {
            sub_of.remove(*epic);
        }
    }

    let mut kids: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    let mut up: BTreeMap<String, String> = BTreeMap::new();
    for f in facts {
        let (key, node) = match place(f, &resolved, &sub_of) {
            Place::Under(p) => (p.to_string(), Node::Ticket(f.id.clone())),
            Place::Root(g) => (g.to_string(), Node::Ticket(f.id.clone())),
        };
        up.insert(f.id.clone(), key.clone());
        kids.entry(key).or_default().push(node);
    }
    // A sub-shortcode's row comes after its host's own ticket children.
    for (epic, host) in &sub_of {
        up.insert(epic.clone(), host.clone());
        kids.entry(host.clone())
            .or_default()
            .push(Node::Group(epic.clone()));
    }

    // Top level: every epic that is not a sub-shortcode and has at least
    // one parentless ticket. An epic with none never appears here — its
    // tickets are all rendered under their parents.
    let mut top: Vec<String> = Vec::new();
    for f in facts {
        if sub_of.contains_key(&f.epic) || top.iter().any(|t| t == &f.epic) {
            continue;
        }
        if kids.contains_key(&f.epic) {
            top.push(f.epic.clone());
        }
    }

    // Rollups count descendants: every ticket in the group's subtree,
    // including cross-epic children and nested sub-shortcodes.
    let mut subtree: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for g in top.iter().chain(sub_of.keys()) {
        let mut out: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = vec![g.clone()];
        while let Some(k) = stack.pop() {
            for n in kids.get(&k).into_iter().flatten() {
                if !seen.insert(n.key().to_string()) {
                    continue;
                }
                if !n.is_group() {
                    out.push(n.key().to_string());
                }
                stack.push(n.key().to_string());
            }
        }
        out.sort();
        subtree.insert(g.clone(), out);
    }

    Plan {
        top,
        kids,
        up,
        subtree,
        sub_of,
    }
}

/// Bring the selected row into view (block: nearest — no jump if it is
/// already visible). Called a tick after a route change so the `.sel`
/// class has been applied.
fn scroll_selected_into_view() {
    let Some(doc) = dom::window().document() else { return };
    if let Ok(Some(el)) = doc.query_selector("#nav .row.sel") {
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
    }
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
    // the tab stop), but walk only the visible ones.
    let all = tree_items();
    let items = visible_items();
    let Some(cur) = items.iter().position(|el| el.is_same_node(Some(&current))) else {
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

    let handled = match key.as_str() {
        "ArrowDown" | "j" => {
            if cur + 1 < items.len() {
                keys::focus_and_rove(&all, &items[cur + 1]);
            }
            true
        }
        "ArrowUp" | "k" => {
            if cur > 0 {
                keys::focus_and_rove(&all, &items[cur - 1]);
            }
            true
        }
        "Home" => {
            keys::focus_and_rove(&all, &items[0]);
            true
        }
        "End" => {
            let last = items.len() - 1;
            keys::focus_and_rove(&all, &items[last]);
            true
        }
        // Open a closed node in place; step into an open one. On a row
        // with no children there is nowhere further in — the pattern says
        // do nothing.
        "ArrowRight" | "l" => {
            if expandable {
                if !open {
                    toggle(node_key);
                } else if cur + 1 < items.len() {
                    keys::focus_and_rove(&all, &items[cur + 1]);
                }
            }
            true
        }
        // Close an open node in place; otherwise step out to the row it
        // hangs under (which is where a second Left then closes it).
        "ArrowLeft" | "h" => {
            if expandable && open {
                toggle(node_key);
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
                None => toggle(node_key),
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
    rows: RwSignal<Vec<Row>>,
    route: RwSignal<Route>,
    expanded: RwSignal<BTreeSet<String>>,
    plan: Memo<Plan>,
}

/// The left panel: brand header + the tree. Always mounted; the App hides
/// it with a class so its DOM (and every row's identity) survives a
/// hide/reveal.
#[component]
pub(crate) fn Nav(
    rows: RwSignal<Vec<Row>>,
    route: RwSignal<Route>,
    hidden: RwSignal<bool>,
) -> impl IntoView {
    let expanded = RwSignal::new(load_expanded());
    Effect::new(move |_| expanded.with(save_expanded));

    // The forest, recomputed only when the row *list* changes (an insert
    // or a removal): `kind` and `parent` are immutable for a ticket's
    // lifetime, so they are read untracked and a `status_changed` never
    // re-runs this.
    let plan: Memo<Plan> = Memo::new(move |_| {
        let facts = rows.with(|v| {
            v.iter()
                .map(|r| {
                    let (kind, parent) =
                        r.ticket.with_untracked(|t| (t.kind.to_string(), t.parent.clone()));
                    Facts {
                        id: r.id.clone(),
                        epic: r.epic.clone(),
                        seq: r.seq,
                        kind,
                        parent,
                    }
                })
                .collect::<Vec<Facts>>()
        });
        self::plan(&facts)
    });
    let ctx = Ctx {
        rows,
        route,
        expanded,
        plan,
    };
    let top = Memo::new(move |_| plan.with(|p| p.top.clone()));
    let top_last = Memo::new(move |_| top.with(|t| t.last().cloned()));

    // Selection follows the route. The chain holding the open ticket
    // auto-expands so the selected row is visible — once per navigation
    // (`revealed` remembers which id was handled), so a later live insert
    // does not fight a node the user then chose to close. Tracks the plan
    // too: on a cold deep link the route is known before the snapshot
    // lands, and the expansion happens when the ticket's row appears.
    let revealed = StoredValue::new(None::<String>);
    Effect::new(move |_| {
        let Route::Ticket(id) = route.get() else { return };
        // Every node between the ticket and the top level, walked through
        // the plan (its parent rows, their groups, a sub-shortcode's row
        // and its host's group alike) — a visited guard because a plan is
        // only as sane as the store it was built from.
        let chain = plan.with(|p| {
            let mut out: Vec<String> = Vec::new();
            let mut cur = id.clone();
            while let Some(next) = p.up.get(&cur) {
                if out.iter().any(|k| k == next) {
                    break;
                }
                out.push(next.clone());
                cur = next.clone();
            }
            out
        });
        if chain.is_empty() {
            return; // the ticket's row has not arrived yet
        }
        if revealed.get_value().as_deref() == Some(id.as_str()) {
            return;
        }
        revealed.set_value(Some(id));
        let missing: Vec<String> =
            expanded.with_untracked(|e| chain.into_iter().filter(|k| !e.contains(k)).collect());
        if !missing.is_empty() {
            expanded.update(|e| e.extend(missing));
        }
        spawn_local(async {
            dom::sleep_ms(0).await;
            scroll_selected_into_view();
        });
    });

    // AGS-18: keep exactly one row as the tree's tab stop as the forest
    // changes shape (rows arrive, a node opens, the route moves). The
    // work is a DOM read plus at most two attribute writes, a tick after
    // the render that changed things.
    Effect::new(move |_| {
        rows.with(|v| v.len());
        expanded.with(|e| e.len());
        let _ = route.get();
        spawn_local(async {
            dom::sleep_ms(0).await;
            ensure_tree_tab_stop();
        });
    });

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
            <div class="tree-scroll">
                <div
                    class="tree"
                    role="tree"
                    aria-label="tickets"
                    on:keydown=move |ev| tree_keydown(ev, expanded)
                >
                    <For
                        each=move || top.get()
                        key=|g| g.clone()
                        children=move |g: String| {
                            node_view(Node::Group(g), ctx, Arc::new(Vec::new()), top_last)
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

/// One node of the tree — an epic's shortcode row or a ticket row — and,
/// beneath it, a keyed `<For>` of its own children, recursively.
///
/// MFE-12: the `<For>` exists only while the node is open — collapse
/// unmounts the children rather than hiding them, so a closed node costs
/// its own row and an empty container, whatever it holds. Its child list
/// is likewise only read out of the plan while it is open; a closed node
/// asks the plan nothing but whether it has children at all.
fn node_view(node: Node, ctx: Ctx, ancestry: Ancestry, last: Memo<Option<String>>) -> AnyView {
    let key = node.key().to_string();
    let is_group = node.is_group();

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
        let plan = ctx.plan;
        Memo::new(move |_| plan.with(|p| p.kids.get(&k).is_some_and(|v| !v.is_empty())))
    };
    let kids = {
        let k = key.clone();
        let plan = ctx.plan;
        Memo::new(move |_| {
            if open.get() {
                plan.with(|p| p.kids.get(&k).cloned().unwrap_or_default())
            } else {
                Vec::new()
            }
        })
    };
    let last_kid = Memo::new(move |_| kids.with(|v| v.last().map(|n| n.key().to_string())));
    let child_ancestry: Ancestry = {
        let mut v = (*ancestry).clone();
        v.push(mine_last);
        Arc::new(v)
    };

    let prefix = {
        let anc = ancestry.clone();
        move || {
            let mut s = String::new();
            for a in anc.iter() {
                s.push_str(if a.get() { "    " } else { "│   " });
            }
            s.push_str(if mine_last.get() { "└── " } else { "├── " });
            s
        }
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

    // Only one of these runs, so each may take the row's closures by
    // value; the two row shapes are otherwise unrelated.
    let row = if is_group {
        group_row(key.clone(), ctx, prefix, toggle_glyph, toggle, expanded_attr, owns_attr)
    } else {
        leaf_row(key.clone(), ctx, prefix, toggle_glyph, toggle, expanded_attr, owns_attr)
    };

    view! {
        <div
            class="node"
            class:group=is_group
            role="none"
            data-node-key=key.clone()
            data-group=is_group.then(|| key.clone())
            class:collapsed=move || !open.get()
        >
            {row}
            {move || {
                has_kids
                    .get()
                    .then(|| {
                        let child_ancestry = child_ancestry.clone();
                        view! {
                            <div class="kids" role="group" id=kids_id.clone()>
                                {move || {
                                    let child_ancestry = child_ancestry.clone();
                                    open.get()
                                        .then(|| {
                                            view! {
                                                <For
                                                    each=move || kids.get()
                                                    key=|n: &Node| n.key().to_string()
                                                    children=move |n: Node| {
                                                        node_view(n, ctx, child_ancestry.clone(), last_kid)
                                                    }
                                                />
                                            }
                                        })
                                }}
                            </div>
                        }
                    })
            }}
        </div>
    }
    .into_any()
}

/// An epic's shortcode row: the code chip and a `done/total` rollup over
/// its whole subtree (AGS-20 — descendants included, at every depth and
/// across epics).
fn group_row(
    key: String,
    ctx: Ctx,
    prefix: impl Fn() -> String + Send + Sync + 'static,
    toggle_glyph: impl Fn() -> &'static str + Send + Sync + 'static,
    toggle: impl Fn() + Send + Sync + 'static,
    expanded_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
    owns_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> AnyView {
    // MFE-12: counted over the group's own subtree, each ticket found by
    // its key — O(subtree · log N), so every group's rollup together is one
    // pass over the store — rather than a scan of the whole store per
    // group, which was O(groups · N) on every paint of the walk.
    let rollup = {
        let k = key.clone();
        let (plan, rows) = (ctx.plan, ctx.rows);
        move || {
            plan.with(|p| {
                rows.with(|v| {
                    let (mut done, mut total) = (0usize, 0usize);
                    for id in p.subtree.get(&k).into_iter().flatten() {
                        let Some(i) = row_index(v, id) else { continue };
                        total += 1;
                        if v[i].ticket.with(|t| t.status == Status::Complete) {
                            done += 1;
                        }
                    }
                    format!("{done}/{total}")
                })
            })
        }
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
            <span class="tag roll">{rollup}</span>
        </div>
    }
    .into_any()
}

/// One ticket row: a real link (middle-click / copy-link friendly) whose
/// title and status tag render through the row's inner signal. Selected
/// iff the route is this ticket's page. Carries `data-ticket-id`, the
/// AGS-16 menu's attachment (a right-click anywhere on the row opens it).
/// A ticket with children also carries the collapse toggle — clicking it
/// opens the row instead of following the link.
fn leaf_row(
    id: String,
    ctx: Ctx,
    prefix: impl Fn() -> String + Send + Sync + 'static,
    toggle_glyph: impl Fn() -> &'static str + Send + Sync + 'static,
    toggle: impl Fn() + Send + Sync + 'static,
    expanded_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
    owns_attr: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> AnyView {
    // The row exists because the plan was built from it, and a row's
    // signal is stable for the ticket's lifetime. MFE-12: found by its key,
    // not a scan — an expand mounts thousands of these at once.
    let Some(ticket) = ctx
        .rows
        .with_untracked(|v| row_index(v, &id).map(|i| v[i].ticket))
    else {
        return ().into_any();
    };
    let route = ctx.route;
    let sel = {
        let id = id.clone();
        Memo::new(move |_| route.with(|rt| matches!(rt, Route::Ticket(x) if *x == id)))
    };
    let href = format!("#/ticket/{id}");
    let tag_class = move || format!("tag {}", ticket.with(|t| st_class(t.status.as_str())));
    let kind = move || ticket.with(|t| t.kind.to_string());

    view! {
        <a
            class="row leaf"
            class:sel=move || sel.get()
            role="treeitem"
            tabindex="-1"
            aria-selected=move || sel.get().to_string()
            aria-expanded=expanded_attr
            aria-owns=owns_attr
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
    use super::{kind_glyph, plan, Facts, Node};

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

    fn f(id: &str, kind: &str, parent: Option<&str>) -> Facts {
        let (epic, seq) = id.split_once('-').expect("EPIC-N");
        Facts {
            id: id.to_string(),
            epic: epic.to_string(),
            seq: seq.parse().expect("seq"),
            kind: kind.to_string(),
            parent: parent.map(str::to_string),
        }
    }

    fn keys(nodes: Option<&Vec<Node>>) -> Vec<String> {
        nodes
            .map(|v| v.iter().map(|n| n.key().to_string()).collect())
            .unwrap_or_default()
    }

    /// The live fixture's shape: PERF-4 nests under PERF-3, a cross-epic
    /// child nests under its parent in the parent's group (keeping its
    /// own code), and depth is not capped.
    #[test]
    fn children_nest_under_their_parent_at_every_depth_and_across_epics() {
        let p = plan(&[
            f("AGS-1", "scope", None),
            f("AGS-19", "build", None),
            f("PERF-1", "scope", None),
            f("PERF-3", "build", None),
            f("PERF-4", "execution", Some("PERF-3")),
            f("PERF-5", "gap", Some("PERF-4")),
            f("XPC-1", "build", Some("AGS-19")),
        ]);
        assert_eq!(p.top, vec!["AGS", "PERF"], "XPC has no parentless ticket");
        assert_eq!(keys(p.kids.get("AGS")), vec!["AGS-1", "AGS-19"]);
        assert_eq!(keys(p.kids.get("AGS-19")), vec!["XPC-1"], "cross-epic child");
        assert_eq!(keys(p.kids.get("PERF")), vec!["PERF-1", "PERF-3"]);
        assert_eq!(keys(p.kids.get("PERF-3")), vec!["PERF-4"]);
        assert_eq!(keys(p.kids.get("PERF-4")), vec!["PERF-5"], "three deep");
        assert_eq!(p.up.get("PERF-5").map(String::as_str), Some("PERF-4"));
        assert_eq!(p.up.get("XPC-1").map(String::as_str), Some("AGS-19"));
        // Rollups count descendants, the cross-epic child included.
        assert_eq!(p.subtree["AGS"], vec!["AGS-1", "AGS-19", "XPC-1"]);
        assert_eq!(p.subtree["PERF"], vec!["PERF-1", "PERF-3", "PERF-4", "PERF-5"]);
    }

    /// The owner's sub-shortcode rule: TRANSPORT's scope root has a
    /// parent in PLEXUS, so the TRANSPORT shortcode row nests under
    /// PLEXUS-1 with its tickets beneath it, and TRANSPORT is not a
    /// top-level group.
    #[test]
    fn a_sub_shortcode_nests_under_the_parent_scope_and_leaves_the_top_level() {
        let p = plan(&[
            f("PLEXUS-1", "scope", None),
            f("PLEXUS-2", "build", Some("PLEXUS-1")),
            f("TRANSPORT-1", "scope", Some("PLEXUS-1")),
            f("TRANSPORT-2", "build", Some("TRANSPORT-1")),
        ]);
        assert_eq!(p.top, vec!["PLEXUS"], "TRANSPORT is never top-level");
        assert_eq!(p.sub_of.get("TRANSPORT").map(String::as_str), Some("PLEXUS-1"));
        // The host's own children first, then the sub-shortcode's row.
        assert_eq!(keys(p.kids.get("PLEXUS-1")), vec!["PLEXUS-2", "TRANSPORT"]);
        assert_eq!(keys(p.kids.get("TRANSPORT")), vec!["TRANSPORT-1"]);
        assert_eq!(keys(p.kids.get("TRANSPORT-1")), vec!["TRANSPORT-2"]);
        assert_eq!(p.up.get("TRANSPORT").map(String::as_str), Some("PLEXUS-1"));
        // The host's rollup counts the whole sub-shortcode too.
        assert_eq!(
            p.subtree["PLEXUS"],
            vec!["PLEXUS-1", "PLEXUS-2", "TRANSPORT-1", "TRANSPORT-2"]
        );
        assert_eq!(p.subtree["TRANSPORT"], vec!["TRANSPORT-1", "TRANSPORT-2"]);

        // A cross-epic child whose epic has no scope root of its own is
        // NOT a sub-shortcode: it stays a plain row under its parent.
        let p = plan(&[
            f("AGS-19", "build", None),
            f("XPC-1", "build", Some("AGS-19")),
        ]);
        assert!(p.sub_of.is_empty());
        assert_eq!(keys(p.kids.get("AGS-19")), vec!["XPC-1"]);
        assert_eq!(p.top, vec!["AGS"]);
    }

    /// Nothing a database can hold makes the nav recurse forever: a
    /// ticket on a parent cycle is rendered as a root of its epic, and a
    /// sub-shortcode that would nest inside itself is refused (the epic
    /// stays top-level). Every ticket stays visible either way.
    #[test]
    fn cycles_are_broken_and_every_ticket_still_appears() {
        // A → B → A, with C hanging off A.
        let p = plan(&[
            f("CYC-1", "build", Some("CYC-2")),
            f("CYC-2", "build", Some("CYC-1")),
            f("CYC-3", "build", Some("CYC-1")),
        ]);
        assert_eq!(p.top, vec!["CYC"]);
        assert_eq!(keys(p.kids.get("CYC")), vec!["CYC-1", "CYC-2"]);
        assert_eq!(keys(p.kids.get("CYC-1")), vec!["CYC-3"]);
        assert_eq!(p.subtree["CYC"], vec!["CYC-1", "CYC-2", "CYC-3"]);

        // Two epics naming each other's scope root: at most one nesting
        // survives, and no group ends up inside itself.
        let p = plan(&[
            f("AAA-1", "scope", Some("BBB-2")),
            f("BBB-1", "scope", Some("AAA-1")),
            f("BBB-2", "build", None),
        ]);
        assert!(p.sub_of.len() <= 1, "{:?}", p.sub_of);
        assert!(!p.top.is_empty());
        let reachable: usize = p.top.iter().map(|g| p.subtree[g].len()).sum();
        assert_eq!(reachable, 3, "every ticket is rendered somewhere");

        // A dangling parent is a root, not a disappearance.
        let p = plan(&[f("D-1", "build", Some("NOPE-9"))]);
        assert_eq!(p.top, vec!["D"]);
        assert_eq!(keys(p.kids.get("D")), vec!["D-1"]);
    }
}
