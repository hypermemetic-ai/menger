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
