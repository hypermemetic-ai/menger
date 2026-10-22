#!/usr/bin/env bash
# COMP-17: #/components on a phone, on a throwaway — never the real tracker.
# It starts menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database and a
# scratch key as --admin, files synthetic components (one referenced by 230
# tickets, an orphan, ones with long summaries), then runs
# scripts/components-check.mjs at 390 and 1440, light and dark.
#   scripts/components-check.sh [--port 4498] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist
# (`trunk build --release`) and Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4498 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-components.XXXXXX")
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

log "synthetic components"
S menger tickets create --epic KILN --kind scope --title "the kiln"
long="Counts every ticket on a board that is workable right now, across all of its waves, and says so in words a reader on a phone has to wrap onto several lines before it is done."
mk() { # name summary
  printf 'Defines %s.\n\n```component %s\n<docs>\n<summary>%s</summary>\n</docs>\n<template><b><slot></slot></b></template>\n```\n' "$1" "$1" "$2" >"$work/b.md"
  S menger tickets create --epic KILN --kind doc --title "defines $1" --body @"$work/b.md"
}
mk m-kiln-load "$long"
mk m-glaze-cone "Short one."
mk m-supercalifragilisticexpialidocious-component-with-a-very-long-name "$long"
printf 'Uses <m-kiln-load></m-kiln-load>.\n' >"$work/r.md"
# 230 tickets, 16 at a time (one synapse call each, in a row, is slow).
seq 1 230 | xargs -P 16 -I{} synapse -P "$port" --token-file "$work/op.session" menger tickets create --epic KILN --kind build \
  --title "uses the load {}" --body @"$work/r.md" >>"$work/synapse.log" 2>&1
printf 'Uses <m-glaze-cone></m-glaze-cone>.\n' >"$work/r.md"
S menger tickets create --epic KILN --kind build --title "uses the cone" --body @"$work/r.md"
log "$(grep -c . "$work/synapse.log") lines of synapse output"

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for width in 390 1440; do for theme in ${THEMES:-light dark}; do
  log "$width $theme"
  total=$((total + 1))
  node "$here/scripts/components-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$width" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done; done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
