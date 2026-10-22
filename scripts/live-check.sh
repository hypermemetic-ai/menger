#!/usr/bin/env bash
# APP-110: the live test app in a ticket, on a THROWAWAY pair of servers —
# never the real tracker or test. It starts three menger servers of its own
# (own ports, own databases, a scratch key as --admin):
#   tracker   $PORT/$((PORT+1))    --live-origin = test's origin
#   test      $((PORT+2))/$((PORT+3))  --frame-ancestor = the tracker's origin
#   bare      $((PORT+4))/$((PORT+5))  neither flag: nothing frames it, it frames nothing
# files a ticket on the tracker whose body holds a mockup, a ```live fence, an
# <m-live> element and a live block for prod, records a release on the
# tracker's `menger` project, then runs scripts/live-check.mjs at 390 and 1440,
# light and dark: iframes of the route in each device, nothing overflowing, the
# bar's controls (Reload, Light / Dark, the widths, Full screen, Compare), the
# release line, prod refused, and the headers (the bare server refuses to be framed).
#   scripts/live-check.sh [--port 4660] [--shots DIR]
# Needs a built menger (CARGO_TARGET_DIR's debug binary), frontend/dist from
# `trunk build --release`, and Playwright (PLAYWRIGHT_FROM, default
# ../menger/perf/node_modules). Never a port in 4450-4458.
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4660 shots=""
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-live-check.XXXXXX")
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }
a=$port; a_s=$((port + 1)); t=$((port + 2)); t_s=$((port + 3)); b=$((port + 4)); b_s=$((port + 5))
for p in $a $a_s $t $t_s $b $b_s; do (( p >= 4450 && p <= 4458 )) && { echo "refusing port $p (menger's own environments)" >&2; exit 2; }; done

op=$(node "$here/scripts/throwaway-login.mjs" key "$work/op.key")
start() { # name port shell-port extra-args...
  local n=$1 p=$2 s=$3; shift 3
  "$bin" --port "$p" --shell-port "$s" --db-path "$work/$n.db" --app-dist "$here/frontend/dist" \
    --auth-provider key --admin "$op" "$@" >"$work/$n.log" 2>&1 & pids+=($!)
  for _ in $(seq 150); do curl -sf "http://127.0.0.1:$s/" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null && return 0; sleep 0.2; done
  echo "$n did not start:"; tail -5 "$work/$n.log"; exit 1
}
start tracker "$a" "$a_s" --live-origin "http://127.0.0.1:$t_s"
start test "$t" "$t_s" --frame-ancestor "http://127.0.0.1:$a_s"
start bare "$b" "$b_s"
for n in tracker:$a test:$t bare:$b; do
  node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:${n#*:}" --key "$work/op.key" --out "$work/${n%%:*}.session" --agent owner >/dev/null || { cat "$work/${n%%:*}.log"; exit 1; }
done
S() { local n=$1 p=$2; shift 2; synapse -P "$p" --token-file "$work/$n.session" "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }

cat >"$work/body.md" <<'BODY'
Before and after, one box each.

```mockup widths=390,1440 title="Before"
<!doctype html><html><body style="margin:0"><p>the mockup</p></body></html>
```

```live route="#/components" widths=390,1440 title="After: #/components on test"
```

<m-live route="#/board" widths="390" title="The element form"></m-live>

```live route="#/board" env="prod" title="Never prod"
```
BODY
for n in tracker:$a bare:$b; do
  S "${n%%:*}" "${n#*:}" menger tickets create --epic KILN --kind scope --title "the kiln"
  S "${n%%:*}" "${n#*:}" menger tickets create --epic KILN --kind build --title "a live view" --body @"$work/body.md"
done
S tracker "$a" menger tickets create --epic MENGER --kind scope --title "menger"
S tracker "$a" menger deployments record --project menger --environment test --state succeeded --revision abc1234 --release 20261022T0830Z-abc1234

failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for width in 390 1440; do for theme in ${THEMES:-light dark}; do
  log "$width $theme"
  total=$((total + 1))
  node "$here/scripts/live-check.mjs" "$a_s" "$t_s" "$b_s" "$work/tracker.session" "$work/test.session" "$work/bare.session" "$width" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done; done
# scripts/shot.mjs on a live view: it waits for the frame's document and fonts, never clicks
log "shot.mjs on the ticket with a live view"
shotdir=${shots:-$work/shots}
if node "$here/scripts/shot.mjs" '#/ticket/KILN-2' --origin "http://127.0.0.1:$a_s" --key "$work/op.key" --also "http://127.0.0.1:$t_s" \
     --width 390,1440 --theme both --out "$shotdir/ticket" >"$work/shot.out" 2>&1 && [[ $(ls "$shotdir/ticket" 2>/dev/null | wc -l) -ge 4 ]]; then
  echo "ok   shot.mjs drew the ticket with its live views at 390 and 1440, light and dark"
else
  echo "FAIL shot.mjs on a live view"; tail -8 "$work/shot.out"; failed=$((failed + 1))
fi
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
