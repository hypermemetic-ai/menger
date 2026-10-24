#!/usr/bin/env bash
# FILTER-53: the card on a saved chip, on a throwaway — never the real tracker.
# It starts menger on 127.0.0.1:$PORT/$((PORT+1)) with its own database and a
# scratch key as --admin, files synthetic tickets (PLEXUS and MENGER as hosts,
# shortcodes hosted under each, every kind, several statuses and severities, a
# label) and FILTER-50's saved filters (Plexus-Ready, Needs you, Deployment,
# Created-Active-Weekly, and Plexus-Attention, which names two of them in a
# nested group), then runs scripts/filter-card-check.mjs at 390 and 1440,
# light and dark: hover, pin and the sheet; a name opened in place; add a rule
# and a group; flip; negate; change a field; bin; name a group into a saved
# filter (a taken name refused); save back and undo; the switch; the nav
# under it, tree and flat; the empty line's offers; ⌂'s card; document height
# = viewport. FILTER-54: a rule's time — ◷ on a kind and a status (Became so…,
# Has been so…), changed and taken off, a date of one's own, Created under
# Dates, as text in the new form. FILTER-55: the board's line says only its
# filter's name; its Filter button, the name, `f` and + add a filter open the
# builder on the board's own filter (a change applies at once); every segment
# a dropdown (field, is / is not, values ticked and counted), a ghost row for
# a new part; More's Save as filter… and Reset. And (the owner's `now`) ⌂'s
# chip, F and the phone bar's Filter open the builder on ⌂, editable; the wave
# board's and the inputs' own rules come from This page, each a row with its
# operator and its bin. WIDTHS / THEMES narrow the runs.
#   scripts/filter-card-check.sh [--port 4462] [--db /tmp/menger-NAME.db] [--shots DIR] [--serve]
# --serve: seed, print the origin and the session file, and keep serving
# (Ctrl-C stops it). Needs a built menger (CARGO_TARGET_DIR's debug binary),
# frontend/dist (`trunk build --release`) and Playwright (PLAYWRIGHT_FROM,
# default ../menger/perf/node_modules).
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
port=4462 shots="" db="" serve=0
while [[ $# -gt 0 ]]; do case $1 in --port) port=$2; shift 2 ;; --shots) shots=$2; shift 2 ;; --db) db=$2; shift 2 ;; --serve) serve=1; shift ;; *) echo "unknown $1" >&2; exit 2 ;; esac; done
shell=$((port + 1))
bin=${MENGER_BIN:-${CARGO_TARGET_DIR:-$here/target}/debug/menger}
export PLAYWRIGHT_FROM=${PLAYWRIGHT_FROM:-$(dirname "$here")/menger/perf/node_modules}
work=$(mktemp -d "${TMPDIR:-/tmp}/menger-filter-card.XXXXXX")
db=${db:-$work/tickets.db}
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; wait 2>/dev/null; }
trap 'stop; rm -rf "$work"' EXIT
log() { printf '== %s\n' "$*"; }

op=$(node "$here/scripts/throwaway-login.mjs" key "$work/op.key")
"$bin" --port "$port" --shell-port "$shell" --db-path "$db" --app-dist "$here/frontend/dist" \
  --auth-provider key --admin "$op" >"$work/menger.log" 2>&1 & pids+=($!)
for _ in $(seq 100); do curl -sf "http://127.0.0.1:$shell/" -o /dev/null && (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
node "$here/scripts/throwaway-login.mjs" session --url "ws://127.0.0.1:$port" --key "$work/op.key" --out "$work/op.session" --agent owner >/dev/null || { cat "$work/menger.log"; exit 1; }
S() { synapse -P "$port" --token-file "$work/op.session" "$@" >>"$work/synapse.log" 2>&1 || { echo "synapse $* failed:"; tail -5 "$work/synapse.log"; exit 1; }; }
mk() { # epic kind status title [severity] [parent]
  local id
  id=$(synapse -P "$port" --token-file "$work/op.session" -j menger tickets create --epic "$1" --kind "$2" --title "$4" ${6:+--parent "$6"} | grep -o '"id":"[A-Z]*-[0-9]*"' | head -1 | cut -d'"' -f4)
  [[ -n $id ]] || { echo "create $4 failed"; tail -3 "$work/synapse.log"; exit 1; }
  [[ $3 == Draft || $3 == Doc ]] || S menger tickets set_status --id "$id" --status "$3"
  [[ -n ${5:-} ]] && S menger tickets update --id "$id" --severity "$5"
  echo "$id"
}

log "synthetic tickets"
S menger tickets create --epic PLEXUS --kind scope --title "Plexus, the platform"
S menger tickets create --epic MENGER --kind scope --title "menger"
for sc in SYNAPSE IDENTITY TRANSPORT; do S menger tickets create --epic "$sc" --kind scope --title "$sc, hosted under Plexus" --parent PLEXUS-1; done
for sc in FILTER DEPLOY APP; do S menger tickets create --epic "$sc" --kind scope --title "$sc, hosted under menger" --parent MENGER-1; done
mk SYNAPSE build Ready "The body is a file" >/dev/null
mk SYNAPSE gap Pending "A reconnect drops the stream" High >/dev/null
mk SYNAPSE build Coding "Warm calls in one round trip" >/dev/null
b=$(mk IDENTITY build Ready "Keys rotate on schedule")
mk IDENTITY gap Ready "A stale session survives a restart" Critical "$b" >/dev/null
mk IDENTITY spike Pending "Passkeys or keys" >/dev/null
mk TRANSPORT build Complete "Bind one address" >/dev/null
mk TRANSPORT gap Pending "Hosts refused under a proxy" High >/dev/null
mk PLEXUS build Ready "The registry in the menu" >/dev/null
mk FILTER build Ready "The card on a saved chip" >/dev/null
mk FILTER gap Pending "A nested name never fills in" High >/dev/null
mk FILTER build Coding "Time on the rule it qualifies" >/dev/null
mk DEPLOY build Pending "The suite after the deploy" >/dev/null
mk DEPLOY gap Ready "A red suite posts nothing" Critical >/dev/null
mk APP execution Ready "Fire the screenshots" >/dev/null
mk APP doc Doc "The app's notebook" >/dev/null
S menger tickets label --id FILTER-3 --add needs-you
S menger tickets label --id SYNAPSE-3 --add needs-you
log "saved filters"
codes="in (SYNAPSE, IDENTITY, TRANSPORT, PLEXUS)"
S menger tickets filters save --name "Plexus-Ready" --filter "$codes and not status Complete and not blocked"
S menger tickets filters save --name "Needs you" --filter "queued or asking or asked or label needs-you"
S menger tickets filters save --name "Deployment" --filter "in (ACCESS, DEPLOY, FILTER, VIEW) and not status in (Coding, Complete, Superseded, Canceled) and not kind doc"
S menger tickets filters save --name "Created-Active-Weekly" --filter "not menger doc and created this week and not status in (Pending, Complete) and not kind doc"
S menger tickets filters save --name "Plexus-Attention" --filter 'saved "Plexus-Ready" and (saved "Needs you" or (kind gap and severity in (High, Critical)))'
log "$(grep -c . "$work/synapse.log") lines of synapse output"

if [[ $serve -eq 1 ]]; then
  log "serving http://127.0.0.1:$shell/app/ — session $work/op.session (Ctrl-C stops)"
  wait "${pids[0]}"
  exit 0
fi
failed=0 total=0
[[ -n $shots ]] && mkdir -p "$shots"
for width in ${WIDTHS:-390 1440}; do for theme in ${THEMES:-light dark}; do
  log "$width $theme"
  total=$((total + 1))
  node "$here/scripts/filter-card-check.mjs" "http://127.0.0.1:$shell" "$work/op.session" "$width" "$theme" $shots >"$work/check.out" 2>&1
  rc=$?; cat "$work/check.out"; [[ $rc -eq 0 ]] || failed=$((failed + 1))
done; done
log "$((total - failed)) of $total runs passed"
[[ $failed -eq 0 ]]
