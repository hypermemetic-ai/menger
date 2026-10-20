#!/usr/bin/env bash
# CHAT-66: replying to a message, on a throwaway — never the real tracker.
# It starts menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database and
# a scratch key as --admin, opens a synthetic chat on KILN-1 long enough that
# its first message is out of the window (CHAT-8), posts a reply to that
# first message last, then runs scripts/chat-reply-check.mjs at 390
# (iPhone 13), light and dark: a swipe, Esc, a swipe and send, the header,
# a tap that scrolls to the original.
#   scripts/chat-reply-check.sh [--port 4498] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist (`trunk build --release`), and
# Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4498 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-reply.XXXXXX")
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

log "a synthetic chat"
S menger tickets create --epic KILN --kind scope --title "the kiln"
synapse -P "$port" --token-file "$work/op.session" -j menger chats open --ticket KILN-1 --title "the kiln crew" >"$work/open.json" 2>&1
chat=$(node -e 'for (const l of require("fs").readFileSync(process.argv[1], "utf8").split("\n")) { try { const c = JSON.parse(l).content?.chat; if (c?.id) { console.log(c.id); break; } } catch {} }' "$work/open.json")
[[ -n $chat ]] || { echo "no chat:"; cat "$work/open.json"; exit 1; }
printf 'The first load: archive the expired events, or delete them?\n' >"$work/m.md"
S menger chats post --chat "$chat" --body @"$work/m.md"
first=$(synapse -P "$port" --token-file "$work/op.session" -j menger chats messages --chat "$chat" --limit 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{for(const l of s.split("\n")){try{const m=JSON.parse(l).content?.message;if(m){console.log(m.id);break}}catch{}}})')
# Eight at a time: their order is no matter, only that they come between.
seq 2 160 | xargs -P 8 -I{} sh -c 'printf "Turn {} — the cone is at {} and the door stays shut until it reads cold.\n" >"$1/m{}.md"; synapse -P "$2" --token-file "$1/op.session" menger chats post --chat "$3" --body @"$1/m{}.md" >>"$1/synapse.log" 2>&1' _ "$work" "$port" "$chat"
printf '[↩](message:%s) archive — holds are real.\n' "$first" >"$work/m.md"
S menger chats post --chat "$chat" --body @"$work/m.md"
log "chat #$chat: message $first answered by the last of 161"

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for theme in ${THEMES:-light dark}; do
  log "390 $theme"
  total=$((total + 1))
  node "$here/scripts/chat-reply-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$chat" "$first" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
  # The reply went as the token and the text, nothing of the original copied.
  synapse -P "$port" --token-file "$work/op.session" -j menger chats messages --chat "$chat" --newest true --limit 1 >"$work/last.json" 2>&1
  if node -e 'const s=require("fs").readFileSync(process.argv[1],"utf8");for(const l of s.split("\n")){try{const m=JSON.parse(l).content?.message;if(m){process.exit(/^\[↩\]\(message:\d+\) swiped reply$/.test(m.body)&&m.reply_to&&m.quote?.snippet?.startsWith("Turn ")?0:(console.log("stored:",JSON.stringify(m)),1))}}catch{}}process.exit(1)' "$work/last.json"; then
    echo "ok   390-$theme: stored as the token and the text; reply_to and quote answered"
  else
    echo "FAIL 390-$theme: the stored reply"; failed=$((failed + 1))
  fi
done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
