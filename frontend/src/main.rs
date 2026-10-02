//! MFE-3: the live board — a read-and-status surface. Every ticket
//! renders as a row (ordered by epic, then sequence) from one initial
//! fetch plus `tickets.watch` SSE updates — no polling. Each row carries
//! a status control (the closed seven-value vocabulary; the server stays
//! the validator); in-band `error` events on the app's own calls surface
//! in a visible banner. There is deliberately NO creation affordance
//! (owner, 2026-09-05): tickets are created by agents/CLI/API — the
//! browser reads and changes status.
//!
//! MFE-7: the ticket page. Hash-routed (`/app/#/ticket/{id}` — the
//! deep-link contract recorded in MFE-1): the route never reaches the
//! server, so a cold load rides the existing `/app/` index serving with
//! no SPA fallback, and dist assets can never collide with a route. The
//! page shows every wire-exposed field, renders the body as sanitized
//! markdown (src/md.rs), resolves blockers via `tickets.blockers`, and
//! stays live through the same watch stream as the board: `get` and
//! `blockers` results upsert into the id-keyed row store and the page
//! renders THROUGH those signals, so a status change arriving from
//! anywhere updates the open page — and a `created` event can resolve a
//! dangling blocker in place.
//!
//! Reactivity is signal-grained: each row owns an inner `RwSignal<Ticket>`,
//! so an incoming event updates the affected row's text in place — the
//! row list signal only fires on insert/remove, and untouched rows keep
//! their DOM identity across every update.
//!
//! AGS-12: the persistent left nav (src/nav.rs) — manifest's sidebar
//! pattern, tickets grouped under their epics — sits beside every view
//! and renders through the same row store, so it is live by construction.

//! AGS-14: ```mermaid fences in a body render as SVG. md.rs emits an
//! inert placeholder per fence; once the body's div is in the DOM the
//! page hands it to src/mermaid.rs, which lazily loads the vendored
//! renderer (served by menger, fetched only when a body has a fence) and
//! draws in strict mode, painting failures inline.
//!
//! AGS-16: the ticket menu (src/menu.rs). Every element that renders a
//! ticket carries `data-ticket-id`; one document-level listener turns a
//! right-click on any of them into the same menu (Open / Open in new tab
//! / Copy code / Set status ▸), mounted once by the App.
//!
//! AGS-17: light and dark mode (src/theme.rs). index.html's palette is
//! two value sets under one set of token names; a header toggle cycles
//! system → light → dark by stamping `data-theme` on the root, persisted
//! client-side, and every view re-colors through the tokens with no
//! re-render — only the drawn mermaid diagrams are redrawn.

//! AGS-20: children are visible. A ticket carries `parent`/`children`
//! (AGS-19's model); the nav nests a ticket's children under its row at
//! any depth, across epics, with sub-shortcode epics nesting under their
//! parent scope (src/nav.rs); the ticket page grows a parent crumb and a
//! `children` list beneath the DAG; a board row marks that it decomposes.
//! Nothing new is fetched — `children_changed` rides the same watch
//! stream, so a child born anywhere appears nested without a reload.
//!
//! AGS-18: the app is drivable from the keyboard alone (src/keys.rs): the
//! nav is a WAI-ARIA tree with a roving tabindex, board rows take focus
//! and open on Enter, `/` (or Ctrl/⌘+K) opens a jump palette and `?` a
//! cheat sheet — both `role="dialog"` overlays that trap Tab and give
//! focus back. Mouse behavior is untouched; the keyboard is an addition,
//! not a mode.

//! MFE-9: the app is on the socket. `net.rs` — the REST client and its
//! hand-parsed SSE — is gone; `session.rs` holds one `WsTransport` for the
//! app's lifetime and drives it with the client menger's own activation
//! declarations generate (`menger-tickets-api`). A row is the server's
//! `TicketSummary`, not a mirror of its JSON: rename a field on the server
//! and this crate stops compiling. The board paints page by page as
//! MNG-9's `list { summary, limit, after }` walk lands, so the browser
//! never waits for — or renders — a body it is not showing.

//! MFE-12: the views mount what can be seen. After MFE-9 the data plane was
//! no longer the limit — the render was: 24 DOM nodes per ticket, every
//! board row and every nav leaf (collapsed or not) mounted, 240,359 nodes
//! at 10^4 and a wasm abort at ~64,000 rows. The row store is untouched —
//! one sorted `Vec<Row>`, a signal per row, the same walk — and only the
//! views over it change. The board mounts a *window*: the rows in the
//! content column's viewport plus [`BOARD_MARGIN`] on either side, between
//! two spacer rows that stand in for everything else at the measured row
//! pitch, so the scrollbar still spans the whole store and scrolling slides
//! the window ([`board_window`]). A store taller than a browser will lay
//! out (Chromium stops at 2^25 px — 888,000 rows) is laid out at
//! [`BOARD_MAX_PX`] with the scroll range mapped linearly onto it
//! ([`board_layout`]), so the last ticket of a million is still a scroll
//! away. The nav mounts a node's children when it
//! opens instead of hiding them with a class (src/nav.rs). Neither the
//! keyboard nor the menu depends on a row being mounted: j/k walk the
//! store's order and scroll an unmounted row in before focusing it, the
//! palette filters the store, and the menu reads its ticket from the store
//! by id (src/keys.rs, src/menu.rs).

//! SDLC-7: the vocabulary is nine — the worker states `Coding` and `In
//! Review` joined it. Every chooser renders `Status::ALL`, so both appeared
//! in the board's control and the menu's Set status with no edit here; what
//! this crate adds is their rows in index.html's status table, light and
//! dark, and a class name for the one status with a space ([`st_class`]).

//! MNG-16: a ticket's title and body can change after birth
//! (`tickets.amend`). `amended` upserts its row like any other event, so the
//! board's cell, the nav's label and an open page's heading re-render the
//! title in place; and the open ticket page lends its body signal to the
//! session while mounted, which replaces the body when the event is that
//! ticket's (src/session.rs) — no `get`, no reload.

//! SCALE-5: `watch` says when it fell behind the server's broadcast
//! (`lagged`), instead of going on as if whole. The session answers it with
//! the walk a reconnect makes — same socket, rows upserted in place so they
//! keep their DOM, the open page's body fetched again — and the dock stays
//! `live` (src/session.rs).

//! SCALE-4: a row no longer lists its children — it carries `child_count`,
//! and `children_changed` is the parent's id, the child and the new count,
//! not the whole parent (a root's list was every ticket of its shortcode:
//! 2 MB at 10^6, on every birth). The board's "decomposes" mark reads the
//! count; the ticket page asks for its children by page when it opens
//! (`list { parent }`, 200 at a time, "more" for the next) and keeps what it
//! holds live from the deltas (src/kids.rs). The nav never read the list:
//! its tree is every row's `parent` (src/nav.rs).

//! MFE-17: the app follows a re-parent. `set_parent` (MNG-12) tells `watch`
//! with `parent_changed` (the child, whole) and `child_removed` (the old
//! parent's id and count), and the app used to drop both: after a move the
//! nav showed the ticket where it had been, the old parent's page still
//! listed it, and the child's crumb named the old parent — until a reload.
//! Now the row store takes the child's new `parent` (the crumb and the
//! page's parent field render through it), the nav re-plans its forest on
//! the move (src/nav.rs), the old parent's open page lets the child go and
//! the new parent's takes it (src/kids.rs), and the board's "decomposes"
//! marks follow both counts. Nothing else on the board moves.

mod dom;
mod keys;
mod kids;
mod md;
mod menu;
mod nav;

mod mermaid;
mod session;
mod theme;

use leptos::prelude::*;
use menger_tickets_api::{Status, TicketSummary};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

/// The closed status vocabulary, from the type that owns it
/// (`menger_tickets_api::Status::ALL`) rather than from a literal list the
/// server could out-grow. The server remains the validator; anything else
/// it receives comes back as an in-band `error` event.
pub(crate) const STATUSES: [&str; Status::ALL.len()] = statuses();

const fn statuses() -> [&'static str; Status::ALL.len()] {
    let mut out = [""; Status::ALL.len()];
    let mut i = 0;
    while i < Status::ALL.len() {
        out[i] = Status::ALL[i].as_str();
        i += 1;
    }
    out
}

/// AGS-15: the classes that make an element wear a status as color —
/// `st` (the paint rule) plus `st-<status>` (a row of index.html's status
/// table). Every place a status appears (nav tag, board control, ticket
/// page, blockers pills) puts these on the element that shows the word,
/// so the word stays present and the color is the table's. A status
/// outside the table paints nothing extra and stays legible text.
///
/// SDLC-7: `In Review` carries a space on the wire, and a space in a class
/// attribute is two classes (`st-in`, `review`) — so the row is named with
/// it hyphenated, `st-in-review`.
pub(crate) fn st_class(status: &str) -> String {
    format!("st st-{}", status.to_ascii_lowercase().replace(' ', "-"))
}

/// What the header's health line shows. Not a wire type and not a mirror
/// of one: it is *destructured out of* `TicketsEvent::Health`, so a
/// renamed field on the server is a compile error here rather than a blank
/// dock — which is what the old hand-written `#[derive(Deserialize)]
/// struct Health` could not give.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Dock {
    pub(crate) version: String,
    pub(crate) ticket_count: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Conn {
    Connecting,
    Live,
    Reconnecting,
}

/// One board row. `id`/`epic`/`seq` are immutable for a ticket's
/// lifetime (they define its identity and sort position), so they live
/// outside the signal; everything mutable renders through `ticket`.
#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) epic: String,
    pub(crate) seq: u32,
    /// MFE-9: a row is the server's `TicketSummary` — every ticket field
    /// except the body, which no row shows. The one body the app holds is
    /// the open ticket page's, fetched by `get`.
    pub(crate) ticket: RwSignal<TicketSummary>,
}

impl Row {
    fn of(t: TicketSummary) -> Self {
        Self {
            id: t.id.clone(),
            epic: t.epic.clone(),
            seq: t.seq,
            ticket: RwSignal::new(t),
        }
    }

    /// The row's position in the store's total order — MNG-9's
    /// `(epic, seq)`, which the schema keeps unique. Immutable for a
    /// ticket's lifetime, which is what makes the list a sorted vector a
    /// binary search can address.
    fn key(&self) -> (&str, u32) {
        (self.epic.as_str(), self.seq)
    }
}

fn key_of(t: &TicketSummary) -> (&str, u32) {
    (t.epic.as_str(), t.seq)
}

/// MFE-12: where the ticket `id` sits in the store, in O(log n). Every id
/// the server allocates is `{epic}-{seq}` — exactly the key the vector is
/// sorted by — so an id names its own position; one that is not shaped
/// like that, or that the search misses, falls back to a scan, so the
/// answer is never wrong, only slower. The nav mounts thousands of leaves
/// in one expand and the keyboard asks per keystroke; a scan each was
/// O(N) per leaf.
pub(crate) fn row_index(v: &[Row], id: &str) -> Option<usize> {
    let key = id
        .rsplit_once('-')
        .and_then(|(epic, seq)| Some((epic, seq.parse::<u32>().ok()?)));
    if let Some(key) = key {
        let pos = v.partition_point(|r| r.key() < key);
        if v.get(pos).is_some_and(|r| r.id == id) {
            return Some(pos);
        }
    }
    v.iter().position(|r| r.id == id)
}

/// Fine-grained upsert of one ticket. An existing ticket updates its row's
/// inner signal in place — the list signal never fires, `<For>` never
/// re-runs, and every other row's DOM survives untouched. A new ticket
/// inserts at its `(epic, seq)` position, found by binary search over the
/// order the store already keeps.
///
/// This is the `watch` path: one event, one ticket. The board's initial
/// walk uses [`upsert_page`] instead — see why there.
pub(crate) fn upsert(rows: RwSignal<Vec<Row>>, t: TicketSummary) {
    let existing = rows.with_untracked(|v| {
        let pos = v.partition_point(|r| r.key() < key_of(&t));
        v.get(pos).filter(|r| r.id == t.id).map(|r| r.ticket)
    });
    match existing {
        Some(sig) => {
            if sig.with_untracked(|old| *old != t) {
                sig.set(t);
            }
        }
        None => rows.update(|v| {
            let pos = v.partition_point(|r| r.key() < key_of(&t));
            v.insert(pos, Row::of(t));
        }),
    }
}

/// MFE-9: one page of MNG-9's walk, merged in **one** list update.
///
/// Why this exists rather than a loop over [`upsert`]: each summary
/// arrives as its own WebSocket notification, so a per-row insert would
/// notify the list signal 10,000 times, and each notification re-diffs the
/// board's `<For>` and recomputes the nav's forest memo over every row
/// held so far. That is quadratic with an enormous constant — measured at
/// `--seed-synthetic 10000`, the tab stopped answering entirely. Batching
/// the page makes it one notification per page: 20 re-diffs at 10^4
/// instead of 10,000.
///
/// Rows already present still update through their own inner signals and
/// never touch the list — the property AGS-20's DOM identity rests on. The
/// new ones are merged with the existing order in one pass (both sides are
/// sorted by `(epic, seq)`: the store's walk is in that order and the list
/// is kept in it).
pub(crate) fn upsert_page(rows: RwSignal<Vec<Row>>, page: Vec<TicketSummary>) {
    if page.is_empty() {
        return;
    }
    let mut fresh: Vec<TicketSummary> = Vec::new();
    rows.with_untracked(|v| {
        for t in page {
            let pos = v.partition_point(|r| r.key() < key_of(&t));
            match v.get(pos) {
                Some(r) if r.id == t.id => {
                    if r.ticket.with_untracked(|old| *old != t) {
                        r.ticket.set(t);
                    }
                }
                _ => fresh.push(t),
            }
        }
    });
    if fresh.is_empty() {
        return;
    }
    rows.update(|v| {
        let mut merged: Vec<Row> = Vec::with_capacity(v.len() + fresh.len());
        let mut incoming = std::mem::take(&mut fresh).into_iter().peekable();
        for row in v.drain(..) {
            while incoming
                .peek()
                .is_some_and(|t| key_of(t) < (row.epic.as_str(), row.seq))
            {
                merged.push(Row::of(incoming.next().expect("peeked")));
            }
            merged.push(row);
        }
        merged.extend(incoming.map(Row::of));
        *v = merged;
    });
}

// ---------------------------------------------------------------------
// Routing (MFE-7). The deep-link contract this ticket owns, recorded in
// MFE-1: a ticket's page is `/app/#/ticket/{id}`.

#[derive(Clone, PartialEq)]
pub(crate) enum Route {
    Board,
    Ticket(String),
}

fn parse_route(hash: &str) -> Route {
    let path = hash.strip_prefix('#').unwrap_or(hash);
    let path = path.strip_prefix('/').unwrap_or(path);
    match path.split_once('/') {
        Some(("ticket", id)) if !id.is_empty() => Route::Ticket(id.to_string()),
        _ => Route::Board,
    }
}

fn current_route() -> Route {
    parse_route(&dom::window().location().hash().unwrap_or_default())
}

/// The route as a signal: seeded from the URL (deep links / cold loads),
/// updated on every hashchange — link clicks, row clicks, and the
/// browser's back/forward all funnel through the hash.
fn use_route() -> RwSignal<Route> {
    let route = RwSignal::new(current_route());
    let cb = Closure::<dyn FnMut()>::new(move || route.set(current_route()));
    dom::window()
        .add_event_listener_with_callback("hashchange", cb.as_ref().unchecked_ref())
        .expect("hashchange listener");
    cb.forget(); // lives as long as the app
    route
}

pub(crate) fn navigate_to_ticket(id: &str) {
    let _ = dom::window().location().set_hash(&format!("/ticket/{id}"));
}

/// Unix seconds, human-readable: "2026-09-05T12:34:56Z (1788690896)".
/// js_sys::Date so no chrono rides into the wasm.
fn fmt_unix(unix: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(unix as f64 * 1000.0));
    let iso: String = date.to_iso_string().into();
    let iso = iso
        .strip_suffix(".000Z")
        .map(|s| format!("{s}Z"))
        .unwrap_or(iso);
    format!("{iso} ({unix})")
}

// ---------------------------------------------------------------------
// The board's window (MFE-12)

/// Rows the board keeps mounted beyond each edge of the viewport. Generous
/// on purpose: a mounted row is ~16 DOM nodes, so two margins are a few
/// thousand nodes whatever N is — and they are several screens of j/k,
/// PageUp/PageDown and find-in-page with no remount, and a board of up to
/// about a hundred tickets (menger's own plan today) mounted whole.
pub(crate) const BOARD_MARGIN: usize = 100;

/// The window moves in steps of this many rows — both edges snapped
/// outward — so a scroll of a few rows changes nothing at all, and a long
/// one re-diffs the board's `<For>` once per step rather than per pixel.
pub(crate) const BOARD_STEP: usize = 25;

/// The row pitch (px) assumed until a rendered row has been measured.
/// Every board row is one line tall (index.html), so one measurement holds
/// for all of them, mounted or not; this is only the first frame's guess.
const BOARD_PITCH_GUESS: f64 = 38.0;

/// The tallest the board's `<tbody>` is ever laid out (px). Browsers stop
/// laying out at a fixed height — Chromium at 2^25 px (33,554,432), Firefox
/// near 17.9 M — and at 38 px a row, 10^6 rows is 37.8 M: past it, rows
/// pile up at the limit and the end of the board cannot be scrolled to.
/// Below this height (~395,000 rows) the board is laid out at its natural
/// height; above it, at exactly this height, scaled ([`board_layout`]).
pub(crate) const BOARD_MAX_PX: f64 = 15_000_000.0;

/// The board's window: the half-open range of store indices that is
/// mounted, for `n` rows when `shown` rows starting at index `first` fit
/// in the viewport. [`BOARD_MARGIN`] either side, both edges snapped
/// outward to [`BOARD_STEP`], clamped to the store. A viewport past the
/// end (the store shrank under it) shows the last rows rather than none.
pub(crate) fn board_window(n: usize, first: usize, shown: usize) -> (usize, usize) {
    let first = first.min(n.saturating_sub(shown.max(1)));
    let start = first.saturating_sub(BOARD_MARGIN) / BOARD_STEP * BOARD_STEP;
    let end = (first + shown + BOARD_MARGIN).div_ceil(BOARD_STEP) * BOARD_STEP;
    let end = end.min(n);
    (start.min(end), end)
}

/// Where the board's viewport is, measured from the DOM (px): how far the
/// top of the board's `<tbody>` has scrolled above the content column's
/// top edge (negative while the header shows), how tall that column is,
/// and the row pitch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geom {
    pub(crate) above: f64,
    pub(crate) viewport: f64,
    pub(crate) pitch: f64,
}

impl Geom {
    fn pitch(self) -> f64 {
        if self.pitch > 1.0 {
            self.pitch
        } else {
            BOARD_PITCH_GUESS
        }
    }

    /// True when `n` rows at their natural pitch would be taller than
    /// [`BOARD_MAX_PX`].
    fn scaled(self, n: usize) -> bool {
        n as f64 * self.pitch() > BOARD_MAX_PX
    }

    /// The scroll offset range a scaled board maps onto the store, and the
    /// last row that can sit at the viewport's top.
    fn span(self, n: usize) -> (f64, f64) {
        let span = (BOARD_MAX_PX - self.viewport).max(1.0);
        let last_top = (n as f64 - self.viewport / self.pitch()).max(0.0);
        (span, last_top)
    }

    /// The store row (fractional) at the viewport's top edge when the
    /// board is scrolled `v` px into itself. Natural: `v / pitch`. Scaled:
    /// the scroll range maps linearly onto the store, so the scrollbar's
    /// position is the position in the store.
    pub(crate) fn row_at(self, n: usize, v: f64) -> f64 {
        if !self.scaled(n) {
            return v / self.pitch();
        }
        let (span, last_top) = self.span(n);
        last_top * v.clamp(0.0, span) / span
    }

    /// The inverse of [`Geom::row_at`]: the scroll offset that puts row
    /// `r` at the viewport's top edge.
    pub(crate) fn offset_of(self, n: usize, r: f64) -> f64 {
        if !self.scaled(n) {
            return r * self.pitch();
        }
        let (span, last_top) = self.span(n);
        if last_top > 0.0 {
            r.clamp(0.0, last_top) * span / last_top
        } else {
            0.0
        }
    }
}

/// What the board mounts: the window over the store and the heights of
/// the two spacer rows around it (px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) top_px: f64,
    pub(crate) bottom_px: f64,
}

/// The board for `n` rows and a viewport. At its natural height (up to
/// [`BOARD_MAX_PX`]) row `i` is at `i · pitch` and the spacers are the rows
/// they stand in for: exact, and the window only moves in steps.
///
/// Scaled, the board is [`BOARD_MAX_PX`] tall and the scroll offset maps
/// linearly onto the store ([`Geom::row_at`]); the mounted rows keep their
/// natural pitch and the top spacer is sized so that the row the mapping
/// names sits exactly at the viewport's top — so it changes with every
/// scroll, and a scroll moves the rows 1/scale times as far as natural.
/// The window gives up margin near the two ends rather than let a spacer
/// go negative; the first and last rows are reachable either way.
pub(crate) fn board_layout(n: usize, g: Geom) -> Layout {
    let p = g.pitch();
    let shown = (g.viewport.max(0.0) / p).ceil() as usize + 1;
    if !g.scaled(n) {
        let first = (g.above.max(0.0) / p).floor() as usize;
        let (start, end) = board_window(n, first, shown);
        return Layout { start, end, top_px: start as f64 * p, bottom_px: (n - end) as f64 * p };
    }
    let (span, _) = g.span(n);
    let v = g.above.clamp(0.0, span);
    let r = g.row_at(n, v);
    let (s0, e0) = board_window(n, r.floor() as usize, shown);
    // No more rows above row `r` than fit above `v`, nor below it than fit
    // in what is left of the board — except that the row the viewport's
    // top cuts through is always mounted (which, within a row of the very
    // top, draws it up to a row low rather than leave a blank sliver).
    let first = r.floor() as usize;
    let start = s0.max((r - v / p).ceil().max(0.0) as usize).min(first);
    let end = e0.min((r + (BOARD_MAX_PX - v) / p).ceil() as usize).max(start);
    let top_px = (v - (r - start as f64) * p).max(0.0);
    let bottom_px = (BOARD_MAX_PX - top_px - (end - start) as f64 * p).max(0.0);
    Layout { start, end, top_px, bottom_px }
}

/// A board row's height (px) — the pitch, since every row is one line.
/// Read as the row's computed height: client rects are single-precision,
/// so millions of px down the board a rect's height is off by up to half a
/// pixel (and past a layout limit, positions saturate and a pitch taken
/// from two of them collapses toward zero). The row is the window's
/// second: the first sits against the top spacer and loses half of the
/// collapsed border it would share with a row above.
fn measured_pitch(body: &web_sys::Element) -> Option<f64> {
    let row = body
        .query_selector("tr[data-ticket-id] + tr[data-ticket-id]")
        .ok()
        .flatten()
        .or_else(|| body.query_selector("tr[data-ticket-id]").ok().flatten())?;
    let height = dom::window().get_computed_style(&row).ok()??.get_property_value("height").ok()?;
    // The page's own parseFloat ("37.7812px" -> 37.7812): no float parser
    // rides into the wasm for one number.
    let pitch = js_sys::parse_float(&height);
    (pitch > 1.0).then_some(pitch)
}

/// The board's geometry off the DOM, given the pitch to assume when no
/// row is mounted to measure.
fn measure_board(body: &web_sys::Element, pitch: f64) -> Option<Geom> {
    let scroller = body.closest(".content").ok().flatten()?;
    let (s, b) = (scroller.get_bounding_client_rect(), body.get_bounding_client_rect());
    Some(Geom {
        above: s.top() - b.top(),
        viewport: f64::from(scroller.client_height()),
        pitch: measured_pitch(body).unwrap_or(pitch),
    })
}

/// Scroll the content column the least that puts store row `index` (of
/// `n`) on screen — `nearest`, like `scrollIntoView` — the way the
/// keyboard reaches a row outside the window. Pure geometry: the row need
/// not be mounted, and once the scroll lands the window moves to mount it.
pub(crate) fn scroll_board_to(index: usize, n: usize) {
    let Some(body) = dom::window().document().and_then(|d| d.get_element_by_id("board")) else {
        return;
    };
    let Some(scroller) = body.closest(".content").ok().flatten() else { return };
    let Some(g) = measure_board(&body, BOARD_PITCH_GUESS) else { return };
    let top = g.row_at(n, g.above);
    let fits = g.viewport / g.pitch();
    let i = index as f64;
    let want = if i < top {
        i
    } else if i + 1.0 > top + fits {
        i + 1.0 - fits
    } else {
        return;
    };
    let delta = g.offset_of(n, want) - g.above;
    scroller.set_scroll_top(scroller.scroll_top() + delta.round() as i32);
}

// ---------------------------------------------------------------------
// Views

/// The board (MFE-3), unchanged in behavior; MFE-7 adds click-through:
/// the id cell is a real link (middle-click friendly) and a click
/// anywhere else on the row — except the status control — navigates to
/// the ticket page.
///
/// MFE-12: a window over the store, not a row per ticket. `viewport` is
/// the App's signal that the content column scrolled or resized; the
/// board measures itself on it ([`Geom`]), and the window and its two
/// spacers are a memo of the geometry and the store's length
/// ([`board_layout`]) — so a status change, which moves neither, re-runs
/// nothing here, mounted row or not. Rows that stay in the window keep
/// their `<tr>` (the `<For>` is keyed by id as before).
#[component]
fn Board(
    rows: RwSignal<Vec<Row>>,
    last_error: RwSignal<Option<String>>,
    viewport: Trigger,
) -> impl IntoView {
    let body = NodeRef::<leptos::html::Tbody>::new();
    let geom = RwSignal::new(Geom {
        above: 0.0,
        viewport: dom::window()
            .inner_height()
            .ok()
            .and_then(|h| h.as_f64())
            .unwrap_or(900.0),
        pitch: BOARD_PITCH_GUESS,
    });
    let layout = Memo::new(move |_| board_layout(rows.with(Vec::len), geom.get()));
    let range = Memo::new(move |_| layout.with(|l| (l.start, l.end)));
    let scaled = Memo::new(move |_| geom.get().scaled(rows.with(Vec::len)));

    // Read the geometry off the DOM. `try_` throughout: the deferred call
    // below can land after the board has unmounted.
    let measure = move || {
        let Some(Some(tb)) = body.try_get_untracked() else { return };
        let Some(old) = geom.try_get_untracked() else { return };
        let Some(next) = measure_board(&tb, old.pitch) else { return };
        if next != old {
            geom.try_set(next);
        }
    };
    Effect::new(move |_| {
        viewport.track();
        measure();
    });

    // AGS-18: rows are focusable (roving tabindex — every row renders
    // with -1 and exactly one is promoted to 0), so j/k move a real focus
    // and Enter opens what is focused. The status <select> keeps its own
    // place in the tab order, after the row. MFE-12: the window moves too,
    // and the tab stop can leave with it — so this runs per window, a tick
    // after it renders, and re-reads the geometry off the rows it drew.
    Effect::new(move |_| {
        range.track();
        rows.with(|v| v.len());
        spawn_local(async move {
            dom::sleep_ms(0).await;
            measure();
            keys::ensure_board_tab_stop();
        });
    });

    // The two spacers: every row above and below the window. Each exists
    // only while it stands in for something, so at the top of the store
    // the first row is the tbody's first child.
    let spacer = move |px: fn(&Layout) -> f64| {
        let present = Memo::new(move |_| layout.with(|l| px(l) > 0.0));
        move || {
            present.get().then(|| {
                view! {
                    <tr class="spacer" aria-hidden="true">
                        <td colspan="6" style:height=move || format!("{}px", layout.with(px))></td>
                    </tr>
                }
            })
        }
    };

    view! {
        // ARIA's virtualized-table contract: the table says how many rows
        // it has, each mounted row says which one it is (1 is the head).
        <table class="board" aria-rowcount=move || rows.with(Vec::len) + 1>
            <thead>
                <tr aria-rowindex="1">
                    <th class="c-id">"id"</th><th class="c-title">"title"</th><th class="c-kind">"kind"</th>
                    <th class="c-status">"status"</th><th class="c-sev">"severity"</th><th class="c-blk">"blocked by"</th>
                </tr>
            </thead>
            <tbody
                id="board"
                node_ref=body
                // Scaled, the top spacer changes with every scroll; the
                // browser's scroll anchoring would answer each change with
                // a scroll of its own.
                class:scaled=move || scaled.get()
                on:focusin=move |ev| {
                    let id = ev
                        .target()
                        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                        .and_then(|el| el.closest("tr[data-ticket-id]").ok().flatten())
                        .and_then(|tr| tr.get_attribute("data-ticket-id"));
                    if let Some(id) = id {
                        keys::note_board_focus(id);
                    }
                }
            >
                {spacer(|l| l.top_px)}
                <For
                    each=move || {
                        let (start, end) = range.get();
                        rows.with(|v| v[start.min(v.len())..end.min(v.len())].to_vec())
                    }
                    // id alone: mutations re-render fields inside the
                    // row; the <tr> itself is never replaced.
                    key=|row| row.id.clone()
                    children=move |row: Row| {
                        let ticket = row.ticket;
                        let id = row.id.clone();
                        let nav_id = row.id.clone();
                        let href_id = row.id.clone();
                        let index_id = row.id.clone();
                        view! {
                            <tr
                                data-ticket-id=row.id.clone()
                                tabindex="-1"
                                aria-rowindex=move || rows.with(|v| row_index(v, &index_id)).map(|i| i + 2)
                                on:click=move |ev| {
                                    // The select (and its options) keep their own
                                    // behavior; the id link handles itself.
                                    let hit_control = ev
                                        .target()
                                        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                        .and_then(|el| el.closest("select, option, a").ok().flatten())
                                        .is_some();
                                    if !hit_control {
                                        navigate_to_ticket(&nav_id);
                                    }
                                }
                            >
                                <td class="id">
                                    <a href=format!("#/ticket/{href_id}")>{row.id.clone()}</a>
                                    // AGS-20: no `parent` column — just the
                                    // mark that this row decomposes, and how
                                    // far (the nav is where the tree is).
                                    // SCALE-4: the count the row carries.
                                    {move || {
                                        ticket
                                            .with(|t| {
                                                let n = t.child_count;
                                                (n > 0)
                                                    .then(|| {
                                                        let word = if n == 1 { "child" } else { "children" };
                                                        view! {
                                                            <span class="haskids" title=format!("{n} {word}")>
                                                                {format!("▸{n}")}
                                                            </span>
                                                        }
                                                    })
                                            })
                                    }}
                                </td>
                                // MFE-12: one line per row (the window's
                                // geometry rests on it); a title cut short
                                // keeps its whole text on hover.
                                <td title=move || ticket.with(|t| t.title.clone())>{move || ticket.with(|t| t.title.clone())}</td>
                                <td class="kind">{move || ticket.with(|t| t.kind.to_string())}</td>
                                <td class="status">
                                    <select
                                        // The control is the chip: it wears the
                                        // server-confirmed status's color (AGS-15).
                                        class=move || format!("status {}", ticket.with(|t| st_class(t.status.as_str())))
                                        prop:value=move || ticket.with(|t| t.status.as_str())
                                        on:change=move |ev| {
                                            let Some(sel) = ev
                                                .target()
                                                .and_then(|t| t.dyn_into::<web_sys::HtmlSelectElement>().ok())
                                            else {
                                                return;
                                            };
                                            let current = ticket.with_untracked(|t| t.status);
                                            // The control only ever offers the closed
                                            // vocabulary, so a value it cannot parse is
                                            // a browser doing something we did not ask
                                            // for — leave the ticket alone.
                                            let Ok(chosen) = sel.value().parse::<Status>() else {
                                                sel.set_value(current.as_str());
                                                return;
                                            };
                                            if chosen != current {
                                                session::request_set_status(id.clone(), chosen, last_error, move || {
                                                    sel.set_value(current.as_str())
                                                });
                                            }
                                        }
                                    >
                                        {STATUSES
                                            .iter()
                                            .map(|s| {
                                                let s = *s;
                                                view! {
                                                    <option
                                                        value=s
                                                        class=st_class(s)
                                                        selected=move || ticket.with(|t| t.status.as_str() == s)
                                                    >
                                                        {s}
                                                    </option>
                                                }
                                            })
                                            .collect_view()}
                                    </select>
                                </td>
                                <td>{move || ticket.with(|t| t.severity.map(|s| s.to_string()).unwrap_or_default())}</td>
                                <td title=move || ticket.with(|t| t.blocked_by.join(" "))>{move || ticket.with(|t| t.blocked_by.join(" "))}</td>
                            </tr>
                        }
                    }
                />
                {spacer(|l| l.bottom_px)}
            </tbody>
        </table>
    }
}

/// One entry of the blockers/unlocks lists. Present in the store → a
/// live link (status and title re-render on watch events). Absent: for
/// blockers, once the `blockers` call has resolved (`dangling_when`),
/// the id is a dangling forward reference — visually distinct, unlinked;
/// until then (and for unlocks entries the snapshot hasn't carried yet)
/// a plain link. A `created` watch event upserts the store, so a
/// dangling entry resolves live while the page is open. The entry carries
/// `data-ticket-id` (the AGS-16 menu's attachment) exactly while it names
/// a ticket — a dangling reference is not one.
fn dag_entry(
    id: String,
    rows: RwSignal<Vec<Row>>,
    dangling_when: Option<RwSignal<bool>>,
) -> impl IntoView {
    // SCALE-4: found by its key, as MFE-12's nav leaves are — a page of
    // children is up to 200 of these, and each re-runs on every insert.
    let found = {
        let id = id.clone();
        Memo::new(move |_| rows.with(|v| row_index(v, &id).map(|i| v[i].ticket)))
    };
    let dangling = move || found.get().is_none() && dangling_when.is_some_and(|loaded| loaded.get());
    let attr_id = id.clone();
    let menu_id = id.clone();
    view! {
        <li data-dag-id=attr_id data-ticket-id=move || (!dangling()).then(|| menu_id.clone())>
            {move || match found.get() {
                Some(t) => {
                    let href = format!("#/ticket/{id}");
                    view! {
                        <a href=href>{id.clone()}</a>
                        <span class=move || format!("bstatus {}", t.with(|x| st_class(x.status.as_str())))>
                            {move || t.with(|x| x.status.to_string())}
                        </span>
                        <span class="btitle">{move || t.with(|x| x.title.clone())}</span>
                    }
                        .into_any()
                }
                None if dangling() => {
                    view! {
                        <span class="dangling">{id.clone()}" (dangling — no such ticket yet)"</span>
                    }
                        .into_any()
                }
                None => {
                    let href = format!("#/ticket/{id}");
                    view! { <a href=href>{id.clone()}</a> }.into_any()
                }
            }}
        </li>
    }
}

/// A ticket's ancestors, root first, from the row store (AGS-20). Stops
/// at the first id the store does not hold — a deep link can be on screen
/// before the snapshot lands — and at a repeat, so a hand-edited parent
/// cycle cannot spin the crumb.
fn ancestors(rows: RwSignal<Vec<Row>>, id: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur = id.to_string();
    rows.with(|v| {
        while let Some(row) = v.iter().find(|r| r.id == cur) {
            let Some(p) = row.ticket.with(|t| t.parent.clone()) else { break };
            if p == id || chain.contains(&p) {
                break;
            }
            chain.push(p.clone());
            cur = p;
        }
    });
    chain.reverse();
    chain
}

/// SDLC-9: a commit reference as the ticket page shows it — git's short
/// form, the first seven digits of the hash, after the repository when the
/// reference names one (`plexus-macros@4f7114a`). The server stores at
/// least seven, so a short reference shows whole.
fn short_commit(full: &str) -> String {
    let (repo, hash) = match full.rsplit_once('@') {
        Some((repo, hash)) => (Some(repo), hash),
        None => (None, full),
    };
    let hash: String = hash.chars().take(7).collect();
    match repo {
        Some(repo) => format!("{repo}@{hash}"),
        None => hash,
    }
}

/// The full ticket render: every wire-exposed field. The body is
/// markdown, sanitized in md.rs; a leading YAML frontmatter block is
/// collapsed behind a <details>, never painted raw. Body/blockers/
/// unlocks render through memos so a status change re-renders the status
/// text without rebuilding those sections (an open frontmatter fold
/// survives a live update).
fn ticket_view(
    ticket: RwSignal<TicketSummary>,
    body_raw: RwSignal<Option<String>>,
    rows: RwSignal<Vec<Row>>,
    blockers_loaded: RwSignal<bool>,
    kids: RwSignal<kids::Kids>,
    more: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let blocked_by = Memo::new(move |_| ticket.with(|t| t.blocked_by.clone()));
    let unlocks_ids = Memo::new(move |_| ticket.with(|t| t.unlocks.clone()));
    // AGS-20: the ticket's own decomposition. SCALE-4: fetched by page
    // when the page opens (`TicketPage`), and grown or shrunk in place by
    // the deltas `watch` carries for this parent — a child born anywhere
    // still appears with no re-fetch.
    let children_ids = Memo::new(move |_| kids.with(|k| k.ids.clone()));
    let unshown = Memo::new(move |_| {
        let count = ticket.with(|t| t.child_count);
        kids.with(|k| k.unshown(count))
    });
    let parent_id = Memo::new(move |_| ticket.with(|t| t.parent.clone()));
    // SDLC-9: the commits that finished this ticket, in link order, each in
    // git's short form with the whole reference in its title. They are on
    // the row, so a link made anywhere reaches this page as the row's
    // update on `watch` — no fetch, no reload — and the memo keeps any
    // other change to the row from rebuilding them.
    let commits = Memo::new(move |_| ticket.with(|t| t.commits.clone()));
    let commits_view = move || {
        let refs = commits.get();
        if refs.is_empty() {
            return view! { "—" }.into_any();
        }
        refs.into_iter()
            .map(|full| {
                let short = short_commit(&full);
                view! { <code class="commit" title=full.clone() data-commit=full>{short}</code> }
            })
            .collect_view()
            .into_any()
    };

    let blockers = move || {
        let ids = blocked_by.get();
        if ids.is_empty() {
            view! { <p class="none">"none"</p> }.into_any()
        } else {
            view! {
                <ul id="blockers">
                    {ids
                        .into_iter()
                        .map(|bid| dag_entry(bid, rows, Some(blockers_loaded)))
                        .collect_view()}
                </ul>
            }
                .into_any()
        }
    };
    let unlocks = move || {
        let ids = unlocks_ids.get();
        if ids.is_empty() {
            view! { <p class="none">"none"</p> }.into_any()
        } else {
            view! {
                <ul id="unlocks">
                    {ids
                        .into_iter()
                        .map(|uid| dag_entry(uid, rows, None))
                        .collect_view()}
                </ul>
            }
                .into_any()
        }
    };
    let children = move || {
        let ids = children_ids.get();
        if ids.is_empty() {
            let word = if kids.with(|k| k.loaded) { "none" } else { "loading…" };
            view! { <p class="none">{word}</p> }.into_any()
        } else {
            view! {
                <ul id="children">
                    {ids
                        .into_iter()
                        .map(|cid| dag_entry(cid, rows, None))
                        .collect_view()}
                </ul>
            }
                .into_any()
        }
    };
    // The rest of a large parent's children, a page at a time.
    let more_view = move || {
        let n = unshown.get();
        (n > 0).then(|| {
            view! {
                <button id="kids-more" class="more" on:click=move |_| more()>
                    {format!("{n} more")}
                </button>
            }
        })
    };
    let body = move || {
        // MFE-9: the body is not a row field — it arrives with this page's
        // own `get` and lives here. `None` is "not fetched yet"; an empty
        // string is a ticket that genuinely has no body.
        let Some(raw) = body_raw.get() else {
            return view! { <p class="loading">"loading body…"</p> }.into_any();
        };
        if raw.trim().is_empty() {
            return view! { <p class="none">"(no body)"</p> }.into_any();
        }
        let (fm, md_src) = md::split_frontmatter(&raw);
        let fm_view = fm.map(|f| {
            let f = f.to_string();
            view! {
                <details class="frontmatter">
                    <summary>"frontmatter"</summary>
                    <pre>{f}</pre>
                </details>
            }
        });
        let md::Rendered { html, diagrams } = md::render_body(md_src);
        // AGS-14: once this div is in the DOM, hand its mermaid
        // placeholders to the renderer. `on_load` fires once per mounted
        // div, and a body re-render makes a new div (and a new hook), so
        // navigation never re-renders a stale one; a diagram-free body
        // never touches the renderer at all.
        let md_node = NodeRef::<leptos::html::Div>::new();
        if diagrams > 0 {
            md_node.on_load(move |el: web_sys::HtmlDivElement| mermaid::mount(&el));
        }
        view! {
            {fm_view}
            <div class="md" node_ref=md_node inner_html=html></div>
        }
            .into_any()
    };

    // The id never changes for a row's lifetime (see `Row`), so the
    // header's menu attachment is static.
    let header_id = ticket.with_untracked(|t| t.id.clone());
    view! {
        <article class="ticket">
            <h2 data-ticket-id=header_id>
                <span class="id">{move || ticket.with(|t| t.id.clone())}</span>
                {move || ticket.with(|t| t.title.clone())}
            </h2>
            <dl class="meta">
                <div>
                    <dt>"status"</dt>
                    <dd id="t-status" class=move || format!("status {}", ticket.with(|t| st_class(t.status.as_str())))>
                        {move || ticket.with(|t| t.status.to_string())}
                    </dd>
                </div>
                <div><dt>"kind"</dt><dd id="t-kind" class="kind">{move || ticket.with(|t| t.kind.to_string())}</dd></div>
                <div>
                    <dt>"severity"</dt>
                    <dd>{move || ticket.with(|t| t.severity.map_or_else(|| "—".to_string(), |s| s.to_string()))}</dd>
                </div>
                // AGS-20: the ticket this one is an output of, as a link
                // (the crumb above walks the whole chain).
                <div>
                    <dt>"parent"</dt>
                    <dd id="t-parent">
                        {move || match parent_id.get() {
                            Some(p) => {
                                let (href, code) = (format!("#/ticket/{p}"), p.clone());
                                view! { <a class="crumb" data-ticket-id=p href=href>{code}</a> }
                                    .into_any()
                            }
                            None => view! { "—" }.into_any(),
                        }}
                    </dd>
                </div>
                <div><dt>"epic"</dt><dd>{move || ticket.with(|t| t.epic.clone())}</dd></div>
                <div><dt>"seq"</dt><dd>{move || ticket.with(|t| t.seq)}</dd></div>
                <div><dt>"created"</dt><dd>{move || ticket.with(|t| fmt_unix(t.created_unix))}</dd></div>
                <div><dt>"updated"</dt><dd>{move || ticket.with(|t| fmt_unix(t.updated_unix))}</dd></div>
                <div><dt>"commits"</dt><dd id="t-commits">{commits_view}</dd></div>
            </dl>
            <section class="dag">
                <h3>"blocked by"</h3>
                {blockers}
                <h3>"unlocks"</h3>
                {unlocks}
            </section>
            // AGS-20: decomposition, beneath the DAG — the DAG is what
            // this ticket waits on and frees; this is what it is made of.
            <section class="dag kidsec">
                <h3>"children"</h3>
                {children}
                {more_view}
            </section>
            <section class="bodysec">
                <h3>"body"</h3>
                {body}
            </section>
        </article>
    }
}

/// The ticket page (MFE-7). `get` + `blockers` fetch on entry and upsert
/// into the shared row store; the page renders THROUGH the store's
/// signals, so the watch stream keeps it live exactly like the board —
/// no data path of its own. A `not_found` answer shows the not-found
/// state; if the id comes into existence later (watch `created`), the
/// store lookup wins and the page upgrades itself.
#[component]
fn TicketPage(
    id: String,
    rows: RwSignal<Vec<Row>>,
    /// MFE-9: the socket's generation counter. It ticks the moment a
    /// connection is up — *before* the board's page walk finishes — so a
    /// cold deep link still renders from two point queries without waiting
    /// for the whole board, which is the one browser path PERF-4 found
    /// flat from 10^2 to 10^6. It ticks again on every reconnect, which is
    /// what re-fetches an open page's body after the server comes back.
    socket: RwSignal<u32>,
    last_error: RwSignal<Option<String>>,
) -> impl IntoView {
    let not_found = RwSignal::new(false);
    let blockers_loaded = RwSignal::new(false);
    // The one body the app holds. `None` until `get` answers.
    let body = RwSignal::new(None::<String>);
    // MNG-16: lent to the session while the page is mounted, so an
    // `amended` event for this ticket replaces the body without a `get`.
    session::hold_body(id.clone(), body);
    on_cleanup(move || session::release_body(body));
    // SCALE-4: this ticket's children, by page, kept live by `watch`.
    let kids = RwSignal::new(kids::Kids::default());
    session::watch_children(id.clone(), kids);

    {
        let id = id.clone();
        Effect::new(move |_| {
            if socket.get() == 0 {
                return; // no connection yet; this effect re-runs when there is one
            }
            session::load_ticket(
                id.clone(),
                rows,
                body,
                not_found,
                blockers_loaded,
                last_error,
            );
            session::load_children(id.clone(), None, rows, kids, last_error);
        });
    }
    let more = {
        let id = StoredValue::new(id.clone());
        move || {
            if let Some(after) = kids.with_untracked(|k| k.next.clone()) {
                session::load_children(id.get_value(), Some(after), rows, kids, last_error);
            }
        }
    };

    let ticket = {
        let id = id.clone();
        Memo::new(move |_| rows.with(|v| v.iter().find(|r| r.id == id).map(|r| r.ticket)))
    };

    // AGS-20: the crumb is the ancestor chain — the decomposition read
    // downward, root first — resolved through the same row store, so a
    // parent that arrives later fills itself in.
    let crumbs = {
        let id = id.clone();
        move || {
            ancestors(rows, &id)
                .into_iter()
                .map(|a| {
                    let (href, code) = (format!("#/ticket/{a}"), a.clone());
                    view! {
                        <span class="sep" aria-hidden="true">" / "</span>
                        <a class="crumb" data-ticket-id=a href=href>{code}</a>
                    }
                })
                .collect_view()
        }
    };

    let id_view = id.clone();
    view! {
        <div id="ticket-page">
            <p class="crumbs"><a href="#/">"← board"</a>{crumbs}</p>
            {move || match ticket.get() {
                Some(t) => ticket_view(t, body, rows, blockers_loaded, kids, more).into_any(),
                None if not_found.get() => {
                    let id = id_view.clone();
                    view! {
                        <div id="notfound">
                            <h2>{id}" — not found"</h2>
                            <p>
                                "No ticket has this id. "
                                <a href="#/">"Back to the board"</a>
                                "."
                            </p>
                        </div>
                    }
                        .into_any()
                }
                None => {
                    view! { <p class="loading">"loading "{id_view.clone()}"…"</p> }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn App() -> impl IntoView {
    let rows = RwSignal::new(Vec::<Row>::new());
    let dock = RwSignal::new(None::<Dock>);
    let conn = RwSignal::new(Conn::Connecting);
    let socket = RwSignal::new(0u32);
    let last_error = RwSignal::new(None::<String>);
    let route = use_route();

    // MFE-9: the whole data plane is one task holding one WebSocket
    // (session.rs) — `watch` first, then MNG-9's summary pages walked with
    // `after = next`, then live. ~1s retry keeps a server restart to a few
    // seconds of "reconnecting…" (MFE-2 criterion 3). It runs regardless of
    // route: the ticket page's liveness is this same stream.
    session::drive(rows, dock, conn, socket, last_error);

    let conn_view = move || match conn.get() {
        Conn::Connecting => ("", "connecting…"),
        Conn::Live => ("live", "live · WS"),
        Conn::Reconnecting => ("reconnecting", "reconnecting…"),
    };
    let health_view = move || {
        dock.get()
            .map(|h| format!("menger v{} · {} tickets", h.version, h.ticket_count))
    };

    // AGS-12: the nav's hidden/shown state, persisted. The nav stays
    // mounted either way (a class hides it), so its rows keep their DOM
    // identity across hide/reveal and the content column reflows.
    let nav_hidden = RwSignal::new(nav::load_hidden());
    Effect::new(move |_| nav::save_hidden(nav_hidden.get()));

    // AGS-18: one window-level key listener for the app's lifetime, and
    // the overlay it opens. Both are additive — every existing pointer
    // path is untouched.
    let overlay = RwSignal::new(keys::Overlay::None);
    keys::install(overlay, rows);

    // MFE-12: the content column is the board's viewport. Its scroll and
    // the window's resize are the two things that move which rows are on
    // screen; the board measures itself when this fires.
    let viewport = Trigger::new();
    window_event_listener(leptos::ev::resize, move |_| viewport.notify());

    view! {
        <div id="app" class:nonav=move || nav_hidden.get()>
            <nav::Nav rows route hidden=nav_hidden />
            <div class="content" on:scroll=move |_| viewport.notify()>
                <header>
                    {move || {
                        nav_hidden.get().then(|| view! {
                            <button
                                class="nav-reveal"
                                title="show the nav"
                                on:click=move |_| nav_hidden.set(false)
                            >
                                "☰"
                            </button>
                        })
                    }}
                    <h1><a href="#/">"menger"</a></h1>
                    <span class="tag">
                        {move || match route.get() {
                            Route::Board => "the live board (MFE-3)",
                            Route::Ticket(_) => "ticket (MFE-7)",
                        }}
                    </span>
                    <span id="conn" class=move || conn_view().0>{move || conn_view().1}</span>
                    // AGS-17: the theme toggle (system → light → dark).
                    <theme::Toggle />
                </header>
                <main>
                    <div id="health">{health_view}</div>
                    {move || {
                        last_error.get().map(|msg| view! {
                            <div id="error" role="alert">
                                <span class="msg">{msg}</span>
                                <button on:click=move |_| last_error.set(None)>"dismiss"</button>
                            </div>
                        })
                    }}
                    {move || match route.get() {
                        Route::Board => view! { <Board rows last_error viewport /> }.into_any(),
                        Route::Ticket(id) => {
                            view! { <TicketPage id rows socket last_error /> }.into_any()
                        }
                    }}
                </main>
            </div>
            // AGS-16: mounted once, outside every ticket element, so a
            // right-click inside the menu itself never re-targets it.
            <menu::TicketMenu rows last_error />
            <keys::Overlays rows overlay />
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[cfg(test)]
mod tests {
    use super::{
        board_layout, board_window, row_index, st_class, Geom, Row, BOARD_MARGIN, BOARD_MAX_PX,
        BOARD_STEP, STATUSES,
    };

    /// MFE-12: the window is the viewport plus a margin either side,
    /// snapped outward to the step, inside the store — and it holds still
    /// while the viewport moves within one step.
    #[test]
    fn the_board_window_is_the_viewport_plus_a_margin_in_steps() {
        let (m, s) = (BOARD_MARGIN, BOARD_STEP);
        // The top of a big store: from row 0, the viewport and one margin
        // below it, snapped up to a step.
        let (start, end) = board_window(10_000, 0, 28);
        assert_eq!(start, 0);
        assert_eq!(end, (28 + m).div_ceil(s) * s);
        // The middle: a margin both sides, both edges on a step.
        let (start, end) = board_window(10_000, 5_000, 28);
        assert!(start <= 5_000 - m && start % s == 0, "{start}");
        assert!(end >= 5_000 + 28 + m && end % s == 0, "{end}");
        assert!(end - start <= 28 + 2 * m + 2 * s, "bounded: {start}..{end}");
        // Scrolling within a step changes nothing; a step later it moves.
        let w = board_window(10_000, 5_000, 28);
        assert_eq!(board_window(10_000, 5_001, 28), w);
        assert_ne!(board_window(10_000, 5_000 + s, 28), w);
        // The bottom: the last row is mounted, nothing past the store.
        let (start, end) = board_window(10_000, 10_000 - 28, 28);
        assert_eq!(end, 10_000);
        assert!(start < 10_000 - 28 - m + s);
        // A viewport past the end (the store shrank under it) still shows
        // the last rows; a small store is mounted whole; an empty one is empty.
        assert_eq!(board_window(10_000, 50_000, 28).1, 10_000);
        assert!(board_window(10_000, 50_000, 28).0 < 10_000 - 28);
        assert_eq!(board_window(90, 0, 28), (0, 90));
        assert_eq!(board_window(90, 60, 28), (0, 90));
        assert_eq!(board_window(0, 0, 28), (0, 0));
    }

    /// MFE-12: under the height cap the board is its natural self — row
    /// `i` at `i · pitch`, the spacers exactly the rows they replace, the
    /// window [`board_window`]'s.
    #[test]
    fn under_the_cap_the_board_is_laid_out_at_its_natural_height() {
        let p = 37.78125;
        let n = 10_000;
        for above in [-120.0, 0.0, 1.0, 9_000.0, 188_000.0, n as f64 * p] {
            let g = Geom { above, viewport: 880.0, pitch: p };
            let l = board_layout(n, g);
            let first = (above.max(0.0) / p).floor() as usize;
            assert_eq!((l.start, l.end), board_window(n, first, (880.0 / p).ceil() as usize + 1), "{above}");
            assert_eq!(l.top_px, l.start as f64 * p);
            assert_eq!(l.bottom_px, (n - l.end) as f64 * p);
            assert_eq!(g.row_at(n, above), above / p);
            assert_eq!(g.offset_of(n, 12.5), 12.5 * p);
        }
    }

    /// MFE-12: over the cap (10^6 rows is 37.8 M px; browsers stop laying
    /// out at 17.9–33.5 M) the board is exactly [`BOARD_MAX_PX`] tall at
    /// every scroll offset, neither spacer goes negative, the rows on
    /// screen are always mounted and drawn where the mapping says, and the
    /// two ends of the store are the two ends of the scroll range.
    #[test]
    fn over_the_cap_every_row_is_reachable_and_the_board_never_grows() {
        let (p, vp) = (37.78125, 880.0);
        let n = 1_000_000;
        let g = |above: f64| Geom { above, viewport: vp, pitch: p };
        let span = BOARD_MAX_PX - vp;
        let mut last_r = -1.0;
        let mut v = -200.0;
        while v <= span + 60.0 {
            let l = board_layout(n, g(v));
            let r = g(v).row_at(n, v);
            assert!(r >= last_r, "monotonic at {v}");
            last_r = r;
            assert!(l.top_px >= 0.0 && l.bottom_px >= 0.0, "{v}: {l:?}");
            // the board is the cap tall (a last row's worth more at most,
            // where the end rounds up to keep the last row whole)
            let height = l.top_px + (l.end - l.start) as f64 * p + l.bottom_px;
            assert!(height >= BOARD_MAX_PX - 1e-3 && height < BOARD_MAX_PX + p, "{v}: height {height}");
            // every row overlapping the viewport is mounted…
            let first = r.floor() as usize;
            let last = ((r + vp / p).ceil() as usize).min(n);
            assert!(l.start <= first && last <= l.end, "{v}: rows {first}..{last} vs {l:?}");
            // …and the row the mapping names sits at the viewport's top —
            // exactly, but for the first row's worth of scroll
            let drawn = l.top_px + (r - l.start as f64) * p;
            let want = v.clamp(0.0, span);
            let slack = if want < p { p } else { 1e-6 };
            assert!((drawn - want).abs() < slack, "{v}: drawn at {drawn}");
            // the mapping round-trips
            assert!((g(v).offset_of(n, r) - v.clamp(0.0, span)).abs() < 1e-3);
            v += 9_973.0; // a stride that lands everywhere in a row
        }
        // the top: row 0, no spacer above; the bottom: the last row, none below
        let top = board_layout(n, g(0.0));
        assert_eq!((top.start, top.top_px), (0, 0.0));
        let bottom = board_layout(n, g(span));
        assert_eq!(bottom.end, n);
        assert!(bottom.bottom_px < 1e-3, "{bottom:?}");
        // mid-board the window is the ordinary stepped one, margins and all
        let mid = board_layout(n, g(span / 2.0));
        let r = g(span / 2.0).row_at(n, span / 2.0);
        assert_eq!((mid.start, mid.end), board_window(n, r.floor() as usize, (vp / p).ceil() as usize + 1));
        assert!((r - (n as f64 - vp / p) / 2.0).abs() < 1.0, "halfway down the scroll range is halfway down the store: {r}");
    }

    fn row(id: &str) -> Row {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        Row::of(menger_tickets_api::TicketSummary {
            id: id.to_string(),
            epic: epic.to_string(),
            seq: seq.parse().expect("seq"),
            title: String::new(),
            status: menger_tickets_api::Status::Pending,
            kind: menger_tickets_api::Kind::Build,
            severity: None,
            parent: None,
            child_count: 0,
            blocked_by: Vec::new(),
            unlocks: Vec::new(),
            commits: Vec::new(),
            created_unix: 0,
            updated_unix: 0,
        })
    }

    /// MFE-12: an id is its own `(epic, seq)` key, so it finds its row by
    /// binary search — across epics, multi-digit seqs and a dashed epic —
    /// and an id the store does not hold is `None`, never a neighbor.
    #[test]
    fn row_index_finds_a_ticket_by_its_own_key() {
        let v: Vec<Row> = ["AGS-1", "AGS-2", "AGS-10", "MFE-3", "SUB-X-4", "TRANSPORT-2"]
            .into_iter()
            .map(row)
            .collect();
        for (i, r) in v.iter().enumerate() {
            assert_eq!(row_index(&v, &r.id), Some(i), "{}", r.id);
        }
        assert_eq!(row_index(&v, "AGS-3"), None);
        assert_eq!(row_index(&v, "ZZZ-1"), None);
        assert_eq!(row_index(&v, "no-dash-seq"), None);
        assert_eq!(row_index(&[], "AGS-1"), None);
    }

    fn index_html() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/index.html")).unwrap()
    }

    /// The app's stylesheet: what sits between the shell's <style> tags.
    fn app_css() -> String {
        let html = index_html();
        let start = html.find("<style>").expect("<style>") + "<style>".len();
        let end = html.find("</style>").expect("</style>");
        html[start..end].to_string()
    }

    /// AGS-15: every status in the closed vocabulary has a row in
    /// index.html's status table (a bg and an fg), and the class the
    /// views put on a chip names exactly that row. A new status without
    /// a row would paint as plain text; this fails first. AGS-17: and a
    /// row in the dark table, under both dark mechanisms. SDLC-7: the row
    /// is the status lowercased with its space hyphenated (`In Review` →
    /// `st-in-review`), one class — never two.
    #[test]
    fn every_status_has_a_row_in_the_status_table() {
        let css = app_css();
        let slug = |s: &str| s.to_lowercase().replace(' ', "-");
        for s in STATUSES {
            let class = st_class(s);
            let row = class.strip_prefix("st ").unwrap();
            assert_eq!(row, format!("st-{}", slug(s)));
            assert!(!row.contains(' '), "{s}: one class, not two");
            for prefix in ["", &format!("{DARK_ATTR} "), &format!("{DARK_MEDIA} ")] {
                let line = css
                    .lines()
                    .find(|l| l.trim_start().starts_with(&format!("{prefix}.{row} ")))
                    .unwrap_or_else(|| panic!("no status-table row for {s} ({prefix}.{row})"));
                assert!(
                    line.contains("--st-bg: #") && line.contains("--st-fg: #"),
                    "{s}: {line}"
                );
            }
        }
        assert_eq!(st_class("Blocked"), "st st-blocked");
        assert_eq!(st_class("In Review"), "st st-in-review");
        let scan = scan(&css);
        let want: std::collections::BTreeSet<String> = STATUSES.iter().map(|s| slug(s)).collect();
        for (table, rows) in [
            ("light", &scan.st_light),
            ("dark (attribute)", &scan.st_dark_attr),
            ("dark (media)", &scan.st_dark_media),
        ] {
            let got: std::collections::BTreeSet<String> = rows.iter().map(|r| r.0.clone()).collect();
            assert_eq!(got, want, "the {table} status table names exactly the vocabulary");
            assert_eq!(rows.len(), STATUSES.len(), "the {table} status table: one row per status");
        }
    }

    /// True if `line` holds a color literal: `#` + 3/4/6/8 hex digits as a
    /// whole token (an id selector like `#board` or `#error` is not one —
    /// its letters are not all hex, or it runs on into more word
    /// characters), an `rgb(`/`rgba(`/`hsl(` function, or a named color.
    fn has_color_literal(line: &str) -> bool {
        let bytes = line.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b != b'#' {
                continue;
            }
            let run = bytes[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            let next = bytes.get(i + 1 + run).copied();
            let ends_token = !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
            if matches!(run, 3 | 4 | 6 | 8) && ends_token {
                return true;
            }
        }
        let lower = line.to_ascii_lowercase();
        if ["rgb(", "rgba(", "hsl(", "hsla(", "color("].iter().any(|f| lower.contains(f)) {
            return true;
        }
        // The named colors a scaffold reaches for; `transparent`, `inherit`,
        // `none`, and `currentColor` are not palette choices.
        let named = [
            "white", "black", "red", "green", "blue", "gray", "grey", "yellow", "orange",
            "purple", "pink", "silver", "navy", "teal", "maroon", "olive", "lime", "aqua",
        ];
        // A hyphen is part of the word: `white-space` names no color.
        lower
            .split(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
            .any(|w| named.contains(&w))
    }

    /// The two dark selectors (AGS-17): the attribute the toggle stamps,
    /// and the media-query guard that a stored "light" opts out of.
    const DARK_ATTR: &str = ":root[data-theme=\"dark\"]";
    const DARK_MEDIA: &str = ":root:not([data-theme=\"light\"])";
    const MEDIA_DARK: &str = "@media (prefers-color-scheme: dark) {";

    /// The stylesheet, sorted into the places a color may be named — the
    /// light `:root` block, the two dark blocks, the light status table,
    /// the dark status table (under both mechanisms) — and everything
    /// else that names one (`strays`). Status rows are `(status, body)`;
    /// block declarations are kept verbatim (trimmed) so the two dark
    /// copies can be compared line by line.
    #[derive(Default, Debug)]
    struct Scan {
        light: Vec<String>,
        dark_attr: Vec<String>,
        dark_media: Vec<String>,
        st_light: Vec<(String, String)>,
        st_dark_attr: Vec<(String, String)>,
        st_dark_media: Vec<(String, String)>,
        strays: Vec<String>,
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Block {
        None,
        Light,
        DarkAttr,
        DarkMedia,
    }

    /// A status row's `(status, normalized body)` from what follows `.st-`.
    fn status_row(rest: &str) -> (String, String) {
        let name = rest.split(|c: char| c == ' ' || c == '{').next().unwrap_or_default();
        let body = rest[name.len()..].split_whitespace().collect::<Vec<_>>().join(" ");
        (name.to_string(), body)
    }

    fn scan(css: &str) -> Scan {
        let mut out = Scan::default();
        let mut block = Block::None;
        let mut in_media = false;
        for line in css.lines() {
            let t = line.trim();
            if block != Block::None {
                if t == "}" {
                    block = Block::None;
                } else {
                    match block {
                        Block::Light => out.light.push(t.to_string()),
                        Block::DarkAttr => out.dark_attr.push(t.to_string()),
                        Block::DarkMedia => out.dark_media.push(t.to_string()),
                        Block::None => unreachable!(),
                    }
                }
                continue;
            }
            if t == MEDIA_DARK {
                assert!(!in_media, "nested prefers-color-scheme media blocks");
                in_media = true;
                continue;
            }
            if in_media && t == "}" {
                in_media = false;
                continue;
            }
            if t == ":root {" {
                block = Block::Light;
                continue;
            }
            if t == format!("{DARK_ATTR} {{") {
                assert!(!in_media, "the attribute block must not sit inside the media query");
                block = Block::DarkAttr;
                continue;
            }
            if t == format!("{DARK_MEDIA} {{") {
                assert!(in_media, "the guarded block must sit inside the media query: {t}");
                block = Block::DarkMedia;
                continue;
            }
            if let Some(rest) = t.strip_prefix(".st-") {
                out.st_light.push(status_row(rest));
                continue;
            }
            if let Some(rest) = t.strip_prefix(DARK_ATTR).and_then(|r| r.strip_prefix(" .st-")) {
                assert!(!in_media, "{t}");
                out.st_dark_attr.push(status_row(rest));
                continue;
            }
            if let Some(rest) = t.strip_prefix(DARK_MEDIA).and_then(|r| r.strip_prefix(" .st-")) {
                assert!(in_media, "the guarded status rows must sit inside the media query: {t}");
                out.st_dark_media.push(status_row(rest));
                continue;
            }
            if has_color_literal(t) {
                out.strays.push(t.to_string());
            }
        }
        assert_eq!(block, Block::None, "unterminated block");
        assert!(!in_media, "unterminated media query");
        out
    }

    /// A block's declarations as `(property, value)`, comments stripped
    /// (a comment may span lines, and may say "black").
    fn declarations(block: &[String]) -> Vec<(String, String)> {
        let mut text = block.join("\n");
        while let Some(start) = text.find("/*") {
            let end = text[start..].find("*/").map(|e| start + e + 2).unwrap_or(text.len());
            text.replace_range(start..end, " ");
        }
        text.split(';')
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(|d| {
                let (name, value) = d.split_once(':').unwrap_or_else(|| panic!("not a declaration: {d}"));
                (name.trim().to_string(), value.trim().to_string())
            })
            .collect()
    }

    /// The names of a block's color tokens (custom properties whose value
    /// holds a color literal) — the set the themes must agree on.
    fn color_tokens(block: &[String]) -> std::collections::BTreeSet<String> {
        declarations(block)
            .into_iter()
            .filter(|(name, value)| name.starts_with("--") && has_color_literal(value))
            .map(|(name, _)| name)
            .collect()
    }

    /// AGS-15 criterion 4, evolved by AGS-17: color literals live only in
    /// the light `:root` block, the two dark blocks, and the two status
    /// tables (the dark one under both mechanisms). Every other rule
    /// paints through `var(--…)`.
    #[test]
    fn colors_live_only_in_the_theme_blocks_and_the_status_tables() {
        let scan = scan(&app_css());
        let light_literals = scan.light.iter().filter(|l| has_color_literal(l)).count();
        assert!(light_literals >= 10, "the light block holds the palette ({light_literals} lines)");
        for (dark, name) in [(&scan.dark_attr, "attribute"), (&scan.dark_media, "media")] {
            let n = dark.iter().filter(|l| has_color_literal(l)).count();
            assert!(n >= 10, "the dark ({name}) block holds the palette ({n} lines)");
        }
        for (rows, table) in [(&scan.st_light, "light"), (&scan.st_dark_attr, "dark attribute"), (&scan.st_dark_media, "dark media")] {
            assert_eq!(rows.len(), STATUSES.len(), "one {table} table row per status");
            for (status, body) in rows {
                assert!(has_color_literal(body), "{table} status-table row without colors: {status} {body}");
            }
        }
        assert!(
            scan.strays.is_empty(),
            "color literals outside the theme blocks / status tables:\n{}",
            scan.strays.join("\n")
        );
    }

    /// AGS-17: the dark blocks define every color token the light block
    /// defines and nothing else — a token defined in one theme only is a
    /// bug (it would paint the wrong theme's value, or nothing). The type
    /// tokens (`--mono`, `--sans`) are theme-invariant and live once, in
    /// the light block; a dark block may set `color-scheme` (form
    /// controls, scrollbars) and no other plain property.
    #[test]
    fn dark_blocks_define_exactly_the_light_blocks_color_tokens() {
        let scan = scan(&app_css());
        let light = color_tokens(&scan.light);
        assert!(light.contains("--bg") && light.contains("--ink") && light.contains("--shadow"), "{light:?}");
        for (dark, name) in [(&scan.dark_attr, "attribute"), (&scan.dark_media, "media")] {
            assert_eq!(color_tokens(dark), light, "dark ({name}) block's color tokens vs the light block's");
            for (prop, value) in declarations(dark) {
                if prop.starts_with("--") {
                    assert!(has_color_literal(&value), "dark ({name}) token without a color: {prop}: {value}");
                } else {
                    assert_eq!((prop.as_str(), value.as_str()), ("color-scheme", "dark"), "dark ({name}) block: unexpected declaration");
                }
            }
        }
    }

    /// AGS-17: the two dark mechanisms — the media query (guarded) and the
    /// stamped attribute — carry the same values, token for token and
    /// status for status, so the toggle and the system preference paint
    /// one dark, not two.
    #[test]
    fn both_dark_mechanisms_carry_identical_values() {
        let scan = scan(&app_css());
        assert_eq!(declarations(&scan.dark_attr), declarations(&scan.dark_media), "dark token blocks differ");
        assert_eq!(scan.st_dark_attr, scan.st_dark_media, "dark status tables differ");
        assert_eq!(scan.st_dark_attr.len(), STATUSES.len());
    }

    /// The guard is live: a literal planted anywhere outside the allowed
    /// blocks is reported — in a component rule, as a named color, as
    /// `rgb()`, and inside the media query but outside the guarded block
    /// — and a token added to one dark block only breaks the set equality.
    #[test]
    fn literal_guard_catches_a_planted_stray() {
        let css = app_css();
        assert!(scan(&css).strays.is_empty(), "precondition: the real stylesheet is clean");
        let plants = [
            "  #nav .row.sel { background: #cfe2f7; }",
            "  header h1 { color: black; }",
            "  td.id { color: rgb(0, 102, 204); }",
        ];
        for plant in plants {
            let planted = format!("{css}\n{plant}\n");
            let strays = scan(&planted).strays;
            assert_eq!(strays, vec![plant.trim().to_string()], "planted: {plant}");
        }
        // Inside the media query, but not in the guarded block: still a stray.
        let planted = css.replacen(MEDIA_DARK, &format!("{MEDIA_DARK}\n    body {{ background: #000; }}"), 1);
        assert_eq!(scan(&planted).strays, vec!["body { background: #000; }".to_string()]);
        // A token in one dark block only: the set equality fails.
        let planted = css.replacen(&format!("{DARK_ATTR} {{"), &format!("{DARK_ATTR} {{\n    --only-here: #123456;"), 1);
        let s = scan(&planted);
        assert!(s.strays.is_empty(), "a token inside a dark block is not a stray");
        assert_ne!(color_tokens(&s.dark_attr), color_tokens(&s.light));
        assert_ne!(declarations(&s.dark_attr), declarations(&s.dark_media));
    }

    /// AGS-18: focus is painted wherever a key can put it — the nav
    /// tree's rows, the board's rows, the palette's input — and it is
    /// painted as a *ring*, from the token layer. The selected row keeps
    /// its own idiom (a `--row-sel` tint with an inset accent bar), so
    /// "where I am" and "what is open" cannot collapse into one look.
    #[test]
    fn focus_is_a_token_ring_and_not_the_selected_row_s_tint() {
        let css = app_css();
        let lines: Vec<&str> = css.lines().map(str::trim).collect();
        let start = lines
            .iter()
            .position(|l| l.starts_with("#nav .row:focus,"))
            .expect("no focus rule for the nav and board rows");
        let block: String = lines[start..]
            .iter()
            .take_while(|l| !l.starts_with('}'))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        for reachable in ["#nav .row:focus", "tbody#board tr:focus", "#palette-input:focus"] {
            assert!(block.contains(reachable), "{reachable} has no focus ring: {block}");
        }
        assert!(block.contains("outline:") && block.contains("var(--accent)"), "{block}");
        // A ring, not a fill: the selected row is the one with a tint.
        assert!(!block.contains("background"), "focus paints a background: {block}");
        let sel = lines
            .iter()
            .find(|l| l.starts_with("#nav .row.sel {"))
            .expect("no selected-row rule");
        assert!(sel.contains("var(--row-sel)") && sel.contains("inset"), "{sel}");
    }

    #[test]
    fn color_literal_scanner_knows_ids_from_hex() {
        assert!(has_color_literal("  --bg: #fbfbf9;"));
        assert!(has_color_literal("  .st-blocked { --st-bg: #fdecec; --st-fg: #a23; }"));
        assert!(has_color_literal("  a { color: rgb(1, 2, 3); }"));
        assert!(has_color_literal("  a { background: #fff; }"));
        assert!(has_color_literal("  a { color: white; }"));
        assert!(!has_color_literal("  #error button { color: inherit; }"));
        assert!(!has_color_literal("  td.id { color: var(--accent); white-space: nowrap; }"));
        assert!(!has_color_literal("  tbody#board tr:hover td { background: var(--hover); }"));
        assert!(!has_color_literal("  #app.nonav #nav { display: none; }"));
        assert!(!has_color_literal("  .st { border-color: transparent; }"));
        assert!(!has_color_literal("  #blockers a, #unlocks a { color: var(--accent); }"));
    }
}
