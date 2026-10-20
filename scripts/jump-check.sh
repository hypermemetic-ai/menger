#!/usr/bin/env bash
# CHAT-62: the round ↓ on a throwaway — never the real tracker. It starts
# menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database and `key` for
# two scratch keys (the reader and an agent, both --admin), opens a chat on a
# synthetic ticket, has the agent post 60 long messages, then runs
# scripts/jump-check.mjs as the reader — 390 as an iPhone 13 in WebKit
# (Playwright's image by default, PW_IMAGE), and Chromium at 1440 — with the
# keyboard up at 390 too — read back a few screens: the ↓ shows, its rect meets no
# turn's text, a tap lands at the bottom; then a message arrives and is its
# badge.
#   scripts/jump-check.sh [--port 4530]
#   PW_IMAGE= scripts/jump-check.sh          # WebKit on the host itself
# SHOTS=DIR keeps a screenshot per run (read back, the badge showing).
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist, and
# Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4530
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-jump-check.XXXXXX")
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; [[ -n ${KEEP:-} ]] && echo "kept $work" || rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }

me=$(node "$here/scripts/throwaway-login.mjs" key "$work/me.key")
agent=$(node "$here/scripts/throwaway-login.mjs" key "$work/agent.key")
"$bin" --port "$port" --shell-port "$shell" --db-path "$work/tickets.db" --app-dist "$here/frontend/dist" \
  --auth-provider key --admin "$me" --admin "$agent" >"$work/menger.log" 2>&1 & pids+=($!)
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/me.key" --out "$work/me.session" --agent owner >/dev/null || { cat "$work/menger.log"; exit 1; }
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/agent.key" --out "$work/agent.session" --agent builder >/dev/null || exit 1
S() { local t=$1; shift; synapse -P "$port" --token-file "$work/$t.session" "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }

log "a ticket, a chat on it, 60 long messages"
S me menger tickets create --epic KILN --kind scope --title "the kiln"
synapse -P "$port" --token-file "$work/me.session" -j menger chats open --ticket KILN-1 --others "$agent" >"$work/open.json" 2>&1
chat=$(node -e 'for (const l of require("fs").readFileSync(process.argv[1], "utf8").split("\n")) { try { const c = JSON.parse(l).content?.chat; if (c?.id) { console.log(c.id); break; } } catch {} }' "$work/open.json")
[[ -n $chat ]] || { echo "no chat:"; cat "$work/open.json"; exit 1; }
for i in $(seq 1 60); do
  printf 'Message %s. The first load fired to cone 6 and the shelves held; the glaze on the north side ran a little thin, so the next firing moves the test tiles to the middle shelf and logs the pyrometer every minute until it cools.\n' "$i" >"$work/m.md"
  S agent menger chats post --chat "$chat" --body @"$work/m.md"
done
log "chat #$chat"

failed=0 total=0
# WebKit at 390 by default, in Playwright's image when this host lacks its
# libraries (PHONE_ENGINE=chromium: Chromium's iPhone, whose emulated
# viewport scrolls the document back after a jump; WebKit's does not).
phone_engine=${PHONE_ENGINE:-webkit}
PW_IMAGE=${PW_IMAGE-mcr.microsoft.com/playwright:v1.63.0-noble}
for run in "390 $phone_engine" "1440 chromium"; do
  set -- $run
  log "$1 $2"
  total=$((total + 1))
  args=("http://127.0.0.1:$shell" "$work/me.session" "$chat" "$1" "$2" "ws://127.0.0.1:$port" "$work/agent.session")
  if [[ $2 == webkit && -n ${PW_IMAGE:-} ]]; then
    # WebKit's libraries are not on this host: Playwright's own image, the
    # same version as perf/'s (SELinux: label=disable for the mounts).
    docker run --rm --network host --security-opt label=disable --user "$(id -u):$(id -g)" -e HOME=/tmp \
      -v "$here:$here:ro" -v "$work:$work:ro" -v "$PLAYWRIGHT_FROM:/pw/node_modules:ro" -e PLAYWRIGHT_FROM=/pw/node_modules \
      ${SHOTS:+-v "$SHOTS:$SHOTS"} -e SHOTS="${SHOTS:-/tmp}" \
      "$PW_IMAGE" node "$here/scripts/jump-check.mjs" "${args[@]}" >"$work/check.out" 2>&1
  else
    node "$here/scripts/jump-check.mjs" "${args[@]}" >"$work/check.out" 2>&1
  fi
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
