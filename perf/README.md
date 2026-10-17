# perf/ — the magnitude ladder (PERF-3)

A self-contained Node + Playwright package, deliberately **not** part of
`cargo build` / `cargo test`. One command seeds 10², 10³, 10⁴, 10⁵ and
10⁶ synthetic tickets (PERF-2's `--seed-synthetic`) into temp databases,
measures every user-visible path against a release `menger` on ephemeral
ports, and writes `plans/PERF/artifacts/MAGNITUDE-<date>.md` with the raw
JSON and screenshots beside it.

```sh
cd perf
npm install                       # playwright, pinned in package-lock.json
npx playwright install chromium   # the browser (into ~/.cache/ms-playwright)

npm run smoke                     # 10^2, one run, to perf/out/smoke — must pass
node magnitude.mjs                # the full ladder (report to plans/PERF/artifacts)
node magnitude.mjs --magnitudes 4 # one magnitude (merges into the same report)
node magnitude.mjs --help
```

Options: `--magnitudes 2,3,4` · `--runs N` (override the schedule
5/5/3/2/1) · `--date YYYY-MM-DD` (report name) · `--out DIR` ·
`--tmp DIR` (temp databases; default `target/perf-tmp`, on disk — not
tmpfs) · `--bin PATH` (default `$CARGO_TARGET_DIR/release/menger`, else
`../target/release/menger`) · `--timeout-scale X` · `--no-build` ·
`--keep-db` · `--headed`.

**Signed in** (ACCESS-52): menger has no open mode and no REST bridge
(ACCESS-24), so every server the ladder starts runs `--auth-provider key
--no-rest` with a key made in `--tmp` as its `--admin`; once its socket
answers, a session is minted with that key (`scripts/throwaway-login.mjs`),
every socket carries it as `Authorization: Bearer …` and every browser
context as the shell's session cookie. The probes (`*-probe.mjs`,
`*-identical.mjs`), which talk to a server you started, sign in the same
way from `$MENGER_SESSION_FILE`:

```sh
op=$(node scripts/throwaway-login.mjs key /tmp/probe.key)
target/release/menger --port 4650 --shell-port 4651 --no-rest --auth-provider key --admin "$op" \
  --db-path /tmp/probe.db --seed-synthetic 1000 &
node scripts/throwaway-login.mjs session --url ws://127.0.0.1:4650 --key /tmp/probe.key --out /tmp/probe.session
MENGER_SESSION_FILE=/tmp/probe.session node perf/ready-probe.mjs pages 4650 3 -
```

A session belongs to the server that minted it, so the two-server
comparisons (`*-identical.mjs BEFORE AFTER`) take one file per server:
`MENGER_SESSION_FILES=BEFORE_FILE,AFTER_FILE` (`filter-sheet.py` passes it).
The sheets (`filter-sheet.py`, `tree-sheet.py`, `large-parent.py`) start
their servers the same way, through `perf/menger_ws.py`.

## What it measures, per magnitude

Server: seed time (as logged by the server), startup load-into-cache
time (a restart on the seeded file, after a WAL checkpoint — what a
clean close does), RSS after load / after `list` / after the browser /
peak; latencies over the WebSocket for `health`, `get`, `work.ready`,
`work.blockers` (the REST rows until ACCESS-24 removed the bridge; `list`'s
payload is the streamed WS list's below); create → `watch` push latency,
both over the socket (SSE until ACCESS-24).

Two WebSocket `list` rows, the same set two ways (MNG-9):

- **WS list, streamed (control)** — `list {}`: every ticket, bodies and
  all, one notification each. The store's cost with no envelope to
  collect.
- **WS list, paged (200)** — the walk a client that *renders* does:
  `list { summary: true, limit: 200 }`, then the same call again with
  `after` = the previous page's `page.next`, until `next` is null. The
  row reports the whole walk (bytes, rows, pages) plus the slowest single
  page — the number a UI actually feels — and the harness asserts the
  walk visited every id exactly once and that no summary row carried a
  body. Against the control it prices both the paging and the body.

Every `list` / `ready_set` response now ends with one `page` event, so
the event counts here are the tickets, with that item subtracted.

Browser (Playwright chromium, fresh context per run): app load → "live"
(with the WebSocket frames the page sent before it); the board's first rows
painted (plus what the page holds at live — DOM elements, nodes, rows
mounted, the rows the client holds, wasm memory, JS heap); every nav
shortcode visible; scrolling the board to its last ticket; opening the
ticket page from the board and as a cold deep link; a ticket created from
outside the page (curl until ACCESS-24, a `tickets.create` on its own socket since)
appearing where the user is looking (the board at its end, the nav open on
that epic and scrolled to its end); and, last, expanding the largest nav
group until its first children show and its last is a scroll away. Since
MFE-12 the app mounts a *window* of the board and a nav node's children only
when it opens, and since SCALE-24/SCALE-26 it holds only what it shows, so
"every row in the DOM" (or in memory) is no longer something to wait for;
`perf/notes.md` ("What MFE-12 changed", "What SCALE-26 changed") defines
each browser row exactly.

Every measurement runs under its own budget (`budgetMs` in
`magnitude.mjs`, scaled by magnitude) and is recorded as a **FAIL** row
with the reason when it exceeds it — the breaking point is the finding,
and nothing hangs. ≥ 3 runs per measurement at the small magnitudes;
medians in the report, every run in the raw JSON.

The production server (ports 4455–4457) and `~/.local/share/menger/` are
never touched: ports come from the kernel (`listen(0)`), databases live
under `--tmp`, and the harness refuses a `--tmp` under the data dir.

## One write under a small and a large parent — `large-parent.py` (SCALE-3)

The ladder's create → `watch` row times one create inside a full browser
run; this probe isolates what a create costs as the store and its parent
grow. Plain Python 3, no dependencies, against `target/release/menger`
(`cargo build --release` first):

```sh
python3 perf/large-parent.py 1000000          # [EPIC] defaults to MAP
```

It seeds N (`--seed-synthetic N`, seed value 1) into a fresh database under
`target/perf-tmp` on kernel-chosen ports with the shell off, then over REST
times 20 creates under `MAP-1` — the largest epic's root, which seed value
1 generates at every N (23 % of the store at 10⁴, 16 % at 10⁶) — and 20
under a new `TINY` root, and prints both medians and their ratio (and,
since SCALE-28, the medians of moving each of those 40 to `Complete` — a
status change that moves every roll-up around it), the size
of the root's `get` and summary row, the time of a page of the root's
children (`list { parent, summary, limit: 200 }`, the first page and one
90 % of the way down), the largest `children_changed` event seen on
`watch`, and the server's RSS. The small parent isolates the store's size
(SCALE-3's criterion: its median at 10⁶ within 2× of 10⁴); the large one
added the parent's `children` list, which every create under it cloned,
persisted and broadcast — until SCALE-4 replaced it with `child_count`
(its criterion: the two medians within 1.5× at 10⁶). The probe reads
either row shape, so one command measures a binary from before and after:
`MENGER_BIN=path` runs another binary, `LARGE_PARENT_PORTS=WS,REST` pins
the ports. The database is deleted afterwards; the server log stays
beside it.

## The ready set, status pages and the fan-out — `ready-probe.mjs` (SCALE-32)

Two probes over the WebSocket against a server you start yourself (a
`--db-path` under `target/`, test ports, `PLEXUS_NO_REGISTRY=1`):

```sh
node perf/ready-probe.mjs pages PORT 21 - TURU-2071 MAP-2   # "-" is the head
python3 perf/fan-out.py target/fan.db                        # then serve it, and:
node perf/ready-probe.mjs fan-out PORT 7
```

`pages` times the first page of `ready_set { summary, limit: 200 }` and of
`list { status: "Ready", summary, limit: 200 }` after each cursor, beside
an unfiltered `list { summary, limit: 200 }` as the transport's own floor
(~2 ms at 10⁶). `TURU-2071` is 90 % of the way through seed value 1's store
at 10⁶; `MAP-2` the start of its largest shortcode — the most a status
page's `remaining` still walks. `fan-out.py` writes a store no seed makes:
one ticket blocking 1, 10³ and 10⁵ `Ready` others; `fan-out` completes each
blocker and sets it back, RUNS times each way, and reports the latency, the
bytes a `watch` subscriber receives for that write (the blocker's
`unlocks` rides whole in `status_changed` — SCALE-23) and the ready set
after it. The file has no derived state, so either binary builds its own
on first open; for steady-state numbers, open it once, stop, checkpoint
the WAL, and measure on the next start.

## Report layout

`plans/PERF/artifacts/MAGNITUDE-<date>.md` — machine specs, the ladder
at a glance, a scaling table (median ratio per ×10 in N, first failing
magnitude), one table per magnitude, then the analysis sections. The
tables are regenerated from `raw/1e<m>.json`; the analysis ("Degradation
curve", "Recommendations") is authored in `perf/notes.md` and included
verbatim, so a re-run keeps it.

## Toolchain notes (what worked on this machine, 2026-09-06)

No Node on PATH and no version manager. Installed Node LTS from the
official tarball, no root:

```sh
V=v24.20.0
cd ~/.local
curl -sSLO https://nodejs.org/dist/$V/node-$V-linux-x64.tar.xz
curl -sSL https://nodejs.org/dist/$V/SHASUMS256.txt | grep " node-$V-linux-x64.tar.xz$" | sha256sum -c
tar -xJf node-$V-linux-x64.tar.xz && ln -sfn ~/.local/node-$V-linux-x64 ~/.local/node
for b in node npm npx corepack; do ln -sfn ~/.local/node/bin/$b ~/.local/bin/$b; done
```

Playwright 1.63.0's bundled chromium (153.0.8010.12) launched headless
without any extra system libraries on Bazzite 44 (Fedora Kinoite base);
`npx playwright install-deps` was not needed. Node 24's built-in
`node:sqlite` does the post-seed WAL checkpoint (falls back to the
`sqlite3` CLI, then to none — recorded in the raw JSON either way).
