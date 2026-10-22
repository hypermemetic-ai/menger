#!/usr/bin/env bash
# APP-112: the desktop window never scrolls, on a throwaway — never the real
# tracker. It starts menger on 127.0.0.1:$PORT/$((PORT+1)) with its own
# database and a scratch key as --admin, files synthetic tickets (one with a
# body long enough to push its composer far below the fold) and a chat, then
# runs scripts/desktop-scroll-check.mjs at 1440×900 and 1280×720 (the document
# exactly the viewport's height on the board, a long ticket, chats, a chat and
# #/components, a wheel at the end of the nav or the content moving nothing
# else) and at 390×844 (the phone's window still scrolls, to the page's end),
# light and dark.
#   scripts/desktop-scroll-check.sh [--port 4498] [--db /tmp/menger-NAME.db] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist
# (`trunk build --release`) and Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4498 shots="" db=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; --db) db=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-desktop-scroll.XXXXXX")
db=${db:-$work/tickets.db}
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }

op=$(node "$here/scripts/throwaway-login.mjs" key "$work/op.key")
"$bin" --port "$port" --shell-port "$shell" --db-path "$db" --app-dist "$here/frontend/dist" \
  --auth-provider key --admin "$op" >"$work/menger.log" 2>&1 & pids+=($!)
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/op.key" --out "$work/op.session" --agent owner >/dev/null || { cat "$work/menger.log"; exit 1; }
S() { synapse -P "$port" --token-file "$work/op.session" "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }

log "synthetic tickets"
S menger tickets create --epic KILN --kind scope --title "KILN, the area"
for i in $(seq 2 40); do S menger tickets create --epic KILN --kind build --title "Cone $i witness reads from the probe"; done
{ echo "## The firing log"; for i in $(seq 1 120); do
  printf '\nFiring %d: the kiln held cone 6 for twenty minutes, the witness bent at the fourth, and the glaze on the test tiles came out glossy where it was thick and matte where it was thin.\n' "$i"; done; } >"$work/long.md"
S menger tickets create --epic KILN --kind build --title "A long firing log" --body "@$work/long.md"
S menger chats open --ticket KILN-41 --title "the kiln crew"
log "$(grep -c . "$work/synapse.log") lines of synapse output"

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for size in 1440x900 1280x720 390x844; do for theme in ${THEMES:-light dark}; do
  log "$size $theme"
  total=$((total + 1))
  node "$here/scripts/desktop-scroll-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$size" "$theme" KILN-41 $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done; done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
