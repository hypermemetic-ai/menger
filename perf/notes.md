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

## What SCALE-26 changed (re-run, branch `scale-26-board-by-window`)

The section above is MFE-19, the ladder after MFE-12. Since then SCALE-24
put the nav on the server's roll-ups and children by page, SCALE-25 the
jump palette on `find`, and SCALE-26 the board on windows: `extents` (each
shortcode's last sequence — sequences are dense, so row k of the board is a
ticket id by arithmetic) and one `list { after, limit, summary }` for the
rows a window shows. The session's walk of every summary is gone; the app
asks for what it shows and holds only that. This is the ladder's first run
on that app: `--date 2026-10-01-scale26`, the same machine, server unchanged
since SCALE-25 (`467bc01`; the header's "+ uncommitted changes" is the
SCALE-26 frontend and this harness — the code committed, but for doc
comments, these notes and keyboard fixes no ladder row exercises: a step
taken while the board's window holds no fetched row yet, the scroll to a row
measured with the board's pitch rather than the first frame's guess, and a
focused row put back in two mutation batches — `src/keys.rs`, `src/main.rs`).

### The browser rows, redefined again

| row | measures now | was (MFE-12) |
|---|---|---|
| app load → live · WS | `page.goto` to `#conn.live`. `live` is now the socket up, `watch` subscribed, `health`, `groups`, `extents` and the board's one window answered — five requests at any N. Detail: the WebSocket frames the page sent while the dock did not read `live`, counted in the page (`WebSocket.prototype.send`, as `plans/MFE/artifacts/SCALE-26-board.js` does) | the same goto, but `live` came after MNG-9's whole summary walk (2,000 pages at 10⁶) |
| board first rows painted | unchanged; detail adds the rows the client holds (`#app[data-rows-held]`, the row cache's size) and the wasm module's linear memory (trunk's `TrunkApplicationStarted` hands the page the module's exports) | the same, without them — there was nothing to say: every row was held |
| board scroll to the last ticket | unchanged definition, but no longer a re-render of rows in memory: the window at the end is asked for (one page) and drawn. Detail adds the rows held after | the walk had every row in hand |
| created ticket appears (board + open nav) | as before, plus one untimed step: the nav, open on the last epic's root, is scrolled to its end — since SCALE-24 an open node of more than a page is a window, and a birth at its end is mounted only where its end is shown (SCALE-24-nav.js's `created_open`) | the nav's open node mounted every child |
| expand largest nav group | a click on the group's row and on its root's toggle, timed to the first of the root's children rendered; then the open list scrolled to its end until its last child — `aria-posinset` = `aria-setsize` — is on screen (detail: how long, and of how many) | until every ticket of the epic was mounted — which SCALE-24's window never does by design (MFE-19: 2.51 s at 10⁵, the wasm aborted at 10⁶) |

Not a browser row, but fixed on the way: the sample the `REST blockers` row
needs is scanned out of a streamed `list` with a pattern of the ticket's
keys in order, and SDLC-9's `commits` (between `child_count` and
`created_unix`) broke it — the row read "no multi-blocker sample" at every
magnitude. The pattern now allows it.

### What it found

**No browser row grows more than ×1.5 over four decades, and none fails at
10⁶.** Load → live is 181 ms, 198 ms, 201 ms, 223 ms and 263 ms from 10² to
10⁶ (MFE-19: 168 ms → 50.75 s); the page sent 5 frames before `live` at
every magnitude (`watch`, `health`, `groups`, `extents`, `list`) where the
unchanged app sent 4 at 10², 23 at 10⁴, 203 at 10⁵ and 2,003 at 10⁶ — a
page of 500 per 500 tickets (SCALE-26-board.js on the unchanged build,
below). The first board rows are on screen in 172–225 ms, at `live` or a
frame before it. The rows the client holds at live are the
window, 125 at a 900 px viewport, at every N; the wasm module's memory at
live is 2.6 MiB at 10² and 5.0 MiB at 10⁶ (the walk's app at 10⁶: 682 MB,
measured by SCALE-26-board.js on the unchanged build). What still grows a
little is O(shortcodes), not O(N): 7 → 252 shortcode rows in the nav, 1,938
→ 4,308 elements at live, and the `groups`/`extents` answers.

**Every row that used to wait for the walk is flat.** Nav shortcodes
visible 198–280 ms (MFE-19: 50.77 s at 10⁶). A created ticket appears on
the board and in the open nav in 23–54 ms (2.03 s), because nothing
rebuilds over every row and the board holds the stretch it lands in.
Scrolling to the last ticket is 59–69 ms from 10³ up — the page at the end
asked for and drawn — and the board then holds the window it shows (126 at
10⁴, 125 at 10⁶; it gives back what scrolled more than 1,000 rows away).
The largest group opens in 43–118 ms to its first children, and the last of
MAP's 163,530 children is on screen 54 ms after a scroll to the list's end;
the page holds 5,779 elements with it open (MFE-19: a `RuntimeError:
unreachable` at 10⁶). The ticket page, from the board or cold, stays at
34–231 ms.

**What is left is the server's, and it is not new.** `REST list` returns
413 from 10⁴ and `REST ready_set` from 10⁵ — the REST gateway's 10,000-item
collect cap (TRANSPORT-2), unchanged since the MNG-9 section above; no
browser path uses either. The WebSocket controls (a streamed `list` of
everything, a paged walk of every summary) are linear by definition and
are kept as the store's own cost.

### Before and after, the same session

`plans/MFE/artifacts/SCALE-26-board.js`, release server on
`--seed-synthetic N` (seed value 1), 1400×950, medians of 3; "before" is
the unchanged build (`467bc01`), "after" this branch. Raw output in
`plans/MFE/artifacts/SCALE-26/`.

| | 10⁴ before → after | 10⁵ before → after | 10⁶ before → after |
|---|---|---|---|
| first board rows | 194 → 211 ms | 211 → 218 ms | 232 → 220 ms |
| `live · WS` | 679 ms → 211 ms | 4.43 s → 218 ms | 43.2 s → 220 ms |
| frames sent before `live` | 23 → 5 | 203 → 5 | 2,003 → 5 |
| rows the client holds at `live` | 10,000 → 150 | 100,000 → 150 | 1,000,000 → 150 |
| most rows held, scrolled top to bottom in 100 jumps | 10,000 → 1,150 | 100,000 → 500 | 1,000,000 → 250 |
| wasm memory at `live` | 9.8 MB → 3.6 MB | 69.3 MB → 4.2 MB | 681.7 MB → 5.6 MB |
| JS heap at `live` | 3.4 → 3.5 MB | 2.2 → 4.1 MB | 2.6 → 3.1 MB |
| checks passed | 8/15 → 15/15 | 7/15 → 15/15 | 6/15 → 15/15 |

At 100 tickets the page sends the same 5 frames before `live` (before: 4 —
`watch`, `health`, `groups` and the walk's one page; `live` 147 → 177 ms).
The unchanged app held every row it had walked; the harness reads that as
its board list's length (`aria-rowcount` − 1), since it has no
`data-rows-held`. Two of its failures were not the walk's: k pressed up
across the window's top edge lost the focus (the window's `<For>` moves the
rows that stay behind the new ones, and a moved element is blurred) at every
magnitude, and at 10⁶ j lost the focus 25 rows down and began again at the
first row (the browser's scroll into view fought the scaled layout) — both
fixed here (src/keys.rs).

## What AGS-23 changed (branch `ags-23/nav-root-rows`)

AGS-23 drew a shortcode with a root as its root's own ticket row — kind
glyph, code, title, status tag and the shortcode's roll-up — where AGS-20
drew a row of the code and the roll-up whose one child was the root; and it
halved the indent. The top level still paints from `groups {}` alone: the
group carries the root's title, status and kind (`root_row`), so no `get` is
asked per shortcode, before the nav shows or after.

### The browser rows, read in either shape

| row | measures now | was |
|---|---|---|
| nav shortcodes visible | every epic's top-level row present — `#nav .tree > .node[data-group] > .row`, a shortcode row or (AGS-23) its root's row, drawn with its title and status from the group; detail `roots`: how many are root rows | every `.grouprow` present |
| created ticket appears (board + open nav) | unchanged, but "the nav opened on the last epic's scope root" is one toggle now — the root's row is the top row | a click on the shortcode row, then the root's toggle |
| expand largest nav group | one click on the root's toggle, timed to its first child rendered (`group_ms` is the same click to the first node mounted, which is that child now); then the scroll to the last child as before | the shortcode row's click (to the root mounted, its `get` answered), then the root's toggle |

The harness tells the two shapes apart by the top row (`.grouprow` or a
ticket row), so one harness measures either app: the before/after below are
the same harness on the two builds, back to back.

### Before and after, the same session

`node perf/magnitude.mjs --magnitudes 4,5 --runs 5 --no-build`, before (the
unchanged build, `78a6010`) then after, medians of 5:

| | 10⁴ before → after | 10⁵ before → after |
|---|---|---|
| app load → live · WS | 195 → 205 ms | 211 → 221 ms |
| nav shortcodes visible | 212 → 221 ms | 228 → 239 ms |
| nav visible − live (the harness's own step) | 16.8 → 17.2 ms | 16.4 → 17.3 ms |
| created ticket appears | 34 → 35 ms | 48 → 32 ms |
| expand largest nav group | 106 → 52 ms | 99 → 79 ms |
| ticket page, cold deep link | 159 → 157 ms | 166 → 168 ms |

**Nav shortcodes visible is ~10 ms later (4–5 %), and all of it is load →
live.** The harness polls for the rows after `live`, so this row is `live`
plus its own step; the nav paints before `live`. Measured in the page
(`plans/AGS/artifacts/AGS-23/ags23-paint.mjs`: an observer installed before
the app, the two builds interleaved, 21 loads each, 10⁵ — 101 top rows):
the `groups` answer arrives at the same moment (113.5 → 115.1 ms), the top
rows are in the DOM at 128.8 → 134.9 ms and `live` at 175.7 → 182.6 ms. The
old app against the new server is +2 ms (the larger `groups` answer to
decode); a trace of the main thread to `live` puts the rest in paint
(+2.0 ms: titles and status tags on screen where codes were), script
(+3.1 ms: a ticket row is more to build than a code chip) and style and
layout (+0.9 ms). That is the cost of drawing at first paint what the
ticket asks the top row to show; none of it is network, and it is per
drawn shortcode (O(shortcodes), 101 rows here). Tried and kept: the top
level built from a keyed list of nodes rather than a reactive wrapper per
shortcode, a row's kind read once (it never changes), list-only attributes
only on rows in a list, the tree's memos reading the groups in place. Tried
and dropped: `content-visibility` on rows (no change: the cost is before
layout) and asking `groups` and `extents` together (worse: the two streams
interleave and the groups arrive later).

**Opening the largest shortcode is up to twice as fast**: its top row is the
root, so opening it is one page of the root's children; the shortcode row
used to `get` the root first.
