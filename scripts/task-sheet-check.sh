#!/usr/bin/env bash
# TICKETS-23: a task opens in a sheet that is a full ticket body, drawn by the
# ticket page's own renderer — on a throwaway, never the real tracker. It
# starts menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database and a
# scratch key as --admin, defines m-task / m-tasks (TICKETS-10's, as filed)
# and a stand-in m-decision (scripts/fixtures/task-components.md), files a
# ticket whose m-tasks hold a task with markdown, a code block, an
# m-decision with an id, a ```mockup block and a task inside it, then runs
# scripts/task-sheet-check.mjs at 390 and 1440, light and dark, and reads
# the answer the sheet recorded back off the server.
#   scripts/task-sheet-check.sh [--port 4462] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist
# (`trunk build --release`) and Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4462 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-task-sheet.XXXXXX")
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }

op=$(node "$here/scripts/throwaway-login.mjs" key "$work/op.key")
"$bin" --port "$port" --shell-port "$shell" --db-path "$work/tickets.db" --app-dist "$here/frontend/dist" \
  --auth-provider key --admin "$op" >"$work/menger.log" 2>&1 & pids+=($!)
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/op.key" --out "$work/op.session" --agent owner >/dev/null || { cat "$work/menger.log"; exit 1; }
S() { synapse -P "$port" --token-file "$work/op.session" "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }

log "synthetic tickets"
S menger tickets create --epic KILN --kind scope --title "the kiln"
S menger tickets create --epic KILN --kind doc --title "the task components" --body @"$here/scripts/fixtures/task-components.md"
cat >"$work/b.md" <<'BODY'
The firing, in two tasks.

<m-tasks label="Subtasks">

<m-task id="glaze" status="Coding" name="Glaze the test tiles" files="kiln/glaze.rs, kiln/tiles.rs" model="sonnet" verify="cargo test glaze">

## The brief

Fire the **test tiles** at cone 6 and write down what came out, tile by tile.

```rust
fn fire(cone: u8) -> Vec<Tile> {
    tiles().map(|t| t.at(cone)).collect()
}
```

<m-decision id="cone" data-field="decision">
<m-question data-field="question">Which cone for the test firing?</m-question>
<m-option data-field="option" name="Cone 6" value="6" recommended></m-option>
<m-option data-field="option" name="Cone 10" value="10"></m-option>
</m-decision>

The tile card, as the report will show it:

```mockup widths=390 title="The tile card" description="One tile: its cone and its glaze."
<!doctype html>
<html><head><meta name="viewport" content="width=device-width, initial-scale=1"><style>
body{margin:0;font:16px system-ui;padding:24px;background:#fff;color:#111}
.card{border:1px solid #ccc;border-radius:10px;padding:16px}
@media (prefers-color-scheme: dark){body{background:#111;color:#eee}.card{border-color:#444}}
</style></head><body><div class="card" id="tile"><b>Tile 3</b><p>Cone 6 · celadon</p><button onclick="this.textContent='Picked'">Pick</button></div></body></html>
```

<m-task id="glaze-note" status="Draft" name="A note inside the task">

A task inside a task folds in place.

</m-task>

</m-task>

<m-task id="stack" status="Ready" name="Stack the kiln" depends="glaze" model="haiku">

Load the shelves, heaviest at the bottom.

</m-task>

</m-tasks>
BODY
S menger tickets create --epic KILN --kind build --title "the test firing" --body @"$work/b.md"

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for width in 390 1440; do for theme in ${THEMES:-light dark}; do
  log "$width $theme"
  total=$((total + 1))
  node "$here/scripts/task-sheet-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$width" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done; done
log "the answer, as the server keeps it"
total=$((total + 1))
answer=$(synapse -P "$port" --token-file "$work/op.session" -j menger tickets inputs list --ticket KILN-3 2>/dev/null \
  | jq -r 'select(.content.input) | .content.input | select(.id == "cone") | "\(.state) \(.answer.value.option // "-")"')
echo "cone: $answer"
if [[ $answer == "answered 6" ]]; then echo "ok   the sheet's answer is KILN-3's input cone, recorded"; else echo "FAIL the sheet's answer is not recorded on KILN-3 (got '$answer')"; failed=$((failed + 1)); fi
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
