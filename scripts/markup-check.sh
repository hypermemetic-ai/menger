#!/usr/bin/env bash
# APP-104: mark up any image, on a throwaway — never the real tracker. It
# starts menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database and a
# scratch key as --admin, files KILN-1 and a chat on it, puts an image in
# the chat (frontend/icons/icon-192.png) and posts a turn showing it, then
# runs scripts/markup-check.mjs at 390 (iPhone 13), light and dark.
#   scripts/markup-check.sh [--port 4492] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist
# (`trunk build --release`: a debug build is over the shell's cap), and
# Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4492 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-markup.XXXXXX")
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
first() { node -e 'const k=process.argv[2].split(".");for (const l of require("fs").readFileSync(process.argv[1], "utf8").split("\n")) { try { let o = JSON.parse(l).content; for (const x of k) o = o?.[x]; if (o) { console.log(o); break; } } catch {} }' "$1" "$2"; }

log "synthetic work"
S menger tickets create --epic KILN --kind scope --title "the kiln"
synapse -P "$port" --token-file "$work/op.session" -j menger chats open --ticket KILN-1 --title "the kiln crew" >"$work/open.json" 2>&1
chat=$(first "$work/open.json" chat.id)
[[ -n $chat ]] || { echo "no chat:"; cat "$work/open.json"; exit 1; }
base64 -w0 "$here/frontend/icons/icon-192.png" >"$work/shot.b64"
synapse -P "$port" --token-file "$work/op.session" -j menger media put --chat "$chat" --mime image/png --data @"$work/shot.b64" >"$work/put.json" 2>&1
hash=$(first "$work/put.json" hash)
[[ -n $hash ]] || { echo "no image:"; cat "$work/put.json"; exit 1; }
printf 'The kiln door, as it stands:\n\n![door](media:%s)\n' "$hash" >"$work/m.md"
S menger chats post --chat "$chat" --body @"$work/m.md"
log "chat #$chat, image $hash"

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for theme in ${THEMES:-light dark}; do
  log "390 $theme"
  total=$((total + 1))
  node "$here/scripts/markup-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$chat" "$hash" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
