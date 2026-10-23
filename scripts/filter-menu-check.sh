#!/usr/bin/env bash
# FILTER-49: the filter menu, on a throwaway — never the real tracker.
# It starts menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database and a
# scratch key as --admin, files synthetic tickets (four shortcodes, every
# kind, several statuses, a label, two saved filters), then runs
# scripts/filter-menu-check.mjs at 390 and 1440, light and dark: Filter opens
# the popover (the sheet at 390), the board does not move under it, a
# category opens one at a time, the search narrows every value, Escape and a
# press outside close it, `f` opens it again.
#   scripts/filter-menu-check.sh [--port 4496] [--db /tmp/menger-NAME.db] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist
# (`trunk build --release`) and Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4496 shots="" db=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; --db) db=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-filter-menu.XXXXXX")
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
for sc in ATLAS KILN PANTRY GLAZE; do S menger tickets create --epic "$sc" --kind scope --title "$sc, the area"; done
mk() { # epic kind status title
  local id
  id=$(synapse -P "$port" --token-file "$work/op.session" -j menger tickets create --epic "$1" --kind "$2" --title "$4" | grep -o '"id":"[A-Z]*-[0-9]*"' | head -1 | cut -d'"' -f4)
  [[ -n $id ]] || { echo "create $4 failed"; exit 1; }
  [[ $3 == Draft || $3 == Doc ]] || S menger tickets set_status --id "$id" --status "$3"
}
mk ATLAS build Coding "Tile layer 1 renders at zoom 14"
mk ATLAS build "In Review" "Tile layer 2 renders at zoom 14"
mk ATLAS gap Pending "Tiles flicker on pan in Safari"
mk ATLAS spike Ready "Vector or raster for the coastline"
mk KILN build Coding "Cone 1 witness reads from the probe"
mk KILN build Pending "Cone 2 witness reads from the probe"
mk KILN gap Pending "The kiln stalls at cone 01"
mk KILN execution Ready "Fire the first glaze load"
mk PANTRY build Pending "Jars by shelf"
mk PANTRY build Ready "Expiry nudges"
mk PANTRY spike Pending "Barcode lookup"
mk GLAZE build Coding "Celadon test tile 1"
mk GLAZE doc Doc "The glaze notebook"
S menger tickets label --id KILN-4 --add needs-you
S menger tickets filters save --name "Open builds" --filter "kind build and not category done"
S menger tickets filters save --name "Hot gaps" --filter "kind gap"
log "$(grep -c . "$work/synapse.log") lines of synapse output"

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for width in 390 1440; do for theme in ${THEMES:-light dark}; do
  log "$width $theme"
  total=$((total + 1))
  node "$here/scripts/filter-menu-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$width" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done; done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
