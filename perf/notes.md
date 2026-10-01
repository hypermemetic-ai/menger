## Degradation curve

The ladder splits cleanly into three kinds of path.

**Paths that scale linearly and stay cheap — the store and its
transports.** Seeding, the startup load, streaming a full list over the
WebSocket, and memory all move ×10 per decade beyond 10³ (the ratios
below 10³ are dominated by fixed costs). The cache loads at ~2.3 µs per
ticket — a million tickets are in memory and serving 2.3 s after spawn —
and holds ~1.1 KiB resident per ticket (1.09 GiB at 10⁶, half of it the
~500-byte body). The database costs ~700 B per ticket on disk. The point
queries — `health`, `get`, `blockers` — are flat at 1–2 ms from 10² to
10⁶: hash lookups that never see N. The WebSocket `list` control streams
a million tickets (980 MiB of JSON-RPC frames) in 7.8 s, ~7.8 µs per
ticket; its first item arrives after 1.6 s at 10⁶ because `Store::list`
clones and sorts every ticket before yielding the first one — the same
clone shows up as the process's peak RSS, three times its resident size
(3.3 GiB at 10⁶): every full list, over either transport, materializes a
second copy of the cache. The create → `watch` push is flat at 2.5–3 ms
through 10⁴ and then grows (5.7 ms at 10⁵, 35 ms at 10⁶): `create` walks
the whole cache twice under the write-serial lock — once for the epic's
max seq, once for dangling forward references to the new id — and those
two O(N) scans are the only write-path cost that moved.

**The path that falls off a cliff — a unary `list` over REST.**
plexus-transport 0.2.3's HTTP handler collects a method's whole stream
into one JSON array before answering, capped at `MAX_BUFFERED_ITEMS =
10_000` (and 100 MB) — compile-time constants in `src/http/handler.rs`,
not configuration. At 10⁴ the cap is met exactly: `list` returned 10,000
items in 104 ms (7.4 MiB). One ticket more and it answers **HTTP 413
`Stream exceeded maximum item count of 10000`** in ~150 ms, having
already cloned the entire cache to get there. `ready_set` follows as
soon as the ready set itself passes 10,000 (≈14,700 at 10⁵). The cliff
is visible inside the 10⁴ row: the first browser run loaded all 10,000
tickets in 3.49 s; the harness's own probe tickets (`PROBE-1`, one per
live-create check) then took the store to 10,001, and the second and
third loads died with the same 413 the app shows at 10⁵ and 10⁶. So the
usable ceiling of the current app is not a gradual slope — it is 10,000
tickets, sharp.

**The paths that were already bending — rendering.** Below the cliff
the browser is super-linear-ish: load → live 117 ms → 401 ms → 3.49 s
(×3.4, ×8.7 per decade), because the app's session (`frontend/src/main.rs`)
opens `watch`, then fetches `health` and an unfiltered `list`, parses it
in wasm, and renders every ticket into two trees before it declares
itself live: a board row (6 cells, an id link, and a 7-option `<select>`
per row) plus a nav leaf (5 spans) — 22 DOM elements per ticket, 220,315
at 10⁴ (760,577 nodes counting text nodes, on a 32 MiB JS heap — the
ticket data lives in wasm linear memory, not the JS heap). The nav keeps
every leaf in the DOM even while its group is collapsed (collapse is a
CSS class, so expanding costs only layout: 274 ms for 2,328 leaves),
which is why "nav shortcodes visible" and "board render" track
load → live to within 100 ms — the cost is paid up front for all of them.
Switching from the board to a ticket page grows ×10 per decade
(35 → 72 → 713 ms) because it tears down the 10k-row board and scrolls
the auto-expanded nav; a live insert grows ×5 per decade
(19 → 35 → 200 ms) as the keyed `<For>` re-diffs the row list. Extrapolating
the trend past the cap, 10⁵ would be ~30 s to live with 2.2 M DOM nodes
and a 74 MiB JSON parse in wasm; 10⁶ minutes, 22 M nodes, 740 MiB — the
app would be unusable at 10⁵ with or without the transport cap. The one
browser path that survives every magnitude is the cold deep link to a
ticket page: flat ~100 ms from 10² to 10⁶, because the page renders from
`get` + `blockers` (two point queries) through the shared store before —
or, at ≥10⁵, without ever — the full list landing; at 10⁵ and 10⁶ the
page shows the ticket, its blockers and unlocks correctly while the
header still says "reconnecting…" (screenshot `1e5-ticket-cold.png`).

**Where the UI stops being usable, and why.** Comfortable to a few
thousand tickets (10³: 0.4 s to live, sub-100 ms navigation). Degraded at
10⁴: 3.5 s to a first paint of everything, 0.7 s to open a ticket from the
board, 0.2 s for a created ticket to appear, a 7.4 MiB list on every
(re)connect. Broken at 10,001: the REST `list` the app depends on returns
413, the session loop retries it every second forever, and the board and
nav stay empty under a header that still reports the count
(`1e5-board.png`). The transport cap is the proximate cause; the app's
"list everything, render everything, then go live" design is the
underlying one — the cap only moved a failure that rendering would have
produced one decade later.

## Recommendations (first fixes — recommendations only)

1. **Load what is shown, not everything.** The nav needs an epic roll-up
   (code, total, done — O(epics), not O(N)); the board needs a page or a
   window of one epic (`list` already filters by `epic`/`status`
   server-side, unused by the app; add a cursor on `(epic, seq)` and a
   limit); the ticket page already does it right — `get` + `blockers`
   is flat at 10⁶ and is the model. This removes the unary full list
   from the browser's path entirely, which is what makes 10⁵ and 10⁶
   reachable at all.
2. **Never return an unbounded unary response over REST.** With (1),
   no page exceeds the gateway's 10,000-item / 100 MB collect buffer.
   For clients that do want everything, expose `list`/`ready_set` as
   streaming methods (SSE, like `watch`) rather than raising the cap:
   the WebSocket control shows the store streams 10⁶ in 7.8 s, while a
   raised cap would only make the server assemble a 740 MiB response.
3. **Virtualize the two trees.** Render only the visible window of board
   rows and of a group's nav leaves (mount leaves on expand, not on
   load — the largest epic at 10⁶ has 163,531 tickets, 800k nodes if
   materialized), and replace the per-row 7-option `<select>` (8 nodes
   per row, a third of the board's DOM) with a control rendered on
   demand. 22 elements per ticket is what turns 10⁴ into 3.5 s.
4. **Stop cloning the cache per query.** Keep a sorted index
   (`BTreeMap<(epic, seq), id>` or a sorted `Vec` of ids) beside the
   `HashMap` so `list`/`ready_set` pages are O(page) and a full stream
   yields without a second copy (peak RSS would stay near resident
   instead of 3×; first byte would not wait 1.6 s at 10⁶). Give `create`
   a per-epic max-seq map and a forward-reference index so the write path
   stops scanning N twice under the write-serial lock (35 ms at 10⁶).
5. **Slim the row shape.** The body is ~70 % of a list row; a summary
   event for list/board/nav (no body) and the body only on `get` cuts the
   payload and the wasm parse by that much before any paging lands.

Everything above is measured, not tuned: the server and app under test
are `0ad2c58` unchanged.

## What MNG-9 changed (re-run, `c9b58bf`)

Everything above is the ladder at `0ad2c58`, before paging. This section
is the same ladder re-run after MNG-9 gave `list` and `ready_set` a
cursor (`after`), a `limit`, body-less `summary` rows and a closing
`page { next, remaining }` event, and moved the cache into the store's
own `(epic, seq)` order. It reads against those tables, not beside them:
the machine is the same but the session is not, so compare shapes and
the A/B pairs quoted below, which were measured master-against-branch on
one database, alternating, the same minute.

**The new row: WS list, paged (200).** The walk a client that renders
does — `list {summary: true, limit: 200}`, then the same call with
`after` = the previous page's `next`, until `next` is null.

| | 10² | 10³ | 10⁴ | 10⁵ |
|---|---:|---:|---:|---:|
| pages | 1 | 5 | 50 | 500 |
| whole walk | 3.6 ms | 12.5 ms | 107 ms | 1.06 s |
| **worst single page** | 1.9 ms | 2.6 ms | 2.8 ms | **6.1 ms** |
| first row | 1.6 ms | 1.9 ms | 1.6 ms | **2.2 ms** |
| bytes | 50.5 KiB | 496 KiB | 4.97 MiB | 49.8 MiB |
| the same set streamed whole | 4.2 ms · 0.10 MiB | 12.7 ms · 0.99 MiB | 71 ms · 9.89 MiB | 688 ms · 99.1 MiB |

Two things move and one does not. **Latency stops scaling**: the first
row lands in ~2 ms and the worst page in 6.1 ms at 100,000, against a
92 ms first item for the streamed control at the same magnitude — a
renderer now paints its first screen in constant time whatever N is,
which is the whole point of the ticket. **The bytes halve** — 49.8 MiB
against 99.1 MiB — because a summary row is a ticket without its body,
and no board shows a body. What does *not* improve is the whole walk:
1.06 s against 688 ms, because 500 round trips cost 500 round trips. A
client that genuinely wants every row with bodies should still ask for
them in one call; a client that renders wants the first page, and gets
it 40× sooner.

**The store's order is now the cache's shape.** `list`/`ready_set` used
to clone every matching ticket and sort; the tickets now live in a
`BTreeMap<(epic, seq), Ticket>` with a hash map from id to position, so
an ordered read is a range walk over what is already sorted and already
in hand, and a page is a range scan from a cursor. Measured A/B at
100,000 (first item after the subscribe reply, medians of 7, three
alternating rounds):

| call | master | MNG-9 |
|---|---:|---:|
| `list {}` (everything, bodies) | 158 / 177 / 177 ms | 75 / 96 / 139 ms |
| `list {epic: MAP}` (19,195 rows) | 28 / 34 / 29 ms | 16 / 19 / 20 ms |
| `ready_set {}` (14,652 rows) | 26 / 29 / 28 ms | 25 / 31 / 28 ms |

An epic filter is a *position*, not a predicate — `(epic, seq)` puts an
epic's tickets in one contiguous stretch — so it bounds the walk instead
of testing every ticket in the store. What still costs O(what is left)
is an exact `remaining` under a status/parent filter or the ready-set
rule: `ready_set {summary, limit: 200}` answers its first row in 13 ms
at 100,000 (13 ms of counting the 14,649 that remain). That number is
pinned here as the accepted price of an exact count.

The order costs once per start: loading 100,000 tickets into the cache
went 347 / 349 / 347 ms (master) to 553 / 524 / 563 ms in the same A/B —
the sort and the tree build — and resident memory 116 → 124 MiB. It is
paid before the server serves.

**The REST cliff moved down by one ticket.** Every `list`/`ready_set`
response now ends with one `page` item, and plexus-transport 0.2.3's
unary handler collects a stream into one array capped at 10,000 items
(TRANSPORT-2). So the magnitude at which a unary REST `list` returns
**413** is now exactly 10,000 tickets rather than 10,001: at 10⁴ the
`REST list` row and every browser row fail here where the 0ad2c58 run
had one browser run in three succeed. This is the cap, not the store —
the same 10⁴ set streams over the WebSocket in 71 ms, and pages in 50
pages of 2.8 ms — and it is the reason MFE-9 moves the app onto the
socket and onto these pages rather than raising a buffer.

Browser rows are otherwise unchanged in shape: the app still lists
everything and renders everything, because MNG-9 is the server half.
10² and 10³ stay comfortable, 10⁴ is now the wall, and the flat ~140 ms
cold deep link (`get` + `blockers`, two point queries) still proves the
model the rest of the app has yet to adopt.

## What MFE-12 changed (re-run, branch `mfe-12-window-the-board`)

The two sections above are the ladder on the REST client. MFE-9 then put
the app on the Plexus WebSocket — its numbers were taken by
`plans/MFE/artifacts/MFE-9-numbers.js`, and the ladder was not re-run — and
MFE-12 changed what the app mounts: the board is a *window* over the row
store (the rows on screen plus 100 either side, between two spacer rows at
the row pitch) and the nav mounts a node's children when it opens instead
of hiding them with a class. The row store and the walk are untouched. This
is the ladder's first run on the socket client and on a windowed app:
`--date 2026-10-01-mfe12`, the same machine, the server unchanged since
MFE-11 (`49394a5`; the header's "+ uncommitted changes" is the MFE-12
frontend and this harness, measured one lint-only edit short of the
commit — `map_or(true, …)` became `is_none_or(…)` in `keys.rs`). Server
rows read against the MNG-9 re-run; only the browser rows are new.

### The browser rows, redefined

Four rows waited for things a windowed app never does by design — every
ticket as a board `<tr>`, every leaf in the nav — so each browser row now
measures something true of this app:

| row | measures | was |
|---|---|---|
| app load → live · WS | `page.goto` to `#conn.live`: socket up, `watch`, `health`, MNG-9's whole summary walk (`limit 500`, painted on MFE-9's doubling schedule) | the same, on REST |
| board first rows painted | navigation start (the page's own time origin) to the first `tbody#board tr[data-ticket-id]`, polled per frame from an init script as MFE-9-numbers.js does; recorded even if live never comes. Its detail is what the page holds at live with the board open: DOM elements (`getElementsByTagName('*')`), DOM nodes with text (CDP `Nodes`), board rows mounted, JS heap | *new*; replaces "board render (all rows)", which waited for N `<tr>`s |
| nav shortcodes visible | every epic's group row present (group rows are always mounted) | unchanged |
| board scroll to the last ticket | from setting the content column's `scrollTop` to its `scrollHeight` until the last ticket in (epic, seq) order — the paged walk's last id, or the probe this run last created — is mounted and inside the column's viewport (MFE-12 criterion 4, at every magnitude) | *new* |
| open ticket page (from board), ticket page cold deep link | unchanged | unchanged |
| created ticket appears (board + open nav) | the board scrolled to its end and the nav opened on the store's last epic and its scope root — untimed: the user looking where the ticket will land — then a ticket created in that epic with curl, so it sorts last on the board and hangs under the root (MNG-14); timed from the create call until its row is on the board **and** its leaf in the nav | a `PROBE` ticket, waited for as a board row and a nav leaf anywhere in the DOM — which neither the window nor a closed group holds |
| expand largest nav group (every ticket) | a click on the largest group's row, then on the toggle of each child that has children — under MNG-14 the epic's scope root holds every other ticket — until every ticket of the epic is mounted and the first is rendered; timed from the first click; detail: the group click alone, elements after. Last in each run: it is the one measurement that can take the page down. Its clicks get the browser budget: the click that opens the root does not return until every leaf is mounted (the first 10⁶ pass recorded the harness's own 5 s click timeout instead — kept as `raw/1e6-first-pass.json`) | one click on the group row, waiting for leaves that were already mounted behind `display: none` — a reflow, not a mount |

At a glance, "board DOM nodes" becomes **page DOM elements at live, board
open** and **board rows mounted at live**. A failed browser row now carries
the page's own errors in its reason.

### What it found

**The render is O(window).** From 10² to 10⁶ the page holds 1,734 →
4,058 elements at live with the board open (MFE-9 measured 240,359 at 10⁴),
the board mounts 125 rows at any magnitude past 10², and the first rows are
on screen in 190–210 ms flat from 10³ to 10⁶. Scrolling to the last ticket is ~40 ms at every magnitude from
10³ — at 10⁶ through the scaled layout: 10⁶ rows at 37.8 px is 37.8 M px,
past where Chromium stops laying out (2²⁵ = 33,554,432 px; Firefox near
17.9 M), so above 15 M px (~395,000 rows) the board is drawn at exactly
15 M px with the scroll range mapped linearly onto the store. The ticket
page, from the board or cold, stays flat at 40–170 ms.

**What is left is linear, and it is not the render.** Load → live is
0.95 s, 4.5 s and 51 s at 10⁴, 10⁵, 10⁶ — ×10 per decade — and nav
shortcodes visible tracks it to the millisecond, because both wait for the
same walk: every one of N summaries over the socket, decoded in wasm,
merged into the row store, with the nav's forest (`plan`) rebuilt once per
paint. At 10⁵ the page holds 2,850 elements while it waits. That is
SCALE-7's evidence, as SCALE-2 asked to record it: 10⁵ and 10⁶ both reach
live now, and what costs N is the walk and the row store, not the DOM.

**Two view costs still grow with N.** A *created* ticket appears in 44 ms
at 10⁴, 223 ms at 10⁵ and 2.0 s at 10⁶: an insert changes the row list,
and the nav rebuilds its whole forest and recounts every group's rollup —
O(N log N) per insert (a status change does not: it never fires the list).
And *expanding* a group is linear in the group: 354 ms for MAP's 2,328
tickets at 10⁴, 2.5 s for 19,195 at 10⁵. Mount-on-expand bounds a closed
node, not an open one — at 10⁶ MAP holds 163,531 tickets, opening its scope
root mounts all of them at once, and the wasm aborts (`RuntimeError:
unreachable`) the way the board did at ~64,000 rows before MFE-12: the
row's FAIL. A windowed tree (an open node's children mounted by viewport,
like the board) or SCALE-4's children by page is what bounds it; both are
past this ticket's design, which mounts every child of an open node.

### Before and after, this session

MFE-9-numbers.js, `trunk build --release`, 1400×950, medians of 3 (the
10⁵ stall is one run with a 150 s budget, `plans/MFE/artifacts/MFE-12/`):

| | 10⁴ before | 10⁴ after | 10⁵ before | 10⁵ after |
|---|---:|---:|---:|---:|
| first board row | 321 ms | 187 ms | 366 ms | 343 ms |
| live · WS | 4.13 s | 660 ms | never — stalled at 64,000 rows, `RuntimeError: unreachable` | 4.67 s |
| page DOM elements | 240,439 | 2,763 | 1,586,199 at the stall | 3,250 |
| expand MAP (group + root, every leaf) | 1.15–1.21 s | 0.34–0.39 s | — | 2.5–2.8 s (19,195 leaves) |

`plans/MFE/artifacts/MFE-12-window.js` (33 checks: the window, the scroll
to the end, j/k past the window, the palette, the menu, live create and
status change, the nav's expand and collapse, ARIA) passes 33/33 at 10⁴
and at 10⁵. The ladder's own load → live at 10⁴ (946 ms) is timed from
Playwright's `goto` call rather than the page's time origin, on a 900 px
viewport, in a browser that has run the smaller magnitudes;
MFE-9-numbers.js is the measurement MFE-12's criterion names.
