# menger 0.05

A plan is a program whose leaves are prose.

0.05 moves menger onto Plexus RPC and Rust. The 0.04 rule carries over:
**frontend and backend are online from the first commit** — every noun added
is visible the day it is written. What's new is *how* the frontend stays
current: 0.04 polled `/rev`; 0.05 subscribes. The same typed event streams
that answer RPC calls push every mutation to the browser as it happens —
over one WebSocket, for the vanilla page and for the Leptos app alike
(MFE-9). The REST + SSE bridge stays for curl, scripts and non-Rust
clients; the app does not use it.

## Run

```sh
cargo run                     # the app http://127.0.0.1:4456/, WebSocket :4455,
                              # REST :4457, day-one page /vanilla
cargo run -- --stdio          # MCP-compatible stdio mode
cargo test
```

`http://127.0.0.1:4456/` is the Leptos app (MFE-4). Until you build the
frontend it serves the day-one vanilla page instead — the binary alone
always serves something live — and that page keeps its own URL,
`/vanilla`, once the app is built. `/app/` is the app's asset root and
the deep-link prefix (`/app/#/ticket/MNG-1`); it serves the app too.

Open the app in two browser windows; create a ticket or flip a status in
one and watch the other update. No polling — the server pushes.

### Synthetic seeding (PERF-2)

For measurement, not for real data: `--seed-synthetic N` (env
`MENGER_SEED_SYNTHETIC`) bulk-seeds N synthetic tickets into the database
before serving — through the store's cache layer, in a few large
transactions rather than one fsync per row — and then serves them like
any other tickets. The shape is realistic (several Zipf-sized epics, ids
allocated the normal way, every status and kind, a short markdown body,
about a quarter of the tickets carrying valid acyclic `blocked_by` edges
with `unlocks` mirrored), and deterministic: the same `--seed-value`
(`MENGER_SEED_VALUE`, default 1) produces byte-identical tickets. The
startup log carries the count and the elapsed time. A database that
already holds tickets **refuses** the seed and the server exits; pass
`--seed-into-existing` (`MENGER_SEED_INTO_EXISTING=1`) to add anyway,
with ids continuing after the existing ones. No new RPC method: the wire
surface is unchanged.

```sh
cargo run --release -- --db-path /tmp/scratch.db --seed-synthetic 100000 \
    --port 4470 --shell-port 4471 --rest-port 4472
```

The magnitude ladder in `perf/` (PERF-3) drives this option per
magnitude on temp databases and ephemeral ports.

## Frontend (the Leptos app)

The MFE epic's browser client lives in `frontend/` — a **separate cargo
unit** (Leptos 0.8, CSR only, built with trunk). The server never needs
it to compile: `cargo build` / `cargo test` work with no wasm toolchain
installed. Until you build it, `/` serves the vanilla page and
`http://127.0.0.1:4456/app/` answers a plain "frontend not built" page
(HTTP 200 with build instructions, never a 500).

One-time toolchain (steps that worked on this machine):

```sh
rustup target add wasm32-unknown-unknown
# trunk, prebuilt via cargo-binstall — preferred here because
# `cargo install trunk --locked` fails to compile on GCC 16
# (libdeflate-sys 1.23 uses the removed `evex512` target attribute):
curl -L --proto '=https' --tlsv1.2 -sSf \
  https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
cargo binstall -y trunk
```

Build and run:

```sh
cd frontend && trunk build --release   # production build -> frontend/dist/
cargo run                              # serves it at http://127.0.0.1:4456/
```

Dev loop: keep `cargo run` up and run `cd frontend && trunk watch` — it
rebuilds `dist/` on every source change; reload the browser to pick it
up (the server reads `dist/` from disk per request, no restart needed).
`--app-dist DIR` / `MENGER_APP_DIST` point the server at a `dist`
elsewhere.

Diagrams (AGS-14): a ```mermaid fence in a ticket body renders as SVG on
the ticket page. The renderer is mermaid 11.17.2 (MIT), **vendored** at
`frontend/vendor/mermaid/` (version, checksum, license, and upgrade steps
in its README) and copied by trunk into `dist/vendor/`, so the shell
serves it at `/app/vendor/mermaid-11.17.2.min.js` like any other asset —
the app still talks to nothing but menger. It is **lazy**:
`frontend/src/md.rs` turns each fence into an inert placeholder holding
the escaped source, and `frontend/src/mermaid.rs` injects the `<script>`
only the first time a rendered body holds one (once per page load, shared
by every diagram on it), then drives `mermaid.render` under
`securityLevel: 'strict'` — no HTML labels or click handlers from diagram
source, and the key is in mermaid's `secure` list, so a `%%{init}%%`
directive cannot loosen it. A diagram that fails to parse shows the error
and its source in place; the rest of the page is unaffected.
`RUST_LOG=menger=debug` logs one line per served asset, which is how to
see that a diagram-free page never pulls the bundle.

Data plane (MFE-1 pins: REST for calls, SSE for `watch`; the view layer
owns no routing):

- The REST port (:4457) emits **no CORS headers**, so the app never
  calls it cross-origin. Instead the shell server hosts a same-origin
  reverse proxy: `/rest/{*}` on :4456 forwards verbatim to :4457. The
  REST surface stays the contract; the proxy adds no data path — its one
  liberty is prepending an ignorable SSE comment line (`:`) to
  `text/event-stream` responses, because Firefox does not resolve a
  `fetch()` until the first body bytes arrive and `watch` sends no
  preamble of its own.
- `tickets.watch` streams SSE **over POST**, which browser `EventSource`
  (GET-only) cannot consume — the reason the app was written against
  `fetch` + ReadableStream up to MFE-8. It no longer is: since **MFE-9**
  the app's whole data plane is one `WsTransport` on :4455 speaking
  `menger.call`, driven by the client menger's own activation
  declarations generate (`tickets-api`, the crate the server and the
  browser share). It opens `watch` first, then walks
  `list { summary: true, limit: 500 }` with `after = next` (MNG-9),
  painting as pages land, and reconnects on a ~1 s loop — a server
  restart returns the page to live on its own, with rows keeping their
  DOM identity. The port comes from `<meta name="menger-ws-port">`, which
  the shell substitutes wherever it serves the app's entrypoint (`/` and
  `/app/`), exactly as it always has for `web/index.html`.

Rendering (MFE-12): the app holds every row but mounts only what can be
seen. The board is a window — the rows on screen plus 100 either side,
between two spacer rows at the measured row pitch — so every board row is
one line (a long title ends in an ellipsis; hover shows it whole); a store
taller than a browser will lay out (about 888,000 rows in Chromium) is
drawn at a fixed height with the scroll range mapped onto it. The nav
mounts a node's children when it opens. j/k, the jump palette and the
ticket menu work from the row store, so none of them needs a row to be
mounted. At 10,000 tickets the page holds ~2,800 elements and is live in
~0.7 s; 100,000 is live in ~5 s and 1,000,000 in ~50 s — the walk of the
store is what is left.

## Surface

menger is DB-first: the database is the system of record, and the service
is CRUD over tickets plus a live subscription. One activation so far,
`tickets`. Statuses use the house ticketing vocabulary; every ticket also
carries a **kind** — `scope | execution | build | gap | spike` (AGS-13, the
owner's closed set: a scope decomposes into children, a build produces
capability, an execution carries out a procedure, a gap is a defect in
shipped behavior, a spike answers a question). A database written before
AGS-13 (`ticket_type`) is migrated in place, durably, the first time the
server opens it — `epic→scope`, `implementation→build`, `analysis→spike`,
`checkpoint→execution`, `spike→spike`; any other legacy value refuses the
open, naming the row.

A ticket's outputs are tickets (AGS-19): every ticket carries an optional
**`parent`** — the ticket it is an output of, in any epic — and a
server-derived **`children`**, the mirror the store keeps as it keeps
`unlocks` from `blocked_by`. The parent must exist at creation and the
chain is acyclic (a ticket cannot be its own ancestor; there is no
re-parent method, so the check at create is the whole story); a
nonexistent or cyclic parent is an in-band `error` naming the reason. A
child's birth updates its parent, and `watch` carries that as
`children_changed` with the full parent ticket, so a watch-only client
stays coherent. A database written before AGS-19 gains both columns on
first open, durably, with typed parents backfilled from each body's
frontmatter `parent: <id>` — one log line per ticket; an id that names no
ticket is logged and left empty, never fatal.

| method | yields |
|---|---|
| `tickets.health` | `health { version, unix_now, ticket_count }` |
| `tickets.create { epic, title, kind?, severity?, blocked_by?, body?, parent? }` | `created { ticket }` — id is the next `EPIC-N`, status starts `Pending`; `kind` defaults to `build`, one outside the five is an in-band `error` naming them; a `parent` that does not exist (or would make the ticket its own ancestor) is an in-band `error` naming the reason; **every shortcode starts with a scope** (MNG-12): the first ticket of a prefix must be `kind: scope`, anything else is an in-band `error` naming the rule and nothing is allocated; **`parent` defaults to the shortcode's scope root and must stay inside the shortcode** (MNG-14): omitted, a non-root hangs under its prefix's root; named, it must be in the same prefix, else an in-band `error` — the scope root itself is the exception, its parent (if any) is the shortcode it hangs under, a sub-shortcode |
| `tickets.get { id }` | `ticket` or `not_found` |
| `tickets.list { epic?, status?, parent?, after?, limit?, summary? }` | one `ticket` per match, ordered (epic, seq), then one `page { next, remaining }`; `parent` yields exactly that ticket's children; `summary` swaps the rows for `summary { summary }` (every field but the body); `after` is the exclusive cursor and `limit` caps the page — `after` naming no ticket is one in-band `error` |
| `tickets.ready_set { epic?, after?, limit?, summary? }` | one row per member of the ready set (Ready, every blocker Complete or Superseded), ordered (epic, seq), then one `page { next, remaining }` |
| `tickets.set_status { id, status }` | `status_changed { ticket, previous }` |
| `tickets.set_parent { id, parent }` | `parent_changed { ticket, previous }` (MNG-12) — files an existing ticket under `parent`: the child's `parent`, the new parent's `children` and the old parent's `children` (if any) move in one transaction; the same guards as `create` (`not_found`, must exist, inside the shortcode unless the ticket is its root, no self-ancestry — in-band `error` naming the reason); the parent it already has is a no-op |
| `tickets.groups {}` | one `group { group: { epic, total, done, root, host } }` per shortcode row the nav draws, in shortcode order, then the stream's own `done` (MNG-10) — the nav's tree without its rows: `total`/`done` count the row's whole subtree as AGS-20's `plan()` nests it (sub-shortcodes and cross-shortcode children included; `done` is `Complete` only), `root` is the lowest-seq `scope`, `host` the ticket a sub-shortcode's row nests under (`null` at top level). Kept by the store on every write, so the cost is the number of shortcodes, not tickets |
| `tickets.extents {}` | one `extent { epic, last_seq }` per shortcode that holds a ticket, in shortcode order (the store's), then the stream's own `done` (SCALE-16) — where each shortcode ends: ids are allocated highest-plus-one and never deleted, so a shortcode holds `last_seq` tickets, row k of it is `EPIC-k`, and a position in the store's order becomes a `list { after }` cursor without a walk; every `last_seq` together sums to `health.ticket_count` (a hand-edited database with a hole still reports the highest sequence; `after` naming the missing id is `list`'s cursor error). Not a roll-up: the shortcode's own tickets only, and every shortcode has one whether or not the nav draws it a row. Read from the sequence `create` allocates from (SCALE-3), so the cost is the number of shortcodes; no `watch` event — `created` carries the shortcode's new `last_seq` as its `seq` |
| `tickets.watch { epic? }` | open-ended stream of `created` / `status_changed` / `unlocks_changed` / `children_changed` / `parent_changed` / `child_removed` / `group_changed` — a write that moves a roll-up is followed, after its ticket events, by one `group_changed { group }` per row that moved, innermost first (a row no longer drawn arrives once with `total: 0`); `epic` filters a group by its own shortcode |

Paging (MNG-9). `(epic, seq)` is a total order the schema keeps unique, so
a ticket id is a cursor: a client that renders opens `watch` first, then
walks `list { summary: true, limit: 200 }`, passing the previous page's
`page.next` as the next call's `after`, until `next` is `null`. Each page
is `≤ limit` rows and one `page { next, remaining }`; `remaining` counts
what is left under the same filters. A client that wants everything with
bodies calls `list {}` exactly as before and ignores the trailing `page`.
The body is ~70 % of a row, and only the ticket page shows one — it comes
from `get`.

Via synapse (once on PATH — 4455 is inside the `_self scan` range):

```sh
synapse -P 4455 menger tickets health
synapse -P 4455 menger tickets create --epic MNG --title "..."
synapse -P 4455 menger tickets watch
```

Raw wire: JSON-RPC 2.0 over WebSocket. Every logical call is a
`menger.call` subscription with `{method: "tickets.watch", params: {...}}`;
events arrive as `result` notifications carrying `PlexusStreamItem`s.
`web/index.html` is a complete, dependency-free client.

## Layout

```
src/
  main.rs              CLI args, transports, spawns the shell
  builder.rs           build_rpc() → Arc<DynamicHub> ("menger")
  shell.rs             / (the Leptos app, degrading to the vanilla page when
                       unbuilt), /vanilla (the embedded day-one page), /app/
                       (the app + its assets from disk), /rest/* (same-origin
                       proxy to the REST port)
tickets-api/           the ticket vocabulary, the `tickets` activation and the
                       typed Plexus client it emits — ONE declaration, shared by
                       the server (default features: `server`) and the app
                       (default-features = false, wasm32)
  types.rs             Ticket · Status · Kind · Severity · TicketSummary · Group · TicketsEvent
  store/               the service-wide cache (SQLite system of record behind it)
    mod.rs             the Store: the cache, reads, the write-through path
    rollup.rs          the per-shortcode roll-ups it keeps (MNG-10)
    ids.rs             each shortcode's last sequence and the ids named before
                       birth — what create reads instead of scanning (SCALE-3)
    db.rs              the schema, upserts, the in-place migrations
    synthetic.rs       the --seed-synthetic generator (PERF-2)
    tests.rs           the store's unit tests
  nav_plan.rs          test-only: the nav's plan(), the roll-ups' oracle
  activation.rs        #[plexus_macros::activation(client, server_cfg)]
web/index.html         the day-one reactive shell
frontend/              the Leptos CSR app (own cargo unit; trunk build → dist/)
plans/MNG/             the backend epic; plans/MFE/ the frontend epic
```

## Status

Phase one of the backend epic is complete — see
`plans/MNG/artifacts/CHECKPOINT.md` for the verified state map. Tickets
live in SQLite behind the service-wide cache (MNG-2); the DAG answers
ready-set/blockers queries with cycle rejection at create (MNG-4); REST +
SSE serve on :4457 via plexus-transport 0.2.3 (MNG-8 — consumed as a
git-tag patch until it's published to crates.io). The Leptos app at
`/app/` is the live board (MFE-3): every ticket as a row with a per-row
status control, pushed over SSE — read-and-status only, no creation
form by design (tickets are created by agents/CLI/API). Parked as Idea:
MNG-5/6 (schema-derivation and framework theses). Next: the default
page swap and the ready-set view (`plans/MFE/`), and the HelixDB
graph-store epic (`plans/HLX/`, spike-gated).

## History

`menger-hs` (0.04, Haskell) is the ancestor; its `web/` window manager —
tabs, tear-out, desks, fuzzy find — is the eventual consumer of the
framework this repo grows.

## License

MIT
