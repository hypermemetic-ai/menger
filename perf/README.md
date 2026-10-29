# perf/ — menger's probes

A self-contained Node + Python package, deliberately **not** part of
`cargo build` / `cargo test`. Every probe runs against a throwaway menger —
one it starts on kernel-chosen ports, or one you started on test ports with a
database of its own — never the production ports or `~/.local/share/menger/`.
What each measured, and when, is in `notes.md` (its older rows are v1's).

```sh
cd perf
npm install                       # playwright, pinned in package-lock.json (pages-probe.mjs only)
npx playwright install chromium   # the browser (into ~/.cache/ms-playwright)
npm run smoke                     # the concurrency ladder's smoke — must pass
```

| probe | what it measures | server |
| -- | -- | -- |
| `pages-probe.mjs` | UI-8: the first pages on the engine — board first rows painted, wasm memory, rows held | its own (`scripts/ui-throwaway.mjs`) |
| `concurrency.mjs` (+ `concurrency-worker.mjs`) | PERF-12: memory and CPU at 1 … 10ⁿ signed-in WebSockets, idle, under load and after | its own |
| `flood.mjs` | ACCESS-29: a flood of upgrades and logins leaves memory flat | its own |
| `large-parent.py` | SCALE-3 / SCALE-4: one write under a small and a large parent | its own |
| `filter-sheet.py` | FILTER-4: two binaries' start, memory and filter reads, side by side | its own (two) |
| `tree-sheet.py` | FILTER-13: two binaries on a nested store — migration, load, writes, tree reads | its own (two) |
| `filter-probe.mjs` | FILTER-4: the filter's reads — `counts`, `list`, `facets` | yours |
| `reads-probe.mjs` | FILTER-15: `find`, `member`, `ready_set` under a filter | yours |
| `tree-probe.mjs` | FILTER-13: the kept tree's reads | yours |
| `ready-probe.mjs` | SCALE-32: ready-set and status pages; the fan-out of one blocker (`fan-out.py`'s store) | yours |
| `filter-identical.mjs` | FILTER-4: unfiltered pages byte for byte what another server answers | yours (two) |

`fan-out.py` writes the store `ready-probe.mjs fan-out` reads; `menger_ws.py`
is the Python sheets' throwaway server and signed-in socket.

## The pages' browser rows — `pages-probe.mjs` (UI-8)

```sh
node perf/pages-probe.mjs --seed 1000 --runs 1 --debug   # small; the default is 10⁶, release, 5 runs
```

A throwaway menger seeded N, the bundle from `plexus-ui build` pushed to
it, then cold loads of `/app/`, each in a fresh browser context signed in by
cookie. Budgets: first rows ≤ 0.3 s and wasm memory ≤ 10 MB at a million;
exit 1 past one, 2 when setup failed.

## The concurrency ladder — `concurrency.mjs` (PERF-12)

One throwaway `menger` (`--auth-provider key`, a temp database, kernel ports,
the ACCESS-29 connection caps raised on it alone), climbed at 1, 10, … 10ⁿ
signed-in WebSockets. Per rung: *idle* (each socket holding `tickets.watch`
with revisions), *load* (a fixed writer of status changes fanned out to every
watch, plus a read mix per socket) and *after* (every socket closed: the
floor), with the server's RSS, peak and CPU-seconds per wall-second sampled
from /proc. The clients are forked `concurrency-worker.mjs` processes (one
per 250 sockets) over 127.0.x.y source addresses and 8 keys. Report, SVG
curves and raw JSON in `perf/out/concurrency` (`--out`); no Playwright needed:

```sh
node perf/concurrency.mjs --rungs 0..4          # on a quiet machine; --help for the knobs
node perf/concurrency.mjs --smoke --bin "$CARGO_TARGET_DIR/debug/menger"   # rungs 0..1, short phases
```

## A flood — `flood.mjs` (ACCESS-29)

```sh
node perf/flood.mjs --bin "$CARGO_TARGET_DIR/debug/menger" --port 4730 --rounds 2 --upgrades 200 --logins 500 --settle 1
```

A throwaway behind a "proxy" (`--trusted-proxy 127.0.0.1`) with a fixture
OIDC issuer, flooded in rounds of anonymous upgrades and logins; exit 0 when
the last round's RSS is within `--tolerance` of the round at `--settle`.

## Signed in (ACCESS-52)

menger has no open mode, so every server a probe starts runs `--auth-provider
key` with a key of its own as `--admin`, and the probes against a server you
started sign in from `$MENGER_SESSION_FILE`:

```sh
op=$(node scripts/throwaway-login.mjs key /tmp/probe.key)
target/debug/menger --port 4720 --shell-port 4721 --auth-provider key --admin "$op" \
  --db-path /tmp/probe.db --data-dir /tmp/probe-data --seed-synthetic 1000 &
node scripts/throwaway-login.mjs session --url ws://127.0.0.1:4720 --key /tmp/probe.key --out /tmp/probe.session
MENGER_SESSION_FILE=/tmp/probe.session node perf/filter-probe.mjs after 4720 3
MENGER_SESSION_FILE=/tmp/probe.session node perf/reads-probe.mjs 4720 3
MENGER_SESSION_FILE=/tmp/probe.session node perf/tree-probe.mjs after 4720 3
MENGER_SESSION_FILE=/tmp/probe.session node perf/ready-probe.mjs pages 4720 3 -
```

A session belongs to the server that minted it, so `filter-identical.mjs
BEFORE AFTER SEED` takes one file per server:
`MENGER_SESSION_FILES=BEFORE_FILE,AFTER_FILE`. The sheets start their servers
the same way, through `menger_ws.py`.

## The sheets — `filter-sheet.py`, `tree-sheet.py`

```sh
python3 perf/filter-sheet.py BEFORE_BIN AFTER_BIN BEFORE_DB AFTER_DB [ROUNDS]
python3 perf/tree-sheet.py BEFORE_BIN AFTER_BIN FORM4_DB WORK_DIR [ROUNDS]   # FORM4_DB: --seed-shape nested
```

Each database must be checkpointed (`sqlite3 F 'PRAGMA wal_checkpoint(TRUNCATE)'`):
the sheets copy the main file alone.

## One write under a small and a large parent — `large-parent.py` (SCALE-3)

```sh
python3 perf/large-parent.py 1000000          # [EPIC] defaults to MAP; MENGER_BIN=path for another binary
```

It seeds N into a fresh database under `target/perf-tmp` and, over the
WebSocket, times 20 creates under `MAP-1` (the largest epic's root) and 20
under a new `TINY` root, then each moved to `Complete`: the medians and their
ratio, the root's `get` and summary row, a page of its children, the largest
`children_changed` on `watch`, and the server's RSS.

## The ready set, status pages and the fan-out — `ready-probe.mjs` (SCALE-32)

```sh
node perf/ready-probe.mjs pages PORT 21 - TURU-2071 MAP-2   # "-" is the head
python3 perf/fan-out.py target/fan.db                        # then serve it, and:
node perf/ready-probe.mjs fan-out PORT 7
```

`pages` times the first page of `ready_set` and of `list { status: "Ready" }`
after each cursor, beside an unfiltered `list` as the transport's floor.
`fan-out.py` writes a store where one ticket blocks 1, 10³ and 10⁵ `Ready`
others; `fan-out` completes each blocker and sets it back, and reports the
latency, the bytes a `watch` subscriber receives and the ready set after it.
