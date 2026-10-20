#!/usr/bin/env bash
# DEPLOY-47: Ask the builder about this, on a throwaway — never the real
# tracker, environments or runner. It starts, in a scratch directory:
#   - a loopback OIDC issuer (scripts/fixture-idp.mjs) for the owner, idp:<a UUID>;
#   - menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database, `key` for the
#     builder (a scratch key signed in as `builder`) and that issuer for the
#     owner, both --admin;
# records synthetic work (MENGER-1, KILN-2 running on test) and a request to
# test whose steps are reported as a runner reports them — `build` succeeded
# with its lines, `deploy` failed with 60 — then runs
# scripts/deploy-ask-check.mjs as the owner at 390 and 1440, light and dark.
#   scripts/deploy-ask-check.sh [--port 4494] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist, and
# Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
owner=0d0e4e57-0000-4000-8000-00000000a1c7 # the synthetic owner (an idp sub is a UUID)
port=4494 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1)) iport=$((port - 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-ask-check.XXXXXX")
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }

node "$here/scripts/fixture-idp.mjs" serve --port "$iport" --sub "$owner" --out "$work" >"$work/idp.log" 2>&1 & pids+=($!)
op=$(node "$here/scripts/throwaway-login.mjs" key "$work/op.key")
for _ in $(seq 50); do [[ -s $work/idp.jwt ]] && break; sleep 0.1; done
"$bin" --port "$port" --shell-port "$shell" --db-path "$work/tickets.db" --app-dist "$here/frontend/dist" \
  --auth-provider key --auth-provider "idp=http://127.0.0.1:$iport/,plexus:menger" --admin "$op" --admin "idp:$owner" \
  >"$work/menger.log" 2>&1 & pids+=($!)
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/op.key" --out "$work/op.session" --agent builder >/dev/null || { cat "$work/menger.log"; exit 1; }
node "$here/scripts/fixture-idp.mjs" session --url "ws://127.0.0.1:$port" --jwt "$work/idp.jwt" --out "$work/owner.session" || exit 1
S() { synapse -P "$port" --token-file "$work/op.session" "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }

log "synthetic work"
S menger tickets create --epic MENGER --kind scope --title "menger"
S menger tickets create --epic KILN --kind scope --parent MENGER-1 --title "the kiln"
S menger tickets create --epic KILN --kind build --title "fire the first load"
S menger tickets commits link --id KILN-2 --commits a1a1a1a
S menger tickets set_status --id KILN-2 --status Complete
S menger deployments record --project menger --environment test --state succeeded --revision a1a1a1a --release 20261020T010101Z-a1a1a1a --commits a1a1a1a

log "a request to test, its steps as a runner reports them"
synapse -P "$port" --token-file "$work/op.session" -j menger deployments request --project menger --environment test --branch main --note "main → test, from the check" >"$work/request.json" 2>&1
request=$(node -e 'for (const l of require("fs").readFileSync(process.argv[1], "utf8").split("\n")) { try { const d = JSON.parse(l).content?.deployment; if (d?.id) { console.log(d.id); break; } } catch {} }' "$work/request.json")
[[ -n $request ]] || { echo "no request:"; cat "$work/request.json"; exit 1; }
step() { # NAME STATE [DETAIL_FILE]
  node -e 'const fs=require("fs");const [r,n,s,f,o]=process.argv.slice(1);const p={request:+r,name:n,state:s};if(f)p.detail=fs.readFileSync(f,"utf8");fs.writeFileSync(o,JSON.stringify(p))' "$request" "$1" "$2" "${3:-}" "$work/step.json"
  S -p @"$work/step.json" menger deployments step
}
printf '$ deploy/pipeline build abc1234\ncompiling menger\nrelease 20261020T020202Z-abc1234\n' >"$work/build.log"
{ echo '$ deploy/pipeline deploy test'; for i in $(seq 1 59); do echo "deploy line $i"; done; } >"$work/deploy.log"
step build started
step build succeeded "$work/build.log"
step deploy started
step deploy failed "$work/deploy.log"
log "request #$request"

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for width in ${WIDTHS:-390 1440}; do
  for theme in ${THEMES:-light dark}; do
    log "$width $theme"
    total=$((total + 1))
    node "${CHECK_MJS:-$here/scripts/deploy-ask-check.mjs}" "http://127.0.0.1:$shell" "$work/owner.session" "$width" "$theme" "$request" $shots >"$work/check.out" 2>&1
    rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
  done
done
log "the chats it opened"
synapse -P "$port" --token-file "$work/op.session" menger chats list 2>&1 | grep -E "title|id:" | head -8
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
