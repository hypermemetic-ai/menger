#!/usr/bin/env bash
# CHAT-61: what loads above the view never moves the chat, on a throwaway —
# never the real tracker. It starts menger on 127.0.0.1:$PORT/$((PORT+1)) with
# its own database and `key` for a scratch key signed in as `builder`, seeds a
# chat of one on KILN-1 with sixty turns — every third a message carrying
# components (three <m-stat>s and prose, so it draws in a lazy frame, CHAT-21)
# — and runs scripts/chat-anchor-check.mjs: in WebKit (no overflow-anchor) and
# Chromium with overflow-anchor: none, at 390, scrolled up through it, the
# first turn seen moves ≤ 1 px while the frames above it load.
#   scripts/chat-anchor-check.sh [--port 4630] [--browsers webkit,chromium] [--hold: seed, then serve until killed]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist, and
# Playwright (perf/node_modules, else ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4630 browsers=webkit,chromium hold=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --browsers) browsers=$2; shift 2 ;; --hold) hold=1; shift ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
dist=${MENGER_DIST:-$here/frontend/dist}
perf=$here/perf; [[ -d $perf/node_modules/playwright ]] || perf=$(dirname "$here")/menger/perf
image=${PLAYWRIGHT_IMAGE:-mcr.microsoft.com/playwright:v$(node -p "require('$perf/node_modules/playwright/package.json').version")-noble}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-anchor-check.XXXXXX")
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }

op=$(node "$here/scripts/throwaway-login.mjs" key "$work/op.key")
"$bin" --port "$port" --shell-port "$shell" --db-path "$work/tickets.db" --app-dist "$dist" \
  --auth-provider key --admin "$op" >"$work/menger.log" 2>&1 & pids+=($!)
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/op.key" --out "$work/op.session" --agent builder >/dev/null || { cat "$work/menger.log"; exit 1; }
S() { synapse -P "$port" --token-file "$work/op.session" "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }

log "a chat of sixty turns, every third with components"
S menger tickets create --epic KILN --kind scope --title "the kiln"
synapse -P "$port" --token-file "$work/op.session" -j menger chats open --ticket KILN-1 >"$work/open.json" 2>&1
chat=$(node -e 'for (const l of require("fs").readFileSync(process.argv[1], "utf8").split("\n")) { try { const c = JSON.parse(l).content?.chat; if (c?.id) { console.log(c.id); break; } } catch {} }' "$work/open.json")
[[ -n $chat ]] || { echo "no chat:"; cat "$work/open.json"; exit 1; }
for i in $(seq 1 60); do
  if (( i % 3 == 0 )); then
    {
      echo "Firing $i — the week so far:"; echo
      echo "<m-stat value=\"$((i * 7))\" label=\"loads fired\"></m-stat>"
      echo "<m-stat value=\"$((i * 13))\" label=\"pieces\"></m-stat>"
      echo "<m-stat value=\"$((i % 9 + 1))\" label=\"cracked\"></m-stat>"; echo
      for p in 1 2 3; do echo "Paragraph $p of firing $i: the kiln climbed to cone six over nine hours, held, and cooled overnight; the shelves came out clean and the glaze ran where it was meant to."; echo; done
    } >"$work/m.md"
  else
    printf 'Turn %d. %s\n' "$i" "$(printf 'a plain line of the conversation %.0s' $(seq 1 $((i % 5 + 1))))" >"$work/m.md"
  fi
  S menger chats post --chat "$chat" --body @"$work/m.md"
done
log "chat $chat"
if [[ -n $hold ]]; then cp "$work/op.key" "${TMPDIR:-/tmp}/chat-anchor.key"; log "held: http://127.0.0.1:$shell/#/chats/$chat (key ${TMPDIR:-/tmp}/chat-anchor.key)"; wait; exit 0; fi

failed=0
for b in ${browsers//,/ }; do
  log "$b"
  # WebKit wants Ubuntu's libraries: on a host without them, Playwright's own
  # image (the version perf/ pins), the checkout and the scratch bind-mounted.
  if [[ $b == webkit ]] && ! (cd "$perf" && node -e "require('playwright').webkit.launch().then(b=>b.close())" >/dev/null 2>&1) && docker image inspect "$image" >/dev/null 2>&1; then
    run=(docker run --rm --security-opt label=disable --network host --user "$(id -u):$(id -g)" -e HOME=/tmp
      -v "$here:$here:ro" -v "$perf:$perf:ro" -v "$work:$work" "$image" node)
  else
    run=(node)
  fi
  "${run[@]}" "$here/scripts/chat-anchor-check.mjs" --origin "http://127.0.0.1:$shell" --key "$work/op.key" --chat "$chat" --browser "$b" >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done
[[ $failed -eq 0 ]]
