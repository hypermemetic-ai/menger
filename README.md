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
up, no restart needed. The server holds the build as a table (ACCESS-33):
`/app/…` answers only the files of the build it read — a request path is
a key, never a path on disk — and a new `index.html` (every trunk build
writes one) has it read the build again.
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
  called it cross-origin: until MFE-9 the shell hosted a same-origin
  reverse proxy, `/rest/{*}` on :4456 forwarding verbatim to :4457.
  **ACCESS-33 removed it** — the app speaks the WebSocket (below), nothing
  else called it, and it forwarded anything to a bridge that carries no
  identity. The shell serves menger's pages and the app's build, nothing
  else; the REST port itself, where it runs, is for curl and scripts.
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

The nav's header chooses `Tree | Flat` (FILTER-20), kept in local storage
with its other state (`menger.nav.view`). Flat lists every ticket the nav's
filter admits — every ticket with none — as the tree's leaf rows in id order,
windowed like the board: `counts { filter }` places each row, and a page is
`list { filter, after: <the row before it> }` scrolling on or `list { filter,
at }` after a jump; a membership watch keeps it live. At 10^6 its first rows
show in ~0.3 s and the last ticket is a scroll away (five pages).

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

**Filters** (FILTER-9) reach every view. There is one **global** filter
(the header's `Filter` button, or <kbd>F</kbd>; a bar under the header while
it holds anything, and `n of N` beside the button) and one per surface: the
**board**, the **wave board**, the **nav** and the **inputs** page (each
surface's own bar, or <kbd>f</kbd> on it). A surface shows what *both*
admit — `global.and(own)`, which merges groups on the same field (lists
intersect, exclusions unite) and names a contradiction under the bar. Every
bar is the same component (WAVE-12's menu made generic, `src/lens.rs`):
categories with counts (`facets`, Linear's: a category's counts ignore its
own clause, never the global one), pills, a text mode in FILTER-3's
language, and the global pills shown muted in each surface's bar. The
board windows over `counts` (FILTER-12); the nav shows each shortcode with
its matches and, inside, only the matches, each with the matches below it —
a ticket the filter excludes is not drawn, and the matches under it move up
to its nearest matching ancestor or the top of the shortcode, whose row is
then a heading (`tree { matches_only }`, FILTER-13/14/23); the
palette searches inside the global filter with a "Search everything" row,
and a ticket page says when its ticket is outside and filters its children
(FILTER-16). Each level rides the route's query — `g=` global, `n=` nav,
`f=` the surface's own — and the viewer's local storage, so global and nav
filters survive navigation and reload.

The menu builds a NOT (FILTER-19): a category's values open with an `is` /
`is not` switch (Title's `contains` / `does not contain`) showing the
clause's operator. Set to `is not`, it makes the clause a pill's toggle
makes, and values checked under it are excluded — Filter ▸ Kind, `is not`,
doc and spike is `not kind in (spike, doc)`. A flag (Ready, Blocked,
Commits) has no switch; its `yes` / `no` is the negation. Tab from the
search reaches the switch; Space or Enter flips it.

Menger's own documents are a filter (FILTER-21): `menger doc` admits the
tickets whose path starts with `_MENGER/` (each shortcode's board note) on
every filtered read, and every menu offers it as the flag **Menger docs**.
The nav's default is `not menger doc`, and `reset` returns to it; the
header's "menger docs" toggle is gone. The flag is a cell coordinate, so
stored form 6 gives `cells` a `menger` column: a form-5 file is built again
once on first open, and an older build cannot open it after.

**Saved filters** (FILTER-6): `Saved ▾` beside each level's `Filter`
button (the global one in the header) lists the saved filters by name; a
name applies its filter to that level (a surface's own categories cleared),
`rename` and `remove` act on it, and `Save as…` keeps the level's filter
under a name. They live in the tracker, so every browser and agent reads
the same ones: the collection is `VIEW-1`, a doc titled "Saved filters"
under `FILTER-1` (the first save creates it), whose body lists them in
order as `- <m-demand id="open-builds">open builds</m-demand>` (DEMAND-1's
element); each is a doc under `VIEW-1` at `_MENGER/view/<slug>`, titled with
its name, whose body is `filter: <canonical text>`. A name already saved
(compared by slug) offers to replace that filter or to choose another
name; a rename changes the doc's title and the line (the path stays);
remove takes the line out and leaves the doc, unlisted — saving that name
again takes it over. The wave board's Wave, Readiness, Off-board blockers
and Title (the inputs page's State and Removed) are not in the language,
so Save as… leaves them out and says so. The app reads `VIEW-1` when a
menu opens — nothing at startup — and, once read, follows its `amended`.

**A tag is a filter** (FILTER-22, `src/tags.rs`): a right-click on any tag
the app draws — a shortcode, a ticket id, a status, a kind, a severity, on
the ticket page, the board, the nav and the wave board — opens a menu at the
pointer: **Filter …** makes the filter exactly that clause at once, **Add …
to the filter** joins it to the filter as it stands, **Build a filter from
…** makes it the clause and opens the filter menu on its category. A
shortcode's clause is its tree (what checking it in the menu makes); a
ticket id offers its shortcode first and `under` the ticket second. It
builds the surface's own filter, or on the ticket page the global one; a
tag inside a row is the tag's, the rest of the row keeps the ticket menu.
The palette takes `filter <shortcode>`: the global filter becomes it.

```sh
synapse -P 4455 menger tickets get --id VIEW-1                          # the list
synapse -P 4455 menger tickets get --id VIEW/_MENGER/view/open-builds   # one filter
```

### Screenshots (DEPLOY-7)

`scripts/shot.mjs` is the one way to look at the app from a script — use it
rather than a throwaway Playwright file. It waits on the app's own signals,
never a timer: `#conn` reading `live`, the nav and the route drawn (no
`loading…` left in `<main>`), every framed body's `menger:size { settled:
true }` (diagrams drawn, calls answered), fonts and images loaded in the
page and every frame, and the layout still for 600 ms. A page that never
gets there fails with what was still pending. It never clicks, types or
writes, so it is safe on prod; where the environment redirects to `/login`
(test) it signs in once — the smoke account from `~/.config/menger/test.env`,
or `--as USER` through the dev sign-in (IDENTITY-35).

```sh
scripts/shot.mjs '#/ticket/DESIGN-3' --env prod --theme both --width 1440,400
scripts/shot.mjs '#/board' --env test --as viewer --screen --out /tmp/look
scripts/shot.mjs '#/ticket/FILTER-13' --port 4550 --selector '#ticket-page'
scripts/shot.mjs mock.html --wait-for '#drawn'      # a local file: no app checks
scripts/shot.mjs --help
```

Files land in `$TMPDIR/menger-shots/` unless `--out` says otherwise, named
`ENV.ROUTE.THEME.WIDTH.png` (`prod.ticket-DESIGN-3.dark.400.png`); the
paths are printed on stdout. The whole page is the default: the app scrolls
inside its content column, so the window grows to the tallest scroller
(up to 16,000 px). Playwright comes from `perf/node_modules`
(`cd perf && npm ci`).

## Surface

menger is DB-first: the database is the system of record, and the service
is CRUD over tickets plus a live subscription. Two activations: `tickets`,
and (COMMENT-2) `comments` — below, after `tickets`. Statuses use the house ticketing vocabulary; every ticket also
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

A kind is set at birth and changed only by **`set_kind`** (SDLC-13),
which moves everything kept by kind — the roll-ups and their groups, the
status counts, the ready set, the placements, the filter cells, the kept
tree — in the one write, and `--verify-derived` holds it to a scan like
any other. The status goes with the kind across the doc line: to a doc it
becomes `Doc` (a `status` other than `Doc` is an in-band `error`), and a
ticket that waits on a blocker, or that a ticket waits on, is refused,
naming them; from a doc it becomes the `status` given (never `Doc`), else
`Epic` for a scope and `Pending` for any other kind; between two kinds of
work it stays unless one is given. A shortcode's root never becomes a kind
that roots nothing (refused, naming the shortcode; doc ↔ scope it may), a
ticket holding a path address (WAVE-10) stays a doc, and a lower-seq
ticket made a scope or a doc becomes its shortcode's root:

```sh
synapse -P 4455 menger tickets set_kind --id IDENTITY-1 --kind scope
synapse -P 4455 menger tickets set_kind --id MNG-40 --kind gap --status Ready
```

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

A ticket can define **components** (COMP-3, COMP-1): custom elements
written in its body, usable as a tag in any ticket. The server keeps the
**registry** — every component, its source, its owner and what uses it.

````markdown
```component m-wave-summary
<template><div class="summary"><slot></slot></div></template>
<style>.summary { font-weight: 600 }</style>
<script>/* the element class's `this` */</script>
```
````

- **A definition** is a fenced block whose info string is `component`
  and then the tag (` ```component m-name `, or a `~~~` fence; what follows
  the tag is the preview line — below); its **source** is the block's content exactly
  as written between the fences. The tag is a custom element's name:
  lowercase ASCII, starting with a letter, holding a hyphen (`m-x`,
  `m-wave-summary`; not one of HTML's eight reserved names). A
  ` ```component ` block that names no such tag, or a body defining one
  name twice, is an in-band `error` on the `create` or `amend` that carries
  it, and nothing is written. A block inside an indented code block or
  inside a fence of another language (` ````markdown `) is that example's
  text, not a definition.
- **One owner**: the ticket whose body defines it. Another ticket's body
  defining an owned name is refused in-band, naming the owner. The owner
  amending the block replaces the source.
- **A reference** is the tag, `<m-name`, as HTML in a body — an HTML block
  or inline HTML as the markdown renderer finds them, the name folded to
  lowercase as HTML does — outside code spans, fences, indented code,
  HTML comments, the frontmatter, and the definition's own block. A ticket
  references a name once however often it writes it. A component's
  **source** is HTML through and through: each `<m-other` in it (its
  `<script>` included, outside comments) is a reference by that component —
  never by itself. A reference made before the name is defined counts from
  the definition's birth.
- **Orphans**: when the owner's body loses the block and something still
  uses the component, it stays, ownerless (`owner: null`), read-only,
  source kept. It is kept while a ticket references it or a live component
  — owned, or a kept orphan — does; the write that takes its last such
  reference removes it (and anything only it kept, and orphans that only
  reference each other). With no reference at all, losing the block
  removes it at once. A ticket that defines an orphan's name **adopts** it:
  owner that ticket, source its block, references as they were.
- Kept by the store on `create` and on an `amend` of the body, in the
  write's transaction, from the new body and the registry's own record of
  what the ticket defined and referenced (the old body is not read); bodies
  are stored exactly as written. Stored in a `components` table (`name`,
  `source`, `owner`, `references`), which a database written before COMP-3
  gains on first open, durably, once, with the registry built from every
  body in the store's order — a name two bodies define is the earlier
  one's (the later is logged, and refused when it next changes its body
  while still defining it); a malformed block is logged and skipped. The
  table also holds the names referenced before anyone defined them (`source`
  null), which never reach the wire.

**Docs** (COMP-8, COMP-1 decision 6). A source may hold one `<docs>`
element, at its top level (outside `<template>`, `<script>`, `<style>` and
comments; anywhere among them) — the component's help, served on the wire
as `docs` (`null` without one):

````markdown
```component m-wave-summary board=RETURNS label="Platform team"
<template>…</template>
<docs>
  <summary>How many of a board's tickets are workable now (wave 1).</summary>
  <attr name="board" type="shortcode" required>The shortcode to count, e.g. RETURNS.</attr>
  <attr name="label" type="string">Shown before the count.</attr>
  <slot name="title">Optional title markup.</slot>
  <example><m-wave-summary board="RETURNS"></m-wave-summary></example>
  Longer help, as markdown prose.
</docs>
<script>…</script>
```
````

| in `<docs>` | on the wire, `docs.…` |
| -- | -- |
| `<summary>` (at most one) | `summary` — its text: tags dropped, whitespace collapsed, trimmed (`""` without one) |
| `<attr name type? required?>` | `attrs[]` — `{ name, kind, required, description }`, in order; `name` lowercased; `kind` the `type`: `string` (the default), `number`, `boolean`, `shortcode` (an existing shortcode), `ticket` (an existing id) or `enum(a\|b\|c)`, internally tagged — `{"type": "shortcode"}`, `{"type": "enum", "values": ["a", "b", "c"]}`; `required` whether the attribute is present |
| `<slot name?>` | `slots[]` — `{ name, description }`; `name` `null` for the default slot |
| `<example>` | `examples[]` — each one's inner HTML as written (its common indentation and surrounding blank lines removed) |
| everything else | `help` — markdown: the four elements' lines removed, the rest de-indented, runs of blank lines one |

The four are the `<docs>` element's own children: one inside another
element of the help (`<details><summary>`) or a backtick code span or fence
is help. A malformed `<docs>` — two of them, one never closed, two
`<summary>`s, an `<attr>` without a `name`, an unknown `type`, an `enum()`
naming nothing, an attribute or slot declared twice — is an in-band `error`
on the `create` or `amend` that carries it, naming the component and the
problem, and nothing is written.

**The preview line**: what follows the tag on the opening line is
attributes as on a tag — `board=RETURNS label="Platform team" compact` —
served as `preview`, `[name, value]` pairs in order (names lowercased; a
value bare, double- or single-quoted; a bare name's value `""`); what a
live preview of the definition renders with. Changing only it moves the
component (`component_changed`). Stored with the component (a `components`
table from before COMP-8 gains the column on first open, each owned
component's read from its owner's body); the docs are parsed from the
stored source.

```sh
synapse -P 4455 menger tickets components
```

**In the app** (COMP-4, COMP-5) a body holding HTML or a component tag
renders in a sandboxed frame (no `allow-same-origin`) under menger's SDK
(`frontend/sdk/menger-sdk.js`, whose header is the protocol's reference),
and every component in the registry is a custom element there. A custom
element cannot be redefined, so each name's class delegates to its current
source: amending a definition re-renders every live instance in every open
frame, without a reload; an orphan keeps rendering; a component that left
the registry renders `unknown component m-x`. An instance is an open shadow
root holding the source's `<style>`s and its `<template>`'s content
(`<slot>`s take the element's children); the source's `<script>`s then run
as one function with the element as `this`:

| in a component script | |
| -- | -- |
| `this.root` | its shadow root |
| `this.attr(name)` | the attribute, or `null` — an attribute read this way re-renders the instance when it changes |
| `this.menger` | `window.menger` itself |
| `return () => …` | runs before the next render and when the element leaves the document; so does the end of every `menger.on` made while the script ran |

`window.menger` (1.4, frozen): `version`; **`tickets`** (COMP-10) — the
`tickets` activation's typed client (below); **`call(method, params)`** → a
Promise of the call's items (the stream's data events as the wire spells
them, an in-band `error` included), answered by the app over its own
connection — **read-only**: `get`, `list`, `find`, `waves`, `groups`,
`extents`, `blockers`, `health`, `components`, `inputs` (INPUT-4), with the
activation's parameter names (`menger.call('waves', { epics: ['PLEXUS']
})`, `menger.call('inputs', { ticket: 'INPUT-9' })`); any other method —
`answer` and `clear_answer` included — rejects with `read-only: …`, which
names the two writes and how they are made (below), and a parameter the
method does not take rejects naming it; **`on('change', cb)`** →
`cb(event)` for every `watch` event the app hears (`input_changed` too),
and returns the function that stops it; **`navigate(route)`** (COMP-9,
below); **`input`** (INPUT-4, frozen) — `ticket`, `submit` and (INPUT-9,
1.4) `clear`, the two writes (below). Its types are
`frontend/sdk/menger.d.ts`, published at `/app/sdk/menger.d.ts`.

**The typed SDK** (COMP-10, COMP-1 decision 7). `menger.tickets` is the
client the house generator writes from the activation's schema —
`synapse-cc` → `hub-codegen`'s TypeScript target, never a hand-written
mirror — over the same read-only bridge:

```js
for await (const ev of this.menger.tickets.waves(['RETURNS'])) {
  if (ev.type === 'wave_counts') count = ev.waves[0] ?? 0;
}
const all = await Array.fromAsync(menger.tickets.components());
```

It is hub-codegen's surface as generated: every method of the activation,
camelCased (`setStatus`, `linkCommits`, `readySet`), with the activation's
parameters positionally (required first, in the generated order —
`waves(epics, kinds?)`; the `.d.ts` says each), each an
`AsyncGenerator<TicketsEvent>` (`schema()` a Promise) whose events' keys the
generated runtime camelCases (`waitingOn`, `blockedBy`; `type` values stay
as the wire spells them). A read is answered as `menger.call` is —
`inputs(epic?, state?, ticket?)` among them (INPUT-4); every other method,
`answer` included, is present and throws a `PlexusError` "read-only: …" when
iterated — the host's allow-list is the only gate. `menger.call` stays, the
untyped escape hatch.

The generated client's only seam is its `RpcClient` (Layer 1): the bridge
transport (`frontend/sdk/bridge.ts`) implements it over the frame's
postMessage channel — the call's items become `data` items and a `done`, a
rejection an `error` item — so nothing generated is edited.
**`scripts/sdk-codegen.sh`** produces it all: it builds menger, starts a
throwaway server on its own ports (`SDK_CODEGEN_PORT`, default 4466–4468;
never 4450–4462) over a temporary database, and in the `plexus-codegen`
image runs `synapse-cc build` (generate, install, compile, smoke-test),
emits the client's declarations with `tsc`, typechecks `bridge.ts` and
`menger.d.ts` against them, and bundles the bridge transport with the
client (`bun build`, IIFE). It writes the snapshot into
`frontend/sdk/generated/` — the generated sources and `.d.ts`,
`manifest.json` (each method's parameter names, from the IR) and
`sdk-bundle.js` — and splices the bundle into `menger-sdk.js` between its
`@generated-begin` / `@generated-end` lines, where it assigns the shell's
own `mengerTyped` and defines no global. All of it is committed: `cargo
build`, `trunk build` and the tests need neither Docker nor Node. A run
takes ~4 s in the container (plus menger's build); a second run with
nothing changed changes nothing. **Run it whenever the activation's surface
changes** — the drift guard, `tickets-api/tests/sdk_manifest.rs`, holds the
activation's methods and parameter names to `manifest.json` and fails
`cargo test --workspace` with "run scripts/sdk-codegen.sh" until it is; a
native test in `frontend/src/frame.rs` holds the spliced region to
`sdk-bundle.js` (never edit it in place) and the bundle to the manifest.

```sh
scripts/sdk-codegen.sh        # needs Docker and the plexus-codegen image
```

**`#/components`** (COMP-6; linked from the header beside `waves`) lists
the registry live: a row per component — its tag and (COMP-9) its summary
or "no docs yet", its owner (a ticket link) or **orphan**, its reference
count with each referrer as a link, its source folded. `#/components/<tag>`
is one component — COMP-9: its **help page**: its docs card (below,
framed, examples and preview live), then what references it, then its
source, open.

**Definitions render, instances link to them** (COMP-9, COMP-1 decisions
5–6):

- **The docs card.** A body holding a definition frames, and the
  ` ```component m-x ` block renders inside the frame as its card
  (`frontend/src/card.rs`, built from the block itself — a draft's docs
  show as they are typed, a malformed one as the problem): `<m-x>` and its
  summary · "N references ↗" (to the help page, kept live by the SDK) · a
  **live preview** with the opening line's preview attributes · the
  attributes (name, type, required, description) · the slots · each
  example rendered live beside its markup · the help prose (markdown) · the
  source, folded. The card is addressable: **`#/ticket/<OWNER>?component=m-x`**
  scrolls the app to it and flashes it.
- **Badges and tooltips.** Every component instance in a frame wears a
  small corner badge, `m-x ↗`, shown on hover or keyboard focus and laid
  over the page (absolutely placed at the instance's top-left corner — it
  moves nothing). Hovering or focusing the badge opens a tooltip: the name,
  its owner (or orphan), the summary, and each declared attribute with this
  instance's value — a missing required one, or a value that does not fit
  its `AttrKind` (a number; a boolean; a shortcode that exists; a ticket id
  that exists; one of an enum's values), flagged with the reason (`no
  shortcode 'NOPE'`) and the badge turned to the error colour — with
  "definition ↗" (the owner's card; for an orphan, the help page) and
  "docs ↗" (the help page). A component without docs: its name and "no docs
  yet". The badge is focusable: Enter (or a click) follows "definition",
  Esc closes the tooltip. The host fills the tooltip (`menger:inspect` →
  `menger:inspected`, `frontend/src/inspect.rs`): the registry and the
  board's extents say what exists, and the checks are
  `AttrKind::check` / `AttrDoc::state` in `tickets-api` (wasm-safe).
- **`menger.navigate(route)`** (SDK 1.2) moves the app to an app route —
  `#/ticket/<id>` (with `?component=<tag>`), `#/components`,
  `#/components/<tag>`, `#/waves`, `#/waves/<SHORTCODE>` (with a board's
  query); anything else is ignored. The badge and the tooltip's links use
  it, and so does a click on any `#/…` link in a framed body; a `{wave: ID}`
  clicked in a framed board note reveals the card on the board, as in an
  unframed note.

The order of work for a shortcode tree — the platform's, here — is one call
(WAVE-4); the app's wave board (`#/waves/<SHORTCODE>`) makes the same one:

```sh
synapse -P 4455 menger tickets waves --epics PLEXUS,MACRO,SYNAPSE,TRANSPORT,STARTER,HYPERFORGE,RETURNS,CONTEXT
```

A reader can **answer a ticket in place** (INPUT-1): a body declares
**inputs**, and the server keeps every answer, with its history, in an
**inputs store** beside the tickets — a click never rewrites a body
(INPUT-3).

```html
<m-decision data-field="decision" id="landing">          <!-- kind="choice" is the default -->
  <m-question data-field="question">Expired events: archive or delete?</m-question>
  <m-option data-field="option" name="Archive" value="archive" recommended></m-option>
  <m-option data-field="option" name="Delete"></m-option>  <!-- its value: its name, "Delete" -->
  <m-recommendation data-field="recommendation">Archive: holds are real.</m-recommendation>
</m-decision>

<m-input id="checks" kind="multi">
  <m-question>Which checks ran?</m-question>
  <m-choice value="unit">Unit tests</m-choice>
  <m-choice value="wire">The wire test</m-choice>
</m-input>

<m-input id="notes" kind="text"><m-question>Anything else?</m-question></m-input>
```

- **An input** is an `<m-decision>` or an `<m-input>` with an `id` (not
  blank), unique in its ticket; without one it is not an input — every
  decision written before INPUT-3 is not. Its **kind**: an `<m-decision>` is
  a `choice` (`kind` omitted or `"choice"`); an `<m-input>` says
  `kind="multi"` (any of its options, a set) or `kind="text"` (a free reply).
- **Options**, in the body's order: an `<m-decision>`'s `<m-option>`s — the
  `value`, else the `name`, labelled by the name — and an `<m-input>`'s
  `<m-choice>`s — the `value`, labelled by its text; `recommended` marks
  one. A text input has none. Its **question** is its `<m-question>`'s text;
  its **recommendation** the `<m-recommendation>`'s, else the label of the
  option marked `recommended`. An input's parts are inside it, not inside a
  nested `<m-decision>` / `<m-input>`.
- **Read as the app renders the body** (`tickets-api/src/inputs.rs`, with
  ART-5's scan): raw HTML only, so markup in a code span, a fence or
  indented code — an example — is never an input; outside comments and the
  frontmatter.
- **Refused in-band**, naming the input, by the `create` or `amend` that
  carries it — nothing written, title included: two inputs with one id; a
  kind its element does not take (an `<m-decision kind="multi">`, an
  `<m-input>` with no kind or `kind="choice"`); an option with no value; an
  option value declared twice; a text input with options.
- **Every body write re-reads them**: a new input is born `open`; one
  declared again takes the new declaration and keeps its history; one the
  body no longer declares is **`removed`**, history kept — declared again, it
  is back in the state its history says. An answer stays what was recorded
  when a later body changes the options it named; a new answer is checked
  against the new ones.
- **States**: `open` → `answered` (changeable) → `closed` (frozen: no
  answer, no clear, until `reopen_input`) — and `removed`. The state is
  the history's: the last `answered` / `cleared`, the last `closed` /
  `reopened`.
- **History**: every `answer`, `clear_answer`, `close_input` and
  `reopen_input` is one entry — `{ action, value, rationale, source,
  at_unix }`, oldest first; `source` is `app` (a viewer's click, which the
  host says on an `answer` or, INPUT-9, a `clear_answer`) or `api` (the
  default; `close_input` and `reopen_input` are always `api`). Each
  `answer` is recorded, the same value again included; a clear, close or
  reopen that changes nothing (no answer, closed already, not closed)
  writes nothing and answers `input`.
- **Stored** in two tables, `inputs` (a row per input: `ticket`, `id`,
  `position`, `kind`, `question`, `options`, `recommendation`, `state`) and
  `answers` (a row per history entry), which a database written before
  INPUT-3 gains on first open, durably, once, with every body's inputs read
  in (each `open`; a malformed one logged and skipped). Held whole in the
  cache.

An answer's **value** is typed by its input's kind — internally tagged:
`{"type": "choice", "option": "archive"}`, `{"type": "multi", "options":
["unit", "wire"]}` (kept in the body's order, each once; `[]` is "none
ticked"), `{"type": "text", "text": "…"}` (not blank). Through synapse its
fields are dotted flags; a `multi`'s list goes in `-p` (synapse 3.14.3 sends
a list field inside a tagged param as one string):

```sh
synapse -P 4455 menger tickets inputs --state open             # what is still waiting
synapse -P 4455 menger tickets inputs --ticket INPUT-9
synapse -P 4455 menger tickets answer --ticket INPUT-9 --input landing \
  --value.type choice --value.option archive --rationale "holds are real"
synapse -P 4455 menger tickets answer --ticket INPUT-9 --input notes \
  --value.type text --value.text "Yes, ship it, today"
synapse -P 4455 -p '{"ticket": "INPUT-9", "input": "checks", "value": {"type": "multi", "options": ["unit", "wire"]}}' \
  menger tickets answer
synapse -P 4455 menger tickets close_input --ticket INPUT-9 --input landing
```

**`#/inputs`** (INPUT-6; linked from the header beside `components`) is
what is waiting on the owner across every ticket, live. Inputs are grouped
by state — **open** first, then **answered**, then **closed**; the
**removed** only behind the `removed` toggle, last — each group in the
store's order. A row is one input: its ticket (a link to `#/ticket/<id>`)
and id, its kind, its question, its options with the recommended one marked
(★) and the chosen ones ticked (✓), the recommendation where it says more
than that option's label, the current answer — value, rationale, source
(`app` / `api`) and time (relative; exact on hover) — and its history,
folded. FILTER-16: the filter bar is the app's generic one at the `inputs`
level — the page's own categories, **State** (open · answered · closed:
which groups show) and **Removed** (shown: the removed group, last), then
the ticket categories (Shortcode, Status, Kind, …), which filter the inputs
by their ticket together with the global filter: the rows are then `inputs
{ filter }`, kept live (`member` for a ticket it does not know, the
membership watch's `entered` / `left`). The menu counts the page's own
categories on its rows and the ticket categories by `facets`. The filter
rides the route's query as every level's does —
`#/inputs?f=in%20(INPUT%2C%20MNG)&state=is:open,answered&removed=is:yes` —
and is kept per viewer; an INPUT-6 link (`#/inputs?sc=INPUT,MNG&state=open,
answered&removed`) still opens the filter it named and is rewritten as `f=`.

The page is the app's own copy of the inputs (`frontend/src/inputs.rs`),
held to the component registry's rule (COMP-5): **nothing at startup asks
for it** — the five startup requests are unchanged — and the page's first
opening asks `inputs {}` once (every state, in the store's order; not
`--state open` alone, since every group is shown and a row moves between
them). From then on `watch` keeps it: an `input_changed` carries the whole
input and is applied as an upsert — in place, or after its ticket's inputs
when it is new — so a row moves between groups as the answer lands; the
events heard while a fetch is out are replayed onto its answer, and it is
asked again on every reconnect and every `lagged`. The header's `inputs`
link wears the **open count** as a small badge from the first time the page
is opened, live from then on, after the page is left too; before that it
shows no count rather than add a startup request.

**Answering and clearing from a frame** (INPUT-4, INPUT-9; INPUT-1
decision 3, as amended). A frame is read-only but for two writes: a
viewer's click in a framed body records an answer to an input of **that
body's own ticket**, through `tickets.answer` with source `app`, or clears
the answer it holds, through `tickets.clear_answer` with source `app` —
each made by the host. They are the SDK's **`menger.input`** (1.3: `submit`;
1.4: `clear`):

```js
// in a component script — `this.menger` is `window.menger`
button.onclick = () =>
  this.menger.input.submit('landing', { type: 'choice', option: 'archive' }, 'holds are real')
    .then((answer) => show(answer), (e) => complain(e.message));
// answer: { value, rationale, source: 'app', atUnix } — the generated `Answer`

clear.onclick = () =>
  this.menger.input.clear('landing').then((input) => show(input), (e) => complain(e.message));
// input: the generated `Input`, `open`, its history ending { action: 'cleared', source: 'app', atUnix }

// this ticket's inputs, typed (`ticket` is null in a frame that renders no ticket's body)
for await (const ev of this.menger.tickets.inputs(undefined, undefined, undefined, this.menger.input.ticket)) { … }
```

- **`menger.input.ticket`** — the ticket whose stored body the frame
  renders: the ticket page's, a board note's doc. `null` in a frame that
  renders none — the editor's preview of a draft, a component's help card —
  where `submit` always rejects. The host stamps it on the `<iframe
  data-ticket>` and in the init block; the frame cannot name another.
- **`menger.input.submit(id, value, rationale?)`** → a Promise of the answer
  recorded, in the typed client's spelling (the generated runtime's own
  transform, `atUnix`); `value` is an `InputValue`, as `answer` takes it.
  The SDK sends it (`menger:submit`) **only inside a viewer's gesture** —
  the frame's transient user activation (`navigator.userActivation.isActive`,
  the getter taken before the body's scripts run), or, in a browser without
  that API, a trusted click or key the SDK saw in the last 5 s; otherwise it
  rejects — `only inside a viewer's click … nothing was sent` — so a
  component that calls it on load or from a timer learns so at once.
- **The host checks, in order** (`frontend/src/bridge.rs`), and refuses with
  the reason, sending nothing: **a gesture is under way** — the app window's
  own transient activation, which a click or key in a frame grants its
  ancestors too, read the moment the message arrives (a browser without the
  API cannot say, and the SDK's check stands alone); **the frame renders a
  ticket** (its `data-ticket`); the params are exactly `{ input, value,
  rationale? }` (a `ticket` among them is refused); then, against that
  ticket's inputs as the store holds them (`inputs { ticket }`), **the input
  is declared in its body** — one only another ticket declares is not this
  frame's to answer — **neither closed nor removed**, and **the value answers
  it** (`inputs::check`, what the server runs: one of a choice's options,
  any of a multi's, a reply that is not blank). Then it calls
  `tickets.answer { ticket, input, value, rationale, source: "app" }` and
  answers the frame `menger:submitted { id, answer }`, or `{ id, error }` —
  the host's refusal or the server's own in-band one; the promise rejects
  with it. The answer reaches every frame, the one that made it included, as
  `input_changed` on `menger.on('change')`.
- **`menger.input.clear(id)`** (INPUT-9, SDK 1.4) → a Promise of the input
  as the clear left it — the generated `Input`: `open`, no answer, the
  clear last in its history (as it was, when it held no answer: the server
  writes nothing). Sent (`menger:clear { id, params: { input } }`) under
  `submit`'s gesture rule, and checked by the host as a submit is, **but for
  the value**: a gesture under way, a frame that renders a ticket, params
  exactly `{ input }`, the input declared in that ticket's body and neither
  closed nor removed. Then `tickets.clear_answer { ticket, input, source:
  "app" }`, answered `menger:cleared { id, input }` or `{ id, error }`; the
  clear reaches every frame as `input_changed`.
- **Every other write stays refused**, through `call` and the typed client
  alike — `answer` and `clear_answer` too: a frame answers only through
  `submit` and clears only through `clear`.
- **The house components** (ART-9's `m-decision`, `m-input`): **Submit**
  calls `submit`; **Clear** calls `clear` when an answer is recorded (the
  history then shows the `cleared` entry, via `app`) and only empties the
  form when none is. On an app before SDK 1.4 (no `clear`), Clear empties
  the form and says the recorded answer stays.
- **The limit.** The gesture rule is the browser's: the host can know that
  the viewer is clicking or typing in the app now, not which control a
  click meant. A script in the frame (the body's, or a component's) can
  post a `menger:submit` or `menger:clear` of its own while a gesture is
  under way — for an input its ticket declares (with a value that answers
  it, to submit), recorded as `app`. Who answers (identity, permissions)
  waits on CONTEXT.

| method | yields |
|---|---|
| `tickets.health` | `health { version, unix_now, ticket_count }` |
| `tickets.create { epic, title, kind?, severity?, blocked_by?, body?, parent?, path? }` | `created { ticket }` — id is the next `EPIC-N`, status starts `Pending`; `kind` defaults to `build`, one outside the six is an in-band `error` naming them; a `parent` that does not exist (or would make the ticket its own ancestor) is an in-band `error` naming the reason; **every shortcode starts with a scope** (MNG-12) **or a doc** (SDLC-11): the first ticket of a prefix must be `kind: scope` or `kind: doc`, anything else is an in-band `error` naming the rule and nothing is allocated; a `doc` starts — and stays — in `Doc`, and a doc with a `blocked_by`, a `blocked_by` naming a doc, or a doc born as an id a `blocked_by` already names is an in-band `error` (SDLC-11); **`parent` defaults to the shortcode's scope root and must stay inside the shortcode** (MNG-14): omitted, a non-root hangs under its prefix's root; named, it must be in the same prefix, else an in-band `error` — the scope root itself is the exception, its parent (if any) is the shortcode it hangs under, a sub-shortcode; **`path`** (WAVE-10) gives the ticket a path address in its shortcode — only `_MENGER/…` with something after it, only on a `doc`, only with the shortcode's scope root as `parent` (the default); one another ticket of the shortcode holds is an in-band `error` naming it, as is each other refusal, and nothing is allocated; COMP-3: the body's ` ```component ` blocks register components — a block whose tag is not a component name, a name defined twice, or a name another ticket defines is an in-band `error` (the last naming that ticket) and nothing is allocated; COMP-8: so is a malformed `<docs>`, naming the problem |
| `tickets.get { id }` | `ticket` or `not_found` — `id` is an `EPIC-N`, or (WAVE-10) a path address `SHORTCODE/path` (`PLEXUS/_MENGER/board`; the shortcode case-insensitive, the path exact), and `not_found` carries the id or address as given |
| `tickets.list { epic?, status?, parent?, after?, limit?, summary? }` | one `ticket` per match, ordered (epic, seq), then one `page { next, remaining }`; `parent` yields exactly that ticket's children — the one way to read them (SCALE-4), from an index the store keeps, so a page of a root's 163,530 children costs the page (1.3 ms at 10⁶ over REST, first page or deep) and its `remaining` is arithmetic; `summary` swaps the rows for `summary { summary }` (every field but the body); `after` is the exclusive cursor and `limit` caps the page — `after` naming no ticket is one in-band `error` |
| `tickets.ready_set { epic?, after?, limit?, summary? }` | one row per member of the ready set (Ready, every blocker Complete or Superseded — never a doc), ordered (epic, seq), then one `page { next, remaining }` |
| `tickets.find { query, after?, limit? }` | one `summary` row per ticket whose id or title contains `query`, case-insensitively (ASCII folding) — the jump palette's rule (AGS-18), on the server (SCALE-6) — ordered (epic, seq), at most `limit` (default 50), then one `page { next, remaining }`: `remaining` plus the rows is the total. `query` is trimmed; empty or blank matches every ticket. `after` is `list`'s cursor (a position — it need not match); one naming no ticket is one in-band `error`. Ids and titles only, never bodies. A blank query's page costs the page; any other query's exact `remaining` scans every id and title after the cursor (~145 ms at 10⁶) |
| `tickets.set_status { id, status }` | `status_changed { ticket, previous }` — any of the nine, from any status (SDLC-7: the worker states `Coding` and `In Review` included); one outside them is an in-band `error` naming the nine; SDLC-11: on a doc, an in-band `error` (a doc has no lifecycle), and `Doc` on any other kind an in-band `error` too |
| `tickets.set_kind { id, kind, status? }` | `kind_changed { ticket, previous }` (SDLC-13) — the one write that changes a kind; everything kept by kind moves in its write. To a `doc`: status `Doc` (a `status` other than `Doc` is an in-band `error`), refused while the ticket waits on a blocker or a ticket waits on it (each named); from a doc: `status` if given (never `Doc`), else `Epic` for a scope and `Pending` otherwise; between two kinds of work the status stays unless given (`Doc` an in-band `error`). A shortcode's root becoming a kind that roots nothing is an in-band `error` naming the shortcode (doc ↔ scope is allowed); so is a path holder (WAVE-10) becoming work, and an unknown kind or status; an id naming no ticket is `not_found`. A lower-seq ticket made a scope or a doc becomes its shortcode's root. The kind (and status) it has is no write and no broadcast, answering `previous` equal to its kind. `watch` carries `kind_changed` first, then the write's `group_changed` (a root's whenever its kind or status changed; every row a re-root moved), `placements_changed`, and — filtered — `entered` / `left` |
| `tickets.amend { id, title?, body? }` | `amended { ticket, changed }` (MNG-16) — replaces the title and/or the body, whole values, last writer wins; `changed` names what moved (`["title"]`, `["body"]`, `["title", "body"]`); neither given, or the values already there, is no write and no broadcast and answers `changed: []`; an id naming no ticket is `not_found`; a title that is empty after trimming is an in-band `error` and nothing is written. Stored byte-exact, durable before it is acknowledged; `updated_unix` moves; nothing else about the ticket does — status, kind, edges, parent, id. COMP-3: a new body moves the component registry (definitions, references, orphans); a malformed ` ```component ` block (COMP-8: or `<docs>`), or a name another ticket defines, is an in-band `error` and nothing is written, title included |
| `tickets.link_commits { id, commits }` | `commits_linked { ticket, added }` (SDLC-9) — appends every commit reference in `commits` the ticket does not already carry, in the order given; `added` is what was newly linked, in order. Every entry already linked, or `commits` empty, is no write and no broadcast and answers `added: []`. An entry that is not a commit reference is one in-band `error` naming every such entry, and nothing is written — a call is all or nothing; so is a link that would put more than 256 commits on the ticket. An id naming no ticket is `not_found`. Durable before it is acknowledged; `updated_unix` moves, nothing else does |
| `tickets.unlink_commits { id, commits }` | `commits_unlinked { ticket, removed }` (SDLC-9) — every entry the ticket carries is removed; `removed` names them in the order given; none of them linked is no write, `removed: []`. The same grammar, the same all-or-nothing `error`, `not_found` |
| `tickets.set_parent { id, parent }` | `parent_changed { ticket, previous }` (MNG-12) — files an existing ticket under `parent`: the child's row is written, and it leaves the old parent's children and joins the new one's (SCALE-4: neither parent's row is rewritten — SCALE-28 stores each one's new `child_count` on it, in the move's transaction; `watch` carries `children_changed` and `child_removed` with each parent's new `child_count`); the same guards as `create` (`not_found`, must exist, inside the shortcode unless the ticket is its root, no self-ancestry — in-band `error` naming the reason); the parent it already has is a no-op |
| `tickets.groups {}` | one `group { group: { epic, total, done, docs, root, root_row, host } }` per shortcode row the nav draws, in shortcode order, then the stream's own `done` (MNG-10) — the nav's tree without its rows: `total`/`done` count the row's whole subtree as AGS-20's `plan()` nests it (sub-shortcodes and cross-shortcode children included; `done` is `Complete` only; SDLC-11: the work only — `docs` counts the docs, and a row of docs alone is drawn), `root` is the lowest-seq `scope` (or `doc`), `root_row` the root's `{ title, status, kind }` (AGS-23: the nav draws the shortcode as the root's row; `null` with no root), `host` the ticket a sub-shortcode's row nests under (`null` at top level). Kept by the store on every write, so the cost is the number of shortcodes, not tickets |
| `tickets.components {}` | one `component { component: { name, source, owner, references, docs, preview } }` per component, in name order, then the stream's own `done` (COMP-3) — `source` the ` ```component name ` block's content, `owner` the defining ticket (`null`: an orphan), `references` the tickets whose bodies use `<name`, in the store's order, then the components whose sources do, by name; COMP-8: `docs` the source's `<docs>` parsed (`{ summary, attrs, slots, examples, help }`, `null` without one), `preview` the opening line's attributes (`[[name, value], …]`). From the cache; `watch` keeps it current with `component_changed` / `component_removed` |
| `tickets.inputs { ticket?, epic?, state? }` | one `input { input: { ticket, id, kind, question, options, recommendation, state, answer, history } }` per input of `ticket` (every ticket's without one) whose ticket is in the shortcode `epic` and which is in `state` (`open` / `answered` / `closed` / `removed`), then the stream's own `done` (INPUT-3) — in the store's order of tickets, each ticket's inputs in its body's order and the removed after; `kind` `choice` / `multi` / `text`; `options` `[{ value, label, recommended }]`; `question` and `recommendation` `null` without one; `answer` `{ value, rationale, source, at_unix }` or `null`; `history` every change, oldest first. A ticket that is none is `not_found`; a state outside the four an in-band `error`. From the cache; `watch` keeps it current with `input_changed` |
| `tickets.answer { ticket, input, value, rationale?, source? }` | `input_changed { input }` (INPUT-3) — records `value` (typed, above) against the input, checked against its kind and the options its body declares, with `rationale` (blank: none), `source` (`app` / `api`, default `api`) and the time; every call is one history entry. One in-band `error`, nothing written: the ticket declares no such input (naming those it does), the value does not answer it (naming why), it is closed, or it was removed; `not_found` for a ticket that is none. The body and the ticket's row are untouched |
| `tickets.clear_answer { ticket, input, source? }` | `input_changed { input }` — the answer cleared, `open` again, the clear in its history with its `source` (`app` / `api`, default `api`; INPUT-9: the host says `app` for a frame's `menger.input.clear`); `input { input }` when it held none (nothing written); in-band `error` when closed, removed or not declared, or for a source outside the two (INPUT-3) |
| `tickets.close_input { ticket, input }` | `input_changed { input }` — frozen, with or without an answer (a decision's DECIDED); `input { input }` when closed already; in-band `error` when removed or not declared (INPUT-3) |
| `tickets.reopen_input { ticket, input }` | `input_changed { input }` — `answered` again when it holds an answer, else `open`; `input { input }` when it was not closed; in-band `error` when removed or not declared (INPUT-3) |
| `tickets.extents {}` | one `extent { epic, last_seq }` per shortcode that holds a ticket, in shortcode order (the store's), then the stream's own `done` (SCALE-16) — where each shortcode ends: ids are allocated highest-plus-one and never deleted, so a shortcode holds `last_seq` tickets, row k of it is `EPIC-k`, and a position in the store's order becomes a `list { after }` cursor without a walk; every `last_seq` together sums to `health.ticket_count` (a hand-edited database with a hole still reports the highest sequence; `after` naming the missing id is `list`'s cursor error). Not a roll-up: the shortcode's own tickets only, and every shortcode has one whether or not the nav draws it a row. Read from the sequence `create` allocates from (SCALE-3), so the cost is the number of shortcodes; no `watch` event — `created` carries the shortcode's new `last_seq` as its `seq` |
| `tickets.waves { epics, kinds? }` | the order of work (WAVE-4): one `placed { summary, placement, waiting_on }` per ticket of the shortcodes `epics` (case-insensitive; one with no tickets contributes nothing) whose kind is in `kinds` (default `build`, `spike`, `gap`; a doc is never placed), then exactly one `wave_counts { waves, missing, done }` — `waves[i]` counts the placed tickets in wave `i + 1`. `placement` is computed over the **whole** `blocked_by` graph by `ready_set`'s rule (MNG-4), blockers followed into any shortcode, asked for or not: `{"type": "done"}` when the ticket is `Complete` or `Superseded`; `{"type": "missing_blocker"}` when it, or anything it waits on, names a blocker that is no ticket (or, only by a hand edit, sits on or behind a `blocked_by` cycle); otherwise `{"type": "wave", "n": n}` — 1 with every blocker satisfied, else 1 + the highest wave among its unsatisfied blockers. `parent` orders nothing, and a `Blocked` status is a marker, not an edge. `waiting_on` is the ticket's unsatisfied direct blockers (missing ids included), in `blocked_by` order, distinct; `[]` when done. Order: waves ascending, inside a wave `Coding`, `In Review`, `Ready`, `Pending`, `Blocked`, `Idea`, `Epic`, then (epic, seq); then the missing lane; then the done — each in (epic, seq). `epics` empty, or a kind outside the six, is one in-band `error` and nothing else. WAVE-6: read from the placements the store keeps (below) — the cost is the placed tickets — and `waiting_on` worked out per row |
| `tickets.watch { epic? }` | open-ended stream of `created` / `status_changed` / `kind_changed` / `amended` / `commits_linked` / `commits_unlinked` / `unlocks_changed` / `children_changed` / `parent_changed` / `child_removed` / `group_changed` / `placements_changed` / `component_changed` / `component_removed` / `input_changed` — INPUT-3: an input a write moved is one **`input_changed { input }`** — the whole input, its state, answer and history, so applying it is one upsert — after every other event of the write, the component events included: an `answer`, `clear_answer`, `close_input` or `reopen_input` (its one event), or a `create` / `amend` whose body declared, re-declared or removed inputs (in the body's order, the removed after); `epic` filters it by its ticket's shortcode — COMP-3: a `create` or `amend` that moved the component registry ends — after every other event of the write, `placements_changed` included — with one **`component_changed { component }`** per component it registered or moved (source, owner, references: the whole component, so applying it is one upsert), in name order, then one **`component_removed { name }`** per component it took out; a component is in no shortcode, so both pass any `epic` filter — each carries the whole affected ticket but the parent's side of a birth or a move, `children_changed { id, added, child_count }` / `child_removed { id, removed, child_count }` (SCALE-4: under 1 KB whatever the parent's size; it was the whole parent); a write that moves a roll-up is followed, after its ticket events, by one `group_changed { group }` per row that moved, innermost first (a row no longer drawn arrives once with `total: 0`) — WAVE-6: and a write that moved any kept placement ends with one **`placements_changed { changes: [{ id, placement }] }`**, after its other events (`epic` keeps the changes in its shortcode, and the event only when one is left) — AGS-23: so is a shortcode root's status change (any, not only across `Complete`) or new title, which moves its group's `root_row`; `epic` filters a group by its own shortcode, a parent's side by the parent's; **`lagged { missed }`** (SCALE-5) where a subscriber more than the server's broadcast holds (256 changes) behind lost changes — once per gap, in place of the `missed` changes lost (`missed` ≥ 1, the channel's count), whatever the `epic` filter, then the oldest change still held, in order, none twice; not an error, the stream goes on — what the client holds may be stale, so it fetches again what it shows |

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

### Comments (COMMENT-2)

A **comment** is a reader's note on the app — a page, an element on it, a
ticket's body — kept by the server as a record of its own, beside the
tickets (COMMENT-1: not a ticket, never in a body), and reached as the
`comments` activation; its vocabulary (`Anchor`, `Comment`, `CommentState`,
`CommentsEvent`) and its `CommentsClient<T>` come from `tickets-api/` as the
tickets' do. An **anchor** is where it was left: `route` (the app page,
beginning `#/`), `path` (where on the page), `label` and `excerpt` (what was
there, in words; the excerpt may be empty), and `ticket` when the element
belongs to one — kept as that ticket's id with its `updated_unix` as
`version`, set by the server. A comment keeps its anchor whatever its ticket
goes through (an amend, a move, a status or kind change): nothing a
ticket's write does touches a comment.

| method | yields |
|---|---|
| `comments.add { anchor, body, author? }` | `comment { id, state, author, at, anchor, body }` — the comment as kept: the next id, `open`, the time (unix seconds), `owner` unless `author` is given. An empty body, a blank author, a malformed anchor (a `route`, `path` or `label` left out or blank, a route not beginning `#/`, a `ticket` that is none, a field past its bound) is one in-band `error` naming it, and nothing is kept |
| `comments.list { ticket?, under?, route?, path?, state?, author?, text?, after?, limit? }` | one `comment` per match, newest first, then one `page { next }` — `next` the id to pass as `after`, `null` when nothing more matches. Every filter given holds: `ticket`; `under` — a ticket or a shortcode in the comment's ticket's lineage (itself, its ancestors, its shortcode, the shortcodes hosting it: the filter language's `under`, as the tree stands); `route` exactly; `path` a prefix; `state` `open` / `resolved`; `author` exactly; `text` in the body, the label or the excerpt (any ASCII case). Blank is none. An unknown state, or an `after` naming no comment, is one in-band `error` |
| `comments.resolve { id }` · `comments.reopen { id }` | `comment` — the state flipped; already there, the comment as it is (nothing written or broadcast); an id naming no comment is an in-band `error` |
| `comments.watch {}` | open-ended stream of `added { comment }`, `resolved { comment }`, `reopened { comment }`; `lagged { missed }` as `tickets.watch` |

Kept in the `comments` table behind the service-wide cache: read whole at
open, every read from the cache, every write through it to the database
first. The table is stored form 7: a file in form 6 gains it, empty, once,
on its first open — in place, nothing derived built again (`COMMENT-2: the
file gained stored form 7 …` in the log); an older form is built again and
gains it with the rest. A form-6 build reads a form-7 file as its own.

```sh
synapse -P 4455 menger comments list --under FILTER --state open --text title
synapse -P 4455 menger comments add --anchor.route '#/ticket/DEMAND-2' --anchor.ticket DEMAND-2 \
  --anchor.path '[data-tag="ticket DEMAND-1"]' --anchor.label 'Parent chip' --anchor.excerpt DEMAND-1 \
  --body "show the parent's title too"
synapse -P 4455 menger comments resolve --id 1
# synapse expands a parameter named `path` as a file path: list's `path` goes in -p
synapse -P 4455 -p '{"path": "[data-tag=\"ticket", "state": "open"}' menger comments list
```

**In the app** (COMMENT-3): `a` (or Annotate in the header) is annotation
mode — every element outlined, ticket bodies' frames too; hovering names an
element (and shows a component's info card, which no longer appears on plain
hover); a click opens a comment box at it, nothing else; numbered pins mark
what has open comments, each listing them with Resolve; `a` or Esc leaves.
The anchor's `path` is a CSS selector, stable attributes first
(`#t-status`, `#t-parent [data-tag="id:DEMAND-1"]`, `m-ticket[ticket="RETURNS-1"]`),
positions last; inside a framed body it is the frame, ` >> `, then the
element in it (`iframe[data-ticket="RETURNS-2"] >> body > table > tbody >
tr:nth-of-type(1) > td:nth-of-type(1)`). A comment whose element is gone is
marked "moved" and pinned to its ticket's heading. `#/comments` lists every
comment, filtered by the route's query (`?ticket=&under=&state=&author=&text=`),
each linking back to its page and element (`…?comment=<id>`); a ticket page's
side panel counts its open ones. All of it is live through `comments.watch`.

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
| `mermaid.check { markdown }` | one `diagram { index, line, result }` per mermaid block (a fence, or ART-5's `<pre class="mermaid">` in a body's HTML), in order, then the end — `index` from 1, `line` the block's first source line in the markdown, `result` what `parse` would answer — so an error at `result.line` L is markdown line `line + L - 1`. No blocks, no items. A missing piece is one `error` in place of everything |

A mermaid block is the app's: a fenced block whose info string's first word
is exactly `mermaid`, frontmatter excluded — `diagrams/src/blocks.rs` copies
`frontend/src/md.rs`'s rule and its tests hold the copy to the original
(same functions word for word, same pulldown-cmark version and options,
every case md.rs's tests name). ART-5: a body holding HTML is drawn in a
frame (COMP-4), and there a diagram may also be written as HTML — ought's
form, `<pre class="mermaid">…</pre>` in an HTML block, alone or inside a
field element or a component (`<m-dag>`). The frame's renderer
(`render_body_framed`) puts a fence's placeholder in its place, so the
frame draws it, and `check` finds it by the same rule (`html_diagrams`,
copied verbatim): its source is the element's content, the newline after
the open tag dropped, character references decoded (`--&gt;` is `-->`), a
`<br/>` in a label kept; its `line` is the line that source starts on. A
fence inside a field element or a component is a fence when blank lines
stand around it. Not a diagram, drawn by neither: a `<pre class="mermaid">`
in code, a comment or a script, inline in a paragraph, or one its HTML
block does not close (a blank line inside one nested in a `<section>` ends
that block — write the `<pre>` without blank lines, or as a fence).

`line` within `source` can differ from the number in mermaid's message:
mermaid numbers the text it parsed, after dropping frontmatter, directives,
`%%` comments and leading blank lines; the Node program maps it back with
mermaid 11.17.2's own regular expressions.

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
                       (the app's build, read into a table — ACCESS-33);
                       nothing else
tickets-api/           the ticket vocabulary, the `tickets` activation and the
                       typed Plexus client it emits — ONE declaration, shared by
                       the server (default features: `server`) and the app
                       (default-features = false, wasm32)
  types.rs             Ticket · Status · Kind · Severity · TicketSummary · Group · Placement · TicketsEvent
                       · Input · InputKind · InputState · InputValue · Answer · InputHistoryEntry (INPUT-3)
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
    inputs.rs          the inputs store: each ticket's inputs as its body
                       declares them (removed ones kept), the plan a create or
                       amend makes of them, answer / clear / close / reopen as
                       history, its two tables, and the read of every body on
                       first open (INPUT-3)
    components.rs      the component registry: what a body defines and
                       references, the plan a create or amend makes of it
                       (orphans, adoption, the sweep), its table, and the
                       build from every body on first open (COMP-3)
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
    tests_inputs.rs    the inputs store: each row of its table, refusals,
                       history, removed, the query, events, durability, the
                       tables added to an old file (INPUT-3)
    tests_components.rs  the component registry: each row of its table, code
                       examples and code spans, events, durability, the
                       table built from the bodies of an old file (COMP-3)
  component_docs.rs    a component's help: its source's <docs> parsed
                       (summary, typed attributes, slots, examples, help)
                       and the preview line (COMP-8) — wasm-safe, shared
                       with the app's docs card (COMP-9)
  inputs.rs            a body's inputs (INPUT-3): <m-decision id> / <m-input
                       id kind>, their options, question and recommendation,
                       read with fields.rs's scan; an answer's value checked —
                       wasm-safe
  fields.rs            a body's fields (ART-5): its elements marked
                       data-field, parts nested, or a legacy body's ## headings
                       read through ought's field set — wasm-safe; the piece
                       COMPILE checks (fields_tests.rs: its tests)
  nav_plan.rs          test-only: the nav's plan(), the roll-ups' oracle
  activation.rs        #[plexus_macros::activation(client, server_cfg)]
  tests/sdk_manifest.rs  COMP-10's drift guard: the activation's methods and
                       parameters against frontend/sdk/generated/manifest.json
scripts/sdk-codegen.sh the frames' typed SDK, generated (COMP-10; Docker)
scripts/shot.mjs       screenshots of the app once it is ready, signed in where
                       it must be; only looks (DEPLOY-7)
web/index.html         the day-one reactive shell
frontend/              the Leptos CSR app (own cargo unit; trunk build → dist/) —
                       src/waves.rs the wave board (WAVE-5, WAVE-6),
                       src/filter.rs the generic filter bar and menu (WAVE-12,
                       FILTER-11), src/lens.rs the levels — global, nav,
                       board, waves, inputs — their URL and storage, the
                       header's bar (FILTER-11), src/counts.rs menu counts and
                       the header's n of N (FILTER-12), src/saved.rs saved
                       filters — Saved ▾, Save as…, VIEW-1 (FILTER-6),
                       src/notes.rs its annotation and the body editor (WAVE-11),
                       src/frame.rs framed bodies and their messages (COMP-4,
                       COMP-5), sdk/menger-sdk.js the SDK every frame runs,
                       sdk/menger.d.ts its types, sdk/bridge.ts the bridge
                       transport and sdk/generated/ the typed client
                       (COMP-10, scripts/sdk-codegen.sh),
                       src/registry.rs the app's copy of the component
                       registry, src/bridge.rs menger.call's read-only host
                       side (COMP-5) and the two writes' checks,
                       menger.input.submit (INPUT-4) and
                       menger.input.clear (INPUT-9), src/components.rs
                       #/components (COMP-6)
                       and the help pages, src/card.rs a definition's docs
                       card, src/inspect.rs the tooltip's checks and
                       menger.navigate's routes (COMP-9), src/inputs.rs
                       the app's copy of the inputs and src/inputs_page.rs
                       #/inputs (INPUT-6)
spike/WAVE-2/          WAVE-3's measurement of the kept placements (run.sh)
diagrams/              the `diagrams` service (COMPILE-4): its own binary and hub
                       (WS :4458); the `mermaid` activation and the client it
                       emits, the Node child it owns (src/node.rs), the app's
                       mermaid-block rule (src/blocks.rs: fences, and ART-5's
                       <pre class="mermaid"> in a body's HTML); node/ is the
                       Node program and its lockfile
```

## Status

Phase one of the backend epic (MNG) is complete; its checkpoint, the
verified state map, is a ticket in the tracker. Tickets
live in SQLite behind the service-wide cache (MNG-2); the DAG answers
ready-set/blockers queries with cycle rejection at create (MNG-4); REST +
SSE serve on :4457 via plexus-transport 0.2.3 (MNG-8 — consumed as a
git-tag patch until it's published to crates.io). The Leptos app at
`/app/` is the live board (MFE-3): every ticket as a row with a per-row
status control, pushed over SSE — read-and-status only, no creation
form by design (tickets are created by agents/CLI/API). Parked as Idea:
MNG-5/6 (schema-derivation and framework theses). Next: the default
page swap and the ready-set view (MFE), and the HelixDB graph-store epic
(HLX, spike-gated).

## History

`menger-hs` (0.04, Haskell) is the ancestor; its `web/` window manager —
tabs, tear-out, desks, fuzzy find — is the eventual consumer of the
framework this repo grows.

## License

MIT
