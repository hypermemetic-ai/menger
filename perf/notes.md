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

## UI-8 — v2's first pages on the engine (2026-10-29)

`node perf/pages-probe.mjs --seed N` (release, 1440×900, a warm-up load then
five cold contexts): navigation start to the first `.board-row` showing its
title from `list`, and the engine's wasm memory a second after. The pages
are UI-8's components in the store, on UI-6/UI-10's engine; the bar is v1's
board first rows at 10⁶ (0.22 s, above) and UI-8's budget, 0.3 s and 10 MB.

| N | v2 · board first rows painted | wasm memory · rows held | v1 (MFE-12, above) |
|---|---|---|---|
| 10⁴ | 0.198 s (213, 198, 198, 190, 197 ms) | 5.9 MiB · 200 | 172–225 ms band |
| 10⁵ | 0.243 s (248, 244, 243, 226, 232 ms) | 6.8 MiB · 200 | 172–225 ms band |
| 10⁶ | 0.218 s (226, 208, 219, 215, 218 ms) | 9.3 MiB · 200 | 0.22 s · 5.0 MiB |

The page asks the same few things at every N — `ui.watch`, `nav extents`,
`nav groups`, `nav counts`, one `list` of 200, the palette's empty `find`
and `search` — so the first rows hold flat across the decades. Two engine
changes bought ~60 ms (287 → 224 ms at 10⁵): the components' files fetched
at once (22 sequential requests before), and the route drawn before
`ui.slots.list` answers. The rows held are 200 at every N (a window); what
grows the memory with N is not measured apart here — the nav's shortcode
rows and the extents grow with N (101 shortcodes at 10⁵).

## The first row on the wire — `live-probe.mjs` (FRAME-16, 2026-10-29)

`node perf/live-probe.mjs --token-file FILE` against app-test (976 tickets, release
20261029T220735Z-68c5c3c), 1440×900, a warm-up then five cold contexts signed in by cookie:

| where | first titled row | TTFB | the socket opens | on the wire |
|---|---|---|---|---|
| app-test, from this machine | 0.939 s (1032, 930, 907, 991, 939 ms) | 76 ms | +688 ms, after the wasm instantiated | engine.js 7 KiB, engine.wasm 559 KiB (gzip of 1,868 KiB), manifest 1 KiB, 22 component files 12 KiB |

The chain is serial: the HTML, then engine.js, then the wasm, then the socket, then the manifest,
then the components, then the board's calls. The local probe (0.218 s at 10⁶) sees none of it.
FRAME-16 makes the boot parallel; this row is its "before".

### Before and after, one hour (FRAME-16's hand gate)

`node perf/live-probe.mjs --token-file FILE`, three runs of five cold loads each, the medians;
the marks on the page's own clock (the socket's real `open`, the wasm instantiated). "Before"
is app-test on release 20261029T220735Z-68c5c3c, 2026-10-30 00:10Z, from this machine.

| | first titled row (three runs' medians) | TTFB | socket open · wasm instantiated | socket first | requests by kind |
|---|---|---|---|---|---|
| before | 0.886 s (886, 888, 884 ms) | 55–62 ms | +256–263 ms · +199–209 ms | 0 of 15 | engine.js 1, host.js 1, engine.wasm 1 (578 KiB gzip), manifest 1, components 22 |
| after | 0.448 s (448, 504, 425 ms) | 60–64 ms | +160–187 ms · +171–195 ms | 9 of 15 | engine.js 1, host.js 1, engine.wasm 1 (441 KiB brotli of 1,927 KiB), manifest 1, components 1 (5 KiB brotli of 61 KiB) |

On "before" the boot's last file (the components) is done near +350 ms and the board's `list`
leaves at +480–570 ms (scratch breakdown, two loads): what the parallel boot can take off is the
socket → manifest → components chain after the wasm (~150 ms); what follows the components is the
mount and the calls.

"After" is app-test on release 20261030T001645Z-ea9ce72, 2026-10-30 00:28Z, the same hour, the
same machine: the first titled row halves (0.886 s → 0.448 s). The socket now opens at +160–187 ms
instead of +256 ms and the wasm instantiates at +171–195 ms: the two race, and the socket wins 9 of
15. Every boot file leaves at +63–69 ms (the preload links) and the last of them is back by +141–187
ms; the components are one request of 5 KiB instead of 22. The tail is wider (two loads of 15 at
0.9 s, both with the wasm instantiated late at +434–468 ms: the network, not the boot). What is
left between +190 ms and the row at +450 ms is the mount and the board's calls — the next ticket.

## v2's service at 10⁶ — the server probes on the v2 tip (57d1179, release build, 2026-10-30 00:31Z)

A throwaway seeded a million tickets (`--auth-provider key`, the probes signed in by a session file,
`MENGER_SESSION_FILE`), medians of five, on this machine. v2 cut nothing of the tickets service, and
the numbers say so: every filter read is the transport's floor.

| probe | at 10⁶ |
|---|---|
| `list { summary, limit 200 }` (the floor) | 4.0 ms |
| the nine filters' five reads (`counts`, head page, `after`, `at` half, `facets`) — the slowest | 5.8 ms (`not kind doc`, 1,000,000 matches; `facets`) |
| the slowest first tree page of the nine | 2.6 ms |
| `under X`: `counts`, `list ready`, `tree root` | 1.0, 0.8, 0.2 ms |
| `find "audit"` unfiltered (49,935 matches) · `"the"` (571,678) · `"zzqx"` (0) | 195 · 192 · 188 ms |
| `find` under a filter admitting 18 % · 1 % | 77 · 7 ms |
| `member` of 1,000 ids, empty filter | 10.9 ms |

`find` is the one read that costs at 10⁶: ~190 ms whatever the query, the FTS5 walk over every
row's title (it is proportional to the filter's admits: 77 ms at 18 %). The `ready-probe pages`
run failed on its arguments (`ready_set after 100: after: no ticket 100` — the seed's ids are not
numbers); its page numbers are in the ladders above.

## The board in both profiles — `pages-probe.mjs --path` (FRAME-14, 2026-10-30)

`node perf/pages-probe.mjs --no-build --seed N [--path '/app/#/sv/']`, release build, this machine,
1440×900, a warm-up then five cold loads each; the medians. rhx is `/app/` (the engine's rhx/1
components), Svelte is `/app/#/sv/` (svelte/1 — real Svelte 5.57.2 — in the rhx frame
board-frame.rhx, which only places its sections). wasm is the engine's linear memory a second
after the first rows; JS heap is the page's JS heap in use then, after a collection (CDP
`Runtime.getHeapUsage`, the wasm's memory apart); created row is one ticket made by synapse after
that, its `created` frame to its `.board-row`.

| N | board | first titled row (five runs) | wasm | JS heap | rows held | created row |
|---|---|---|---|---|---|---|
| 10⁴ | rhx | 0.182 s (186, 182, 182, 186, 177 ms) | 5.8 MiB | 1.6 MiB | 200 | 13.9 ms |
| 10⁴ | Svelte | 0.128 s (128, 126, 127, 134, 132 ms) | 3.3 MiB | 2.4 MiB | 200 | 7.4 ms |
| 10⁵ | rhx | 0.207 s (207, 227, 198, 202, 208 ms) | 6.7 MiB | 1.6 MiB | 200 | 13.8 ms |
| 10⁵ | Svelte | 0.142 s (142, 151, 137, 136, 158 ms) | 3.6 MiB | 2.7 MiB | 200 | 9.3 ms |
| 10⁶ | rhx | 0.202 s (197, 193, 279, 202, 256 ms) | 9.3 MiB | 1.7 MiB | 200 | 23.8 ms |
| 10⁶ | Svelte | 0.149 s (140, 181, 167, 149, 142 ms) | 4.0 MiB | 3.2 MiB | 200 | 7.8 ms |

At a million the Svelte board draws its first row about 50 ms sooner, holds 7.2 MiB in all against
rhx's 11.0 (the rows' DOM bindings live in the JS heap instead of the wasm), and puts a created row on
screen in a third of the time. Both stay within UI-8's 0.3 s / 10 MB. The collections themselves are
the engine's on both boards (one cache, in the wasm): every change crosses to JS as the whole
collection, JSON (FRAME-20 would make it the change). Locally the five Svelte modules are free; on the
wire they are five more files (54–75 KB each, each with its own Svelte runtime; 20–28 KB gzipped,
116 KB for the five) — the live probe on app-test is where that shows.

## The Svelte board on FRAME-19's bus — `pages-probe.mjs --runs 15` (FRAME-25, 2026-10-30)

The five `svelte/1` components rewritten on the bus (`when`/`debounce_ms` in the host's registry,
`markdown` from `plexus-ui/host`, `nav-flat-sv`, `nav-children-sv` and `ticket-chip-sv` imported as
siblings) with plexus-ui 633f776's engine. `node perf/pages-probe.mjs --seed N --runs 15 [--path
'/app/#/sv/']`, release build, this machine, 1440×900, a warm-up then fifteen cold loads each; the
medians; the load average (1 min) when each began, another builder's gates running beside it.
Modules is the `components/<name>.js` files a load fetched (the probe's new line): the route's four
sections on `#/sv/` (the siblings travel inside `nav-tree-sv` and `ticket-page-sv`), none on `/`.

| N | board | load | first titled row (15 runs) | wasm | JS heap | rows held | created row | modules |
|---|---|---|---|---|---|---|---|---|
| 10⁴ | Svelte | 10.9 | 0.145 s | 4.0 MiB | 2.4 MiB | 200 | 8.8 ms | 4 |
| 10⁴ | rhx | 10.2 | 0.145 s | 5.9 MiB | 1.3 MiB | 200 | 8.3 ms | 0 |
| 10⁵ | Svelte | 7.3 | 0.138 s | 4.4 MiB | 2.6 MiB | 200 | 8.9 ms | 4 |
| 10⁵ | rhx | 4.4 | 0.165 s | 6.9 MiB | 1.3 MiB | 200 | 8.6 ms | 0 |
| 10⁶ | Svelte | 3.3 | 0.187 s | 5.6 MiB | 3.2 MiB | 200 | 8.9 ms | 4 |
| 10⁶ | rhx | 2.3 | 0.223 s | 9.4 MiB | 1.4 MiB | 200 | 8.3 ms | 0 |

Before and after in one hour, 10⁶, the Svelte board with v2's five (4c919be) and then these:

| 10⁶ | load | first titled row | list sent → answered | created row |
|---|---|---|---|---|
| Svelte, before (v2's five) | 2.1 | 0.191 s | +75 → +185 ms | 13.8 ms |
| Svelte, after | 3.8 | 0.188 s | +75 → +181 ms | 10.7 ms |
| rhx, the same minute | 2.0 | 0.181 s | +49 → +161 ms | 7.6 ms |

The rewrite is within noise of the board it replaces: the same first call (+68–69 ms: a module's
specs are not sent early), the same four modules, the same memory; the list's answer, not the
components, sets the first row. Both boards are ~40 ms slower at 10⁶ than FRAME-14's table on this
machine today (rhx 0.181–0.223 s against 0.202, Svelte 0.187–0.191 against 0.149) — the list's answer
comes later on both, so the machine's hour, not the change. Every created row lands in 8–11 ms on
both boards.

The bundle's Svelte modules (raw / gzip -9; app-test serves brotli, which this machine lacks):

| module | before | after |
|---|---|---|
| board-layout-sv | 54,686 / 20,377 | 54,665 / 20,361 |
| board-rows-sv | 62,637 / 23,283 | 61,901 / 23,000 |
| jump-palette-sv | 61,251 / 22,661 | 61,140 / 22,589 |
| nav-tree-sv (with nav-flat-sv, nav-children-sv inside) | 62,067 / 22,946 | 61,389 / 22,671 |
| ticket-page-sv (with ticket-chip-sv inside) | 75,343 / 28,039 | 62,732 / 23,173 |
| nav-flat-sv, nav-children-sv, ticket-chip-sv (their own modules) | — | 162,743 / 60,394 |
| the five the board loads | 315,984 / 117,306 | 301,827 / 111,794 |
| all of them | 315,984 / 117,306 | 464,570 / 172,188 |

A load of `#/sv/` fetches about the same (the four sections' modules, 89.3 KB gzip → 88.6); a
ticket page 4.9 KB gzip less (28.0 → 23.2: the copied renderer and allow-list gone, the chip
inlined). The bundle grows by the three siblings' own modules — each a
Svelte runtime of ~50 KB raw, fetched only when a right-click puts one in a section (FRAME-26: a
shared runtime).

## The first row after the boot — `live-probe.mjs`'s table (FRAME-17, 2026-10-30)

`node perf/live-probe.mjs --token-file FILE --runs 5`, app-test (976 tickets), cold loads, the
page's clock. **Before** — release 20261030T001645Z-ea9ce72 (FRAME-16's after), 2026-10-30 ~00:45Z:

| step | median | runs |
|---|---|---|
| the boot done (socket open, wasm instantiated) | +203 ms | 188, 269, 453, 203, 178 |
| the first call, `tickets.list` sent | +242 ms | 235, 313, 495, 242, 217 |
| `tickets.list` first data | +343 ms | 343, 409, 573, 333, 312 |
| `tickets.list` done | +370 ms | 370, 440, 612, 362, 358 |
| the nav drawn | +324 ms | |
| the first titled row (a MutationObserver's stamp; Playwright's poll read 443) | +408 ms | 408, 476, 651, 398, 395 |

Where the 200 ms "mount" of FRAME-16's breakdown went (a marked engine, a local throwaway at 10⁴,
five loads): the boot to the mount 4 ms; the mount 36 ms (the first tree's parse 14 ms — every
template expression compiled three times, the first time with Rhai's engine made —, the rest 2–7 ms
a component, handlers compiled only for the mounted five of eleven); every call left together at
the mount's end (`WsWire::open` sent from a spawned future); the list's answer 39 ms, its 200
frames 30 ms, the fold and the draw 42 ms (a profile: converting each row to Rhai's value once per
expression 19 of 63 ms profiled, the DOM about 9). The breakdown's ~200 ms was its own
instrumentation.

The server's share, on the network's clock (CDP `Network.webSocketFrameReceived`, three loads): the
nine calls leave within 2 ms; their acks come back in bursts — at ~20, ~60–80 and ~160 ms, several
in the same millisecond — not in the order sent, and the list waited 58, 165 and 160 ms for its
ack, then 54–244 ms more to its `done`. Not one call after another, but held and released together
(PLEXUS-10); the 200 frames' share is PERF-15's.

### After, on app-test (release 20261030T015638Z-e75a127, 2026-10-30 02:11Z, load ~2, both boards)

`node perf/live-probe.mjs --path / | '/#/sv/'`, three runs of five cold loads each, after deploy 21
(FRAME-17) with the session minted before the deploy still signed in (FRAME-21). The before was
measured on release a45dbdb at 01:56Z (the machine at load 9–10, so its medians are noisy:
885, 459, 476 ms on the rhx board).

| board | first titled row (three runs' medians) | first `live` snapshot sent | ack wait | stream | done → row | components |
|---|---|---|---|---|---|---|
| rhx `/` | 0.366, 0.343, 0.435 s | +178–233 ms | 33–36 ms | 66–91 ms | 19–22 ms | 1 req, 5 KiB |
| Svelte `#/sv/` | 0.384, 0.418, 0.869 s | +253–307 ms | 51–66 ms | 31–35 ms | 7–8 ms | 5 req, 84 KiB |

The rhx board: the list leaves ~85 ms earlier than before FRAME-17 (+180 instead of +265), its
ack waits a third of what it did (it goes first), the draw is 19 ms (was 37), and the components
are one request. Its stream grew (66–91 ms, was 25–45): the list is first now and its 200 frames
interleave with the other eight calls' answers — PERF-15's share got larger, not smaller, by
moving ahead of the queue.

The Svelte board: the draw is 7–8 ms (Svelte's compiled DOM against rhx's interpreted tree), but
its first call leaves ~75 ms later than the rhx board's: its five modules (84 KiB brotli, each
with its own Svelte runtime) must import before the frame's sections mount and their `live` runs,
and a module's specs cannot be sent early (FRAME-17 derives only rhx specs; FRAME-23 does not
change that). Five of its fifteen loads hit ~0.9 s against one of fifteen on rhx; in those the
module fetches ended at +550 ms instead of +200. From curl the same five files answer in 66–95 ms
each, five at once (brotli, served) — the delay is not the server's; it tracks the client and the
network's hour, and the Svelte board has five more things to wait for. A shared runtime file
(one fetch, cached across versions) would shrink the 84 KiB to ~25 and the five requests to one.

Net, on this machine to the Linode, same hour: rhx 0.34–0.44 s, Svelte 0.38–0.42 s in its fast
mode with a fatter tail. The owner's call for the default (FRAME-14).

## A live change crosses as the change — `pages-probe.mjs --runs 15` (FRAME-20, 2026-10-30)

Locally, release build, 15 cold loads after a warm-up, both boards; **before** is plexus-ui main
(567945c + the README's 633f776) with v2's `ui/`, **after** is plexus-ui FRAME-20 (0dac48b) — built
with `PLEXUS_UI_SRC` pointing at that worktree, the tool too (`scripts/plexus-ui` builds it from
there: the svelte/1 face is compiled into the tool) — with the Svelte board's rows and nav groups
iterated through the handle's getter (`rows.rows`, `groups.rows`; `$rows.rows` compiles to an each
with mutable item sources, every row's effects re-run). Created row: its `created` frame to its
`.board-row`, min / median / max and the interquartile range, ms.

| N | board | | created row min / median / max (IQR) | first row | wasm | JS heap | load (1 min) |
|---|---|---|---|---|---|---|---|
| 10⁵ | rhx | before | 7.0 / 8.1 / 12.9 (4.0) | 0.166 s | 6.9 MiB | 1.3 MiB | 2.8 |
| 10⁵ | Svelte | before | 6.1 / 9.9 / 11.4 (2.5) | 0.135 s | 4.3 MiB | 2.7 MiB | 2.3 |
| 10⁶ | rhx | before | 6.6 / 7.5 / 14.3 (3.5) | 0.220 s | 9.4 MiB | 1.4 MiB | 1.7 |
| 10⁶ | Svelte | before | 5.7 / 15.1 / 24.6 (8.0) | 0.218 s | 5.3 MiB | 3.2 MiB | 2.4 → 11.1 |
| 10⁵ | rhx | after | 7.1 / 8.0 / 9.8 (1.1) | 0.169 s | 6.9 MiB | 1.3 MiB | 1.9 |
| 10⁵ | Svelte | after | 4.4 / 6.8 / 8.0 (1.4) | 0.141 s | 4.3 MiB | 2.7 MiB | 2.6 |
| 10⁶ | rhx | after | 6.7 / 10.3 / 16.3 (6.4) | 0.192 s | 9.4 MiB | 1.4 MiB | 2.1 |
| 10⁶ | Svelte | after | 3.4 / 6.7 / 9.8 (2.8) | 0.187 s | 5.3 MiB | 3.3 MiB | 2.0 |

(Before's Svelte 10⁶ ran while the machine's load climbed to 11; another builder's probe ran beside
the after runs.) **The acceptance** (review of FRAME-20's phase 1): the Svelte board passes when its
median is within rhx's median plus the larger IQR — at 10⁶ after, 6.7 ms against rhx's 10.3 ms
(+6.4): within, and below rhx's median; at 10⁵ 6.8 against 8.0. The first row, wasm and JS heap
are unchanged: the first page still crosses once, whole.

**What crosses per change** (phase 0's instrument: busOf's `live` wrapped, every `JSON.parse` of a
delivery counted; the Svelte board at 10⁵, one `created`, one `status_changed` of a row on screen):

| change | before | after |
|---|---|---|
| `created` | 3 collections whole: `tickets.list` 201 rows 71,785 B, `nav.groups` 102 rows 21,584 B, `nav.extents` 102 rows 5,326 B — 98,695 B, every row object new | the three moves, one row each: 371 + 371 + 231 = 973 B; 200 of 201, 101 of 102, 101 of 102 row objects kept |
| `status_changed` | `tickets.list` whole, 72,361 B, 0 of 201 kept | 984 B (the row), 200 of 201 kept |

New row objects and row renders per change are 1 each (the framework's smoke,
`a_created_row_is_one_new_object`: 87 bytes, A-201 alone rendered; a `status_changed` 84 bytes,
A-7 alone). The JS heap over 100 changes after a GC: the first hundred +250–300 KiB on both
before and after (warming, once), the second +45 KiB before, +23–60 KiB after; rhx +6–10 KiB.
