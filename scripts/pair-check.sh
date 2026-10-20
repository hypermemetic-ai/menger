#!/usr/bin/env bash
# TETHER-11: #/pair on a throwaway — never the real tracker. It starts menger
# on 127.0.0.1:$PORT/$((PORT+1)) with its own database and `key`, makes three
# scratch keys — the operator (--admin), alice (admin on KILN), bob (viewer
# on KILN) — and runs scripts/pair-check.mjs at 390 and 1440: the page shows
# the code tether prints for the fixture key (MMG4-2KLG); alice is offered
# editor on KILN and Pair grants it (members.list says so); Not mine grants
# nothing; bob is told who can and has no Pair; a link with no key says so.
#   scripts/pair-check.sh [--port 4640] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist from `trunk build --release` (a debug
# build's wasm is too large for the shell to serve), and
# Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4640 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-pair-check.XXXXXX")
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }

op=$(node "$here/scripts/throwaway-login.mjs" key "$work/op.key")
alice=$(node "$here/scripts/throwaway-login.mjs" key "$work/alice.key")
bob=$(node "$here/scripts/throwaway-login.mjs" key "$work/bob.key")
"$bin" --port "$port" --shell-port "$shell" --db-path "$work/tickets.db" --app-dist "$here/frontend/dist" \
  --auth-provider key --admin "$op" >"$work/menger.log" 2>&1 & pids+=($!)
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
for who in op alice bob; do
  node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/$who.key" --out "$work/$who.session" --agent "$who" >/dev/null || { cat "$work/menger.log"; exit 1; }
done
S() { synapse -P "$port" --token-file "$work/op.session" menger "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }
S tickets create --epic KILN --kind scope --title "the kiln"
S members grant --principal "$alice" --scope KILN --role admin
S members grant --principal "$bob" --scope KILN --role viewer
# What the check reads back: the grants (members.list, as the operator).
grants() { synapse -P "$port" --token-file "$work/op.session" -j menger members list 2>/dev/null; }
export -f grants; export port work

[[ -n $shots ]] && mkdir -p "$shots"
failed=0 total=0
for width in ${WIDTHS:-390 1440}; do
  for mode in alice notmine bob bad; do
    total=$((total + 1))
    sess=$work/alice.session; [[ $mode == bob ]] && sess=$work/bob.session
    log "$mode $width"
    node "$here/scripts/pair-check.mjs" "http://127.0.0.1:$shell" "$sess" "$mode" "$width" $shots >"$work/check.out" 2>&1
    rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
  done
done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
