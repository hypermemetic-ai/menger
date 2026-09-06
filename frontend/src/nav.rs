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
//! group holding the currently open ticket auto-expands once per
//! navigation so the selected row is visible. The hidden/shown state
//! persists alongside.
//!
//! Grouping is deliberately trivial and isolated in [`group_of`]: today a
//! ticket's group is its epic code. AGS-2 (parent hierarchy) re-roots the
//! forest by replacing that function and the `groups` memo — nothing else
//! here knows what a group is.
//!
//! AGS-13: each leaf's marker (the slot a group row uses for its ▸/▾
//! toggle) is its kind's glyph — see [`kind_glyph`] — so the tree stays
//! one column of monospace text while every row shows what shape of work
//! it is; the row carries `data-kind` and a `title` for the reader who
//! wants the word.

use std::collections::BTreeSet;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::{net, Route, Row};

const EXPANDED_KEY: &str = "menger.nav.expanded";
const HIDDEN_KEY: &str = "menger.nav.hidden";

/// localStorage, or None where it is unavailable (private mode, blocked
/// site data). Every persistence call degrades to "not remembered".
fn storage() -> Option<web_sys::Storage> {
    net::window().local_storage().ok().flatten()
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

/// The one place the forest's shape is decided (see module doc): a
/// ticket's parent row is its epic.
fn group_of(row: &Row) -> &str {
    &row.epic
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

/// Bring the selected row into view (block: nearest — no jump if it is
/// already visible). Called a tick after a route change so the `.sel`
/// class has been applied.
fn scroll_selected_into_view() {
    let Some(doc) = net::window().document() else { return };
    if let Ok(Some(el)) = doc.query_selector("#nav .row.sel") {
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
    }
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

    // Distinct group keys in row order. Rows are kept sorted by
    // (epic, seq), so consecutive dedup is the grouping; the memo only
    // notifies when a group appears or disappears, so the outer <For>
    // never re-runs for a plain insert into an existing group.
    let groups = Memo::new(move |_| {
        rows.with(|v| {
            let mut out: Vec<String> = Vec::new();
            for r in v {
                let g = group_of(r);
                if out.last().map(String::as_str) != Some(g) {
                    out.push(g.to_string());
                }
            }
            out
        })
    });

    // Selection follows the route. The open ticket's group auto-expands
    // so the selected row is visible — once per navigation (`revealed`
    // remembers which id was handled), so a later live insert does not
    // fight a group the user then chose to close. Tracks `rows` too: on a
    // cold deep link the route is known before the snapshot lands, and
    // the expansion happens when the ticket's row appears.
    let revealed = StoredValue::new(None::<String>);
    Effect::new(move |_| {
        let Route::Ticket(id) = route.get() else { return };
        let group = rows.with(|v| v.iter().find(|r| r.id == id).map(|r| group_of(r).to_string()));
        let Some(g) = group else { return };
        if revealed.get_value().as_deref() == Some(id.as_str()) {
            return;
        }
        revealed.set_value(Some(id));
        if !expanded.with_untracked(|e| e.contains(&g)) {
            expanded.update(|e| {
                e.insert(g);
            });
        }
        spawn_local(async {
            net::sleep_ms(0).await;
            scroll_selected_into_view();
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
                <div class="tree" role="tree">
                    <For
                        each=move || groups.get()
                        key=|g| g.clone()
                        children=move |g: String| group_view(g, rows, groups, expanded, route)
                    />
                </div>
            </div>
        </aside>
    }
}

/// One parent row (an epic shortcode) plus its children. The children are
/// a keyed `<For>` over this group's rows by id — the same discipline as
/// the board — so an insert adds exactly one node. Collapse (the default)
/// is a class on the wrapper (CSS hides the kids) rather than an unmount,
/// so expanding a group hands back the very same row nodes.
fn group_view(
    g: String,
    rows: RwSignal<Vec<Row>>,
    groups: Memo<Vec<String>>,
    expanded: RwSignal<BTreeSet<String>>,
    route: RwSignal<Route>,
) -> impl IntoView {
    let open = {
        let k = g.clone();
        Memo::new(move |_| expanded.with(|e| e.contains(&k)))
    };
    let last_group = {
        let k = g.clone();
        Memo::new(move |_| groups.with(|gs| gs.last() == Some(&k)))
    };
    let kids = {
        let k = g.clone();
        move || rows.with(|v| v.iter().filter(|r| group_of(r) == k).cloned().collect::<Vec<Row>>())
    };
    // Which child draws the `└──` connector; a memo so only this group's
    // rows re-check their prefix, and only when the answer changes.
    let last_id = {
        let k = g.clone();
        Memo::new(move |_| {
            rows.with(|v| v.iter().filter(|r| group_of(r) == k).last().map(|r| r.id.clone()))
        })
    };
    let rollup = {
        let k = g.clone();
        move || {
            rows.with(|v| {
                let mine = v.iter().filter(|r| group_of(r) == k);
                let (mut done, mut total) = (0usize, 0usize);
                for r in mine {
                    total += 1;
                    if r.ticket.with(|t| t.status == "Complete") {
                        done += 1;
                    }
                }
                format!("{done}/{total}")
            })
        }
    };
    let toggle = {
        let k = g.clone();
        move |_| {
            expanded.update(|e| {
                if !e.remove(&k) {
                    e.insert(k.clone());
                }
            })
        }
    };

    view! {
        <div class="group" data-group=g.clone() class:collapsed=move || !open.get()>
            <div
                class="row grouprow"
                role="treeitem"
                aria-expanded=move || open.get().to_string()
                on:click=toggle
            >
                <span class="prefix">{move || if last_group.get() { "└── " } else { "├── " }}</span>
                <span class="toggle clickable">{move || if open.get() { "▾" } else { "▸" }}</span>
                <span class="code">{g.clone()}</span>
                <span class="tag roll">{rollup}</span>
            </div>
            <div class="kids" role="group">
                <For
                    each=kids
                    key=|r| r.id.clone()
                    children=move |r: Row| leaf_view(r, last_id, last_group, route)
                />
            </div>
        </div>
    }
}

/// One ticket row: a real link (middle-click / copy-link friendly) whose
/// title and status tag render through the row's inner signal. Selected
/// iff the route is this ticket's page.
fn leaf_view(
    r: Row,
    last_id: Memo<Option<String>>,
    last_group: Memo<bool>,
    route: RwSignal<Route>,
) -> impl IntoView {
    let id = r.id;
    let ticket = r.ticket;
    let sel = {
        let id = id.clone();
        Memo::new(move |_| route.with(|rt| matches!(rt, Route::Ticket(x) if *x == id)))
    };
    let prefix = {
        let id = id.clone();
        move || {
            let bar = if last_group.get() { "    " } else { "│   " };
            let conn = if last_id.with(|l| l.as_deref() == Some(id.as_str())) {
                "└── "
            } else {
                "├── "
            };
            format!("{bar}{conn}")
        }
    };
    let href = format!("#/ticket/{id}");
    let tag_class = move || format!("tag st st-{}", ticket.with(|t| t.status.to_lowercase()));
    let kind = move || ticket.with(|t| t.kind.clone());

    view! {
        <a
            class="row leaf"
            class:sel=move || sel.get()
            role="treeitem"
            aria-selected=move || sel.get().to_string()
            data-nav-id=id.clone()
            data-kind=kind
            href=href
        >
            <span class="prefix">{prefix}</span>
            <span class="toggle kind" title=kind>{move || kind_glyph(&kind())}</span>
            <span class="code">{id.clone()}</span>
            <span class="label">{move || ticket.with(|t| t.title.clone())}</span>
            <span class=tag_class>{move || ticket.with(|t| t.status.clone())}</span>
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::kind_glyph;

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
}
