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
