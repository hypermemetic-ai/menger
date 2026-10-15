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

**Hosts** (ACCESS-25). Every listener — the WebSocket, the shell, the REST
bridge, and `diagrams` — answers only its own names, so a page that rebinds
its own name to 127.0.0.1 (DNS rebinding) gets nothing: a request naming any
other `Host` is refused before anything routes it,

```
HTTP/1.1 421 Misdirected Request

this server does not answer to that name
```

Each port always answers `127.0.0.1:<its port>` and `localhost:<its port>`,
and `--public-url`'s host (as it names it, and its hostname on the port);
`--allowed-host` adds more — repeatable, `host:port` or `host` for a default
port, every listener; `MENGER_ALLOWED_HOSTS` with `;` between. `diagrams`
is loopback only and takes no list. The check is plexus-transport's (the
WebSocket upgrade and the REST bridge) and the shell's own, with or without
login:

```sh
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --rest-port 4462 \
  --db-path /tmp/menger-dev.db --allowed-host menger.lan:4461 --allowed-host menger.lan:4460
curl -si -H 'Host: evil.example' http://127.0.0.1:4462/debug   # 421
```

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
`docs`) form 3, … MENGER-12's (a cell's `asking` and labels) form 10 —
has it built once, on first open, durably, and the log says so
(`SCALE-28: stored the derived state for the first time …`); every later
start logs `read the derived state from the database … nothing rebuilt`.
A seed stores it after its last chunk of tickets; killed before that, the
file says its derived state is behind, and the next open builds it again.

```sh
menger --verify-derived --db-path /path/to/tickets.db
```

opens the file read-only, rebuilds all of it by a scan of its tickets,
compares them with what is stored — and (SEARCH-2) the search index
with the tickets' titles and text — and prints each difference — the
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

Code (MENGER-22) is always highlighted: every fenced or `<pre><code
class="language-x">` block — in a plain body, a framed body and `m-code`, a
comment, a board note, a component's docs card and its source on
`#/components`. The highlighter is highlight.js 11.11.1 (BSD-3-Clause),
**vendored** the same way at `frontend/vendor/highlight/` (its README: the
bundle's makeup, checksums, upgrade steps) and served at
`/app/vendor/highlight-11.11.1.min.js`, fetched the first time a page — or a
frame, from `highlight.url` in its init block — holds a code block
(`frontend/src/highlight.rs`; the frame SDK's `mountCode`). A block's
`language-x` names its language (rust, typescript, javascript, json, toml,
yaml, bash, sql, html, css, markdown, haskell, python, diff — and every other
language in the bundle); `text` or `plain` stays plain; a fence with no
language is guessed from those fourteen (`highlight::UNLABELED`, MENGER-22's
open decision `unlabeled`; a guess scoring under 2 stays plain). The colours
are the `--code-*` tokens (`keyword`, `string`, `comment`, `number`,
`title`, `type`, `meta`, `builtin`, `attr`, `variable`), mapped from the
`hljs-*` classes by index.html's rules and the SDK's base — no vendor
stylesheet — so a THEME-8 theme, a region's scope or the light/dark toggle
recolours code with no parse again. An unknown language, a bundle that will
not load or a highlighter that throws leaves the block as written; an
amended or still-streaming body is highlighted as it lands. Whole-document
bodies (MENGER-15) are their own pages and are left as they are.

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
Beside each `find` it asks `search` (SEARCH-3): below the jump rows, an
"In bodies" section with the best eight hits no jump row shows, each with
its passage, the matched words marked; when a word was corrected the
heading says what was searched for (`In bodies · annotation mode`).

The nav's header chooses `Tree | Flat` (FILTER-20), kept in local storage
with its other state (`menger.nav.view`). Flat lists every ticket the nav's
filter admits — every ticket with none — as the tree's leaf rows in id order,
windowed like the board: `counts { filter }` places each row, and a page is
`list { filter, after: <the row before it> }` scrolling on or `list { filter,
at }` after a jump; a membership watch keeps it live. At 10^6 its first rows
show in ~0.3 s and the last ticket is a scroll away (five pages).

The nav's cursor is the open ticket (AGS-26). Every navigation selects its
row and centres it in the nav, as near as the list's ends allow, in Tree and
Flat. On a ticket page ↑/↓ (k/j) open the previous/next ticket in the nav —
from the nav or from anywhere else outside a text field, a dialog or a menu —
passing over shortcode headings; in the tree, h/l open and close (← and →
are not the app's keys, AGS-27). `f` on a ticket page makes the nav's filter
`under <the ticket>` (it and its descendants); ↑ on the nav's first ticket
then widens that `under` by one parent — a shortcode's to its root's host —
and drops it at a top root.

The nav selects like a file list (ACT-3), one selection for the whole app —
the tree, the flat list and the board draw and take the same one. A click
opens the ticket and selects its row; Shift+click selects from the anchor
(the last plain or Ctrl/Cmd click) to the row, in the order drawn (a closed
node's children are not drawn, so not selected); Ctrl/Cmd+click toggles a
row; neither opens anything. On a focused row Shift+↑/↓ (Shift+j/k) extend
the selection, Space toggles the row, Ctrl/Cmd+A selects every drawn row, and
Esc clears. A shortcode's row stands for its root ticket. Selected rows wear
`--sel` and `aria-selected` (the tree is `aria-multiselectable`; the open
ticket is `aria-current`), and the nav's foot says `3 selected · 2 hidden ·
clear` — hidden: selected, but outside the nav's filter. The selection is
ids: it survives a filter change, a route change and a live reorder, not a
reload; one range takes at most 1,000 rows.

**One menu for a ticket** (ACT-4, `src/menu/`): a right-click — or Menu /
Shift+F10 on a focused one — on a ticket anywhere it is drawn (its nav row,
its id or status chip, a board row, a wave card, the ticket page's heading,
a relation, a chip inside a framed body) opens the same menu: Open, Open in
new tab, the wave boards and Copy code; every update — **Set status ▸**,
**Set kind ▸**, **Set severity ▸**, **Labels ▸** (add or remove; Add a
label…), **Move under…** and **Blocked by…** (a ticket picker, the palette's
`find`), **Unblock ▸**, **Rename…**, **Link commits…**; the filter items
(FILTER-22's — a status, kind, severity or label chip puts its own first);
**Nav ▸** in the nav; and **Delete…**. On a selected row of a selection of
more than one it acts on all of them: "N tickets", Copy codes, every update
as one `update_many` (ACT-2), the shortcodes they share or `under` each,
Delete N… (Open, Rename… and Link commits… step aside); a submenu marks one
ticket's value, or how many of the selection hold each. One ticket's update
is one `update` (status: `set_status`, which keeps MENGER-13's restore
prompt; labels: `label`). A refusal lands in the error banner in the
server's words. The updates are absent for a reader who cannot write, Delete
for anyone but an admin (with login off, everyone may). Delete… confirms in
the app — every ticket, its children (moved to its parent, or deleted too)
and the tickets that wait on it (unblocked) — and then offers **Undo**
(`undelete`) in its toast. A shortcode's row is its root's menu with the
shortcode's Wave board. A tag outside any ticket keeps FILTER-22's menu.

**The status chooser, three ways** (ACT-10). A status, kind, severity or
label chip — on a row, a card, a nav row, the ticket page's Status / Kind /
Severity — puts its own chooser first, flat: the same options, marks and
writes as its submenu, then a rule and the rest of the menu, with no
`Set … ▸` for it (a label chip's ends with Add a label…; a reader who cannot
write gets the menu as before). `]` and `[` step the focused ticket — or the
selection it is in, or the ticket page's own — along the worker line, Idea →
Pending → Ready → Coding → In Review → Complete (`Status::LINE`, `next` /
`prev` in `tickets-api`), through the menu's write (`set_status` for one,
one `update_many` for a selection holding one status); off the line
(Blocked, Epic, Superseded, Canceled), at an end, on a doc or over a mixed
selection, nothing is written and a toast says why. `s` opens the one menu
on the focused ticket with **Set status** already expanded and its current
status focused: ↓/↑, Enter.

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
then a heading (`tree { matches_only }`, FILTER-13/14/23). The open
ticket is always in the nav, whatever its filter (FILTER-24): it is drawn
where a match would be, its matches under it, marked `outside` when the
filter excludes it — and listed in the Flat view at its place — by
`keep: <the open ticket>` on `tree`, `list` and `counts`; the
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

What waits on the owner is a filter too (MENGER-12): **`asking`** admits
the tickets with an input in the state `open` (INPUT-1 — a question nobody
has answered yet: an answer, a close or the body dropping it takes the
ticket out; a clear, or a reopen with no answer, puts it back), and
**`label <name>`** the tickets holding that label (`label needs-you`,
`label in (p1, later)`; a ticket holds many, so two `label` inclusions never
merge). Both are cell coordinates — a cell holds its tickets' `asking` and
their set of labels — answered from the cells on every filtered read and on
`watch { filter }`, kept by the write that moves them (a `label`; an
`answer`, `clear_answer`, `close_input` or `reopen_input`; a `create` or
`amend` whose body opens or drops an input), and `facets` counts `asking`
(yes / no) and `label` (a count per label held, in name order) after
`menger doc`, before `in`. Stored form 10 gives `cells` `asking` and
`labels` columns: a form-9 (or older) file is built again once on first
open, and an older build cannot open it after.

**Needs you** (MENGER-12, MENGER-43) is a filter on the nav: what waits
on the owner is the saved filter named "Needs you"
(`VIEW/_MENGER/view/needs-you`; until one is saved, the default
`(queued or asking or label needs-you)` — MENGER-39's queue first). `g n`,
or a click on the header's **needs you N**, puts it on the nav exactly as
`Saved ▾` puts a saved filter there (the nav's own clauses replaced; its
pills, its text and the URL's `n=` following), focuses the nav's first row,
and leaves the route where it is. The nav shows `global and nav`
(FILTER-21), so the global filter reaches it: filtered to one shortcode,
the nav shows what waits on the owner there. The same press while the nav
holds exactly that filter puts the nav's default back. The header's count
is the saved filter's, live (`entered` / `left` / `recount` of a membership
watch), whatever the lens holds. The filter is changed as any saved filter
is — apply it, edit the pills, **Save as…** "Needs you" (replace); the next
`g n` applies the new text. The questions are where they are answered: the
ticket page (its frame scrolls to the input), `#/inputs`, and the tray for
a queued ticket. Home is the board; MENGER-12's panel, which answered to
no filter, is gone. Labels are chips on every row (board, nav, flat, wave cards) and on
the ticket page, where an editor adds and removes them; the menus offer
**Asking** (yes / no) and **Label** (a value per label, with `facets`'
counts), and the palette takes `filter <text>` — `filter label needs-you`,
`filter asking` — as the global filter.

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

**When** (FILTER-27): every menu has a **When** category that builds the
language's time clauses by clicks — **Created** (today · yesterday · this
week · in the last hour · 12 hours · Custom…), **Moved into ▸** and **Moved
out of ▸** a status (then a window, and **by**: anyone, me, the system, a
member, or a name typed), **In status for** (over 1 day · 3 days · 1 week ·
2 weeks · 1 month, over or under, a custom length; any status, or chosen
categories or statuses) and **Changed** (MENGER-13's Changed recently,
folded in). Every choice is a pill of its own, read in words — "created
today", "Pending for over 2 weeks", "moved into Ready in the last 2 days
by owner" — and each part of it (the status, the window, the length, who)
is a button that reopens the menu on that part alone; a time clause typed
in the text mode comes back as the same pill, and the pill prints the
canonical text. **Custom…** opens the window editor: *within* (a number
and a unit), *since* or *before* a moment — a day on the calendar with an
optional time, or a length ago — and *between* two moments, picked as a
range on one calendar (the first click its start, the second its end, the
days between tinted) with a time for each end; "now" and "start of today"
are one click, and the zone the moments are read in is shown. The calendar
and the time selector are the app's own (`src/calendar.rs`: a month grid
with ‹ › and a year menu, today marked, the ARIA grid keys — arrows,
PageUp / PageDown, Home / End, Enter; hours and minutes typed or stepped
with ↑ ↓ in the reader's 12- or 24-hour clock), drawn from the theme's
tokens, never the browser's date input; `#/components` shows them live
under "The app's own" (the registry above them is the tickets'
components). **Hasn't changed** (FILTER-30) builds `not touched <window>`
— within 1d · 3d · 1w · 2w · 1mo, or Custom… for since a day — a pill
reading "hasn't changed in 2 weeks"; any time pill's ⋯ menu has
**Negate** (`created today` ⇄ `not created today`), and a group's ⋯ menu
one per time clause inside it. `Saved ▾` suggests four filters until one of
their names is saved — **New today** (`created today`), **Moved today**
(`changed today`), **Stuck** (`category in (ready, active, review) for over
1w`), **Untouched 2w** (`not touched within 2w and not category done and
not kind doc`) —
each applied by a click, and `keep` saves it as an ordinary saved filter
(the app offers them; nothing is written into the tracker until you keep
one). A ticket's History says since when it has held its status — "since
at least …" for a ticket from before the log.

**A tag is a filter** (FILTER-22, `src/tags.rs`): a right-click on any tag
the app draws — a shortcode, a ticket id, a status, a kind, a severity, on
the ticket page, the board, the nav and the wave board — opens a menu at the
pointer: **Filter …** makes the filter exactly that clause at once, **Add …
to the filter** joins it to the filter as it stands, **Build a filter from
…** makes it the clause and opens the filter menu on its category. A
shortcode's clause is its tree (what checking it in the menu makes); a
ticket id offers its shortcode first and `under` the ticket second. It
builds the surface's own filter, or on the ticket page the global one. A
tag inside a ticket (a row, a card, a heading) opens the ticket's one menu
(ACT-4), these items first in its filter group.
The palette takes `filter <shortcode>`: the global filter becomes it.

**Any chip sets the global filter** (FILTER-25): on a surface with a filter
of its own (the nav, the board, the wave board) the menu also offers
**Filter everywhere: …** for each clause — the global filter becomes exactly
it. **Alt+click** on any tag sets the global filter to its first clause at
once (a ticket id: its shortcode), without navigating; a plain click is
unchanged. A ticket chip in a framed body does both: its menu says **Filter
everywhere: …**, and an Alt+click reaches the app as `menger:filter-everywhere
{ id }` (SDK 1.17).

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

### Keys and gestures (AGS-28)

Every key and click the app answers is one row of `BINDINGS` in
`frontend/src/bindings.rs`: an id (`nav.expand-subtree`), where it applies
(everywhere, the nav, the board, a ticket page, a menu, a dialog, a chip…),
its chords (`*`, `Shift+Numpad*`, `g h`, `Mod+k` — ⌘ on a Mac, Ctrl
elsewhere — `Alt+Click`), its group and what it does. Handlers ask
`bindings::hit(&ev, Context::Nav)` (or `hit_mouse`, `is`, `key_hit`) and
match the id; nothing else reads an event's key, button or modifiers — a
native test lints the sources for it, another fails when two bindings share
a chord where both are live, another when a binding is dispatched nowhere.
The `?` sheet and Settings → Keyboard (with a search by key or by words)
draw the same table. To add a key: a row in `BINDINGS`, a match arm in the
handler.
The groups are Move, Open, Status (ACT-10's `]`, `[`, `s`), Select, Nav,
Filter, Annotate, View, Menus & dialogs, Forms and Help.

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

**`update`** (MENGER-30) changes any part of a ticket in one write — its
title, body, kind, status, parent, severity and blockers, every field
optional and absent meaning unchanged — and all of it lands or none does.
Each aspect keeps its own write's rules, asked together before anything is
written: `amend`'s (a title not blank, the body cap, the component
registry, input markup), `set_kind`'s (the kind before the status, checked
against the blockers the update leaves), `set_status`'s for a status alone
(`Canceled` cascades as there), `set_parent`'s (a shortcode's root moving
takes admin on both sides), and `create`'s for blockers — `block` adds and
`unblock` removes; itself or a cycle is refused (the cycle named), an id
not born yet is a forward reference, a doc never blocks and is never
blocked, and an id in both is refused. A refusal is one in-band `error`
naming every reason. `severity none` clears the severity. The answer is
`updated { ticket, changed }` (`changed` empty when nothing would move:
nothing written, nothing sent); `watch` carries each aspect's own event —
`amended`, `kind_changed` or `status_changed` (with their transitions),
`blockers_changed { ticket, added, removed }` with each blocker's
`unlocks_changed` / `unlocks_removed`, `parent_changed` with the parents'
sides, `severity_changed { ticket, previous }` — then the write's groups and
placements. Everything kept moves in the one write (the blockers' `unlocks`,
the forward references, the ready set, the `blocked` cells, the waves, the
roll-ups, the kept tree), and `--verify-derived` holds it to a scan:

```sh
synapse -P 4455 menger tickets update --id THEME-4 --block THEME-7
synapse -P 4455 menger tickets update --id THEME-4 --unblock THEME-3 --severity High
synapse -P 4455 menger tickets update --id MENGER-40 --title "Board loses focus on reload" --kind gap --status Ready --parent MENGER-2
synapse -P 4455 menger tickets update --id MENGER-40 --severity none
```

**`update_many`** (ACT-2) makes the same update to many tickets in one
write, all of it or none of it: each ticket gets `update`'s change by
`update`'s rules — `kind`, `status`, `parent`, `severity`, `block` /
`unblock`, and `label` / `unlabel` (`label`'s rules); a title and a body are
each ticket's own and stay `update`'s. The tickets are `ids` (each once, in
the order given), or the tickets a `filter` admits at the write — in the
caller's reach, at most `max` (default 500; more is refused, naming the
count). Under login each ticket takes the write grant on its own (the gate
asks it of every id; a filter's are asked the same way), and one missing
refuses the write, naming it. Every ticket is checked first: any refusal —
an id that names nothing, a rule, a parent or blocker cycle among the moved
— writes nothing and is one in-band `error` naming each refused id with
every reason. A ticket nothing would change for is no write and is answered
in `unchanged`. `message` is on every transition the write makes, and the
write's first transition is the `cause` of every other of the caller's, so
the write reads as one in each ticket's history. The answer is
`updated_many { ids, changed, tickets, unchanged }` — the ids it changed,
the aspects that moved in any (`update`'s order, `labels` last), and every
ticket selected as the write left it; `watch` carries each ticket's own
events, then one `updated_many { ids, changed }` (its `tickets` and
`unchanged` empty), then the filtered moves. A write over many tickets
sends many events at once — past the broadcast's 256, a watch says
`lagged { missed }` (SCALE-5) and the tail, `updated_many` last, still
arrives. `label` and `link_commits` take **`ids`** in place of `id`: the
same change to each, in one write, all or none, one `labels_changed` /
`commits_linked` per ticket in the order named:

```sh
synapse -P 4455 menger tickets update_many --ids THEME-9,THEME-10,THEME-11 --status Ready
synapse -P 4455 menger tickets update_many --ids PANTRY-5,PANTRY-6 --parent PANTRY-2 --severity High --block PANTRY-4
synapse -P 4455 menger tickets update_many --filter 'label stale and not category done' --status Canceled --message "swept"
synapse -P 4455 menger tickets label --ids A-1,A-2 --add needs-you
```

**`delete`** (ACT-5; admin only) removes tickets for good — `ids`, or a
`filter` (ACT-2's selection), all or none. It is refused, naming every
reason of every ticket, while something points at one: its **children**,
unless `children: reparent` (each moves to the nearest ancestor that stays;
a hosted shortcode's root never to the top level) or `children: delete`
(its whole subtree goes too, every id in the answer); a ticket that **waits
on it**, unless `unblock` (the block is dropped, and that ticket's
`history` names the delete in `deletions`); its **shortcode**, while it is
the root and the shortcode holds a ticket that stays; a **component** or a
**theme** it defines; an input of its that is `open` or `answered`. A
deleted ticket leaves every read — the nav, the board, every filter,
`find`, `search`, the ready set, the counts — and `get` on its id answers
`deleted { ids: [id], tombstones: [{ id, title, by, at_unix, message,
restorable_until_unix, parent, reparented, unblocked }] }`. What is kept
(the owner's decision `keep`): the **tombstone** and the **history**
(`history` still answers it), and the whole ticket, body and all, for **30
days** — `undelete { ids }` (admin) brings it back within them, its moved
children back under it and its dropped blocks back where nothing has moved
them since. Then the body is purged: at every start and once a day inside
the server, the tombstone kept. Its **number is never reused**: the next
ticket of the shortcode takes the number after its highest, deleted or not,
and `extents` names the deleted numbers as `holes`. Nothing may wait on a
deleted id. The write rebuilds the derived state from the rows it leaves —
what a first open does, the cell ids kept — so `--verify-derived` holds
after it; `watch` carries each moved ticket's own event (`parent_changed`,
`blockers_changed`, the parents' `child_removed`, the blockers'
`unlocks_removed`), then `deleted { ids, tombstones }`, then the groups and
placements that moved, and a filter's `left`:

```sh
synapse -P 4455 menger tickets delete --ids MENGER-40 --message "filed twice"
synapse -P 4455 menger tickets delete --ids THEME-12 --children reparent      # its children move to its parent
synapse -P 4455 menger tickets delete --ids DEMO-1 --children delete --unblock true
synapse -P 4455 menger tickets undelete --ids MENGER-40
```

**`reshortcode`** (ACT-6; editor on both shortcodes) moves tickets into
another shortcode, renumbered there, all or none (ACT-2). `to` names it by
any short form: one there is — each ticket takes its next number in the
order given and hangs under its root, or `parent` (inside it); with
`new_scope TITLE` (and `parent`), one minted first, its root `TO-1` a new
scope under `parent`, the tickets `TO-2`, …; none, and the first ticket a
scope or a doc — that ticket becomes the new shortcode's root `TO-1`,
hosted where it hangs (ORG-5's way out for a scope inside its parent's
shortcode). A shortcode's root moves only alone, with `with_children` and
every ticket of the shortcode: a **rename**, every number kept, the code
new in its namespace. A ticket whose parent moves with it keeps it;
`with_children` brings each one's descendants in its own shortcode,
numbered after it; without it they stay in their shortcode under the
nearest ancestor that stays, and a child rooting another shortcode stays
hosted under the moved ticket, its namespace following. A ticket's id is
its key, so the move rewrites the old id to the new wherever a row names
it — parents, blockers and `unlocks`, the search index, inputs and
answers, comments (and their route to the ticket's page), the transition
log, grants, notes, themes, components, tombstones, the queue, deployments
— in one transaction, and **pins the old id to the new key**: `get`, every
write's id, blockers, filters, the app's `#/ticket/OLD` and a chip in a body
naming the old id all reach the moved ticket, and `get` says
`moved_from: [OLD]`. Bodies are not rewritten. No number is given out
twice — a shortcode's next ticket is past the ones that left it. The cache
is read back from the file after the commit, the derived state built from
the rows (cell ids kept), so `--verify-derived` holds after it; `watch`
carries `reshortcoded { moves: [{from, to}], tickets }`, each child that
stayed and moved's `parent_changed`, the groups and placements that moved,
and a filter's `left` for an old key and `entered` for a new one. A frozen
target (MENGER-33) is not refused yet: freezing is not built.

```sh
synapse -P 4455 menger tickets reshortcode --ids THEME-9,THEME-10 --to LOOK      # THEME-9 → LOOK-3, THEME-10 → LOOK-4
synapse -P 4455 menger tickets reshortcode --ids MENGER-35 --to PALETTE --new-scope "The palette" --parent MENGER-2
synapse -P 4455 menger tickets reshortcode --ids MENGER-2 --to APP --with-children true   # MENGER-2 → APP-1, its tree after it
synapse -P 4455 menger tickets reshortcode --ids LOOK-1 --to SKIN --with-children true    # a rename: LOOK-n → SKIN-n
```

The statuses of the lifecycle are ten (SDLC-7, SDLC-15): `Pending`, `Ready`,
**`Coding`**, **`In Review`**, `Blocked`, `Complete`, `Idea`, `Epic`,
`Superseded`, `Canceled` — and a doc's `Doc` (SDLC-11) beside them, outside it. The two
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
`Complete`, `Superseded` or `Canceled`, and a roll-up's `done` still counts
`Complete`. **`Canceled`** (SDLC-15) is work that will not be done and that
nothing replaced — `Superseded` without the successor — and is treated as
`Superseded` is everywhere: done with, placed `done`, category `dropped`,
not counted in a roll-up's `done`; `cancelled` parses to it too. Stored as text, so a database written before them opens
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

A ticket carries **labels** (MENGER-12): free-form tags on every row and
ticket (`labels`, `[]` when none), written by `label { id, add, remove }`
alone. A label is letters, digits and `-`, at most 32 characters, folded to
lower case (`Needs-You` is `needs-you`); a ticket holds at most 16, kept
sorted, each once. A label named in both `add` and `remove` is added; a
call that moves nothing answers `labels_changed` with `added: []` and
`removed: []` and writes nothing; an entry that is not a label (or a result
past 16) is one in-band `error` naming it, and nothing is written. Labelling
moves `updated_unix` and the ticket's cell (`label <name>`), nothing else.
**`needs-you`** is the label that puts a ticket in front of the owner: the
Needs you filter (`g n` puts it on the nav) admits every ticket labelled
`needs-you` beside every ticket `asking` and queued. A database written before MENGER-12 gains the `labels`
column on first open, durably — `ADD COLUMN … DEFAULT '[]'`, instant at any
size.

```sh
synapse -P 4455 menger tickets label --id MENGER-40 --add needs-you
synapse -P 4455 menger tickets label --id MENGER-40 --remove needs-you
synapse -P 4455 menger tickets list --filter 'asking or label needs-you' --summary
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
| `<part name scripted?>` | `parts[]` — `{ name, scripted, description }`, in order (THEME-5): what a theme may skin as `m-name::part(name)`; the template must carry `part="name"` on an element, unless `scripted` (the component's script makes it) |
| `<example>` | `examples[]` — each one's inner HTML as written (its common indentation and surrounding blank lines removed) |
| everything else | `help` — markdown: the five elements' lines removed, the rest de-indented, runs of blank lines one |

The five are the `<docs>` element's own children: one inside another
element of the help (`<details><summary>`) or a backtick code span or fence
is help. A malformed `<docs>` — two of them, one never closed, two
`<summary>`s, an `<attr>` without a `name`, an unknown `type`, an `enum()`
naming nothing, an attribute, slot or part declared twice, a part with no
name, a documented part no element of the `<template>` carries — is an in-band `error`
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

**HTML with blank lines** (COMP-11). In a framed body, an element opened in
an HTML block runs to its close, blank lines included: the markup inside is
passed through as written (CommonMark alone ends the block at the first
blank line, and the rest becomes markdown: indented lines code blocks,
inline tags wrapped in `<p>`). Markdown inside HTML is still written
CommonMark's way: a blank line **right after an element's opening** — its
open tag, or that and a `<summary>` or heading line — starts markdown (the
house components' `<m-summary>`, `<m-field>`, `<details><summary>`), unless
the next line is indented as code; and a fence with blank lines around it
is a fence. A block that starts as `<pre>`, `<script>`, `<style>`,
`<textarea>` or a comment already ran to its end and is untouched; an
element nothing closes ends at the blank line, as before. The rule is
`tickets-api/src/html_runs.rs`; the frame's renderer (`frontend/src/md.rs`),
the server's readers of a body (fields, inputs, component references) and
`diagrams` all read through it. An unframed body holds no element, and
renders as before.

**In the app** (COMP-4, COMP-5) a body holding HTML or a component tag
renders in a sandboxed frame (no `allow-same-origin`, no `allow-forms`;
IDENTITY-28: under a CSP, `connect-src 'none'; form-action 'none'`, so a
body opens no connection of its own and submits no form — scripts, styles,
images and fonts load from anywhere, and a link opens the real page in a
new tab once the app has asked — MENGER-14, "Links" below) under menger's SDK
(`frontend/sdk/menger-sdk.js`, whose header is the protocol's reference),
and every component in the registry is a custom element there. ACCESS-28:
the frame is not a `srcdoc` — a srcdoc inherits the app page's strict policy
("Security headers" below), which would refuse a body's inline scripts and
esm.sh imports — but `/app/frame#<channel>` (`web/frame.html`), served under
the frame's policy alone; it posts `menger:boot`, and the host answers
`menger:doc` with the document it built, which the frame writes over itself. A custom
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

**The frame's base typography** (COMP-12) is the app's ticket-text rules
(headings, `code`, `pre`, links, block quotes, tables, images), in the
cascade layer `menger-base` at zero specificity: any rule the body writes
wins, whatever its specificity. By default it reaches every element — the
body's markdown and its own HTML alike. A body that styles itself opts out
with

```html
<meta name="menger-base" content="markdown">
```

anywhere in it: the base then styles only what markdown produced (md.rs
marks those elements `data-md`; an element of the body's own may carry
`data-md` to take it), so the body's own `code` gets no border it did not
ask for. Either way the frame keeps its ground (the theme's ink and font,
no margin), `box-sizing: border-box` on every element, the mermaid
placeholders and the docs cards; nothing reaches a component's shadow root.

**Full screen, mockups and whole pages** (MENGER-15;
`frontend/src/present.rs`, `frontend/sdk/menger-page.js`). Every framed
body sits under a slim bar whose **⤢ Full screen** (or `z` — from the
ticket page, or inside a focused body) makes the same box, the same
`<iframe>`, fill the viewport: the Fullscreen API when a gesture allows it,
else a fixed overlay over the app. The sandbox and CSP are unchanged — it is
the frame resized, not a new origin or window. Esc (or the bar's exit)
leaves it; full, the bar names the ticket's id and title.
`#/ticket/ID?view=full` opens a ticket straight into it, and entering or
leaving rewrites that query in place.

A body **declares how it is shown** in its frontmatter:

```yaml
---
view: mockup            # inline (default) | full | mockup
widths: 390, 768, 1440  # a mockup's devices — W, or WxH; at most 6
---
```

`full` opens the ticket page full screen, with **← Back to the ticket** one
click away; `mockup` draws the body as a website mockup. A whole document
that should stay a plain HTML file can say it in its head instead —
`<meta name="menger-view" content="mockup">`,
`<meta name="menger-widths" content="390,768x1024,1440">` (the frontmatter
wins). A **mockup** renders the body once per device, each a frame at the
device's own viewport (`390` → 390 × 844, `768` → 768 × 1024, `1440` →
1440 × 900; its media queries see that width) in device chrome, scaled to
fit — all side by side at one common scale, or one at a time — with
**Light / Dark** and **Reduced motion** toggles the framed page itself sees,
live; full screen scales the devices to the screen.

**A body that is a whole HTML document** — `<!doctype html` (then an
element) or `<html` first, past comments — renders as itself: its own head,
styles and scripts, inline media included, never markdown and never under
the app's typography, theme tokens or SDK (`window.menger` is absent; it is
a page). It is still the sandboxed frame, and IDENTITY-28's CSP is still the
first element of its head (injected inside its `<head>`, ahead of anything
the page holds), followed by the page shim. Inline it gets a viewport of
its own (80vh) and scrolls inside it; its `#fragment` links scroll the page
(the frame document's base URL is `/app/frame`, so the browser would
navigate the frame away), any other link takes MENGER-14's path (`menger:link`: the leave dialog first; full screen steps aside for it), and its `prefers-color-scheme`
follows the app's painted mode.

**What makes `prefers-color-scheme` follow inside the frame.** An iframe's
`prefers-color-scheme` is the *embedder's* in Chromium, whatever the
`<iframe>` element's `color-scheme` says (checked: Chromium 153 reports the
browser's preference in both a `color-scheme: dark` and a `color-scheme:
light` frame); Firefox follows the element's used `color-scheme` (Firefox
155: a `color-scheme: dark` iframe matches dark in a light browser).
Nothing anywhere sets `prefers-reduced-motion` per frame. So the host sets
the `<iframe>`'s `color-scheme` (Firefox's native path) **and** the page
shim, inlined before anything the page holds, answers the page's own
queries as emulated: every `@media` rule naming `prefers-color-scheme` or
`prefers-reduced-motion` (nested and `@import`ed ones too) has that feature
rewritten to an always-true / always-false condition, reversibly; so is the
`media` of every `<source>` (a `<picture>`'s dark variant), `<link>` and
`<style>`; `matchMedia()` of such a query returns a live list whose
`matches` follows the emulation and which fires `change` on a toggle (a
page that reads it once and listens, like the landing's carpet, follows);
the root carries `data-menger-scheme` / `data-menger-motion`, and its
`color-scheme` when the page declared `light dark`. Toggles arrive as
`menger:emulate { scheme, reduce }` — no reload. A cross-origin style sheet
cannot be read and is left as the browser answers it.

**A ticket body's size cap** (MENGER-15). `create` and `amend` refuse a body
past `--max-body-bytes` (`MENGER_MAX_BODY_BYTES`; default 8 MiB, room for a
page with its media inlined) with the typed `body_too_large { id, bytes,
max, message }`, writing nothing; the app's editor says it with the cap.
What it means for memory: bodies are not in the cache (SCALE-14 — rows are;
a body is read from the database for the call that answers it), so a large
body costs the process only while one `get` or one write carries it, and
`list`, `watch`, `--summary` and the board carry no bodies at all (summary
rows; `watch`'s whole-ticket events — `created`, `amended`, … — do carry the
ticket, so one amend of an 8 MB body broadcasts 8 MB to each open watch).
The search index keeps a body's text only (its `<script>`/`<style>` and
attribute values but a few dropped), so inlined media cost it nothing.
Transport limits sit around it: the WebSocket takes a request up to 10 MB
(jsonrpsee's default — JSON escaping grows a body, so a cap much above
~9 MB is refused there first, untyped); the REST bridge refuses a request
past 2 MB (axum's default body limit) with `413`, so a large body is
written over the WebSocket; and synapse cannot send one from the command
line (a single argument is at most 128 KB on Linux, and synapse has no
file or stdin parameter) — a script speaks the WebSocket instead.

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
and returns the function that stops it. IDENTITY-36: **a frame reads its
own subtree** — every read and every change event is held to the shortcode
of the ticket whose body the frame renders and the shortcodes hosted under
it (`under SHORTCODE`, what an editor of that shortcode reads), whatever
the viewer could read: `list`, `find` and `inputs` (each also taking
`filter`) answer under `(<filter>) and under SHORTCODE`; `get` and
`blockers` of a ticket outside are `not_found`, and a blocker outside is
left out; `waves` of a shortcode outside rejects; `groups`, `extents` and
`health`'s count are the subtree's; `components` is the whole registry.
The host stamps the scope on the frame (`data-scope`); an editor's preview
reads as the ticket it edits, a component's help card as its owner, and a
frame with neither reads only `components`; **`navigate(route)`** (COMP-9,
below); **`input`** (INPUT-4, frozen) — `ticket`, `submit` and (INPUT-9,
1.4) `clear`, the two writes (below), and (1.7, COMP-14) `displayOnly`,
true in a cited window. Its types are
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
  unframed note. COMP-14: a ticket route may name a section instead —
  `#/ticket/<id>?field=<name>` or `?anchor=<id>` — which the frame showing
  that ticket scrolls to and flashes.

**Links** (MENGER-14). Every link people write — a markdown link, a bare
`http(s)://` URL (autolinked by md.rs, as GitHub does), an `<a href>` in an
HTML body, a URL in a comment — is followed by the app, never by the
browser on its own. In a framed body the SDK (1.8) takes the click (Enter on
a focused link, a middle click) and posts `menger:link { href }` — the href
as written — over the frame's channel; the sandbox and the CSP are
unchanged. In the app's own page one delegated listener takes links in
`.md` and `[data-links]`. The host classifies (`frontend/src/links.rs`, the
`url` crate — WHATWG's parser): `#/…`, or this origin's app path, navigates
in the app; another path of this origin (`/vanilla`) opens in a new tab;
`http(s)` anywhere else and `mailto:` get **"This link leaves menger"** —
the ticket it came from, the full URL with its registrable domain
emphasised (the Public Suffix List, vendored at `frontend/vendor/psl/`), an
IDN host decoded and as punycode, a `user@` before the host called out, and
Open (a new tab, `noopener,noreferrer`) / Cancel; Enter opens, Esc cancels,
Tab stays inside, focus returns. "Don't ask again for <domain>" (web links
only) is kept in this browser per signed-in user (localStorage
`menger.links`) and listed, revocable, in `#/settings` → Links; a trusted
domain opens at once only inside a viewer's gesture on the frame it came
from. `javascript:`, `data:`, `file:`, `vbscript:` and every other scheme
are refused with a short notice. The vanilla page (`/vanilla`) links only to
the app's own tickets, so it has nothing to ask about.

**Cites** (COMP-14). A body can show a **live window into a section of
another ticket** — cited, never copied:

```html
<m-cite ref="AUTH-10" field="findings"></m-cite>   <!-- its [data-field="findings"]: an m-field, m-cases, m-decision… -->
<m-cite ref="AUTH-10#the-table"></m-cite>          <!-- the element with id="the-table" -->
```

The window has a header — the cited ticket's id, its **live** status, its
title and the section's name, a link to `#/ticket/AUTH-10?field=findings` —
over the section, drawn as it is drawn in its own ticket, at its natural
height up to 480 px, then scrolling inside the window. An `amended` of the
cited ticket redraws it in place (no reload, no flash); a status change
repaints the header. The first element carrying the `data-field` is the
section (a name several elements carry, the first of them); the `#id` form
takes any element id the body writes. Headings are not addressed: md.rs
gives them no ids (write `<h2 id="…">` to cite one). Failure is a named
line, never a blank: `AUTH-10: no ticket you can read` (no such ticket —
or, under login, one outside your reach: the server answers both
`not_found` and never says which), `you cannot read AUTH-10` (a refusal),
`AUTH-10 has no section `findings`` (`has no element `#x``), a malformed
tag (`m-cite: …`), and a cite nested too deep (windows nest two deep; a
cycle stops there).

**How it is drawn — and why the citing body never holds it.** A cited
section's HTML, and its scripts, must not run in the citing frame: that
would give one shortcode's body the other's reach (IDENTITY-36), and the
citer's script could read the cited text. So `<m-cite>` is built into the
SDK (1.7) as a **placeholder** only: it reserves room and tells the host
what it cites and where it is (`menger:cite { cite, ref, field, rect }`,
`menger:uncite`). The **host** (`frontend/src/cite.rs`) then asks for the
cited ticket over the viewer's own socket (`tickets.get` — the viewer's
reach decides, never the citing frame's scope), lays the window over the
placeholder in the app's own DOM (clamped to the citing frame's box), and
renders the cited body in a frame of its own: the same sandbox, CSP and
SDK as every frame (IDENTITY-28 unchanged), stamped as the **cited**
ticket's — `data-scope` its shortcode, so its reads are what they are in
its own ticket; no `data-ticket`, so the host answers no write from it —
and told its section (`cite` in the init block), which its SDK alone
shows, everything else hidden. The citing frame is answered **only the
window's height** (`menger:cited { cite, height }`). So:

- the cited content never enters the citing document — the citer's script
  reads nothing of it, and its own reads are what they were;
- the cited body's script runs with the cited shortcode's reach, never the
  citer's;
- a ticket the viewer cannot read draws its line and nothing of it — no
  title, status or body;
- inputs in a window are **display only**: they show their answers, and
  `menger.input.submit` / `clear` there reject at once
  (`menger.input.displayOnly` is `true`; the host refuses them too) — an
  input is answered in its own ticket;
- the citing body holds only the tag, so search, the inputs store and the
  component registry count the section as the cited ticket's; the citer
  references `m-cite`.

The limits: the citing frame learns the window's height (it lays out
around it), so a script there can tell roughly how tall a section the
viewer can read is, and whether a window opened — never its text; a body
opens at most 64 windows; the whole cited body loads in the
window's frame (its scripts and diagrams run, hidden but for the section;
a mermaid diagram in the section draws as in its own ticket); a window
scrolls inside itself past 480 px.

The order of work for a shortcode tree — the platform's, here — is one call
(WAVE-4); the app's wave board (`#/waves/<SHORTCODE>`) makes the same one:

```sh
synapse -P 4455 menger tickets waves --epics PLEXUS,MACRO,SYNAPSE,TRANSPORT,STARTER,HYPERFORGE,RETURNS,CONTEXT
```

A ticket has two boards of its own (MENGER-31), each on the ticket menu
(right-click a ticket anywhere — a chip inside a framed body too, MENGER-34:
the frame's SDK posts `menger:ticket-menu { id, x, y }` and the app opens the
same menu there — or Shift+F10 on the focused one) beside
**Open**: **Wave board for <parent>** — `waves_under`, the parent's subtree
(any depth, hosted shortcodes included, from the kept tree), at
`#/waves/under/<PARENT>?at=<ID>` with the ticket scrolled to and outlined;
absent for a root — and **Waves to <ticket>** — `waves_to`, the board whose
final state is the ticket: its blockers transitively (and a scope's or an
execution's open descendants and what they wait on), each where the store
keeps it, done blockers `done` and not followed past, the ticket alone in
the last wave, and a `wave_cycle { ids }` per `blocked_by` cycle, at
`#/waves/to/<ID>`. Both are live as the shortcode board is.

```sh
synapse -P 4455 menger tickets waves_to --id MENGER-31
synapse -P 4455 menger tickets waves_under --id MENGER-2 --kinds build,gap
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
  the reason, sending nothing: **a gesture on that very frame is under way**
  (IDENTITY-36, `frontend/src/gesture.rs`), read the moment the message
  arrives — the app window's own transient activation is on (a browser
  without the API is refused); the app's focused element is that `<iframe>`;
  since the app's own last click or key, the pointer came onto that frame
  or a Tab carried focus into it; and, while that click or key of the app's
  is younger than 5 s, focus entered the frame after it. Any click or key
  elsewhere in the app is no gesture on the frame; **the frame renders a
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
| `tickets.create { epic, title, kind?, severity?, blocked_by?, body?, parent?, path? }` | `created { ticket }` — id is the next `EPIC-N`, status starts `Pending`; `kind` defaults to `build`, one outside the six is an in-band `error` naming them; a `parent` that does not exist (or would make the ticket its own ancestor) is an in-band `error` naming the reason; **every shortcode starts with a scope** (MNG-12) **or a doc** (SDLC-11): the first ticket of a prefix must be `kind: scope` or `kind: doc`, anything else is an in-band `error` naming the rule and nothing is allocated; a `doc` starts — and stays — in `Doc`, and a doc with a `blocked_by`, a `blocked_by` naming a doc, or a doc born as an id a `blocked_by` already names is an in-band `error` (SDLC-11); **`parent` defaults to the shortcode's scope root and must stay inside the shortcode** (MNG-14): omitted, a non-root hangs under its prefix's root; named, it must be in the same prefix, else an in-band `error` — the scope root itself is the exception, its parent (if any) is the shortcode it hangs under, a sub-shortcode; **`path`** (WAVE-10) gives the ticket a path address in its shortcode — only `_MENGER/…` with something after it, only on a `doc`, only with the shortcode's scope root as `parent` (the default); one another ticket of the shortcode holds is an in-band `error` naming it, as is each other refusal, and nothing is allocated; COMP-3: the body's ` ```component ` blocks register components — a block whose tag is not a component name, a name defined twice, or a name another ticket defines is an in-band `error` (the last naming that ticket) and nothing is allocated; COMP-8: so is a malformed `<docs>`, naming the problem; MENGER-15: a body past the cap (`--max-body-bytes`, 8 MiB by default) is the typed `body_too_large { id: null, bytes, max, message }` and nothing is allocated |
| `tickets.get { id }` | `ticket` or `not_found` (ACT-6: a ticket an old id names carries `moved_from`, every id it had) — `id` is an `EPIC-N`, or (WAVE-10) a path address `SHORTCODE/path` (`PLEXUS/_MENGER/board`; the shortcode case-insensitive, the path exact), and `not_found` carries the id or address as given; ACT-5: a deleted ticket's id answers `deleted { ids: [id], tombstones: [tombstone] }` |
| `tickets.list { epic?, status?, parent?, after?, limit?, summary? }` | one `ticket` per match, ordered (epic, seq), then one `page { next, remaining }`; `parent` yields exactly that ticket's children — the one way to read them (SCALE-4), from an index the store keeps, so a page of a root's 163,530 children costs the page (1.3 ms at 10⁶ over REST, first page or deep) and its `remaining` is arithmetic; `summary` swaps the rows for `summary { summary }` (every field but the body); `after` is the exclusive cursor and `limit` caps the page (ACCESS-29: 1,000 rows unless given, never more than 10,000 — page with `next`) — `after` naming no ticket is one in-band `error` |
| `tickets.ready_set { epic?, after?, limit?, summary? }` | one row per member of the ready set (Ready, every blocker Complete, Superseded or Canceled — never a doc), ordered (epic, seq), then one `page { next, remaining }` |
| `tickets.find { query, after?, limit? }` | one `summary` row per ticket whose id or title contains `query`, case-insensitively (ASCII folding) — the jump palette's rule (AGS-18), on the server (SCALE-6) — ordered (epic, seq), at most `limit` (default 50), then one `page { next, remaining }`: `remaining` plus the rows is the total. `query` is trimmed; empty or blank matches every ticket. `after` is `list`'s cursor (a position — it need not match); one naming no ticket is one in-band `error`. Ids and titles only, never bodies. A blank query's page costs the page; any other query's exact `remaining` scans every id and title after the cursor (~145 ms at 10⁶) |
| `tickets.search { query, filter?, limit?, offset? }` | SEARCH-2: search inside tickets — one `hit { summary, title, snippet, corrected }` per ticket whose title or body's text holds every word of `query`, best first, then one `searched { matched, next, words }`. Each word also matches as a prefix; a misspelt one (unknown to the index, or rare in it) of four letters or more also matches the index's words one edit away (two from eight letters); `FILTER-23`, `set_status` and `"a phrase"` are phrases. Uncorrected matches first, then corrected; BM25 within, the title weighing 4 to the body's 1. `title` and `snippet` are `[{ text, hit }]` stretches, the matched words marked. `filter` as `find`'s; `limit` 20 by default, at most 100; `offset` skips matches — `next` is the next page's. A blank query is no hits and `matched: 0`. See "Search" below |
| `tickets.set_status { id, status restore_children?, source?, message? }` | `status_changed { ticket, previous }` — any of the nine, from any status (SDLC-7: the worker states `Coding` and `In Review` included); one outside them is an in-band `error` naming the nine; SDLC-11: on a doc, an in-band `error` (a doc has no lifecycle), and `Doc` on any other kind an in-band `error` too; MENGER-13: a transition (the caller's, from `source` — `app` / `api` / `pipeline` —, with `message`), said on `watch` as `transition` right after the `status_changed`; into `Canceled` it cancels every open descendant in the same write and withdraws the open inputs, out of it restores the inputs and, with `restore_children`, what its cancel canceled — see "History and cancellation" |
| `tickets.history { id }` | MENGER-13: one `transition { transition }` per change of the ticket's status or kind, oldest first, then one per transition those caused in other tickets, then `history_end { began_unix, restorable, status_since_unix, status_since_exact, deletions }` (FILTER-27: since when it has held its status — its latest status transition, `exact`; or, for a ticket from before the log, its last update or the log's start, whichever is earlier: "since at least"; ACT-5: `deletions`, the tombstones of the deletes that moved it or dropped its block, oldest first); a deleted ticket's history still answers; `not_found` for an id naming no ticket |
| `tickets.set_kind { id, kind, status? }` | `kind_changed { ticket, previous }` (SDLC-13) — the one write that changes a kind; everything kept by kind moves in its write. To a `doc`: status `Doc` (a `status` other than `Doc` is an in-band `error`), refused while the ticket waits on a blocker or a ticket waits on it (each named); from a doc: `status` if given (never `Doc`), else `Epic` for a scope and `Pending` otherwise; between two kinds of work the status stays unless given (`Doc` an in-band `error`). A shortcode's root becoming a kind that roots nothing is an in-band `error` naming the shortcode (doc ↔ scope is allowed); so is a path holder (WAVE-10) becoming work, and an unknown kind or status; an id naming no ticket is `not_found`. A lower-seq ticket made a scope or a doc becomes its shortcode's root. The kind (and status) it has is no write and no broadcast, answering `previous` equal to its kind. `watch` carries `kind_changed` first, then the write's `group_changed` (a root's whenever its kind or status changed; every row a re-root moved), `placements_changed`, and — filtered — `entered` / `left` |
| `tickets.amend { id, title?, body? }` | `amended { ticket, changed }` (MNG-16) — replaces the title and/or the body, whole values, last writer wins; `changed` names what moved (`["title"]`, `["body"]`, `["title", "body"]`); neither given, or the values already there, is no write and no broadcast and answers `changed: []`; an id naming no ticket is `not_found`; a title that is empty after trimming is an in-band `error` and nothing is written. Stored byte-exact, durable before it is acknowledged; `updated_unix` moves; nothing else about the ticket does — status, kind, edges, parent, id. COMP-3: a new body moves the component registry (definitions, references, orphans); a malformed ` ```component ` block (COMP-8: or `<docs>`), or a name another ticket defines, is an in-band `error` and nothing is written, title included; MENGER-15: a body past the cap is the typed `body_too_large { id, bytes, max, message }` and nothing is written, title included |
| `tickets.link_commits { id \| ids, commits }` | ACT-2: `ids` instead of `id` links them to each in one write, all or none (a ticket that names nothing, or past 256, refuses all, each named), one `commits_linked` per ticket in the order named. `commits_linked { ticket, added }` (SDLC-9) — appends every commit reference in `commits` the ticket does not already carry, in the order given; `added` is what was newly linked, in order. Every entry already linked, or `commits` empty, is no write and no broadcast and answers `added: []`. An entry that is not a commit reference is one in-band `error` naming every such entry, and nothing is written — a call is all or nothing; so is a link that would put more than 256 commits on the ticket. An id naming no ticket is `not_found`. Durable before it is acknowledged; `updated_unix` moves, nothing else does |
| `tickets.unlink_commits { id, commits }` | `commits_unlinked { ticket, removed }` (SDLC-9) — every entry the ticket carries is removed; `removed` names them in the order given; none of them linked is no write, `removed: []`. The same grammar, the same all-or-nothing `error`, `not_found` |
| `tickets.label { id \| ids, add?, remove? }` | ACT-2: `ids` instead of `id` moves each one's in one write, all or none, one `labels_changed` per ticket in the order named. `labels_changed { ticket, added, removed }` (MENGER-12) — each entry a label (letters, digits, `-`, ≤ 32, lower-cased), `add` added and `remove` taken off (a label in both is added), the ticket's labels kept sorted, each once, ≤ 16; `added` / `removed` are what moved, in the order given — both empty is no write and no broadcast. A bad entry or too many is one in-band `error`, nothing written; `not_found`. Durable before it is acknowledged; `updated_unix` and the ticket's cell (`label <name>`) move, nothing else; `watch` carries every `labels_changed` that moved something |
| `tickets.update { id, title?, body?, kind?, status?, parent?, severity?, block?, unblock?, source?, message? }` | `updated { ticket, changed }` (MENGER-30) — any part of a ticket in one write, all of it or none: each aspect by its own write's rules (`amend`, `set_kind` — the kind before the status —, `set_status` for a status alone, `set_parent`, `create`'s for `block`; `unblock` removes; an id in both refused), all asked first — a refusal is one in-band `error` naming every reason, nothing written; `severity: "none"` clears it; `changed` names what moved, in the order `title`, `body`, `kind`, `status`, `parent`, `severity`, `blocked_by`, empty (no write, no broadcast) when nothing would; `not_found`, or `body_too_large`. `watch` carries each aspect's own events, in that order — `amended`, `kind_changed` / `status_changed` and their `transition`s, `blockers_changed { ticket, added, removed }` then each blocker's `unlocks_changed { ticket, added }` / **`unlocks_removed { ticket, removed }`**, `parent_changed` / `children_changed` / `child_removed`, **`severity_changed { ticket, previous }`** — then the write's `group_changed`, `placements_changed`, components, inputs and (filtered) `entered` / `left` |
| `tickets.update_many { ids \| filter, max?, tz?, kind?, status?, parent?, severity?, block?, unblock?, label?, unlabel?, source?, message? }` | `updated_many { ids, changed, tickets, unchanged }` (ACT-2) — `update`'s change to every ticket `ids` names, or that `filter` admits at the write (in the caller's reach, at most `max`, default 500 — more is an in-band `error` naming the count), in one write, all or none: each ticket by `update`'s rules (`label` / `unlabel` by `label`'s), each id's write grant asked; any refusal is one in-band `error` naming each refused id with its reasons, nothing written. `ids` the tickets it changed, `changed` the aspects that moved in any (`update`'s order, then `labels`), `tickets` every one selected as the write left it, `unchanged` those nothing changed for (no write). `message` on every transition; the write's first is the `cause` of the rest. `watch` carries each ticket's own events, then one `updated_many { ids, changed }` (`tickets`, `unchanged` empty), then (filtered) `entered` / `left` |
| `tickets.delete { ids \| filter, max?, tz?, children?, unblock?, message? }` | `deleted { ids, tombstones }` (ACT-5; admin) — the tickets gone for good, all or none: refused, naming every reason of every ticket, while one has children (unless `children: reparent` — to the nearest ancestor that stays — or `delete` — the subtree too, every id in `ids`), a ticket waits on it (unless `unblock`), it roots a shortcode that holds a ticket that stays, it defines a component or a theme, or an input of its is `open` or `answered`. A tombstone each (`id, title, by, name?, at_unix, message?, restorable_until_unix?, parent?, reparented, unblocked`); the history kept; the body 30 days, then purged (at start, then daily); the number never reused. `watch`: each moved ticket's own event, then `deleted`, then groups, placements and a filter's `left` |
| `tickets.reshortcode { ids, to, new_scope?, parent?, with_children?, message? }` | `reshortcoded { moves: [{from, to}], tickets }` (ACT-6; editor on the moved tickets and where they go) — the tickets moved into `to`, renumbered in the order given, all or none (the section above); every old id pinned to its new ticket (`get` answers it with `moved_from`). `watch`: `reshortcoded` first, then the stayed children's `parent_changed`, groups, placements, a filter's `left` / `entered` |
| `tickets.undelete { ids }` | `undeleted { ids, tickets }` (ACT-5; admin) — deleted tickets back, whole, within 30 days, all or none: each child its delete moved back under it and each block it dropped back, where nothing has moved them since; refused for an id not deleted, a purged body, a parent or blocker still deleted (undelete it too), a path or a component taken since. `watch`: `undeleted` first, then the parents' and children's events, a filter's `entered` |
| `tickets.set_parent { id, parent }` | `parent_changed { ticket, previous }` (MNG-12) — files an existing ticket under `parent`: the child's row is written, and it leaves the old parent's children and joins the new one's (SCALE-4: neither parent's row is rewritten — SCALE-28 stores each one's new `child_count` on it, in the move's transaction; `watch` carries `children_changed` and `child_removed` with each parent's new `child_count`); the same guards as `create` (`not_found`, must exist, inside the shortcode unless the ticket is its root, no self-ancestry — in-band `error` naming the reason); the parent it already has is a no-op |
| `tickets.groups {}` | one `group { group: { epic, total, done, docs, root, root_row, host } }` per shortcode row the nav draws, in shortcode order, then the stream's own `done` (MNG-10) — the nav's tree without its rows: `total`/`done` count the row's whole subtree as AGS-20's `plan()` nests it (sub-shortcodes and cross-shortcode children included; `done` is `Complete` only; SDLC-11: the work only — `docs` counts the docs, and a row of docs alone is drawn), `root` is the lowest-seq `scope` (or `doc`), `root_row` the root's `{ title, status, kind }` (AGS-23: the nav draws the shortcode as the root's row; `null` with no root), `host` the ticket a sub-shortcode's row nests under (`null` at top level). Kept by the store on every write, so the cost is the number of shortcodes, not tickets |
| `tickets.components {}` | one `component { component: { name, source, owner, references, docs, preview } }` per component, in name order, then the stream's own `done` (COMP-3) — `source` the ` ```component name ` block's content, `owner` the defining ticket (`null`: an orphan), `references` the tickets whose bodies use `<name`, in the store's order, then the components whose sources do, by name; COMP-8: `docs` the source's `<docs>` parsed (`{ summary, attrs, slots, examples, help }`, `null` without one), `preview` the opening line's attributes (`[[name, value], …]`). From the cache; `watch` keeps it current with `component_changed` / `component_removed` |
| `tickets.inputs { ticket?, epic?, state? }` | one `input { input: { ticket, id, kind, question, options, recommendation, state, answer, history } }` per input of `ticket` (every ticket's without one) whose ticket is in the shortcode `epic` and which is in `state` (`open` / `answered` / `closed` / `removed` / MENGER-13 `withdrawn`), then the stream's own `done` (INPUT-3) — in the store's order of tickets, each ticket's inputs in its body's order and the removed after; `kind` `choice` / `multi` / `text`; `options` `[{ value, label, recommended }]`; `question` and `recommendation` `null` without one; `answer` `{ value, rationale, source, at_unix }` or `null`; `history` every change, oldest first. A ticket that is none is `not_found`; a state outside the four an in-band `error`. From the cache; `watch` keeps it current with `input_changed` |
| `tickets.answer { ticket, input, value, rationale?, source? }` | `input_changed { input }` (INPUT-3) — records `value` (typed, above) against the input, checked against its kind and the options its body declares, with `rationale` (blank: none), `source` (`app` / `api`, default `api`) and the time; every call is one history entry. One in-band `error`, nothing written: the ticket declares no such input (naming those it does), the value does not answer it (naming why), it is closed, or it was removed; `not_found` for a ticket that is none. The body and the ticket's row are untouched |
| `tickets.clear_answer { ticket, input, source? }` | `input_changed { input }` — the answer cleared, `open` again, the clear in its history with its `source` (`app` / `api`, default `api`; INPUT-9: the host says `app` for a frame's `menger.input.clear`); `input { input }` when it held none (nothing written); in-band `error` when closed, removed or not declared, or for a source outside the two (INPUT-3) |
| `tickets.close_input { ticket, input }` | `input_changed { input }` — frozen, with or without an answer (a decision's DECIDED); `input { input }` when closed already; in-band `error` when removed or not declared (INPUT-3) |
| `tickets.reopen_input { ticket, input }` | `input_changed { input }` — `answered` again when it holds an answer, else `open`; `input { input }` when it was not closed; in-band `error` when removed or not declared (INPUT-3) |
| `tickets.extents {}` | one `extent { epic, last_seq, holes? }` per shortcode that holds or held a ticket (ACT-5: `last_seq` the highest number it has given, a deleted ticket's included — numbers are never reused — and `holes` the numbers below it it does not hold, absent when none; a client's arithmetic skips them), in shortcode order (the store's), then the stream's own `done` (SCALE-16) — where each shortcode ends: ids are allocated highest-plus-one and never deleted, so a shortcode holds `last_seq` tickets, row k of it is `EPIC-k`, and a position in the store's order becomes a `list { after }` cursor without a walk; every `last_seq` together sums to `health.ticket_count` (a hand-edited database with a hole still reports the highest sequence; `after` naming the missing id is `list`'s cursor error). Not a roll-up: the shortcode's own tickets only, and every shortcode has one whether or not the nav draws it a row. Read from the sequence `create` allocates from (SCALE-3), so the cost is the number of shortcodes; no `watch` event — `created` carries the shortcode's new `last_seq` as its `seq` |
| `tickets.waves { epics, kinds? }` | the order of work (WAVE-4): one `placed { summary, placement, waiting_on }` per ticket of the shortcodes `epics` (case-insensitive; one with no tickets contributes nothing) whose kind is in `kinds` (default `build`, `spike`, `gap`; a doc is never placed), then exactly one `wave_counts { waves, missing, done }` — `waves[i]` counts the placed tickets in wave `i + 1`. `placement` is computed over the **whole** `blocked_by` graph by `ready_set`'s rule (MNG-4), blockers followed into any shortcode, asked for or not: `{"type": "done"}` when the ticket is `Complete`, `Superseded` or `Canceled`; `{"type": "missing_blocker"}` when it, or anything it waits on, names a blocker that is no ticket (or, only by a hand edit, sits on or behind a `blocked_by` cycle); otherwise `{"type": "wave", "n": n}` — 1 with every blocker satisfied, else 1 + the highest wave among its unsatisfied blockers. `parent` orders nothing, and a `Blocked` status is a marker, not an edge. `waiting_on` is the ticket's unsatisfied direct blockers (missing ids included), in `blocked_by` order, distinct; `[]` when done. Order: waves ascending, inside a wave `Coding`, `In Review`, `Ready`, `Pending`, `Blocked`, `Idea`, `Epic`, then (epic, seq); then the missing lane; then the done — each in (epic, seq). `epics` empty, or a kind outside the six, is one in-band `error` and nothing else. WAVE-6: read from the placements the store keeps (below) — the cost is the placed tickets — and `waiting_on` worked out per row |
| `tickets.watch { epic? }` | open-ended stream of `created` / `status_changed` / `kind_changed` / `amended` / `commits_linked` / `commits_unlinked` / `labels_changed` / `unlocks_changed` / `unlocks_removed` / `blockers_changed` / `severity_changed` / `children_changed` / `parent_changed` / `child_removed` / `group_changed` / `placements_changed` / `component_changed` / `component_removed` / `input_changed` — INPUT-3: an input a write moved is one **`input_changed { input }`** — the whole input, its state, answer and history, so applying it is one upsert — after every other event of the write, the component events included: an `answer`, `clear_answer`, `close_input` or `reopen_input` (its one event), or a `create` / `amend` whose body declared, re-declared or removed inputs (in the body's order, the removed after); `epic` filters it by its ticket's shortcode — COMP-3: a `create` or `amend` that moved the component registry ends — after every other event of the write, `placements_changed` included — with one **`component_changed { component }`** per component it registered or moved (source, owner, references: the whole component, so applying it is one upsert), in name order, then one **`component_removed { name }`** per component it took out; a component is in no shortcode, so both pass any `epic` filter — each carries the whole affected ticket but the parent's side of a birth or a move, `children_changed { id, added, child_count }` / `child_removed { id, removed, child_count }` (SCALE-4: under 1 KB whatever the parent's size; it was the whole parent); a write that moves a roll-up is followed, after its ticket events, by one `group_changed { group }` per row that moved, innermost first (a row no longer drawn arrives once with `total: 0`) — WAVE-6: and a write that moved any kept placement ends with one **`placements_changed { changes: [{ id, placement }] }`**, after its other events (`epic` keeps the changes in its shortcode, and the event only when one is left) — AGS-23: so is a shortcode root's status change (any, not only across `Complete`) or new title, which moves its group's `root_row`; `epic` filters a group by its own shortcode, a parent's side by the parent's; **`lagged { missed }`** (SCALE-5) where a subscriber more than the server's broadcast holds (256 changes) behind lost changes — once per gap, in place of the `missed` changes lost (`missed` ≥ 1, the channel's count), whatever the `epic` filter, then the oldest change still held, in order, none twice; not an error, the stream goes on — what the client holds may be stale, so it fetches again what it shows |

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

### Addresses and namespaces (ORG-3)

A shortcode is unique only within its **namespace** — its organization and
the shortcode it is hosted under (none for a root). Two shortcodes named
THEME, one under MENGER and one under PLEXUS, both exist; a second THEME
under MENGER is refused. Every ticket and every group carries:

- `epic` — the shortcode's letters (`THEME`), what a reader types and a row
  shows; never identity (in Rust it is `Code`, which keys no map);
- `shortcode` — the store's identity of it (`ShortcodeId`): the code itself
  for every shortcode made before ORG-3, `CODE.N` for a namesake made since;
  a ticket's key (`id`) is `{shortcode}-{seq}`, so every id that existed
  before ORG-3 is unchanged;
- `address` — the full address, `org:ROOT/…/SC-N` (a group's without
  `-N`): `hypermemetic:MENGER/ACCESS/ORG-3`.

Events that name a shortcode (`extent`, `count`, `entered`, `left`,
`recount`) carry `shortcode` beside `epic`.

**Naming a ticket.** Every method that takes a ticket or a shortcode takes
any short form of its address — the full one, any subsequence of its hosts
(`MENGER/ORG-3`, `ACCESS/ORG-3`), the org alone (`hypermemetic:ORG-3`), the
bare id, a path address (`PLX/_MENGER/board`) — case-blind in the codes. An
id that existed when the file met ORG-3 is **pinned**: it names that ticket
forever, whatever namesake is made later. A name that fits two is refused
in-band, nothing read or written:

```json
{"type": "ambiguous", "id": "THEME-1", "candidates": ["hypermemetic:MENGER/THEME-1", "hypermemetic:PLEXUS/THEME-1"]}
```

**Writes that make or move a shortcode** check (namespace, code) and refuse
a clash naming the one there (`hypermemetic:MENGER/THEME is already
there: a shortcode is unique in its namespace, and hypermemetic:MENGER holds
a THEME — give another code, or another place (nothing was created)`): `create` of a scope or a
doc under another shortcode's ticket (it starts a new shortcode there,
hosted where its parent is — a namesake if the letters are taken
elsewhere); `set_parent` / `update --parent` of a root (the shortcode's
namespace moves with it, and so does every address under it — each moved
group is a `group_changed`); `set_kind` (a ticket that becomes its
shortcode's root); `notes upgrade`; `undelete`. A build or an execution
never starts a shortcode: it files into the one its `--epic` names.

**A scope roots its own shortcode** (ORG-5; owner, 2026-10-13: *a child
scope roots its own shortcode*). A scope is born only as its shortcode's
root: `create --kind scope --parent X` naming X's shortcode as `--epic` is
refused (`a scope under X roots its own shortcode; pass a new --epic`), and
with a new `--epic` it is that shortcode's `-1`, hosted under X. `set_kind`
(and `update --kind`, `update_many --kind`) to a scope is refused for a
ticket below its shortcode's root (`ScopeNotRoot`, naming `reshortcode
--new-scope` as the way); `notes upgrade` is a create and takes the same
rule. A root goes between scope and doc as before. A scope that sits inside
its parent's shortcode from before ORG-5 moves out with `reshortcode --ids X
--to NEW --with-children true` (it becomes NEW-1, its tree with it);
`scripts/org-5-move.sh PORT` moves the tracker's.

```sh
synapse -P 4455 menger tickets create --epic THEME --kind scope --parent PLEXUS-1 --title "the platform's themes"
synapse -P 4455 menger tickets create --epic PLEXUS/THEME --title "a build in it"
synapse -P 4455 menger tickets get --id PLEXUS/THEME-1
```

**Filters and search.** `in THEME` matches the letters, wherever hosted
(every THEME); `under X` binds one: a shortcode whose identity is `X` first
(so a filter written before a namesake keeps its meaning), else the one
shortcode or ticket the address names — a name that fits several is
refused (`under THEME/CSS` names more than one). `find` and the palette
also match the shortcode's letters alone, partial (`the` finds every
THEME), and an address (`MENGER/ACCESS/ORG`); every hit shows its full
address. `search` (SEARCH-2) still indexes titles and bodies only.

**Stored.** Form 11 adds three tables beside the tickets: `orgs`
(`hypermemetic` is 1), `shortcodes (id, org, host, code, pinned)` with a
unique index on `(org, host, code)`, and `pins (text, ticket)`. A form-10
file gains them at its first open, in place, with nothing built again:
every shortcode its code, in hypermemetic, in the namespace where its root
hangs; every id and tombstone pinned. `--verify-derived` checks the index:
every ticket's shortcode registered, no two of one code in one namespace,
each stored host where its root hangs, every full address distinct, every
pin a ticket or a tombstone.

### Organizations (ORG-4)

Every ticket and every shortcode is in exactly one **organization**;
today's tracker is `hypermemetic` (ORG-1). Owner, 2026-10-13: *"we need to
be able to easily create new orgs and scope roles to them."* An
organization is a name — lower-case letters, digits and `-`, unique
app-wide among every name an organization has or had — and one call to
make (the `orgs` activation; its logic is `tickets-api/src/store/orgs.rs`):

| method | who | yields |
|---|---|---|
| `orgs.list {}` | any member (viewer, editor) | one `org { name, aliases, roots, shortcodes, tickets }` per organization the caller holds a role in — every one for a role on `*`, and with login off |
| `orgs.create { name, admin? }` | admin on `*` | `created { org, admin }`: the organization, empty, and `admin`'s grant of admin on `name:` (in one transaction) |
| `orgs.rename { name, to }` | admin on the org, or on `*` | `renamed { from, org }`: the old name becomes an alias — every address and grant scope written with it keeps resolving; every grant on the org and every full address moves to the new name |
| `orgs.delete { name }` | admin on the org, or on `*` | `deleted { name, grants }` — refused while it holds a shortcode (a ticket, or a deleted ticket kept for its undo), and for the first organization; its grants go with it |

An organization the caller holds no role in is `not_found` to it (never
refused by name). Tickets go into one by their address — `--epic acme:MENGER`
— or by `tickets.create --org acme`; with neither, a create goes to the
caller's **only** organization, and is refused, naming them, when it holds
roles in several and the epic names no shortcode in exactly one of them. A
new root in an organization takes `tickets.create` on it (`acme:`), not on
`*`. Login off: one organization, as before, and nothing needs naming.

```sh
synapse -P 4460 -t "$ADMIN" menger orgs create --name acme --admin key:9f86…
synapse -P 4460 -t "$ACME" menger tickets create --epic MENGER --kind scope --title "acme's menger"   # acme:MENGER-1
synapse -P 4460 -t "$ACME" menger members grant --principal key:4b… --scope acme: --role editor
synapse -P 4460 -t "$ACME" menger members grant --principal key:4b… --scope acme:MENGER --role viewer
synapse -P 4460 -t "$ADMIN" menger orgs rename --name acme --to acme-co
```

**A grant's scope** (ACCESS-37's grammar, stored as text in `members`):
`*` — every ticket; `org:` — every ticket in that organization; `org:ID` —
a shortcode in it, by its identity (`hypermemetic:IDENTITY`,
`acme:MENGER.12` for a namesake), and every shortcode hosted under it. A
reader writes a shortcode by any short form of its address (`IDENTITY`,
`acme:MENGER/THEME`), and `members.grant` / `auth.grant` keep it by
identity, so a grant follows its shortcode wherever its root moves.
ACCESS-21 adds `under ID` and `ID` (`members::Scope`, one variant each). An
org admin (admin on `acme:`) grants and revokes inside acme and nowhere
else; `members.list` shows them acme's grants.

**What a reader sees.** A reader holds roles in some organizations; every
read answers from where its grant is held, as before — `under acme:` is the
filter language's every ticket in acme (bound to its roots), so another
organization's tickets, shortcodes, counts and groups are not in the
answer, and `get` of one is `not_found`. A short form is read in the
reader's organizations: `get --id MENGER-2` by a reader of acme alone is
acme's MENGER-2 even where hypermemetic's is pinned to the same text; an
`ambiguous` answer names only candidates in the reader's organizations
(none left: `not_found`). The gate decides a **write** before the method
reads anything, from the whole store, so a short form there must fit one
ticket (`acme:MENGER-2` always does); a write on another organization's
ticket is refused naming the ticket as written — the same words as for a
ticket that does not exist (`needs tickets.set_status on PLEXUS-2`).

**Where a target lives** (the gate's `Resolve::chain`): the ticket as
written, its shortcode `org:ID` and bare `ID`, itself and its ancestors,
each hosting shortcode the same way, and last its organization, `org:` —
so a membership on `acme:` reaches every ticket in acme, and a session's
ceiling minted before ORG-4 (`editor@IDENTITY`) still holds.

**Migration.** No stored form moves (ORG-3's form 11 made `orgs`); the file
gains `org_aliases` beside it. The first open by this build moves every
grant from before organizations into its own — `IDENTITY` becomes
`hypermemetic:IDENTITY`, `*` stays site-wide — durably, once (`ORG-4:
moved N grant(s) …`). An older build reads the moved grants as scopes it
cannot place: it holds them nowhere (fail closed); promote's rollback
restores the backup it took first.

**In the app**: the header shows an organization switcher when the reader
holds roles in more than one; the URL carries the choice (`o=acme`, carried
from route to route as `g=` is, and kept per viewer), and every surface the
lens filters reads within it (`under acme:` ANDed into each level's
effective filter: the board, the nav, the wave boards, the inputs page, the
palette).

### Search (SEARCH-2)

`find` jumps to a ticket by its id or title; **`search`** looks inside
tickets. Its index is SQLite FTS5 in the same database file — no second
store, nothing else to run: two tables (`search_rows`, each ticket's id and
index row; `search`, FTS5 over `title` and `text`, `unicode61` with
diacritics folded) and FTS5's vocabulary view over them (`search_terms`).
A file gains them once, on its first open, every ticket indexed in one
transaction (`SEARCH-2: the file gained the search index — N tickets' …`);
from then on `create`, an `amend` that changes the title or the body, and a
seed's chunks write a ticket's index row in the transaction that writes the
ticket. `--verify-derived` holds the index to the tickets (one row each,
each the title and text the ticket gives now, and FTS5's own integrity
check).

What is indexed is what a reader reads: the body's text without its tags,
attributes, HTML comments, `<style>`/`<script>`, ` ```component ` blocks,
frontmatter and a mermaid diagram's syntax — but with the text between the
tags, a diagram's labels, the values of `ref`, `name`, `title`, `alt`, `label`, `labels`,
`cost` and `benefit` (an `<m-ticket ref>`'s id, an option's name), and code
as written.

```sh
synapse -P 4455 menger tickets search --query 'plcement wave'
synapse -P 4455 menger tickets search --query 'set_status' --filter 'not category done' --limit 5
synapse -P 4455 menger tickets search --query '"ready set"' --offset 20
```

How a query is read: every word is required, and matches itself and as a
prefix (`plac` → placement). A word of four letters or more that the
index does not know — or holds in at most 3 tickets, as a typo some
ticket repeats does — also matches the index's words one edit away (two
from eight letters; an insertion, a deletion, a substitution, a swap of
neighbours), the first letter kept, at most 8, and a rare word only words
five times as common; the last word's corrections may be prefixes
(`plcem` → `placem…`). Words with a digit are never corrected. A word
joined by `-` `_` `.` `/` is a phrase with its last part a prefix; a
`"quoted"` one is an exact phrase. The tickets matched as typed come first,
then those that needed a correction (`corrected: true`); `searched.words`
says what each word was corrected to.

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
| `comments.list { ticket?, under?, route?, path?, state?, author?, text?, after?, limit? }` | one `comment` per match, newest first, then one `page { next }` — `next` the id to pass as `after`, `null` when nothing more matches. Every filter given holds: `ticket`; `under` — a ticket or a shortcode in the comment's ticket's lineage (itself, its ancestors, its shortcode, the shortcodes hosting it: the filter language's `under`, as the tree stands); `route` exactly; `path` a prefix; `state` `open` / `resolved`; `author` exactly; `text` in the body, the label or the excerpt (any ASCII case). Blank is none. A page is 500 unless `limit` says, never more than 1,000 (ACCESS-29). An unknown state, or an `after` naming no comment, is one in-band `error` |
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

**A ticket's Conversation** (APP-22): a ticket page ends with its comments,
oldest first — every comment whose anchor names the ticket, whole-ticket and
pinned alike (a pinned one says what it is on and links to its pin), each
with its author, time, body as markdown and Resolve / Reopen — then a box,
"Comment on <ID> — Ctrl+Enter sends" (`c` on the page focuses it; Esc
leaves it). A comment sent there is on the ticket as a whole: its anchor is
`{ route: "#/ticket/<ID>", path: "conversation", label: "Conversation",
ticket: "<ID>" }`, no pin is drawn for it in annotation mode, and its link
back (`?comment=<id>`) flashes its row. Agents read them as any comment:

```sh
synapse -P 4455 menger comments list --ticket APP-22
synapse -P 4455 -p '{"ticket": "APP-22", "path": "conversation"}' menger comments list   # whole-ticket only
synapse -P 4455 menger comments add --anchor.route '#/ticket/APP-22' --anchor.path conversation \
  --anchor.label Conversation --anchor.ticket APP-22 --body "not what I meant: do X instead"
```

### History and cancellation (MENGER-13)

**Every change of a ticket's status or kind is a transition**, kept by the
store, write-through, append-only, in the `transitions` table: its ticket,
the field (`status` / `kind`), `from` → `to` (`from` null at birth), when,
who (`actor`: the caller's principal; `system` for the store's own;
`anonymous` with login off — `name` the caller's display name), from where
(`source`: `app`, `api` — synapse and agents, the default —, `pipeline`, or
`cascade`), why (`message`) and what caused it (`cause`, a transition id,
with `cause_ticket`). `create` records the first status, `set_status` each
change (the same status again records nothing), `set_kind` the kind's change
and — when the status moves with it — the status's, caused by the first.
`set_status` and `set_kind` take `source` and `message`; the app says `app`,
the merge should say `pipeline`. A file gains the table, empty, on its first
open, with the moment it began: nothing before then has a history, and the
app's History section says so. It is no derived state: the stored form is
unchanged (10).

```sh
synapse -P 4455 menger tickets history --id MENGER-7
synapse -P 4455 menger tickets set_status --id MENGER-7 --status Canceled --message "folded into MENGER-9"
synapse -P 4455 menger tickets set_status --id MENGER-7 --status Pending --restore-children true
```

**Cancellation cascades.** `set_status … Canceled` cancels, in the same
write, every descendant still open work — its whole subtree (hosted
shortcodes too) but what is `Complete`, `Superseded` or `Canceled` already;
a doc keeps `Doc`. Each is a transition of `system`'s, from `cascade`, its
message naming the cause and who ("canceled because MENGER-7 was canceled by
Ada"), its `cause` the cancel. The canceled tickets' open inputs become
**`withdrawn`** (history kept, the reason in it): out of `inputs { state:
open }`, out of `asking`, so out of Needs you, and refused an answer until the
ticket is not canceled. A body written while canceled declares its new inputs
withdrawn. **Undo**: leaving `Canceled` restores the ticket's withdrawn
inputs; with `restore_children`, the descendants its latest cancel canceled
and nothing has moved since go back to the statuses they had (each
`system`'s, caused by the restore) — one canceled on its own, by another
cancel, or moved since stays. `history`'s `history_end.restorable` names
them; the app asks before it restores them. Only `set_status` cascades.

A write that moves other tickets beside its own is **one write**: every
step's row, derived state, transition and inputs in one transaction; each
step's derived change is worked out against the cache the steps before it
left, so the batch holds the cache's write lock from its first step to its
commit (no reader sees a step the file does not hold), and a failed commit
reads the cache back from the file. Its events are one write's: every
ticket's `status_changed` and `transition`, then each group that moved
(once), one `placements_changed`, the `input_changed`s.

**In the app**: the ticket page's History section (when, from → to, who,
source, message, the causing ticket as a link; what its changes caused;
when history began). In the nav a canceled ticket is drawn closed with its
count ("3 hidden"); its descendants are not shown under it unless the reader
opens it there (for this view only) or the open ticket is below it. `under
X` still finds them everywhere. The board draws done-with rows (complete,
superseded, canceled) quieter. Needs you's count never counts a canceled
ticket (`not status Canceled` beside its filter).

**Filtering by history.** The language has four more predicates, answered
from the log's kept indexes (each status's entries and exits, every
transition, each actor's — sorted by time, so a window is a range, never a
scan of tickets), composable with every other:

| predicate | holds when some transition of the ticket's … |
|---|---|
| `entered <status> [window] [by who]` | went into the status |
| `left <status> [window] [by who]` | went out of it |
| `changed [window] [by who]` | is any change (a birth, a kind) |
| `by <who> [window]` | is anyone's (`changed … by who`) |

A window is `within <n>{m,h,d,w,mo}` (a month is 30 days), `today`,
`yesterday`, `this week` (from Monday), `since <moment>`, `before <moment>`
or `between <moment> and <moment>` — a moment a date `2026-10-10`, a date
and a time `2026-10-10T08:00` (a date ends a `between` whole), or
(FILTER-27) a length back from now, `2w ago`, `12h ago`, `1mo ago`; none is
"ever, since the log began". Who is `me`, `system`, or a principal. Days
are the viewer's: every filtered read takes `tz` (an IANA name, `UTC`, or
an offset like `+02:00`), the app sends the browser's, and synapse reads
UTC unless `--tz` is given. The filter menu's **When** builds all of it
(see "Filters" above); the palette's `filter …` takes any of it.

**Filtering by when** (FILTER-27) — two more, over time itself rather than
the log, answered from the **times** kept beside it (each ticket's birth,
and since when it has held its status — sorted, so a window is a range):

| predicate | holds when the ticket … |
|---|---|
| `created <window>` | was born in the window (its `created_unix`; no `by`) |
| `for over <length>` / `for under <length>` | has held its current status longer than the length, or not |
| `status S for over D`, `category C for over D` | is in S (or C) and held it that long — sugar for `status S and for over D`, a list too (`category in (ready, active, review) for over 1w`); the canonical form prints the sugar |
| `touched <window>` (FILTER-30) | was last touched in the window — any write to its own row (title, body, status, kind, severity, parent, labels, blockers — the blocker's `unlocks` too —, commits), a comment left on it, or an answer to or a clear of one of its inputs; no `by`. `not touched within 2w` is "hasn't changed in 2 weeks" |

Since when a ticket holds its status is its latest status transition's
moment (a birth's, for one born since the log began). A ticket the log says
nothing of — from before it began — is taken to hold it since its last
update or, updated since the log began, since the log began (a change of
status since then would be on record), never before its birth: a lower
bound, which `history_end` says as `status_since_exact: false`. Every
status write moves the ticket's entry in the same critical section;
`--verify-derived` rebuilds the times from the rows and the log and holds
them to it.

A ticket's **last touch** (FILTER-30; the owner's answer to its decision:
"any edit, plus comments and answers") is the later of its row's
`updated_unix` — which every write to the row moves, and nothing derived
does: a roll-up, a child's count, a placement leave it — and its last
**activity**, a comment anchored to it or an answer to (or a clear of) one of
its inputs, which writes no row. Activity is kept beside the times and read
back at open from the comments and the inputs' histories; a close, a reopen,
a withdrawal and a resolve are not touches. `changed` keeps meaning a
transition. A write that touches a ticket without moving its cell (a body, a
comment, an answer) still moves an open `watch` asking `touched`: the write
works out that watch's `entered` / `left` for it, as it does for `under`.

A `watch` whose filter moves with the clock — a relative window, an `ago`,
a `for` — keeps one timer, set for the next moment time alone moves it (the
first ticket to cross a length, a transition or a birth leaving a window,
tomorrow or next Monday in the reader's zone; at most an hour), and then
says `entered` / `left` for what crossed — no write needed.

```sh
synapse -P 4455 menger tickets list --filter 'created today' --tz Europe/Berlin --summary true
synapse -P 4455 menger tickets list --filter 'left Pending this week' --summary true
synapse -P 4455 menger tickets list --filter 'status Pending for over 2w' --summary true
synapse -P 4455 menger tickets counts --filter 'category in (ready, active, review) for over 1w'
synapse -P 4455 menger tickets list --filter 'created between 3w ago and 1w ago and kind build' --summary true
synapse -P 4455 menger tickets watch --filter 'status Ready for over 3d' --membership true
synapse -P 4455 menger tickets list --filter 'not touched within 2w and not category done' --summary true
synapse -P 4455 menger tickets list --filter 'touched today' --summary true
synapse -P 4455 menger tickets list --filter 'not touched since 2026-10-01 and under THEME' --summary true
```

```sh
synapse -P 4455 menger tickets list --filter 'status "In Review" and entered "In Review" within 12h' --summary true
synapse -P 4455 menger tickets list --filter 'entered Coding yesterday' --tz Europe/Berlin --summary true
synapse -P 4455 menger tickets counts --filter 'changed within 1h and under SCAFFOLD-1'
synapse -P 4455 menger tickets list --filter 'entered Complete this week and kind build' --summary true
synapse -P 4455 menger tickets list --filter 'entered Canceled within 1d by system' --summary true
```

`--verify-derived` holds the log to the tickets: each ticket's latest status
and kind transition ends where the ticket is, each starts where the one
before ended, each `cause` is an earlier transition, a cascade has one, the
system's has a message.

### Notes (NOTE-2)

A **note** is a thought written down before it is a ticket: dated, never
titled, on no shortcode, kept by the server in one place for every client.
`notes` is the activation beside `tickets` and `comments`; its notes are
records of their own behind the service-wide cache, in a `notes` table a
file gains, empty, on open.

```sh
synapse -P 4455 menger notes add --body "the palette should remember its last query"
synapse -P 4455 menger notes list --state open            # newest first; --author, --text, --after/--limit
synapse -P 4455 menger notes get --id 7
synapse -P 4455 menger notes upgrade --id 7 --epic MENGER --parent MENGER-2
```

| method | yields |
| -- | -- |
| `notes.add { body, author? }` | the note kept: a new `id`, `at` (unix seconds), its `author` (`owner` unless given; a signed-in caller's own name, and its `principal`), the `body` as written, `ticket: null`. A blank body, or one over 64 KiB, is one `error` |
| `notes.list { state?, author?, text?, after?, limit? }` | `note`s newest first, then `page { next }`. `state` is `open` (not yet a ticket) or `upgraded`; `text` a substring of the body, any ASCII case |
| `notes.get { id }` | the note, or one `error` |
| `notes.upgrade { id, epic, title?, kind?, parent? }` | the note, its `ticket` and `upgraded_at` set: in one write, a ticket born as `tickets.create` makes one — titled `title`, else the note's first line with words (markdown marks dropped, at most 80 characters, cut at a word), its body the note's text and a line saying when it was noted, `kind` (default build) and `parent` as `create` takes them — and the note marked. A note already a ticket is refused, naming it; a ticket `create` refuses leaves the note open and spends no id |

Under login a note is any member's — `add`, `list` and `get` are a viewer's,
held anywhere — and `upgrade` is an editor's that also needs
`tickets.create` where the ticket lands (the parent, else the shortcode's
root, else `*`).

### Queue (MENGER-39)

A ticket is **queued for the owner** with an urgency and a reason — one
write, by anyone with the editor role, agents included — and every open app
shows it at once in **the tray**: a tab per queued ticket along the bottom
edge of the main column, most urgent first, then the oldest. A tab is the
id, the title and an urgency chip (`today` in the Coding colours; `now` and
`blocking` in the contract's `--urgent` red, taller, nudging every few
seconds until opened); a new one slides up into place. A click raises a
sheet above the tray — the urgency, who queued it and when, the reason, the
named input's question answerable in place (recorded with source `app`;
answering takes the ticket out of the queue, the input stays the agent's to
close), *open the ticket*, *snooze 10m* and *done with it*. A hidden tab's
title carries the count, `(2) menger`. Motion follows the setting: `calm`
keeps the slide and drops the nudge, `none` moves nothing.

```sh
synapse -P 4455 menger tickets queue --id RETURNS-13 --urgency blocking --input landing --reason "idle until decided"
synapse -P 4455 menger tickets queue --id MENGER-41 --urgency today --reason "the promote rolled back" --source pipeline
synapse -P 4455 menger tickets queued --snoozed false
synapse -P 4455 menger tickets snooze --id MENGER-41 --minutes 10
synapse -P 4455 menger tickets unqueue --id MENGER-41
```

| method | yields |
| -- | -- |
| `tickets.queue { id, urgency, reason, input?, source? }` | `queued { entry, ticket, action }` — the entry as the write leaves it (`ticket`, `urgency`, `reason`, `input`, `by`, `name`, `source`, `queued_unix` — when first queued —, `asked_unix`, `snoozed_until_unix`), its ticket's row, and `action`: `queued`, `raised` (a higher urgency) or `requeued` (the urgency kept: never lowered; the reason and an input given are the new ones; a snooze ends). `blocking` requires `input`, an input the ticket declares and has not closed. A blank reason, an unknown urgency, an input the ticket does not hold or has closed is one `error`; a ticket that is none `not_found` |
| `tickets.unqueue { id, source? }` | `unqueued { id, entry, action: unqueued, by, at_unix }`, or `not_queued { id }` (nothing written) |
| `tickets.snooze { id, minutes, source? }` | `snoozed { entry, ticket }` — off the tray until `snoozed_until_unix` (1 minute to a week), still queued (`queued` holds). Not queued: one `error` |
| `tickets.queued { snoozed? }` | a `queue_row { entry, ticket }` per entry in the caller's reach, most urgent first, then the oldest — without the snoozed ones when `snoozed: false` — then `queue_end { count, snoozed, now_unix }` |

A `close_input` of the input an entry names takes it out in the same write:
`watch` hears `input_changed`, then `unqueued` with `action: closed` and `by:
system`. `watch` carries `queued`, `unqueued` and `snoozed` as they happen.
`queued` and `queued <urgency>` are filter cells (FILTER-3's language),
answered from the queue's kept entries — a snoozed ticket is still queued —
on every filtered read and watch; the Needs you filter (`g n`, on the nav)
is `queued or asking or label needs-you`. `history { id }` yields the ticket's **queue records**
after its transitions — `queue_record { record }`: `action` (queued,
raised, requeued, snoozed, unqueued, closed), `urgency`, `reason`, `input`,
`actor`, `name`, `source`, `at_unix`, `until_unix` for a snooze. They are a
log of their own (`queue_log`), not transitions: `changed` and `by` do not
read them, and an older release, which never looks at the queue's tables
(`queue`, `queue_log`, gained empty on open), still opens the file. Under
login `queue`, `unqueue` and `snooze` are an editor's, on the ticket;
`queued` a viewer's, answering only from where it is held.

### Deployments (DEPLOY-11)

menger keeps every deployment any CI records, to any environment, and
answers for any ticket where its work runs and since when. `deployments` is
the activation beside `tickets`; it takes DEPLOY-10's record
(`deploy/record.schema.json`) whole — its fields are `record`'s parameters,
so `deploy/record --send` passes the record as the params object. Three
tables a file gains, empty, on open: `deployments`, `deployment_commits` and
`deploy_environments` (what each environment runs — derived, written
through, checked by `--verify-derived`). menger's own pipeline records every
deploy test, sync, promote and rollback as it runs, and `deploy/pipeline
backfill` replays what it logged before (DEPLOY-12; `deploy/README.md`
"Recording deployments").

```sh
synapse -P 4455 menger deployments record --project menger --environment prod --state succeeded --revision 235ff62 --commits 824e943,235ff62
synapse -P 4455 menger deployments environments --project menger      # each: what it runs, since, the last deploy
synapse -P 4455 menger deployments list --environment prod --limit 20 # each deploy with the tickets it carried
synapse -P 4455 menger deployments for_ticket --id MENGER-34          # running in prod since …, in test since …, not on staging
synapse -P 4455 menger tickets list --filter 'not category done or not deployed prod'
```

- **A project is a shortcode** the store holds, any case (`menger` is
  MENGER) — what a ticket lives in and a grant names. A project name that is
  not a shortcode is refused.
- **An environment is a project's**: (project, name), the name kept lower
  case (`test`, `staging`, `prod`, `pr-412`, `review/pr-412`), shown per
  project test → staging → prod (development and previews first, unknown
  names between test and staging).
- **A deployment's tickets** are those whose linked commits
  (`link_commits`) are among its `commits` and `revision`, plus the ids its
  `tickets` names. menger runs no git: two references are one commit when
  one hash is a prefix of the other (a CI's 40-character SHA matches a
  merge's seven), the `repo@` aside. The join is kept: a commit linked after
  it was deployed counts wherever it runs, and an unlink takes it out.
- **What an environment runs** is its records folded in `(at, id)` order. A
  `succeeded` deploy runs `revision` and carries its tickets from `at`; a
  `rolled_back` record (`revision` the commit returned to, `previous` the one
  taken out), or a later deploy to an older revision, takes out what only the
  undone deploys carried; `started` and `failed` are recorded and shown and
  run nothing. A ticket runs there since the earliest deploy still counting
  that carried it.
- **A deployment never changes a status** — where a ticket runs is a fact
  beside it (DEPLOY-9).
- **A retry is one deployment**: the same project, environment, state,
  revision and `source.run` (else `at`) answers the kept one with
  `duplicate: true` and writes nothing.

| method | yields |
| -- | -- |
| `deployments.record { project, environment, state, revision, previous?, commits?, tickets?, release?, at?, source? }` | `deployed { deployment, carried, unlinked, live, entered, left, duplicate }` — the deployment kept (`at` RFC 3339 or unix seconds, now when omitted; `recorded_by` the caller), the tickets it carried, its commits no ticket links, whether it still counts, and the tickets that began and stopped running in its environment. A bad field is one `error` naming it, nothing kept |
| `deployments.list { project?, environment?, state?, ticket?, after?, limit? }` | `deployment { deployment, carried, unlinked, live }` newest first by `(at, id)`, then `page { next }` |
| `deployments.environments { project? }` | `environment { project, name, order, revision, release, since, deployment, last, last_state, last_at, tickets }` per environment |
| `deployments.for_ticket { id }` | `running { environment, since, deployment, revision, release }` for each environment it runs in, `not_running` for each other environment of its projects (its shortcode's, its hosts', any it ran in) |
| `deployments.running { project?, environment?, ids? }` | DEPLOY-13: `cells { project, environment, running: [{ ticket, since, deployment }] }` per environment (an empty one too), each ticket running there — only `ids`' (at most 1,000) when given — in the store's order: what the app's board marks are drawn from, in one read |
| `deployments.watch` | `deployed`, then `environment_changed`, as each is recorded; `lagged { missed }` |

**Filter cells.** `deployed <env>` (FILTER-3's language) is a ticket running
in an environment of that name — `prod` of any project, `menger/prod` of one —
and `deployed prod within 24h` one running there since a moment in the
window (every window a history predicate takes); `not deployed prod` the rest.
Answered from the kept cells, never by testing a ticket, on every filtered
read; `tickets.watch --filter 'deployed prod'` says `entered` / `left` when a
deploy or a rollback moves a ticket, and a relative window is read again as
time passes.

**In the app** (DEPLOY-13): a ticket page's **Running** says where its work
runs — `prod since 22:04 · test since 22:04 · not on staging`, each linked to
its deploy; a board row and a wave card carry a small mark per environment,
in the project's order; `#/deployments` (the header's `deployments`) shows
each project's environments as columns — what each runs, since when, and a
timeline of its deploys (state, when, the source's link, the tickets each
carried) — under the project's **waiting** work: Complete, not yet on its last
environment (`status Complete and under MENGER and not deployed menger/prod`).
The filter menu's **Deployed** category builds `deployed <env> [window]` and
`not deployed <env>` from the environments recorded (a pill like When's), and
the palette offers them as you type `deployed`, `not deployed` or `waiting`.
The app holds the environments and their cells (`environments`, `running`),
kept by `deployments.watch` — a deploy asks again only for its environment —
so every mark moves live, with no request per row.

**Under login**, `record` is its own grant, held by the `deployer` role
(`deployments.record` and the reads) — what a CI's key carries on the
project's shortcode (a minted session scoped `deployer@MENGER`); `admin`
holds it too, an editor does not. The reads are a viewer's and answer only
of the projects where the caller holds them. DEPLOY-14: `deployer` is a
membership too — `members grant --principal key:<hex> --scope PANTRY --role
deployer` gives a CI's key exactly that, and nothing it could widen.

**From a CI** (DEPLOY-14): a GitHub Actions job records with one step,
`hypermemetic-ai/menger/deploy/record-action@v1`; GitLab, Buildkite or a shell
script with one `deploy/record … --send --url wss://…` (over the WebSocket
through `deploy/menger-call.mjs`, no synapse; a deploy key in
`MENGER_DEPLOY_KEY` signs in), and GitHub's `deployment_status` webhook
through `deploy/github-relay.mjs` with no workflow change. deploy/README.md
"Deployments from any CI" has the snippets, the deploy key and the tunnel.

### Themes and their scopes (THEME-8)

A **theme** is a name and a map of token overrides over the token
contract (below), one map for light and one for dark; `default` overrides
nothing. A token that is not a colour (a face, a size, a radius) set for
light holds in dark too; a colour set for light only leaves dark's default. `themes` is the
activation beside `tickets`; themes and settings are kept behind the cache,
in `themes` and `theme_settings` tables a file gains, empty, on open.

**Four levels, the nearest wins.** The **site** (the shell, nav, board,
Home — with or without a ticket open), the **shortcode** (the ticket's, or
the nearest one hosting it that says anything), the **ticket view** (its
header, properties and body) and the **frame** (one body, or one component;
a component takes its frame's theme). Each may adopt a theme with a
**reach** — `frame` (only the frames inside it), `ticket` (itself and
everything inside; the default) or `site` (and the site, while the ticket is
the open one) — and may say **`takeover: off`**: it and everything inside
ignore the themes above and wear `default`, or their own adoption. The
**reader** is outermost: `#/settings` → Themes keeps their own site theme
(in place of the site's), "project themes off" (THEME-4: no shortcode's
setting reaches them — the site's, a ticket's and a frame's own still do)
and "every takeover off" (their site theme, or `default`, everywhere) in
this browser.

**A project adopts a theme** (THEME-4): a shortcode's setting is kept with
it; a hosted shortcode without one inherits the nearest host's that says
anything, up the kept tree, ending at `default` — the server answers it
(`theme_for`), and the app paints the same answer at once from the
shortcode tree it keeps, so walking from a project into a shortcode hosted
under it never detours by the site's theme. Walking into a project's ticket
repaints the page through THEME-7's one path, and walking out (the board,
Home, any page without a ticket open — they span projects) puts the site
theme back. The tracker's spelling is `themes set --scope PANTRY …` (THEME-8);
the `tickets set_theme --epic` of THEME-4's first draft was not added beside
it.

**The worked example: PANTRY** (THEME-6). The pantry look — warm paper, one
burnt-orange accent, Fraunces for what leads, Inter for what reads,
JetBrains Mono for code, generous radii, code and diagram colours for both
modes, a field's label skinned as a kicker (`m-field::part(label)`) — is
one ` ```theme pantry ` block on a PANTRY doc, and PANTRY adopts it reaching
the site (`themes set --scope PANTRY --theme pantry --reach site`): while a
PANTRY ticket is open all of menger wears it. PANTRY's bodies are house
components and load nothing from outside menger (the theme's fonts come
from Google Fonts, THEME-1 `fonts`). Each region says its paper, ink and
text face again where its own tokens are (`#ticket-page, .mscope-frame`),
since a face, a size or an ink inherits as a value resolved on `body`: a
theme worn by the ticket view or a frame's box alone sets its text too.
A shortcode's own boards (`#/waves/PANTRY`) open no ticket and stay the
site's (THEME-10).

A ticket says it in its body's frontmatter, as MENGER-15's `view:`:

```yaml
---
theme: pantry          # the ticket view adopts it (and its frame)
reach: ticket          # frame | ticket | site
takeover: off          # ignore every theme from above
frame-theme: ocean     # its frame alone adopts one (= theme: + reach: frame)
frame-takeover: off    # its frame alone ignores the themes above
---
```

The site and a shortcode say it over the wire:

```sh
synapse -P 4455 -p '{"name":"ocean","light":{"--accent":"#00796b","--bg":"#eef7f6"},"dark":{"--accent":"#4db6ac"}}' menger themes define
synapse -P 4455 menger themes set --scope site --theme ocean                     # everyone's site theme (admin)
synapse -P 4455 menger themes set --scope PANTRY --theme pantry --reach site     # a project, taking over while open
synapse -P 4455 menger themes set --scope MENGER --takeover off                  # keep MENGER default
synapse -P 4455 menger themes set --scope MENGER                                 # say nothing again
synapse -P 4455 menger themes theme_for --id PANTRY-4                            # who wears what, and from where
synapse -P 4455 menger themes list
```

| method | yields |
| -- | -- |
| `themes.define { name, light?, dark? }` | the `theme` as kept (its maps whole, keys written `--name`). A token not in the contract, a value with `< > { } ; \` or a line break (or over 200 bytes), a name that is not lowercase letters, digits and `-`, or `default` is one `error`, naming each |
| `themes.list` | every `theme` (`default` first), then a `setting` for the site and each shortcode that says anything |
| `themes.get { name }` | the `theme`, or one `error` naming the ones there are |
| `themes.set { scope, theme?, reach?, takeover? }` | the `setting` as kept — `scope` `site` or a shortcode holding a ticket; `theme` none or `default` adopts none; saying nothing clears it. The site takes neither a reach nor `takeover: off`; an undefined theme is refused |
| `themes.theme_for { id }` | `regions { id, regions: { site, ticket, frame }, levels }` — each region's `theme` and the `level` it came from (`none`, `site`, `shortcode` with `from`, `ticket`, `frame`), and what each level said |
| `themes.watch` | `theme` (defined) and `setting` (moved), in order; `lagged { missed }` |

Under login the reads are a viewer's; `define` and `set` an editor's —
`define` on `*` (a theme is the whole site's), `set` on the shortcode it
names — and the site's theme takes admin on `*`.

**In the app** each region is its own token scope: the site's tokens on
`<html>`'s own style, the ticket view's on `#ticket-page`, the frame's on
the box its body sits in (`.mscope-frame`) — a region wearing its outer
region's theme writes nothing and inherits. A frame reads its tokens where
it sits, so it wears its region's. One repaint path (`src/scopes.rs`'s
`repaint`; THEME-7, below) takes every cause — a theme defined, a setting
moved, a ticket opened, left or amended, the reader's choice, the
light/dark toggle — and re-themes every frame and redraws the diagrams in
place; no reload. The
root says what each region wears (`data-theme-site`, `data-theme-ticket`,
`data-theme-frame`: `pantry ticket`).

### On the fly (THEME-7)

Change the theme, or edit it, and every open menger page repaints in place:
no reload, no blank frame, nothing left in the old colours.

**One repaint path for every cause** (`src/scopes.rs`'s `repaint`): a
reader's pick — `#/settings` → Themes, or the palette's **`theme <name>`**
(`theme: pantry`, `theme` alone lists them; "the site's" puts the site's
back), kept in this browser and reaching its other open tabs through
`storage` —, an amended ` ```theme ` block or a setting moved (over
`themes.watch`, so a theme ticket is its own live editor: every open page
wearing it follows the amend), a project entered or left, and the
light/dark toggle (also followed by the other tabs), which is only its
special case. It writes each region's tokens; then, only when what is
painted moved (the mode, a region's theme, its tokens, its rules or fonts),
every frame takes its region's (`menger:theme`) and the diagrams redraw —
the app's own in the colours where each diagram sits. Everything else the
app draws paints through the tokens (THEME-2's lint), so writing them is the
whole repaint. The sign-in page wears the site theme the app last painted
(`menger.themes.site`) from its first paint and follows a change made in
another tab. Whole-document bodies (MENGER-15) follow light/dark only.

**In a frame, `menger.on('theme', cb)`** (SDK 1.13): `cb({ scheme, theme,
tokens })` whenever what the frame is painted with moves — called once the
new tokens are live on `:root`, never for a repaint that moved nothing.
`menger.theme` is what it wears now. A component that draws with colours in
JS (a canvas, a colour copied into an attribute) redraws there; one that
paints through `var(--…)` needs nothing:

```javascript
return this.menger.on('theme', () => draw());   // tokens are already live on :root
```

**Motion in a frame** (DESIGN-40, SDK 1.18): a frame wears the app's
Motion setting (DESIGN-39's `html[data-motion]`: `full`, `calm`, `none`;
absent is `full`) as its own `html[data-motion]` — set from the init block
before the body parses, and followed live (`menger:motion`, one observer on
the app's `<html>`); `menger.motion` reads it, `none` under reduced motion.
A re-rendered body (an amend, a preview's keystroke) also wears
`html[data-entered]`, and a settled frame `html[data-settled]`: the house
components play their entrance moves once, as each comes into the reader's
view, from a shown frame — a figure drawing itself, code typing, an option
settling in, a stat counting up — and a step's glow loops; `calm` keeps
only the count (and code typing, which has its own setting, below),
`none` nothing. The frame's document also declares the
house faces (`<style id="menger-fonts">`, Geist and Geist Mono from the
app's `/app/fonts/`, named absolutely: its own origin is opaque), which the
shell answers with `Access-Control-Allow-Origin: *` and outside a session.

**Typed text in a frame** (DESIGN-43, SDK 1.19): `#/settings` → Motion →
Typed text (`off`, `slow`, `normal` — the default —, `fast`; kept in the
settings record as `typing`, omitted while `normal`) is stamped on the
app's `<html>` as `data-typing` and reaches every frame as Motion does —
`typing` in the init block, `menger:typing { typing }` from the same
observer, `typing` on `menger:theme` — worn as the frame's own
`html[data-typing]` (absent is `normal`). `menger.typing` reads it, `off`
while `menger.motion` is `none`. ART-9's `m-code` types at slow 45 ms,
normal 22 ms, fast 8 ms a character (comments and a session's output at a
third of that), blocks up to 600 / 1,200 / 3,600 characters; `off` shows
the code at once. Motion `calm` no longer stops typing — this setting
alone decides; Motion `none` and reduced motion still do.

**The sweep test** (`scripts/theme-sweep.mjs`, Playwright from
`perf/node_modules`) proves it on a throwaway server. It seeds its own
tickets under `SWEEP` (three themes as ` ```theme ` docs — `pantry`, a
`garish` one setting every colour and face, `ocean` —, a framed body with
code, a diagram and house components, a mockup, and THEME-8's table as
tickets; a `pantry` already owned elsewhere — PANTRY's own — is used as
it is), opens every route at once — the board, a framed body, a mockup, a
full-screen ticket, both wave boards, `#/components` and a docs card,
`#/inputs`, `#/comments`, `#/settings`, the palette, the cheat sheet, a
menu, MENGER-14's leave dialog and a toast — and switches default → pantry
→ garish → garish dark → garish light → the site's → ocean → ocean's
`--accent` amended → default, by the palette, the toggle, `themes set` and
an amend. After each switch every element of every page, open shadow root
and frame is scanned (colour, background, borders, outline, fill, stroke,
font-family): none may hold a value the old theme painted that the new one
does not (compared as the browser reads them, so `#fff` in one theme and
`#ffffff` in the other are one value — THEME-6); no page may reload and no frame be replaced; a component's
`on('theme')` is heard once, after the tokens; the diagrams are redrawn in
the new `--diagram-*`; a planted element in an old value is found (the
control). `--rows` checks THEME-8's table region by region, `--project`
THEME-4's walk (`#/board` → a project's ticket → back: two repaints, no
reload, the shell's and the frame's tokens the project's; a hosted
shortcode inheriting and then saying its own; "project themes off" from
another tab; `--project-ticket PANTRY-4` walks into that ticket's
shortcode), `--signin-shell` the sign-in page:

```sh
cargo build && (cd frontend && trunk build --release)
cp ~/.local/share/menger/tickets.db /tmp/sweep.db      # a copy; never prod's file
PLEXUS_NO_REGISTRY=1 target/debug/menger --port 4510 --shell-port 4511 --rest-port 4512 --db-path /tmp/sweep.db &
node scripts/theme-sweep.mjs --ws 4510 --shell 4511 --seed --browsers chromium,firefox --rows --project
```

It refuses prod's and test's ports (4450–4458): it writes. Exit 0 when every
check held; `report.json` and a screenshot per switch land in `--out`.

### A theme is a ticket (THEME-3)

A theme is written in a ticket's body, as a block — usually a doc under the
project it dresses — and the registry keeps it the way it keeps components
(COMP-3):

````markdown
```theme pantry extends=default
@import url("https://fonts.googleapis.com/css2?family=Fraunces:wght@400;600&display=swap");
:root { --accent: #d4572a; --font-display: "Fraunces", serif; --radius: 6px; }
:root[data-theme="dark"] { --accent: #f08a5d; }
@font-face { font-family: "Plex"; src: url(https://example.org/plex.woff2) format("woff2"); }
m-ring::part(ring) { outline: 1px solid var(--accent); }
```
````

- **What a block may say** — looks only (THEME-1, `skins`): tokens of the
  contract in `:root` (light; a token that is not a colour holds in dark
  too) and `:root[data-theme="dark"]`; `@font-face` (`font-family`, `src`,
  `font-weight`, `font-style`, `font-display`, `font-stretch`,
  `unicode-range`); `@import url("https://…")` (a font's stylesheet); and
  `m-name::part(part)` rules (with `:hover`, `:focus`, `:focus-visible` or
  `:active`), never `behavior`, `-moz-binding`, `content`,
  `pointer-events`, `expression(` or `javascript:`. **Fonts come from any
  host** (THEME-1, `fonts`: owner, any URL) — over https only; the frame's
  CSP (`connect-src 'none'`) leaves styles and fonts free, and the bundles
  menger serves itself (mermaid, the highlighter) still name no CDN.
- **Validated on write** (`tickets.create`, `amend`, `update`): anything
  else is one in-band `error` naming the block and its line — ```` ```theme
  pantry: line 2: `--nope` is not a token of the contract ```` — and
  nothing is written, the title included; so is a second block of one
  name, `default`, or an `extends` that goes round (named:
  `pantry → deep → sea → pantry`).
- **One owner, orphans, adoption.** The ticket whose body holds the block
  owns the theme; another ticket's block naming it is refused, naming the
  owner, and so is `themes.define` over it. When the block goes the theme
  is an orphan — kept, read-only, while a ticket's frontmatter, a setting
  or a live theme's `extends` uses it, and removed by the write that leaves
  it unused. A block naming an orphan (or a theme `define` made) adopts it.
- **`extends`** chains to `default`: a theme states only what differs.
  `themes.get` / `list` answer what each says itself (with `owner`,
  `orphan`, `extends`, `rules`, `imports`); the app paints it flattened.
- **`watch`** says `theme_changed` (defined, a block written, orphaned or
  adopted) and `theme_removed`.
- **The swatch sheet.** In its owning ticket the block renders (the body
  frames) as a sheet: the theme beside `default`, light and dark — paper
  and ink, a title in the display face, a link, every status chip, every
  kind glyph, a highlighted code block and a decision, the theme's
  `::part`s scoped to its panels — and every token of the contract, both
  themes, both modes. A block the server would refuse shows the refusal.
- **Painted**: a region wearing a theme also names it (`--menger-theme`),
  so a frame is handed its theme's rules with its tokens (`menger:theme`'s
  `css`); the app's own document takes the fonts of every theme a region
  wears (`<style id="menger-theme-fonts">`).

### Parts to skin, and the house redone (THEME-5)

A theme changes how a house component looks, never what it does (THEME-1,
`skins`): through the tokens, and through the **parts** each component
exposes. A component documents its parts in its `<docs>` —

```html
<template><section class="f" part="section"><header part="head"><span class="l" part="label"></span>…</header>…</section></template>
<docs>
  <part name="section">The section the field is.</part>
  <part name="label">The field's label.</part>
  <part name="submit" scripted>Submit (the script makes it).</part>
</docs>
```

— and a theme skins them: `m-field::part(label) { font-family: var(--font-display); }`.
A documented part the template does not carry is refused by the write that
holds it, so a part in the docs is always one the component exposes. The
docs card (a definition's card, `#/components/<name>`) lists them under
**parts a theme may skin**, each as the selector a theme writes.
`MENGER_HOUSE_BODIES=a.md:b.md cargo test -p menger-tickets-api house_bodies`
files bodies on a throwaway store and fails unless every component they
define (but one that renders nothing) documents its parts — the check the
house's own bodies are held to before they are filed.

The house components (ART-9; m-cite in COMP-15; m-ring and m-dial in
DESIGN-26; m-sponge in DESIGN-20) live in the tracker, not here. THEME-5's
versions — less chrome (a field is a section under a quiet label, the panel
an option: `panel` on the tag), columns not rows (two options or cards side
by side, more than two or under 720px stacked), the display face for what
leads (a summary's lead, a question, a stat, an option's name), two new
components (`m-beat`, a claim beside its figure; `m-walk`, numbered steps
lit as they scroll in), and parts on every one — are filed by amending
their owners when the owner approves the look. A frame's base styles and
the app's bodies set headings in `--font-display`.

### The token contract (THEME-2)

Everything menger draws takes its colours, faces, sizes, radii and timings
from one named set of tokens, and nowhere else. **The contract is one file,
`tickets-api/src/tokens.toml`**: a table per group, a line per token — its
light value, its dark value (only a colour says a different one) and what
it paints:

```toml
[colour.accent]
"--accent" = ['#0066cc', '#8ab4f8', "links, ids, the selected row's bar, focus"]
```

| group | tokens |
| -- | -- |
| `colour.paper`, `.ink`, `.line`, `.chip`, `.accent`, `.signal` | `--bg --panel --card --hover --row-sel --sel --scrim --shadow`, `--ink --muted`, `--line --tree-line`, `--chip-*`, `--accent --annot --annot-hi`, `--live --err-*` |
| `colour.status` | `--st-<status>-bg` / `-fg` for every status (`.st-<status>` reads its pair) |
| `colour.kind`, `colour.code` | `--kind-*` (AGS-13's glyphs), `--code-*` (MENGER-22's highlighting) |
| `colour.diagram` | `--diagram-node --diagram-border --diagram-line --diagram-text` — `auto` (mermaid's own palette for the mode) unless a theme names a colour, which the app and every frame set into mermaid's `themeVariables` |
| `type.face`, `.scale`, `.weight` | `--mono --sans --font-text` (the app's text) `--font-display` (a ticket's title); `--text-size --text-leading --text-scale` (every other size is `calc(N * var(--text-scale))`); `--weight-normal/-medium/-strong/-bold` |
| `space` | `--space` — density, multiplying the page's gutters |
| `shape.radius`, `.border` | `--radius-xs … --radius-2xl`, `--radius-bubble --radius-device --radius-phone --radius-pill`; `--border` (every hairline's width) |
| `motion` | `--motion-fast --motion-flash --ease` |

Everything else is produced from it: `themes::TOKENS` reads it (a theme
naming anything else is refused; a frame receives exactly these), and
index.html's `:root` blocks — light, and dark twice (the media query and
the toggle's attribute) — are generated between `/* tokens:begin */` and
`/* tokens:end */`. **Edit the contract, never the blocks**:
`MENGER_BLESS=1 cargo test index_htmls_token_blocks` in `frontend/`
rewrites them, and the gate fails while they differ.

**The lint** (`frontend/src/tokens_lint.rs`, in the gate) fails on a colour
(`#hex`, `rgb()`/`hsl()`, a named colour), a face (a family named in
`font-family:` or `font:`) or a radius (a length in `border-radius`; `0`
and a percentage are shapes) written anywhere but the contract: index.html's
stylesheet, the frame SDK (`sdk/menger-sdk.js`) and `frontend/src`. Test
modules are exempt; a line that parses a colour the browser computed says
`tokens-lint:` at its end; an `@font-face` line declares a family and is
not a face written. The default theme is the contract's values (THEME-2's
look; since DESIGN-38 its faces are Geist and Geist Mono, self-hosted from
`frontend/fonts/` at `/app/fonts/` — OFL, `fonts/OFL.txt` — with the system
stacks as fallbacks; a frame's document falls back to those stacks). House
components (ART-9's, DESIGN's) paint with the same tokens — no private
palette.

### Login (IDENTITY-14)

**The login is a crate of its own, [`plexus-login`](plexus-login/README.md)**
(SCAFFOLD-3, SCAFFOLD-9): the provider specs, the login page and its
routes, the sessions, the gate at the upgrade and the `auth` backend live
there, generic over one trait, `LoginAccess`, which menger implements over
its members and sessions (`src/login_access.rs`: `MengerAccess`). `main`
wires it with `plexus_login::Wiring` from the flags below; any other Plexus
app takes the same crate (its README has the dozen lines, with ready-made
in-memory rules, `SimpleAccess`). menger's cookie and metadata names are
`plexus_login::LoginNames::for_app("menger")` — `menger_session_<port>`,
`menger_login_<port>`, `menger_dev_login` — the same as before the lift, so
a signed-in browser stays signed in. What follows is menger's flags and
behavior; the crate's README is the reference for the pieces.

menger does not know identity providers; it knows **Plexus identities**.
Each `--auth-provider` names a provider, and becomes that provider's
**adapter** (plexus-auth-core's `adapters`, IDENTITY-13). The adapters sit
in one chain at the WebSocket upgrade. Whichever claims a caller's
credential verifies it and turns it into the same Plexus types: a
principal, a name, roles and a tenant. Past the upgrade there is only the
connection's identity: `session.whoami` reports it, and `comments.add`
signs with it. No `--auth-provider` means no chain, and menger serves
exactly as before.

| `--auth-provider` | the credential | principal |
|---|---|---|
| `idp=<issuer>,<audience>` | a plexus-idp token (its password grant, or its nostr grant) | `idp:<uuid>` |
| `auth0=<domain>,<audience>[,<roles-claim>]` | an Auth0 access token; roles from the namespaced claim an Auth0 Action adds | `oidc:auth0/<sub>` |
| `oidc:<name>=<issuer>,<audience>[,<roles-claim>]` | any OIDC provider's RS256 token (Logto, Keycloak, …) | `oidc:<name>/<sub>` |
| `workos=<client-id>[,<api-base>]` | a WorkOS access token (AuthKit, social, SSO) for that client: issuer `…/user_management/<client-id>`, audience in `client_id` (IDENTITY-30) | `oidc:workos/<user id>` |
| `nostr=<ws-url>[,<ws-url>…]` | a NIP-98 event (kind 27235) naming that URL, method GET, under a minute old, used once | `nostr:<hex>` |
| `key` or `key=<ws-url>[,<ws-url>…]` | an agent's Ed25519 key proof naming this socket (IDENTITY-42; "Agents" below) | `key:<hex>` |

Several providers can be configured together. An OIDC adapter claims a
token by its `iss`, and the nostr adapter claims an event, so each
credential reaches its own adapter. The credential rides the upgrade in the `access_token`
cookie (a token), the `nostr` cookie (an event: a browser can only send
cookies), or `Authorization: Bearer`. AUTH-7: no credential at all is the
**anonymous view** — the connection is admitted and can reach only the auth
door (`auth.providers`, `auth.login`, `auth.signup`; "The AUTH protocol"
below); a credential presented as a bearer or the `access_token` cookie and
refused is a 401 at the upgrade, with RFC 6750's challenge. (A bad `nostr`
cookie, or a stale browser session cookie, presents nothing: the anonymous
view.) `--require-login` puts the shell's pages behind the login page; it
needs `--no-rest`, because the REST bridge cannot carry an identity yet. `MENGER_AUTH_PROVIDERS` takes the specs separated by `;`, and
`MENGER_REQUIRE_LOGIN=1` sets the flag. A bad spec, or an issuer that is
not `https://` off loopback, stops startup.

| method | yields |
|---|---|
| `session.whoami {}` | `identity { principal, name, roles, tenant, provider }`, or `anonymous` (login off; under login, a signed-out session — AUTH-7: an anonymous connection asks `auth.*`) |
| `session.providers {}` | one `provider { name, kind, issuer, audience }` per `--auth-provider` (`kind` `oidc` or `nostr`), none when login is off: what a sign-in needs, nothing secret |

`comments.add` by a signed-in connection is authored by its identity's
name, whatever `author` says; anonymous, it keeps `author` as before. The
schemas of `tickets` and `comments` are unchanged.

```sh
# a dev server admitting plexus-idp, an Auth0 tenant and nostr keys
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --rest-port 4462 --db-path /tmp/menger-dev.db \
  --auth-provider idp=http://127.0.0.1:4481,plexus:menger \
  --auth-provider auth0=https://acme.auth0.com/,https://menger.example,https://menger.example/roles \
  --auth-provider nostr=ws://127.0.0.1:4460/
synapse -P 4460 -t "$TOKEN" menger session whoami        # -t sends Cookie: access_token=<jwt>
synapse -P 4460 --cookie nostr="$EVENT" menger session whoami
```

`tests/login.rs` runs each provider over the real wire against fixture
issuers.

**In the app** (IDENTITY-15): `#/settings` opens on a **Sign in** group
with one button per provider that `session.providers` lists. For an OIDC
provider, the app reads the provider's discovery and sends the username and
password to its token endpoint (the password grant, which plexus-idp
offers and Auth0 offers when enabled). It keeps the token in the
`access_token` cookie for the token's lifetime. For nostr, a NIP-07
extension signs a fresh NIP-98 event for the WebSocket URL before every
connect, and the event goes in the `nostr` cookie for a minute. menger
never sees a password. Signing in or out reloads the app, so a new session
connects with the credential. The header shows `whoami`'s name, or
`sign in`. The cookies are this page's host's, so sign-in reaches only a
backend on the same host, not one set from `#/settings` elsewhere.

### The login page (IDENTITY-19)

With providers configured, the shell serves **`/login`**, a traditional
sign-in page: a username and password (one provider's password grant),
**Continue with …** buttons (`--login-button Google=auth0:google-oauth2`,
`Apple=auth0:apple`, `SSO=auth0:samlp`) that run the provider's
authorization code flow with PKCE and a connection hint, and **Continue with
nostr** when a NIP-07 extension is present. Each way in ends the same way:
the provider's token is verified by its adapter **in the shell**, a session
starts, and the browser gets an `HttpOnly; SameSite=Lax` session cookie.
No script on any page can read a credential. The cookie is named for the
shell (IDENTITY-38): `menger_session_<port>` over http — cookies ignore
ports, so prod (`_4456`) and test (`_4451`) on 127.0.0.1 keep apart — and
`__Host-menger_session` over https. Only it is read from a browser's
cookies; a provider's token reaches the socket as `Authorization: Bearer`.
Every shell response refuses framing (`frame-ancestors 'none'`), and the
page shows only its own words (no `?error=` text).

**Security headers** (ACCESS-28). Every answer the shell gives carries
`X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin` (nothing
goes to another origin; `no-referrer` would make a browser send `Origin:
null` on the sign-in and sign-out forms' POSTs, which their Origin check
refuses) and `X-Frame-Options: DENY` with at least `frame-ancestors 'none'`.
The app's pages — `/`, `/app/`, `/vanilla`, "not built" — go out under
`src/shell.rs`'s `PAGE_CSP`, filled per page by plexus-shell:

```text
default-src 'self'; script-src 'self' 'wasm-unsafe-eval' 'sha256-<each inline block>';
style-src 'self' 'unsafe-inline' https:; font-src 'self' data: https:; img-src 'self' data: blob:;
connect-src 'self' ws://<host>:<ws port> wss://<host>:<ws port> ws://127.0.0.1:* ws://localhost:*;
frame-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'
```

No inline script runs but the page's own (hashed when the build is read),
nothing executes from another origin, a remote image in a body shown in the
app is refused (it would tell its host who reads it), styles and fonts may
come over https (a theme's `@import`), and the app connects to itself, its
hub and — for `#/settings`' other backend — loopback. The sign-in pages keep
plexus-login's own policy (their blocks hashed, `default-src 'none'`). The
frame document (`/app/frame`, public: it holds no data) goes out under
`connect-src 'none'; form-action 'none'; frame-ancestors 'self'` and
`X-Frame-Options: SAMEORIGIN` — IDENTITY-28's policy, and framed by the app
alone; a body's scripts, styles, images and fonts load as before. The
landing (menger.sh) is served by the edge under its own (`landing/menger.sh.caddy`). The page is
self-contained (`plexus-login/web/login.html`), so an IdP can host it later.

`--require-login` puts the whole app behind it. `/`, `/app/…` and `/vanilla`
redirect to `/login?next=…`. AUTH-7: with any provider, the socket admits an
upgrade without a session or a provider token into the anonymous view (the
auth door, nothing else), the protocol's gate decides **every call**, and
**every method re-checks its caller's sign-in on every call**
(`tickets-api/src/access.rs`). Signing out (`POST /logout`) or a session
expiring stops an open socket from answering, and ends its `watch`. The socket
admits browser pages only from the shell's own origins (IDENTITY-29, with or
without login; synapse sends no Origin). POSTs from another origin and requests
naming another `Host` are refused (the latter on every port, login or not —
ACCESS-25).

| flag | |
|---|---|
| `--login-button <Label>=<provider>[:<connection>]` | a button; the provider needs `client=<id>` in its `--auth-provider` |
| `--login-email <provider>[:<connection>]` | an email field, handed to that IdP as `login_hint` (`workos:authkit`); beside it the password form shows only at `/login?password` |
| `--login-password <provider>` | where the password form goes (default: the first OIDC provider that takes passwords, not WorkOS); `none`: no form |
| `--login-forgot <url>` | a "Forgot password?" link |
| `--public-url <url>` | where people reach the shell; the redirect URI's base (default `http://127.0.0.1:<shell-port>`) |
| `--allowed-origin <origin>` | another origin whose pages may open the socket |
| `--allowed-host <host>` | another `Host` every listener answers to (ACCESS-25, README "Run" → Hosts) |
| `--session-hours <n>` | how long a sign-in lasts (default 12) |

`GET /login/me` answers who the session is, or 401. Sessions live in memory,
so a restart signs everyone out (IDENTITY-18 G2). `tests/login_page.rs` runs
each way in against fixture issuers.

**WorkOS** (IDENTITY-30) is the IdP behind menger's own page: the page
comes first, offers a curated list, and hands each choice to WorkOS. With
`--login-email workos:authkit` the page has an email field: the address goes
to AuthKit as `login_hint`, so an SSO domain goes straight to its company's
IdP, and anyone else finishes on AuthKit's page (password, passkey, a code by
email) with the address filled in. `--login-button Google=workos:GoogleOAuth`
(also `AppleOAuth`, `MicrosoftOAuth`, `GitHubOAuth`, a `conn_…`, an `org_…`)
goes through WorkOS straight to that provider, with no AuthKit page between.
nostr sits on the same list. menger is a public client: the code flow runs
with PKCE and no secret, and menger holds no API key. WorkOS's access token
names nobody, so the person's name and email come from the authenticate
response's `user`, which must be the token's own subject. Signing out ends
the WorkOS session too, and comes back to `/login`. The environment lists
`<public-url>/login/callback` as a redirect URI and `<public-url>/login` as a
logout URI (`workos authkit redirect-uris set …`, `workos authkit logout-uris
set …`).

**Our own sign-in page** (AUTH-22): `--identity <spec>` puts menger's own
page in front of an **identity backend** chosen by configuration — WorkOS's
User Management API with no AuthKit screen (`workos:<client_…>,social=google+github+apple`),
plexus-idp's password grant (`plexus-idp:<issuer>,<audience>`, local only),
or the in-process `memory` backend (tests, development). The page offers
exactly what the backend can do: an email field (a six-digit code by email,
or straight to the company's IdP for an SSO domain), a password form, a
button per social provider, a TOTP code when the account has a factor, and
`/login/account` to add or remove an authenticator app. Every call to the
backend is made by the shell; WorkOS's API key comes from
`MENGER_WORKOS_API_KEY` or `key=@<file>` and never rides a flag, a page or a
log line. A sign-in ends in menger's own session, as every way in does; the
backend's tokens stay server side beside it, are refreshed, and are revoked
at sign-out (`POST /logout`, which lands on `/login` or a `next` the return
rules allow). The hosted-AuthKit buttons above stay available beside it,
and stay the default until `--identity` is set.

| flag | |
|---|---|
| `--identity <spec>` | the backend (`MENGER_IDENTITY`); its provider joins the gate's, so its tokens open the socket as a bearer; `memory` and `plexus-idp` only with `--env test` |
| `--trusted-proxy <cidr>\|none` | the edge whose `X-Forwarded-For` is believed for the client's IP (repeatable), or `none` when people connect directly; with `--identity` off loopback one is required (AUTH-24) — behind the infra's Caddy, the edge's address on the `edge` Docker network |
| `--return-origin <origin>` | another origin a sign-in may return to (repeatable) |

The WorkOS environment lists `<public-url>/login/callback` as a redirect URI.
plexus-login's README ("Our own sign-in page") has the safety design, the
capability model and how a backend is swapped.

**Automation signs in with plexus-idp** at **`/login?password`**: beside an
email field, the password form (`--auth-provider idp=…`, the default password
provider) shows only there, so the smoke and the end-to-end scripts never
depend on WorkOS.

```sh
menger --auth-provider 'workos=client_01…' --auth-provider idp=http://127.0.0.1:4481,plexus:menger \
  --login-email workos:authkit \
  --login-button Google=workos:GoogleOAuth --login-button Apple=workos:AppleOAuth
```

### Members and roles (IDENTITY-21)

With login on, **menger's own members** decide what each person sees and
changes. The IdP only proves who someone is. A grant gives a principal a
role on a **scope**: a shortcode, which covers the shortcodes hosted under
it (the filter language's `under`, so a grant on PLEXUS reaches IDENTITY),
or `*`. ORG-4: or an organization (`acme:`), and a shortcode is kept in its
organization (`hypermemetic:PLEXUS`) — "Organizations" above.

| role | may |
|---|---|
| `viewer` | read under the scope, and comment |
| `editor` | also create and change tickets, and answer inputs |
| `admin` | also grant and revoke roles on the scope |
| `deployer` | (DEPLOY-14) read under the scope and record its deployments — a CI's key; beside the line above, not on it |

Every read is answered under the caller's filter **and** their viewer
scopes. `list --filter 'status Ready'` from someone with IDENTITY and OUGHT
is `status Ready and under in (IDENTITY, OUGHT)`. A ticket out of reach is
`not_found`. `groups`, `extents`, `waves`, `health` and `watch` hear only
what is in reach. A write needs the role on every ticket it touches — AUTH-7:
refused by the gate, typed (`forbidden { grant: tickets.amend, scope: OUGHT,
roles, held_on }`). A comment is signed with the
caller's principal (`principal` beside `author`), whatever `author` says.
Someone signed in with no grant reaches nothing and waits in
`members.requests` for an admin. With login off, nothing here applies.

| method | yields |
|---|---|
| `members.mine {}` | `me { principal, name, admin_everywhere }`, then `grant { grant }` each |
| `members.list { scope? }` | the grants on the scopes the caller administers |
| `members.grant { principal, scope, role }` | the grant as kept; needs admin on the scope |
| `members.revoke { principal, scope }` | `revoked { principal, scope, existed }` |
| `members.requests {}` | `waiting { principal, name, provider, since }` per person without a grant |

The first admin is the operator's: `--admin <principal>` (repeatable,
`MENGER_ADMINS`) holds admin on `*`. Grants live in the `members` table,
stored form 8: a file in form 6 or 7 gains it in place, and an older build
reads a form-8 file as its own. (AUTH-7: form 9 adds the `roles` table, the
same way.) `tests/members.rs` runs the roles over the
real socket.

```sh
synapse -P 4450 -t "$ADA" menger members grant --principal idp:c952acd3-… --scope IDENTITY --role editor
synapse -P 4450 -t "$ADA" menger members requests
```

### Agents (IDENTITY-40, IDENTITY-42)

An agent logs in with an **Ed25519 key** it holds, then talks as itself
for the session without handling a token. `--auth-provider key` (in
`MENGER_AUTH_PROVIDERS`, `key`) admits key proofs at the upgrade; it is
not offered on the login page or by `session.providers`.

1. **The proof opens a connection.** The bearer value (or
   `access_token` cookie) `plexus-key.<base64url JSON>` —
   `{"v":1,"key":"<64 hex>","aud":"<ws url dialed>","agent":"<id>","nonce":"<32 bytes>","ts":<unix s>,"sig":"<Ed25519>"}`,
   signed over `plexus-key-login/v1\n<aud>\n<agent>\n<nonce>\n<ts>`
   (plexus-auth-core's `KeyProofAdapter`, IDENTITY-41). It is accepted
   within −60 s / +30 s, once per `(key, nonce)`, when `aud` is this
   socket: `ws://127.0.0.1:<port>`, `ws://localhost:<port>`, the ws form
   of `--public-url`, and any URL given as `key=<ws-url>,…`. The
   connection is `key:<hex>`, named by its agent id, for five minutes.
2. **`session.login` mints a session.** On any signed-in connection,
   `session.login { ttl_secs?, scope? }` → `login { token, principal,
   agent, expires_at, scope }`. The token (`mgs_…`, kept hashed like a
   browser's session) opens later connections as the same principal,
   agent and name — `Authorization: Bearer mgs_…` or `Cookie:
   access_token=mgs_…` — for `ttl_secs` (default 12 h, at most 24 h).
3. **`scope` is a ceiling.** `role@SCOPE` each — a shortcode (covering
   those hosted under it), one ticket (it and what is under it), or `*`.
   Every check, reads and writes and members' alike, passes only if the
   key's grants **and** the scope allow it: `editor@MENGER-12` cannot
   write elsewhere though the key is admin on `*`, and `admin@*` on a key
   granted editor on A is editor on A. A session minted from a scoped
   session keeps that scope as well (`scope` in the answer says the one
   asked, or inherited; empty is no ceiling beyond the grants).
4. **`session.logout {}`** → `logged_out` ends the connection's session:
   its token opens nothing (a 401 at the upgrade: it is presented and
   refused), and connections on it answer "you have signed out".
   AUTH-7: `auth.login {session {ttl_secs, ceiling}}` and `auth.logout` are
   the same, in the protocol's words (`ceiling` takes `grant@SCOPE` too).

A key is granted like anyone: `members grant --principal key:<hex>`.
`tests/agent_login.rs` runs it over the real socket.

```sh
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --no-rest --db-path /tmp/menger-dev.db \
  --auth-provider key --require-login --admin key:<hex>
synapse -P 4460 _login menger --key agent-7 --as worker-3 --scope editor@MENGER-12   # synapse's side: IDENTITY-43
```

### The AUTH protocol (AUTH-7)

With login on, menger speaks Plexus's own auth (AUTH-1): **every method is
its own grant** (`tickets.set_status`), a **role is an explicit list of
grants**, and plexus-core's **gate** decides every call before it runs — the
grant, held through a role on the scope the call's **target** lives in,
within the session's ceiling. Default deny. menger supplies the gate its
roles, memberships and scopes (`tickets-api/src/access.rs`, `Book`) and an
`AuthBackend` (`src/auth_backend.rs`). Login off: no gate, no `auth`
activation — every method answers everyone, as before; schemas only gain
`grant` and `target` on each method.

| role | its grants (declared with `roles(...)` on each activation) |
|---|---|
| `viewer` | the reads: `tickets.{health, get, list, ready_set, find, search, member, groups, components, inputs, extents, counts, facets, tree, waves, blockers, watch, history}`, `comments.{add, list, watch}`, `orgs.list` (ORG-4) |
| `editor` | viewer's, and the writes: `tickets.{create, set_status, set_kind, set_parent, amend, link_commits, unlink_commits, label, update, update_many, reshortcode, answer, clear_answer, close_input, reopen_input}`, `comments.{resolve, reopen}` |
| `admin` | every grant (built in): `tickets.{delete, undelete}` (ACT-5), `members.{list, grant, revoke, requests}`, `orgs.{create, rename, delete}` (ORG-4), `auth.{roles, role_set, grant, revoke, waiting}` |
| any signed-in caller | `session.*`, `members.mine`, `auth.{whoami, logout, requires}` |

A **write's target** is the ticket it acts on (`id`, or `ticket` for an
input), and resolves to the scopes the filter language's `under` covers:
its shortcode, itself, its ancestors, and the shortcodes hosting its
shortcode — a membership on PLEXUS reaches IDENTITY-12, a ceiling on
MENGER-12 reaches what is under it. `members.grant` / `revoke` target their
`scope`. A **read** has no target: the gate lets it by where its grant is
held anywhere, and it answers only from there (the caller's filter AND
`under` those scopes, and each ceiling layer's) — a ticket out of reach is
still `not_found`. Moving a shortcode's root, creating a ticket and
commenting check the rest themselves, as before.

- **The anonymous view**: an anonymous socket's `_info` says `view:
  anonymous`, names `auth.providers`, `auth.login`, `auth.signup` and the
  RFC 9728 metadata (`GET /.well-known/oauth-protected-resource` on the
  socket's port); every schema shows only those; every other call is
  refused `unauthenticated`.
- **A caller's view**: `_info` and every `schema` list only the methods
  the caller holds, each with `grant`, `target` and `held_on`; `view_hash`
  differs by holdings.
- **A refusal** is the stream's error item: `{type: error, error_kind:
  forbidden, grant, scope, roles, held_on, message: "needs tickets.amend on
  OUGHT"}`.
- **`auth.*`**: `providers`; `login {browser {redirect}}` → `login_url`
  (`<public-url>/login?cli=1&state=…`; the person signs in on the page as
  anyone does, and the shell's `/auth/cli` sends the browser to the
  client's loopback `redirect?code=…&state=…` — only `http://127.0.0.1:<port>`
  or `localhost`; a sign-in older than two minutes asks first), `login {code
  {code, state}}` → `session` (an `mgs_` token; the code works once, for a
  minute, for its state), `login {session {ttl_secs, ceiling}}` (agents);
  `signup` (someone signed in holding nothing waits for an admin); `whoami`,
  `logout`, `requires {method}`; `roles`, `role_set {role, grants}` (admin on
  `*`; kept in the store's `roles` table, stored form 9, and it wins over the
  declared list), `grant`, `revoke`, `waiting`.
- `session.*` and `members.*` stay, unchanged on the wire, over the same
  tables (the browser app speaks them).

```sh
synapse -P 4460 menger                                   # anonymous: the door
synapse -P 4460 -t "$MGS" menger auth whoami
synapse -P 4460 -t "$MGS" menger auth requires --method tickets.set_status
synapse -P 4460 -t "$ADMIN" menger auth role_set --role viewer --grants tickets.get,tickets.list
```

`tests/auth_protocol.rs` runs it over the built binary (and pins the method →
role table against IDENTITY-21's); `tests/login_page.rs` runs the loopback
sign-in end to end.

### Limits and caps (ACCESS-29)

Nothing a caller does — anonymous or signed in — grows menger's memory
without bound or starves the others (ACCESS-4 M1, M2, L1, L2, L5). Each
limit is a flag (and its `MENGER_*` variable) or a named constant:

| what | limit (default) | past it |
|---|---|---|
| WebSocket connections, in all | `--max-connections` (1,024) | upgrade refused 429 + `Retry-After` (AUTH-12, plexus-transport) |
| … from one IP (the peer's) | `--max-connections-per-ip` (256) | 429 |
| … held by one signed-in principal | `--max-connections-per-principal` (64) | 429 |
| … anonymous, in all / per IP | `--max-anonymous-connections` (128) / `--max-anonymous-connections-per-ip` (32) | 429 ("sign in") |
| calls open on one connection (each call is a subscription while it runs; a `watch`, for its life) | `--max-calls-per-connection` (256; `menger::calls`) | one error item `{error_kind: too_many_calls, max, recoverable: true}`; a watch let go of gives its place back at once |
| a connection with no call open | `--idle-timeout-secs` (900; 0: never) | closed by the server |
| a browser's sign-in, unused (no socket open on it, no page loaded) | `--session-idle-minutes` (120; 0: never) | the session ends; its sockets close (AUTH-11). A minted `mgs_` token keeps only its expiry |
| sign-ins live / login transactions waiting / loopback sign-ins and codes | `MAX_LIVE` 100,000 / `MAX_PENDING` 10,000 / 10,000 each (`plexus-login/src/sessions.rs`) | the one nearest its end makes room — found by an index, never a scan |
| the waiting list | `WAITING_MAX` 1,000 (`tickets-api/src/access.rs`) | the one waiting longest is forgotten |
| `/login/start`, `POST /login`, `POST /login/nostr`, from one client | a sign-in flow each — 60 per 10 min per /32 (/64), 600 per /24 (/48), 3,000 per /16 (/32) (AUTH-22's `flows_per_client`) | 429 + `Retry-After`, the page's own words ("Too many attempts…") |
| passwords tried at `POST /login` | reserved before the IdP sees them: 5 per account per neighbourhood, 30 per client, then a backoff (AUTH-22's `try_per_user_network`, `try_per_client`) | 429; a right answer forgives the account, an IdP that is down gives the guess back |
| a ticket's title | `TITLE_MAX` 1,024 bytes (`tickets-api/src/limits.rs`) | in-band `error`, nothing written |
| a ticket's body | `--max-body-bytes` (8 MiB, MENGER-15) | typed `body_too_large` |
| a comment's or a note's body | 64 KiB (`comments::BODY_MAX`, `notes::BODY_MAX`) | in-band `error` |
| a filter's text | `FILTER_MAX` 4,096 bytes | in-band `error` |
| `tickets.list`'s page | 1,000 unless `limit`, at most 10,000 (`LIST_PAGE_DEFAULT`, `LIST_PAGE_MAX`) | the `page` event's `next` carries on |
| `comments.list`'s page | 500 unless `limit`, at most 1,000 | `page.next` |

The login page shows only its own words — never a link's `?error=` or a
provider's `error_description` (IDENTITY-38; ACCESS-4 L1). The dev sign-in
(`/login/dev`, test only) is not throttled. Behind a proxy, the throttles
read the client from `X-Forwarded-For` only from a `--trusted-proxy`; the
WebSocket's per-IP quotas see the proxy (the transport reads no proxy
header yet — ACCESS-29's divergence note). `perf/flood.mjs` floods a
throwaway menger with upgrades and logins from a spread of addresses and
checks its memory settles.

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
stand around it. A blank line inside an element does not end its HTML block
(COMP-11, "HTML with blank lines" above), so a `<pre class="mermaid">` with
blank lines, nested in a `<section>`, is a diagram. Not a diagram, drawn by
neither: a `<pre class="mermaid">` in code, a comment or a script, inline in
a paragraph, or one with no `</pre>`.

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

## As a library (DIST-3)

`menger::local` starts the whole server from code — what the binary does
with no `--auth-provider`, `--no-rest` and no `--stdio`: the store, every
activation, the Plexus WebSocket and the web shell on loopback; no login, no
REST bridge, no registry announcement. It is how a host (DIST-1's desktop
app) runs menger without copying `main.rs`:

```rust
use menger::local::{LocalConfig, LocalServer, TAURI_ORIGINS};

let mut cfg = LocalConfig::new(data_dir.join("tickets.db")); // ports 0: free ones
cfg.extra_origins = TAURI_ORIGINS.iter().map(|o| o.to_string()).collect();
let server = LocalServer::start(cfg).await?;  // both listening when it returns
open_window(&server.shell_url());             // http://127.0.0.1:<port>/
// … server.ws_url() is the socket synapse or a client can use
server.shutdown().await;                      // both stopped, ports released
```

`LocalConfig { db_path, ws_port, shell_port, env, app, extra_origins }`:
a port of 0 is a free one (bound before anything serves, so the reported
port is held); the database's directory is made if missing; `env` titles
the tab as `--env` does; `app` is an `AppSource` — `Dir(path)` or
`Embedded(table)` (plexus-shell's, re-exported as `menger::shell::AppSource`),
defaulting to the embedded build when the binary carries one
(`menger::shell::preferred_app()`);
the socket admits the shell's own origins (127.0.0.1 and localhost at the
shell's port) and `extra_origins` — `TAURI_ORIGINS` for a Tauri webview.
Dropping a `LocalServer` stops it without waiting. `tests/local.rs` is the
executable reference.

**The app inside the binary.** The `embedded-app` feature (off by default)
has `build.rs` compile `frontend/dist` into the binary, and the shell serves
that table — stamped and typed exactly as a dist on disk — whenever no
`--app-dist` names a directory. A checkout leaves it off and keeps serving a
fresh `trunk build` from disk; the published crate carries `frontend/dist`,
so a dependent that turns the feature on gets the Leptos app with nothing
on disk:

```toml
menger = { version = "0.6", features = ["embedded-app"] }
```

## plexus-shell (SCAFFOLD-5)

The serving half of all this is a crate of its own, `plexus-shell/` (a
workspace member, published as `plexus-shell`), so any Plexus RPC app serves
its frontend the way menger does. menger consumes it, and keeps no second
copy:

- **The shell.** `AppSource::{Dir(path), Embedded(table)}` is the build. The
  table follows ACCESS-33: a request path is a key and never a path, and
  there are size caps, a MIME table, and a reread on a new `index.html` for
  `Dir`. `ShellConfig { app, title, mount, stamps, pages, root_fallback,
  not_built, routes, csp }` says what to serve. `router(cfg, ws_port)` builds
  the router and `serve_on(listener, router, shutdown)` serves it, hardened
  (IDENTITY-38's headers). Every page it serves goes out under `csp`
  (ACCESS-28; default `DEFAULT_CSP`), a template whose `{{SCRIPTS}}` is the
  page's inline-script hashes and `{{HUB}}` the hub's WebSocket origins at
  the name the page was asked for. The entrypoint and `pages` are stamped with
  `{{WS_PORT}}`, `{{TITLE}}` and any `(placeholder, value)` in `stamps`.
- **The embedding.** The app crate's `build.rs` calls
  `plexus_shell::build::embed_dir_with_feature("frontend/dist",
  "embedded-app")`, a build-dependency with `default-features = false,
  features = ["build"]`. The table goes into the app's `OUT_DIR`. The app
  takes it with `static APP: EmbeddedTable = plexus_shell::embedded_app!();`
  and hands it to `AppSource::Embedded(APP)`. It has to be the app's own
  build script, because `include_bytes!` resolves in the crate that
  compiles it.
- **`LocalServer`.** `plexus_shell::LocalServer::start(hub, LocalConfig {
  ws_port, shell_port, extra_origins, shell })` serves any `DynamicHub`'s
  WebSocket and the shell on loopback. Ports 0 are bound for real before
  anything serves. It returns `ws_port`, `shell_port`, `shell_url`,
  `ws_url` and `shutdown`. `TAURI_ORIGINS` lives here.

menger's part stays in `src/shell.rs`: `/vanilla` and `/` falling back to
it, the favicon, the tab title rule (MFE-23), the "not built" page's words,
the embedded table, and the login routes, session check and host check
layered around the crate's router. `menger::local` builds menger's hub and
calls the crate. Its API is DIST-3's, unchanged, and menger-desktop builds
on it as is. `plexus-shell/toy/` is the worked example: a hub with one
activation and an embedded two-file app, started through `LocalServer`. Its
test is in the workspace run.

## Publishing (DIST-3)

`plexus-shell`, `menger-tickets-api`, `plexus-login` (SCAFFOLD-9) and
`menger` go to crates.io, in that order. The plexus 0.6 line they depend on goes first: plexus-core,
plexus-macros, plexus-client, plexus-auth-core, plexus-transport. Every
sibling `path` dependency also names its `version`, which is what the
published crates take. Dev dependencies stay path-only and are dropped from
the package.

```sh
scripts/publish.sh stage     # trunk build --release, stage the package, list it; publishes nothing
scripts/publish.sh dry-run   # + cargo publish --dry-run of the crates not yet on crates.io, verified together
scripts/publish.sh publish   # + cargo publish: plexus-shell, menger-tickets-api, plexus-login, then menger (clean tree only)
```

A crate whose version crates.io already holds is skipped. The script reads
the sparse index to tell; a version already published is skipped, never re-uploaded. The
rest go to one multi-package `cargo publish`, which orders them and, in a
dry run, verifies `menger` against the packaged `plexus-shell` before
either is on crates.io.

The script builds the app first and refuses to go on without
`frontend/dist/index.html`. It publishes from a stage beside the repository,
because cargo will not package `frontend/dist` from the checkout: the
directory sits under another package's `Cargo.toml` (`frontend/` is the
app's crate) and is gitignored, and cargo skips both whatever `include`
says. The stage is the files git keeps plus dist, without
`frontend/Cargo.toml` and outside any repository, so the package is what
`include` names — `src/`, `web/`, `build.rs`, `README.md`, `LICENSE` and
`frontend/dist/` (66 files, ~2.3 MB compressed); never `tests/`, `deploy/`,
`perf/`, `diagrams/` or `spike/`. `menger` is published with
`--features embedded-app`, so cargo's verification builds the app in. The
wave3 example of `menger-tickets-api` lives in `../spike` and is left out of
its package (it still runs from a checkout, `spike/WAVE-2/run.sh`).

## Layout

```
src/
  main.rs              CLI args, transports, spawns the shell
  builder.rs           build_rpc() → Arc<DynamicHub> ("menger")
  shell.rs             menger's shell over plexus-shell: / (the Leptos app,
                       degrading to the vanilla page when unbuilt), /vanilla
                       (the embedded day-one page), the favicon, the tab
                       title, the embedded table, and the login routes and
                       checks around the crate's router (SCAFFOLD-5)
  local.rs             menger::local — LocalServer::start(LocalConfig), the
                       whole server without login or REST, as a library call
                       (DIST-3); plexus-shell's LocalServer over menger's hub
build.rs               `embedded-app`: frontend/dist compiled into the binary
                       by plexus-shell's build half (DIST-3); without it, an
                       empty table
plexus-shell/          the shell and LocalServer as a crate (SCAFFOLD-5):
                       AppSource, the app's table (ACCESS-33), stamping,
                       router/serve_on, the build-script embedding,
                       LocalServer, TAURI_ORIGINS; published as plexus-shell
  toy/                 its worked example: a one-activation hub and an
                       embedded two-file app, tested (never published)
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
    history.rs         the transition log (MENGER-13): its table, the kept
                       indexes a history filter is a range of, the windows
                       in a zone (Clock), `--verify-derived`'s look at it
    times.rs           births and status-since, sorted (FILTER-27): what
                       `created` and `for` are ranges of
    cascade.rs         cancellation cascades and restores, as one batch write;
                       `history`; `set_status`'s path
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
scripts/publish.sh     the app built, the package staged, plexus-shell, menger-tickets-api and
                       menger published to crates.io (DIST-3)
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
                       src/when.rs + src/when_view.rs the When category —
                       time clauses as pills, the window editor (FILTER-27),
                       src/calendar.rs the calendar and time selector,
                       src/notes.rs its annotation and the body editor (WAVE-11),
                       src/links.rs links people wrote — classified, the
                       leave dialog, "don't ask again" (MENGER-14;
                       vendor/psl/ the Public Suffix List),
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
                       menger.navigate's routes (COMP-9), src/cite.rs
                       <m-cite>'s windows (COMP-14), src/inputs.rs
                       the app's copy of the inputs and src/inputs_page.rs
                       #/inputs (INPUT-6), src/history.rs a ticket's
                       History section (MENGER-13)
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
