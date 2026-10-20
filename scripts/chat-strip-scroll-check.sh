#!/usr/bin/env bash
# CHAT-60: the ticket strip slides away with the chat's head, on a
# throwaway — never the real tracker. It starts menger on
# 127.0.0.1:$PORT/$((PORT+1)) with its own database and a scratch key as
# --admin, files synthetic tickets in several stages and a chat on KILN-1
# long enough to scroll, whose messages name them, then runs
# scripts/chat-strip-scroll-check.mjs at 390 (iPhone 13), light and dark.
#   scripts/chat-strip-scroll-check.sh [--port 4496] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist (`trunk build --release`: a debug build is over the shell's cap), and
# Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4496 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-strip-scroll.XXXXXX")
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

log "synthetic work"
S menger tickets create --epic KILN --kind scope --title "the kiln"
for t in "fire the first load" "glaze the second" "cool slowly" "log the cone"; do S menger tickets create --epic KILN --kind build --title "$t"; done
S menger tickets set_status --id KILN-2 --status Ready
S menger tickets set_status --id KILN-3 --status Coding
S menger tickets set_status --id KILN-4 --status "In Review"
synapse -P "$port" --token-file "$work/op.session" -j menger chats open --ticket KILN-1 --title "the kiln crew" >"$work/open.json" 2>&1
chat=$(node -e 'for (const l of require("fs").readFileSync(process.argv[1], "utf8").split("\n")) { try { const c = JSON.parse(l).content?.chat; if (c?.id) { console.log(c.id); break; } } catch {} }' "$work/open.json")
[[ -n $chat ]] || { echo "no chat:"; cat "$work/open.json"; exit 1; }
for i in $(seq 1 30); do
  printf 'Turn %s — KILN-%s moves along; the cone is at %s and the door stays shut until it reads cold.\n\nA second paragraph so the turn has some height.\n' "$i" $((i % 4 + 2)) "$i" >"$work/m.md"
  S menger chats post --chat "$chat" --body @"$work/m.md"
done
synapse -P "$port" --token-file "$work/op.session" menger chats tickets --chat "$chat" >"$work/tickets.out" 2>&1
log "chat #$chat: $(grep -c 'stage:' "$work/tickets.out") tickets"

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for theme in ${THEMES:-light dark}; do
  log "390 $theme"
  total=$((total + 1))
  node "$here/scripts/chat-strip-scroll-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$chat" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
