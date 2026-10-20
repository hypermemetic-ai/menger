#!/usr/bin/env bash
# DEPLOY-40: → prod on a throwaway — never the real tracker, environments or
# pipeline. It starts, in a scratch directory:
#   - a loopback OIDC issuer (scripts/fixture-idp.mjs) for the owner, idp:<a UUID>;
#   - menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database, `key` for agents
#     and that issuer for the owner, both --admin;
#   - deploy/runner.mjs --listen --fake --owner idp:<a UUID> on 127.0.0.1:$((PORT+2)),
#     app-prod's webhook set to it (the secret a 0600 file, never argv);
# records synthetic deployments (test runs a newer release than app-test and
# app-prod; KILN-2, KILN-3 and GLAZE-2 under MENGER ready to go) and runs
# scripts/prod-button-check.mjs at 390 and 1440, light and dark: agent (an
# admin key's request, refused), fail (MENGER_RUNNER_FAKE_FAIL=app-test), ok,
# none; app-prod set back after each width and theme.
#   scripts/prod-button-check.sh [--port 4490] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist, and
# Playwright (PLAYWRIGHT_FROM, default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
owner=0d0e4e57-0000-4000-8000-00000000a1c0 # the synthetic owner (an idp sub is a UUID)
port=4490 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1)) rport=$((port + 2)) iport=$((port - 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-prod-check.XXXXXX")
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
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/" -o /dev/null && break; sleep 0.2; done
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/op.key" --out "$work/op.session" --agent builder >/dev/null || { cat "$work/menger.log"; exit 1; }
node "$here/scripts/fixture-idp.mjs" session --url "ws://127.0.0.1:$port" --jwt "$work/idp.jwt" --out "$work/owner.session" || exit 1
S() { synapse -P "$port" --token-file "$work/op.session" menger "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }

log "synthetic work"
S tickets create --epic MENGER --kind scope --title "menger"
S tickets create --epic KILN --kind scope --parent MENGER-1 --title "the kiln"
S tickets create --epic GLAZE --kind scope --parent MENGER-1 --title "the glaze"
for t in "KILN:fire the first load" "KILN:a cone chart" "GLAZE:the celadon recipe"; do S tickets create --epic "${t%%:*}" --kind build --title "${t#*:}"; done
S tickets commits link --id KILN-2 --commits a1a1a1a
S tickets commits link --id KILN-3 --commits b2b2b2b
S tickets commits link --id GLAZE-2 --commits c3c3c3c
S tickets update_many --ids KILN-2,KILN-3,GLAZE-2 --status Complete
OLD=20261017T094508Z-0dd0000 NEW=20261019T192902Z-8cb79df
S deployments record --project menger --environment test --state succeeded --revision 0dd0000 --release "$OLD"
S deployments record --project menger --environment app-test --state succeeded --revision 0dd0000 --release "$OLD" --from test
S deployments record --project menger --environment app-prod --state succeeded --revision 0dd0000 --release "$OLD" --from app-test
S deployments record --project menger --environment test --state succeeded --revision 8cb79df --release "$NEW" --previous 0dd0000 --commits a1a1a1a,b2b2b2b,c3c3c3c,8cb79df
reset() { S deployments record --project menger --environment app-prod --state rolled_back --revision 0dd0000 --release "$OLD" --previous 8cb79df; }

( umask 077; head -c 32 /dev/urandom | base64 | tr -d '\n=' >"$work/secret" )
node -e 'const fs=require("fs");const [w,u]=process.argv.slice(1);fs.writeFileSync(w+"/webhook.json",JSON.stringify({project:"MENGER",environment:"app-prod",url:u,secret:fs.readFileSync(w+"/secret","utf8").trim()}),{mode:0o600})' "$work" "http://127.0.0.1:$rport/"
synapse -P "$port" --token-file "$work/op.session" -p @"$work/webhook.json" menger deployments webhook set >>"$work/synapse.log" 2>&1 || { tail -5 "$work/synapse.log"; exit 1; }

runner_pid=""
runner() { # [FAIL_STEP]
  [[ -n $runner_pid ]] && { kill "$runner_pid" 2>/dev/null; wait "$runner_pid" 2>/dev/null; }
  MENGER_DEPLOY_KEY_FILE=$work/op.key MENGER_RUNNER_FAKE_FAIL=${1:-} MENGER_RUNNER_FAKE_SECS=2 MENGER_RUNNER_POLL_SECS=1 \
    node "$here/deploy/runner.mjs" --listen --fake --port "$rport" --secret-file "$work/secret" --url "ws://127.0.0.1:$port" \
      --project MENGER --state-dir "$work/runner" --owner "idp:$owner" >>"$work/runner.log" 2>&1 &
  runner_pid=$!; pids+=("$runner_pid")
  for _ in $(seq 50); do curl -sf "http://127.0.0.1:$rport/health" >/dev/null && break; sleep 0.1; done
}

failed=0 total=0
check() { # SESSION MODE WIDTH THEME
  total=$((total + 1))
  node "$here/scripts/prod-button-check.mjs" "http://127.0.0.1:$shell" "$1" "$2" "$3" "$4" $shots >"$work/check.out" 2>&1
  local rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
}
[[ -n $shots ]] && mkdir -p "$shots"
for width in 390 1440; do
  for theme in light dark; do
    log "$width $theme"
    runner;          check "$work/op.session" agent "$width" "$theme"
    runner app-test; check "$work/owner.session" fail "$width" "$theme"
    runner;          check "$work/owner.session" ok "$width" "$theme"
                     check "$work/owner.session" none "$width" "$theme"
    reset
    # DEPLOY-42: test's last deploy failed; then test's record without a release (a runner's)
    S deployments record --project menger --environment test --state failed --revision 8cb79df --release "$NEW" --previous 0dd0000 --commits a1a1a1a,b2b2b2b,c3c3c3c,8cb79df
    check "$work/owner.session" testfailed "$width" "$theme"
    S deployments record --project menger --environment test --state succeeded --revision 0dd0000
    check "$work/owner.session" norelease "$width" "$theme"
    S deployments record --project menger --environment test --state succeeded --revision 8cb79df --release "$NEW" --previous 0dd0000 --commits a1a1a1a,b2b2b2b,c3c3c3c,8cb79df
    reset
  done
done
log "runner log (last lines)"; tail -n 6 "$work/runner.log"
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
