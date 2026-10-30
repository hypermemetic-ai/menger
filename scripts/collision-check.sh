#!/usr/bin/env bash
# TICKETS-36: the app's links by uid and the collision page — on a throwaway,
# never the real tracker. It starts menger on 127.0.0.1:$PORT/$((PORT+1))
# with its own database and a scratch key as --admin, seeds two shortcodes
# named PERF in two namespaces (MENGER/STORE's PERF-2, keyed PERF-2, beside
# PLEXUS's, keyed PERF.N-2), a reader of MENGER's tree alone, then runs
# scripts/collision-check.mjs at 390 and 1440, light and dark: the collision
# page for #/ticket/PERF-2 (two rows, each with its parent; the keys; Copy
# link), a row opening at #/t/<uid>/PERF-2-…, an address replaced by its
# uid route, NOPE-1 not found, a stale slug rewritten, the board's row for
# the original PERF-2, the shell's /PERF-2 and /t/<uid>/x. With --shots,
# also scripts/shot.mjs '#/ticket/PERF-2' at 390 and 1440, light and dark.
#   scripts/collision-check.sh [--port 4476] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist
# (`trunk build --release`) and Playwright (PLAYWRIGHT_FROM, default
# ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4476 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-collision.XXXXXX")
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }

op=$(node "$here/scripts/throwaway-login.mjs" key "$work/op.key")
"$bin" --port "$port" --shell-port "$shell" --db-path "$work/tickets.db" --app-dist "$here/frontend/dist" \
  --auth-provider key --admin "$op" >"$work/menger.log" 2>&1 & pids+=($!)
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/login" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/op.key" --out "$work/op.session" --agent owner >/dev/null || { cat "$work/menger.log"; exit 1; }
S() { synapse -P "$port" --token-file "$work/op.session" "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }
J() { synapse -P "$port" --token-file "$work/op.session" -j "$@" 2>>"$work/synapse.log"; }

log "two shortcodes named PERF, in two namespaces (synthetic)"
S menger tickets create --epic MENGER --kind scope --title "menger"
S menger tickets create --epic STORE --kind scope --title "the store and the server" --parent MENGER-1
S menger tickets create --epic PERF --kind scope --title "Magnitude — how menger degrades" --parent STORE-1
S menger tickets create --epic PERF --title "Compression on the WebSocket" --parent PERF-1
S menger tickets create --epic PLEXUS --kind scope --title "the platform"
theirs=$(J menger tickets create --epic PERF --kind scope --title "Self-description cached" --parent PLEXUS-1 | jq -r 'select(.content.ticket) | .content.ticket.id' | head -1)
[[ $theirs == PERF.*-1 ]] || { echo "the namesake's root is keyed '$theirs', not PERF.N-1"; exit 1; }
S menger tickets create --epic PLEXUS/PERF --kind gap --title "the schema answers 3.24 MB" --parent "$theirs"
# Both on the board (its default hides Draft): each named by its address, as
# the bare id fits two.
S menger tickets set_status --id hypermemetic:MENGER/STORE/PERF-2 --status Ready
S menger tickets set_status --id hypermemetic:PLEXUS/PERF-2 --status Ready
uids=$(J menger tickets get --id PERF-2 | jq -r 'select(.content.choices) | .content.choices | map(.uid) | join(" ")')
read -r ouid nuid <<<"$uids"
[[ ${#ouid} -eq 8 && ${#nuid} -eq 8 ]] || { echo "get --id PERF-2 named no two choices: '$uids'"; exit 1; }
echo "the original's uid $ouid, the namesake's $nuid"

log "a reader of MENGER's tree alone"
bob=$(node "$here/scripts/throwaway-login.mjs" key "$work/bob.key")
S menger members grant --principal "$bob" --scope hypermemetic:MENGER --role viewer
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/bob.key" --out "$work/bob.session" --agent bob >/dev/null

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for width in 390 1440; do for theme in ${THEMES:-light dark}; do
  log "$width $theme"
  total=$((total + 1))
  node "$here/scripts/collision-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$work/bob.session" "$ouid" "$nuid" "$width" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done; done
if [[ -n $shots ]]; then
  log "scripts/shot.mjs '#/ticket/PERF-2' at 390 and 1440, light and dark"
  total=$((total + 1))
  if node "$here/scripts/shot.mjs" '#/ticket/PERF-2' --port "$shell" --session "$work/op.session" --theme both --width 390,1440 --screen --out "$shots"; then
    echo "ok   the collision page shot at 390 and 1440, light and dark"
  else
    echo "FAIL shot.mjs"; failed=$((failed + 1))
  fi
fi
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
