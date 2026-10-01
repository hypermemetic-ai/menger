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
tmpfs) · `--timeout-scale X` · `--no-build` · `--keep-db` · `--headed`.

## What it measures, per magnitude

Server: seed time (as logged by the server), startup load-into-cache
time (a restart on the seeded file, after a WAL checkpoint — what a
clean close does), RSS after load / after `list` / after the browser /
peak; REST latencies for `health`, `list` (plus payload bytes and event
count — streamed, never held in memory), `get`, `ready_set`, `blockers`;
create → `watch` push latency over SSE.

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

Browser (Playwright chromium, fresh context per run): app load → "live";
the board's first rows painted (plus what the page holds at live — DOM
elements, nodes, rows mounted, JS heap); every nav shortcode visible;
scrolling the board to its last ticket; opening the ticket page from the
board and as a cold deep link; a curl-created ticket appearing where the
user is looking (the board at its end, the nav open on that epic); and,
last, expanding the largest nav group until every one of its tickets is
mounted. Since MFE-12 the app mounts a *window* of the board and a nav
node's children only when it opens, so "every row in the DOM" is no
longer something to wait for; `perf/notes.md` ("What MFE-12 changed")
defines each browser row exactly.

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
under a new `TINY` root, and prints both medians, the size of the root's
`get` and summary row, the largest `children_changed` event seen on
`watch`, and the server's RSS. The small parent isolates the store's size
(SCALE-3's criterion: its median at 10⁶ within 2× of 10⁴); the large one
adds the parent's `children` list, which every create under it clones,
persists and broadcasts (SCALE-4). The database is deleted afterwards; the
server log stays beside it.

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
