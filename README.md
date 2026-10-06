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
allocated the normal way, every kind and every status but the two worker
states — SDLC-7 left the generator, and so every seeded store, as it was —
a short markdown body, about a quarter of the tickets carrying valid
acyclic `blocked_by` edges with `unlocks` mirrored), and deterministic: the same `--seed-value`
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

### The stored derived state, and checking it (SCALE-28)

What the store derives from the tickets — each shortcode's run of
sequences (the next id, `extents`, `page.remaining`,
`health.ticket_count`), the dangling forward references `create`
resolves, the roll-ups behind `groups`, and each ticket's children
(an index on `(parent, epic, seq)`) with its `child_count` (a column on
its row), and (SCALE-32) each ticket's count of unsatisfied blockers (a
column on its row), the ready set (a partial index over that count and
the status, in store order) and each shortcode's count of each status
and share of the ready set — is stored in the database beside them,
written by every write in the same transaction as its tickets, and read
at start instead of rebuilt by a pass over every ticket. A database written before SCALE-28
— or in an older form of it: SCALE-32's is form 2, SDLC-11's (the roll-ups'
`docs`) form 3 —
has it built once, on first open, durably, and the log says so
(`SCALE-28: stored the derived state for the first time …`); every later
start logs `read the derived state from the database … nothing rebuilt`.
A seed stores it after its last chunk of tickets; killed before that, the
file says its derived state is behind, and the next open builds it again.

```sh
menger --verify-derived --db-path /path/to/tickets.db
```

opens the file read-only, rebuilds all of it by a scan of its tickets,
compares them with what is stored, and prints each difference — the
structure and the key, what is stored and what a scan gives
(`child_count of ALT-2: stored 5; a scan gives 1`) — or `consistent: …`.
Exit 0 when consistent; 1 on any difference, or when the file holds no
derived state to compare (written before SCALE-28 and not opened since);
2 when it cannot be read. It changes nothing, migrates nothing and serves
nothing: run it before a deploy and after any migration.

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
  browser share). It opens `watch` first, asks `health`, `groups` (the
  nav's shortcode rows, SCALE-24) and `extents` (where every ticket sits,
  SCALE-26), then one page for the board's window
  (`list { after, limit, summary }`, MNG-9) — and is `live`: five requests
  whatever the store holds. It reconnects on a ~1 s loop — a server
  restart returns the page to live on its own, with rows keeping their
  DOM identity. A `lagged` on `watch` (SCALE-5) is answered over the same
  socket by asking again for what is shown — the groups, the extents, the
  board's window, the open ticket page's ticket, children and the rows it
  names: rows keep their DOM, `watch` keeps running, and the dock stays
  `live`. The port comes from `<meta name="menger-ws-port">`, which
  the shell substitutes wherever it serves the app's entrypoint (`/` and
  `/app/`), exactly as it always has for `web/index.html`.

Rendering (MFE-12): the app mounts only what can be seen. The board is a
window — the rows on screen plus 100 either side,
between two spacer rows at the measured row pitch — so every board row is
one line (a long title ends in an ellipsis; hover shows it whole); a store
taller than a browser will lay out (about 888,000 rows in Chromium) is
drawn at a fixed height with the scroll range mapped onto it. The nav
mounts a node's children when it opens. j/k, the jump palette and the
ticket menu work from the row store, so none of them needs a row to be
mounted.

The board holds only its window (SCALE-26). `extents` gives each
shortcode's last sequence, and sequences are dense, so row k of the board is
a ticket id by arithmetic and the page that starts there is
`list { after: <row k - 1> }` — a scroll or a jump anywhere asks for one page
(~2 ms at 10^6) and draws a pending line for each row until it lands. The
rows held stay within 1,000 of the window (about 2,400 at most, however far
the board is scrolled); a `created` moves the count and the scrollbar and is
held only inside a stretch the board holds; any other event is applied to a
row some view holds and dropped otherwise. j/k, Home/End and Enter walk
positions, so they cross the window and fetch on the way. At 10^6 the first
rows and `live` come together in ~0.22 s, and the page's wasm memory is
~5.6 MB (the walk held 682 MB).

The nav (SCALE-24) needs no walk: its shortcode rows are the server's
roll-ups (`groups`, kept by `group_changed`), a shortcode is drawn as its
root's own ticket row (AGS-23: from the group's `root_row`, with the
shortcode's roll-up beside the root's status; a shortcode with no root keeps
a row of its own over a flat list), each level indents two columns, and an
opened node's children come by page (`list { parent }`) — past
one page, as a window over them like the board's, so a root of 163,530
children shows its first page in ~70 ms and its last child is a scroll
away. At 10^6 the shortcodes are on screen in ~160 ms and a cold deep link
shows its leaf selected in the nav in ~0.4 s. Every row the app shows lives
in one row cache — a signal per ticket, held while a view shows it — that
the board, the nav and the ticket page share. The jump palette (SCALE-25)
asks the server: `find` once the typing pauses (80 ms), only the newest
question's answer shown, its count the answer's rows plus `remaining` —
`map-12` shows MAP-12 first ~0.26 s after the last keystroke at 10^6.

The wave board (WAVE-5) is `#/waves/<SHORTCODE>` — e.g.
`http://127.0.0.1:4456/app/#/waves/PLEXUS`, the platform's work in order:
the shortcode and every shortcode hosted under it (the nav's `host` chain
from `groups`), placed by one `tickets.waves` (WAVE-4) and grouped here into
columns — a column per non-empty wave, then `missing blocker`, then `Done`
collapsed to its count; or, one toggle away, the stages (`Blocked`, holding
wave 2 and later with a `w<n>` badge, then the wave-1 tickets by status and
`Done`). The layout is remembered per shortcode in localStorage. A card
names each ticket it waits on that the board does not show (an off-board
blocker, a chip linking to it). WAVE-12: that tree and the kinds build,
spike, gap are the board's default; its **filter** builds any other — a
`Filter` button (or `f`) opens a Linear-style menu: a search over the
categories and their values, then Shortcode (the hosting tree, the route's
first, tri-state), Status, Wave, Kind, Severity, Readiness, Off-board
blockers, Title contains — each value with its count on the board's answer.
Applied clauses are pills, `[Category] [is any of | is not] [values +n]
[×]`; `reset` returns to the default. Shortcode and Kind are what `waves` is
asked for; the rest narrow the answer in the browser. The filter is the
route's query, rewritten in place — `#/waves/PLEXUS?sc=is:RETURNS&
status=not:Pending` — so a reload or a pasted link restores the board.
WAVE-6: it moves its cards as `watch` says
they move — each `placements_changed` takes a card to its new column
(however far upstream the write was: a completion two blockers back moves
it), each `status_changed` its stage and what the cards wait on — and asks
`waves` again (coalesced, one in flight and one queued) only for a ticket
born into its shortcodes, on a `lagged`, on a reconnect, and once more
behind an answer in flight when a change lands while it is out; `waves` is
asked only while a wave board is open. `#/waves`
lists the boards; the header links to it, and a shortcode's row in the nav
offers its board in the ticket menu.

Below the board, in either layout, is its **annotation** (WAVE-11): a
markdown document that discusses the board's waves, kept in the board's
root shortcode at the reserved path `_MENGER/board` (WAVE-10) — `get { id:
"PLEXUS/_MENGER/board" }` — a doc under the shortcode's scope root, hidden
from the nav. The panel is titled with that address and folds; with none
written yet it says "write notes for this board". `[ edit ]` opens a
markdown textarea beside a live preview through the ticket page's renderer;
save amends the document (`amend { id, body }`) — the first save creates it
(`create { epic, kind: doc, path: _MENGER/board, title: "board: ROOT",
body }`) — and cancel discards. An `amended` from elsewhere updates the
panel, or, mid-edit, says "changed elsewhere — reload?" and leaves the
draft alone. In any body — the annotation's or a ticket page's — a **wave
reference** `{wave: RETURNS-6}` renders as that ticket's current ordinal:
`wave 2` (on the board a link to the card's column), `done` or `missing
blocker`, from the board's cards when the ticket is one, else from the
ticket's row (its kept `placement`, fetched and held while shown); it moves
live, and an id that is no ticket is left as written. A body without one
renders byte for byte as before.

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

A sixth kind, **`doc`** (SDLC-11, owner 2026-10-02), is not work: a
document — a finding, a manifest, a reference — that "simply exists in
its state". It has no lifecycle. A doc is born in the status **`Doc`**,
which only a doc ever holds, and never leaves it: `set_status` on a doc is
an in-band `error` ("a doc has no lifecycle"), whatever the status, and
`set_status … Doc` on any other kind is one too. A doc is never blocked
and never a blocker — `create` refuses a doc with a `blocked_by`, a
`blocked_by` naming a doc, and a doc born as an id some `blocked_by`
already names — so it is never in the ready set and no ticket waits on
one. It has no completion: a roll-up's `done/total` is the work's alone (a
scope with every piece of work `Complete` is done, whatever docs hang
under it), and `groups` counts docs apart, in `docs`. A doc may be a
shortcode's first ticket — a doc collection's root — as a scope may.
Everything else is a doc's as any ticket's: a body and `amend`, a parent
and children (docs or work), `link_commits`, `find`, `list` (`status:
Doc` finds them), `extents`, `watch`.

The statuses of the lifecycle are nine (SDLC-7): `Pending`, `Ready`,
**`Coding`**, **`In Review`**, `Blocked`, `Complete`, `Idea`, `Epic`,
`Superseded` — and a doc's `Doc` (SDLC-11) beside them, outside it. The two
in bold are the **worker states**: a worker flips its ticket `Ready →
Coding` with `set_status` the moment it starts (so no leaf is claimed
twice) and `Coding → In Review` in the commit that finishes the work —
gate green, committed, the message ending `— in review`; the merge sets
`Complete`. `In Review` is spelled with its space on the wire and in the
database. `set_status` and `list { status }` read any status
case-insensitively, and the worker states leniently — `in review`,
`in_review`, `in-review`, `inreview`, and `active` (the house
methodology's word) for `Coding` — but write only the canonical spelling.
No transition is gated (SDLC-4), and neither worker state is ready or
done: the ready set is still `Ready`, a blocker is still satisfied only by
`Complete` or `Superseded`, and a roll-up's `done` still counts
`Complete`. Stored as text, so a database written before them opens
unchanged — nothing to migrate.

A ticket's outputs are tickets (AGS-19): every ticket carries an optional
**`parent`** — the ticket it is an output of, in any epic — and a
server-derived **`child_count`**, how many tickets name it as their
parent. The children themselves are read by page: `list { parent }`, in
the store's `(epic, seq)` order (SCALE-4; until then a row carried
`children`, every child id — under MNG-14 a shortcode root's list is its
whole shortcode, 163,530 ids and 2 MB at 10⁶, rewritten, stored and
broadcast on every birth). The parent must exist at creation and the
chain is acyclic (a ticket cannot be its own ancestor); a nonexistent or
cyclic parent is an in-band `error` naming the reason. A child's birth
writes the child and, on the parent's row, only its count (SCALE-28; from
SCALE-4 until then, nothing of the parent's) — never a list — and
`watch` carries the parent's side as `children_changed { id, added,
child_count }`: the parent's id, the child, the new count. A database
written before AGS-19 gains `parent` on first open, durably, with typed
parents backfilled from each body's frontmatter `parent: <id>` — one log
line per ticket; an id that names no ticket is logged and left empty,
never fatal.

A database written between AGS-19 and SCALE-4 still has the `children`
column. On first open the server checks every list in it against the
`parent` column (the edge both described; `children` was the mirror),
logs each disagreement — none the server ever wrote — with `parent`
standing, and drops the column (`ALTER TABLE … DROP COLUMN`, which
rewrites every row once: 7.5 s at 10⁶, 4 → 11.5 s to the first `health`
that one time), in one transaction, before anything loads. Every ticket's
children are then the same set as its old list, in the store's order
rather than the order they arrived in, and `child_count` its length. A
restart finds no column and migrates nothing; a pre-SCALE-4 binary cannot
open the migrated file.

A ticket knows the commits that finished it (SDLC-9): every row carries
**`commits`**, in the order they were linked (`[]` when none), written by
`link_commits` and `unlink_commits`. A **commit reference** is a
hexadecimal object name of 7 to 64 characters, optionally prefixed by a
repository and `@` — `77df9a0`, `plexus-macros@4f7114a…` — the repository
being letters, digits, `.`, `_` and `-`; the hash is stored lowercase, the
repository as given, and nothing is trimmed. References are compared as
text: the server holds no repository, so it does not know that a short and
a long hash name one commit — link the form you want kept, and qualify with
`repo@` when the commit is not in the ticket's own repository. At most 256
per ticket. Linking moves `updated_unix` and nothing else — not the status:
a ticket can be `Complete` with no commit and hold commits while `Coding`
(whether `done` requires one is SDLC-4's). The worker links its commit in
the step that flips the ticket to `In Review`; the merge links the merge
commit if it differs. There is no reverse lookup (which tickets a commit
finished). A database written before SDLC-9 gains the column on first open,
durably, every ticket with none linked — `ADD COLUMN … DEFAULT '[]'`,
which rewrites no row, so it is instant at any size; a binary from before
it still opens the file.

```sh
synapse -P 4455 menger tickets link_commits --id MNG-10 --commits 77df9a0
synapse -P 4455 menger tickets link_commits --id SCALE-4 --commits 0bb28b2 --commits 090cdec
```

A ticket may also carry a **path address** (WAVE-10): an optional
**`path`**, unique within its shortcode, so that `<SHORTCODE>/<path>`
names it — `get { id: "PLEXUS/_MENGER/board" }` answers the one ticket of
`PLEXUS` holding the path `_MENGER/board` (the shortcode in any case, the
path exactly as written; an address that names nothing is `not_found`
carrying the address). A path is an address, not an id: the ticket keeps
its `EPIC-N`, and the store's order, extents, paging and readiness never
read it. **`_MENGER/` is reserved** for the documents menger itself keeps
inside a shortcode (WAVE-1 decision 11 — the board's annotation,
`_MENGER/board`, is the first), and is the only prefix accepted for now:
`create { …, path }` takes a path only when it starts with `_MENGER/` and
names something after it, the ticket is a `doc`, and its `parent` is the
shortcode's scope root (or is omitted, which defaults to it); a path
another ticket of the shortcode already holds is an in-band `error` naming
that ticket. A path is set at birth and never moves; it is on the row and
the ticket, omitted when there is none. The nav hides tickets whose path is
under `_MENGER/` unless its "menger docs" toggle is on (remembered in the
browser). A database written before WAVE-10 gains the column, and the
unique index on `(epic, path)`, on first open, durably — `ADD COLUMN` with
no default rewrites no row, so it is instant at any size, and a binary from
before it still opens the file.

```sh
# synapse expands any parameter named `path` as a filesystem path (`--path
# _MENGER/board` arrives as `$PWD/_MENGER/board`, which `create` refuses):
# pass a path address's `path` in the params object instead.
synapse -P 4455 -p '{"epic": "PLEXUS", "kind": "doc", "path": "_MENGER/board", "title": "board: PLEXUS"}' menger tickets create
synapse -P 4455 menger tickets get --id PLEXUS/_MENGER/board
```

Every ticket's place in the order of work is **kept** (WAVE-6): each row
and ticket carries **`placement`** — `{"type": "wave", "n": n}`,
`{"type": "done"}` or `{"type": "missing_blocker"}`, the rule `waves`
answers by (below) — omitted on a doc, which has no place in it. The store
holds it equal to that rule at every moment: it places every ticket from
the rows when it opens the file (nothing stores a placement, so none is
stale after a restart), and the two writes that can move one move it before
they commit — a birth (the newborn, and every ticket a forward reference to
its id held in the missing lane, and theirs) and a status change across
`Complete` / `Superseded` in either direction (the ticket, and everything
downstream of it along `unlocks` whose placement changes; the walk stops
where nothing changes). Nothing else moves one: `blocked_by` is fixed at
birth. A write that moved any says so last, once, in `placements_changed`
(the newborn's own is on its `created`; a status change's own ticket is on
its `status_changed` and in the changes). WAVE-3 measured what keeping them
costs (`spike/WAVE-2/`).

The order of work for a shortcode tree — the platform's, here — is one call
(WAVE-4); the app's wave board (`#/waves/<SHORTCODE>`) makes the same one:

```sh
synapse -P 4455 menger tickets waves --epics PLEXUS,MACRO,SYNAPSE,TRANSPORT,STARTER,HYPERFORGE,RETURNS,CONTEXT
```

| method | yields |
|---|---|
| `tickets.health` | `health { version, unix_now, ticket_count }` |
| `tickets.create { epic, title, kind?, severity?, blocked_by?, body?, parent?, path? }` | `created { ticket }` — id is the next `EPIC-N`, status starts `Pending`; `kind` defaults to `build`, one outside the six is an in-band `error` naming them; a `parent` that does not exist (or would make the ticket its own ancestor) is an in-band `error` naming the reason; **every shortcode starts with a scope** (MNG-12) **or a doc** (SDLC-11): the first ticket of a prefix must be `kind: scope` or `kind: doc`, anything else is an in-band `error` naming the rule and nothing is allocated; a `doc` starts — and stays — in `Doc`, and a doc with a `blocked_by`, a `blocked_by` naming a doc, or a doc born as an id a `blocked_by` already names is an in-band `error` (SDLC-11); **`parent` defaults to the shortcode's scope root and must stay inside the shortcode** (MNG-14): omitted, a non-root hangs under its prefix's root; named, it must be in the same prefix, else an in-band `error` — the scope root itself is the exception, its parent (if any) is the shortcode it hangs under, a sub-shortcode; **`path`** (WAVE-10) gives the ticket a path address in its shortcode — only `_MENGER/…` with something after it, only on a `doc`, only with the shortcode's scope root as `parent` (the default); one another ticket of the shortcode holds is an in-band `error` naming it, as is each other refusal, and nothing is allocated |
| `tickets.get { id }` | `ticket` or `not_found` — `id` is an `EPIC-N`, or (WAVE-10) a path address `SHORTCODE/path` (`PLEXUS/_MENGER/board`; the shortcode case-insensitive, the path exact), and `not_found` carries the id or address as given |
| `tickets.list { epic?, status?, parent?, after?, limit?, summary? }` | one `ticket` per match, ordered (epic, seq), then one `page { next, remaining }`; `parent` yields exactly that ticket's children — the one way to read them (SCALE-4), from an index the store keeps, so a page of a root's 163,530 children costs the page (1.3 ms at 10⁶ over REST, first page or deep) and its `remaining` is arithmetic; `summary` swaps the rows for `summary { summary }` (every field but the body); `after` is the exclusive cursor and `limit` caps the page — `after` naming no ticket is one in-band `error` |
| `tickets.ready_set { epic?, after?, limit?, summary? }` | one row per member of the ready set (Ready, every blocker Complete or Superseded — never a doc), ordered (epic, seq), then one `page { next, remaining }` |
| `tickets.find { query, after?, limit? }` | one `summary` row per ticket whose id or title contains `query`, case-insensitively (ASCII folding) — the jump palette's rule (AGS-18), on the server (SCALE-6) — ordered (epic, seq), at most `limit` (default 50), then one `page { next, remaining }`: `remaining` plus the rows is the total. `query` is trimmed; empty or blank matches every ticket. `after` is `list`'s cursor (a position — it need not match); one naming no ticket is one in-band `error`. Ids and titles only, never bodies. A blank query's page costs the page; any other query's exact `remaining` scans every id and title after the cursor (~145 ms at 10⁶) |
| `tickets.set_status { id, status }` | `status_changed { ticket, previous }` — any of the nine, from any status (SDLC-7: the worker states `Coding` and `In Review` included); one outside them is an in-band `error` naming the nine; SDLC-11: on a doc, an in-band `error` (a doc has no lifecycle), and `Doc` on any other kind an in-band `error` too |
| `tickets.amend { id, title?, body? }` | `amended { ticket, changed }` (MNG-16) — replaces the title and/or the body, whole values, last writer wins; `changed` names what moved (`["title"]`, `["body"]`, `["title", "body"]`); neither given, or the values already there, is no write and no broadcast and answers `changed: []`; an id naming no ticket is `not_found`; a title that is empty after trimming is an in-band `error` and nothing is written. Stored byte-exact, durable before it is acknowledged; `updated_unix` moves; nothing else about the ticket does — status, kind, edges, parent, id |
| `tickets.link_commits { id, commits }` | `commits_linked { ticket, added }` (SDLC-9) — appends every commit reference in `commits` the ticket does not already carry, in the order given; `added` is what was newly linked, in order. Every entry already linked, or `commits` empty, is no write and no broadcast and answers `added: []`. An entry that is not a commit reference is one in-band `error` naming every such entry, and nothing is written — a call is all or nothing; so is a link that would put more than 256 commits on the ticket. An id naming no ticket is `not_found`. Durable before it is acknowledged; `updated_unix` moves, nothing else does |
| `tickets.unlink_commits { id, commits }` | `commits_unlinked { ticket, removed }` (SDLC-9) — every entry the ticket carries is removed; `removed` names them in the order given; none of them linked is no write, `removed: []`. The same grammar, the same all-or-nothing `error`, `not_found` |
| `tickets.set_parent { id, parent }` | `parent_changed { ticket, previous }` (MNG-12) — files an existing ticket under `parent`: the child's row is written, and it leaves the old parent's children and joins the new one's (SCALE-4: neither parent's row is rewritten — SCALE-28 stores each one's new `child_count` on it, in the move's transaction; `watch` carries `children_changed` and `child_removed` with each parent's new `child_count`); the same guards as `create` (`not_found`, must exist, inside the shortcode unless the ticket is its root, no self-ancestry — in-band `error` naming the reason); the parent it already has is a no-op |
| `tickets.groups {}` | one `group { group: { epic, total, done, docs, root, root_row, host } }` per shortcode row the nav draws, in shortcode order, then the stream's own `done` (MNG-10) — the nav's tree without its rows: `total`/`done` count the row's whole subtree as AGS-20's `plan()` nests it (sub-shortcodes and cross-shortcode children included; `done` is `Complete` only; SDLC-11: the work only — `docs` counts the docs, and a row of docs alone is drawn), `root` is the lowest-seq `scope` (or `doc`), `root_row` the root's `{ title, status, kind }` (AGS-23: the nav draws the shortcode as the root's row; `null` with no root), `host` the ticket a sub-shortcode's row nests under (`null` at top level). Kept by the store on every write, so the cost is the number of shortcodes, not tickets |
| `tickets.extents {}` | one `extent { epic, last_seq }` per shortcode that holds a ticket, in shortcode order (the store's), then the stream's own `done` (SCALE-16) — where each shortcode ends: ids are allocated highest-plus-one and never deleted, so a shortcode holds `last_seq` tickets, row k of it is `EPIC-k`, and a position in the store's order becomes a `list { after }` cursor without a walk; every `last_seq` together sums to `health.ticket_count` (a hand-edited database with a hole still reports the highest sequence; `after` naming the missing id is `list`'s cursor error). Not a roll-up: the shortcode's own tickets only, and every shortcode has one whether or not the nav draws it a row. Read from the sequence `create` allocates from (SCALE-3), so the cost is the number of shortcodes; no `watch` event — `created` carries the shortcode's new `last_seq` as its `seq` |
| `tickets.waves { epics, kinds? }` | the order of work (WAVE-4): one `placed { summary, placement, waiting_on }` per ticket of the shortcodes `epics` (case-insensitive; one with no tickets contributes nothing) whose kind is in `kinds` (default `build`, `spike`, `gap`; a doc is never placed), then exactly one `wave_counts { waves, missing, done }` — `waves[i]` counts the placed tickets in wave `i + 1`. `placement` is computed over the **whole** `blocked_by` graph by `ready_set`'s rule (MNG-4), blockers followed into any shortcode, asked for or not: `{"type": "done"}` when the ticket is `Complete` or `Superseded`; `{"type": "missing_blocker"}` when it, or anything it waits on, names a blocker that is no ticket (or, only by a hand edit, sits on or behind a `blocked_by` cycle); otherwise `{"type": "wave", "n": n}` — 1 with every blocker satisfied, else 1 + the highest wave among its unsatisfied blockers. `parent` orders nothing, and a `Blocked` status is a marker, not an edge. `waiting_on` is the ticket's unsatisfied direct blockers (missing ids included), in `blocked_by` order, distinct; `[]` when done. Order: waves ascending, inside a wave `Coding`, `In Review`, `Ready`, `Pending`, `Blocked`, `Idea`, `Epic`, then (epic, seq); then the missing lane; then the done — each in (epic, seq). `epics` empty, or a kind outside the six, is one in-band `error` and nothing else. WAVE-6: read from the placements the store keeps (below) — the cost is the placed tickets — and `waiting_on` worked out per row |
| `tickets.watch { epic? }` | open-ended stream of `created` / `status_changed` / `amended` / `commits_linked` / `commits_unlinked` / `unlocks_changed` / `children_changed` / `parent_changed` / `child_removed` / `group_changed` / `placements_changed` — each carries the whole affected ticket but the parent's side of a birth or a move, `children_changed { id, added, child_count }` / `child_removed { id, removed, child_count }` (SCALE-4: under 1 KB whatever the parent's size; it was the whole parent); a write that moves a roll-up is followed, after its ticket events, by one `group_changed { group }` per row that moved, innermost first (a row no longer drawn arrives once with `total: 0`) — WAVE-6: and a write that moved any kept placement ends with one **`placements_changed { changes: [{ id, placement }] }`**, after its other events (`epic` keeps the changes in its shortcode, and the event only when one is left) — AGS-23: so is a shortcode root's status change (any, not only across `Complete`) or new title, which moves its group's `root_row`; `epic` filters a group by its own shortcode, a parent's side by the parent's; **`lagged { missed }`** (SCALE-5) where a subscriber more than the server's broadcast holds (256 changes) behind lost changes — once per gap, in place of the `missed` changes lost (`missed` ≥ 1, the channel's count), whatever the `epic` filter, then the oldest change still held, in order, none twice; not an error, the stream goes on — what the client holds may be stale, so it fetches again what it shows |

Paging (MNG-9). `(epic, seq)` is a total order the schema keeps unique, so
a ticket id is a cursor: a client that renders opens `watch` first, then
walks `list { summary: true, limit: 200 }`, passing the previous page's
`page.next` as the next call's `after`, until `next` is `null`. Each page
is `≤ limit` rows and one `page { next, remaining }`; `remaining` counts
what is left under the same filters — exactly, and, with no `status` or
`parent` filter, without walking it: the store keeps each shortcode's
count beside its sequence, so a page costs the page (SCALE-19; a
`parent` page counts from its index of children, SCALE-4). A `ready_set`
page is a range of the ready set the store keeps, and its `remaining`
counts at most the members of the one shortcode the cursor cuts into; a
`status` page sums whole shortcodes from kept per-status counts and walks
only the rest of the cut one — but the page itself still tests each row
until it has `limit` matches (SCALE-32). A client that wants everything with
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

`watch` is fed from a bounded broadcast (256 changes) and the server sends
to a socket no faster than the client reads it, so a subscriber that falls
far enough behind loses changes. It is told: one
`{"type": "lagged", "missed": N}` data item where they were (SCALE-5),
then the stream continues. A client that ignores event types it does not
know is unaffected; one that keeps a copy should treat it as "fetch again
what you show" — the Leptos app asks `extents` and its board's window again
(SCALE-26), as after a reconnect.

## Diagrams — the `diagrams` service (COMPILE-4)

A second Plexus service beside menger, in `diagrams/` (`menger-diagrams`):
mermaid diagrams checked by the very bundle the app draws them with
(`frontend/vendor/mermaid/mermaid-11.17.2.min.js`), loaded into a jsdom
window under Node — COMPILE-3 found it gives the browser's verdict on every
live diagram. Its own binary, process and hub (`diagrams`, WebSocket
:4458): menger does not call it, does not need it, and `cargo build` /
`cargo test` never need Node.

```sh
npm ci --prefix diagrams/node           # once: jsdom, pinned by the lockfile
cargo run -p menger-diagrams            # WebSocket :4458
synapse -P 4458 diagrams mermaid parse --source 'graph LR
  A --> B'
synapse -P 4458 diagrams mermaid check --markdown "$(cat ticket-body.md)"
# any body, frontmatter and `$` included, needs synapse >= 3.14.2 (SYNAPSE-4, SYNAPSE-6)
```

| method | yields |
|---|---|
| `mermaid.parse { source }` | exactly one of `parsed { diagram_type }` (`flowchart-v2`, `sequence`, `pie`, …); `parse_error { message, line }` — `message` is mermaid's, verbatim, `line` the 1-based line within `source` (null when mermaid names none); `error { message }` — the service could not ask (a missing piece, the program died, the 5 s limit) |
| `mermaid.check { markdown }` | one `diagram { index, line, result }` per mermaid block, in order, then the end — `index` from 1, `line` the block's first source line in the markdown, `result` what `parse` would answer — so an error at `result.line` L is markdown line `line + L - 1`. No blocks, no items. A missing piece is one `error` in place of everything |

A mermaid block is the app's: a fenced block whose info string's first word
is exactly `mermaid`, frontmatter excluded — `diagrams/src/blocks.rs` copies
`frontend/src/md.rs`'s rule and its tests hold the copy to the original
(same functions word for word, same pulldown-cmark version and options,
every case md.rs's tests name). `line` within `source` can differ from the
number in mermaid's message: mermaid numbers the text it parsed, after
dropping frontmatter, directives, `%%` comments and leading blank lines;
the Node program maps it back with mermaid 11.17.2's own regular
expressions.

**What it needs.** Node.js ^22.22.2, ^24.15.0 or >=26 (jsdom 30's range)
on `PATH`, jsdom (`npm ci --prefix diagrams/node`; `node_modules` is not
committed), and the vendored bundle (in the checkout). Each missing piece
degrades, never crashes: the service starts and serves, and a call answers
one `error` naming the piece and the command that provides it. Flags (and
env): `--port` (4458), `--bundle` (`MENGER_DIAGRAMS_BUNDLE`), `--node`
(`MENGER_DIAGRAMS_NODE`, default `node`), `--node-dir`
(`MENGER_DIAGRAMS_NODE_DIR`, default `diagrams/node`), `--stdio`.

**The Node program** (`diagrams/node/mermaid-parse.js`) is the service's
child, speaking one JSON request per line on stdin and one answer per line
on stdout (its stderr goes to the log, never into the protocol). It starts
on the first call (~0.6 s; ~300 MB resident, the service ~10 MB) and is
kept; one diagram is in flight at a time, each answer matched to its
request by id. Dead between calls, it is replaced before the next is
written; dead mid-call, or past the 5 s limit, that diagram's result is an
`error` and a fresh one starts. It only ever calls `mermaid.parse`, under
`securityLevel: 'strict'`, in a window that runs no scripts and fetches
nothing. Warm, a `parse` round trip over the WebSocket is ~4 ms.

Tests: `diagrams/tests/lifecycle.rs` drives the lifecycle against a shell
stand-in (no Node needed); `wire.rs` and `binary.rs` run the real program
and skip, printing `SKIPPED … <what is missing>`, without Node or jsdom.
Upgrading mermaid: the app's `ASSET_URL` and `diagrams/src/node.rs`'s
`BUNDLE` move together (a test fails until they agree), and the
preprocessing regular expressions in `mermaid-parse.js` are re-read from
the new bundle.

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
  types.rs             Ticket · Status · Kind · Severity · TicketSummary · Group · Placement · TicketsEvent
  store/               the service-wide cache (SQLite system of record behind it)
    mod.rs             the Store: the cache of rows, reads (a body from the
                       database on read-only connections — SCALE-14), the
                       write-through path
    rollup.rs          the per-shortcode roll-ups it keeps (MNG-10)
    ids.rs             each shortcode's last sequence and the ids named before
                       birth — what create reads instead of scanning (SCALE-3);
                       beside the sequence, how many tickets the shortcode
                       holds — what page.remaining is computed from (SCALE-19)
    children.rs        each parent's children, from every ticket's parent, in
                       the store's order — what list { parent } pages from and
                       what child_count is (SCALE-4)
    db.rs              the schema, the writes, a row and a body read back, the
                       in-place migrations
    ready.rs           each ticket's count of unsatisfied blockers, the ready
                       set it decides (store order), each shortcode's count of
                       each status — what ready_set and list { status } page
                       and count from (SCALE-32)
    waves.rs           each ticket's wave in the order of work — MNG-4's rule
                       over blocked_by, the whole graph (WAVE-4); `waves`
                       reads the kept placements (WAVE-6)
    placements.rs      the placements kept on the rows: the re-placement pass
                       a birth or a status change across the satisfied line
                       runs before it commits, and the pass at open (WAVE-6)
    stored.rs          the derived state as stored beside the tickets — its
                       tables, what a write's transaction writes to them, the
                       read at start, the build on first open, and
                       --verify-derived (SCALE-28)
    commits.rs         what a commit reference is, as stored and compared, and
                       how many a ticket holds (SDLC-9)
    synthetic.rs       the --seed-synthetic generator (PERF-2)
    tests.rs           the store's unit tests
    tests_paging.rs    page.remaining against a count, on every shape (SCALE-19)
    tests_find.rs      find's rule against the palette's own tests and code
                       (read from frontend/src/keys.rs), its pages against
                       MNG-9's (SCALE-6)
    tests_children.rs  the children index against a scan, child_count, pages of
                       list { parent } against the old filter, the migration
                       that drops `children` (SCALE-4)
    tests_bodies.rs    whole tickets from rows and stored bodies, a walk of a
                       page of bodies at a time, read-your-writes under a
                       hammer of amends (SCALE-14)
    tests_commits.rs   link_commits / unlink_commits: the grammar, every row of
                       the table, the bound, the column added to an old file
                       (SDLC-9)
    tests_detached.rs  every write dropped by its caller mid-commit still
                       reaches the cache and watch; the next id is the next
                       one (MNG-20)
    tests_stored.rs    the stored derived state, read back from the file,
                       against the scan oracles after every kind of write on
                       every kind of store; the build once, the read at
                       start, the seed's marker, verify (SCALE-28)
    tests_ready.rs     the counts, the ready set and the status counts against
                       a scan after every write, in the cache and the file;
                       every ready_set and list { status } page against the
                       code before it (SCALE-32)
    tests_waves.rs     waves: chains, diamonds, the missing lane, blockers in
                       other shortcodes, docs, kinds, the yield order and the
                       tally, a hand-edited cycle, a chain past any call stack
                       (WAVE-4)
    tests_placements.rs  the kept placements against the rule after random
                       births and status changes, watch's mirror, the
                       two-blockers-back case (WAVE-6)
    tests_paths.rs     path addresses: create, refusals, get by address, the
                       column added to an old file (WAVE-10)
  nav_plan.rs          test-only: the nav's plan(), the roll-ups' oracle
  activation.rs        #[plexus_macros::activation(client, server_cfg)]
web/index.html         the day-one reactive shell
frontend/              the Leptos CSR app (own cargo unit; trunk build → dist/) —
                       src/waves.rs the wave board (WAVE-5, WAVE-6),
                       src/filter.rs its filter and menu (WAVE-12),
                       src/notes.rs its annotation and the body editor (WAVE-11)
spike/WAVE-2/          WAVE-3's measurement of the kept placements (run.sh)
diagrams/              the `diagrams` service (COMPILE-4): its own binary and hub
                       (WS :4458); the `mermaid` activation and the client it
                       emits, the Node child it owns (src/node.rs), the app's
                       mermaid-block rule (src/blocks.rs); node/ is the Node
                       program and its lockfile
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
