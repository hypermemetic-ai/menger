#!/usr/bin/env bash
# CI-2: is there room to build? Says how full the disk is and whether it is
# below the floor, so a gate refuses up front instead of failing half-way
# with `No space left on device` (the disk filled twice in two days: RUN-29).
#
#   scripts/disk-check.sh                 the line; exit 3 when the disk is low
#   scripts/disk-check.sh --alert         … and at 90% used, the owner is told:
#                                         a gap filed under CI-1 and queued `now`
#   --path P              the filesystem holding P (default $HOME)
#   --min-free-gb N       low below N GB free (default 50; DISK_MIN_FREE_GB)
#   --max-used-pct N      low at N% used or more (default 90; DISK_MAX_USED_PCT)
#
# Low means either. `deploy/pipeline build` refuses on it, `status` warns,
# and the hourly sweep (scripts/sweep/run) asks --alert after it has swept.
#
# The alert files one ticket and keeps its id in
# ~/.local/state/menger/disk-alert; while that ticket is not done no other is
# filed (the queue is not refilled every hour). Where: DISK_ALERT_URL
# (wss://app.menger.sh/ws), DISK_ALERT_AS (builder), DISK_ALERT_PCT (90),
# DISK_ALERT_PARENT (CI-1). A tracker that does not answer is said and files
# nothing; the exit is still the disk's.
#
# Tests (scripts/tests/disk-check) say the disk with DISK_CHECK_USED_PCT and
# DISK_CHECK_AVAIL_GB, and stand synapse in with DISK_ALERT_SYNAPSE.
set -uo pipefail

usage() { sed -n '/^#   scripts/,/^#   --max/p' "$0" | sed 's/^# *//' >&2; exit 1; }
path=$HOME alert=0
min_free=${DISK_MIN_FREE_GB:-50} max_used=${DISK_MAX_USED_PCT:-90}
while [[ $# -gt 0 ]]; do
  case $1 in
    --path) shift; path=${1:-} ;;
    --min-free-gb) shift; min_free=${1:-} ;;
    --max-used-pct) shift; max_used=${1:-} ;;
    --alert) alert=1 ;;
    -h|--help) usage ;;
    *) printf 'disk-check: unknown argument %s\n' "$1" >&2; usage ;;
  esac
  shift
done
[[ $min_free =~ ^[0-9]+$ && $max_used =~ ^[0-9]+$ ]] || { echo 'disk-check: --min-free-gb and --max-used-pct take whole numbers' >&2; exit 1; }
[[ -e $path ]] || { echo "disk-check: no such path $path" >&2; exit 1; }

read -r mount used avail < <(df --output=target,pcent,avail -B1G -- "$path" 2>/dev/null | tail -n 1 | tr -d '%')
used=${DISK_CHECK_USED_PCT:-${used:-0}} avail=${DISK_CHECK_AVAIL_GB:-${avail:-0}}
mount=${mount:-$path}

low="" rc=0
(( avail < min_free )) && low="under ${min_free} GB free"
(( used >= max_used )) && low="${low:+$low, }${max_used}% used or more"
[[ -n $low ]] && rc=3
if [[ -n $low ]]; then
  printf 'disk: %s %s%% used, %s GB free — low (%s); scripts/cleanup-gate.sh --sweep says what can go\n' "$mount" "$used" "$avail" "$low"
else
  printf 'disk: %s %s%% used, %s GB free — ok\n' "$mount" "$used" "$avail"
fi

if [[ $alert == 1 ]] && (( used >= ${DISK_ALERT_PCT:-90} )); then
  SYN=${DISK_ALERT_SYNAPSE:-synapse}
  URL=${DISK_ALERT_URL:-wss://app.menger.sh/ws} AS=${DISK_ALERT_AS:-builder}
  state=${XDG_STATE_HOME:-$HOME/.local/state}/menger/disk-alert
  syn() { timeout 60 "$SYN" --url "$URL" --as "$AS" -j menger tickets "$@" 2>/dev/null; }
  prev=$(cat "$state" 2>/dev/null || true)
  if [[ -n $prev ]]; then
    st=$(syn get --id "$prev" | jq -r '.content.ticket.status // empty' 2>/dev/null | head -n 1)
    case $st in
      "") echo "alert: the tracker did not say how $prev is; nothing filed"; exit "$rc" ;;
      Complete|Canceled|Superseded) ;;
      *) echo "alert: $prev is open ($st); nothing more filed"; exit "$rc" ;;
    esac
  fi
  dir=$(mktemp -d); trap 'rm -f -- "$dir/body.md"; rmdir -- "$dir"' EXIT
  {
    printf 'Filed by the hourly sweep (`scripts/disk-check.sh --alert`, CI-2) at %s.\n\n' "$(date -u +%FT%RZ)"
    printf '`%s` is **%s%% used**, %s GB free, after the sweep removed every build output idle long enough to go. What holds the most:\n\n' "$mount" "$used" "$avail"
    for d in "$HOME/.cache/cargo-build" "$HOME/.cache/cargo-target" "$HOME/.cache/sccache" "$HOME/dev/controlflow/hypermemetic" "$HOME/.local/share/containers"; do
      [[ -d $d ]] && printf -- '- `%s`: %s\n' "${d/#$HOME/~}" "$(timeout 120 du -sh -- "$d" 2>/dev/null | cut -f1 || echo '?')"
    done
    printf '\nThe sweep keeps whatever was touched within its threshold or is in use. `scripts/cleanup-gate.sh --sweep --idle-hours 0` says what else could go; `--list` lists old worktrees and gates. Once there is room again, set this ticket Complete: the next alert waits for that.\n'
  } >"$dir/body.md"
  id=$(syn create --epic CI --parent "${DISK_ALERT_PARENT:-CI-1}" --kind gap --severity High \
         --title "The disk is ${used}% full (${mount}, ${avail} GB free) and the sweep could not free enough" \
         --body "@$dir/body.md" | jq -r '.content.ticket.id // empty' 2>/dev/null | head -n 1)
  if [[ -z $id ]]; then echo "alert: the tracker did not take the ticket; nothing filed"; exit "$rc"; fi
  mkdir -p "$(dirname "$state")" && printf '%s\n' "$id" >"$state"
  syn queue --id "$id" --urgency now --reason "the disk is ${used}% full; builds and prod's database share it" >/dev/null \
    && echo "alert: filed and queued $id" || echo "alert: filed $id; the queue did not take it"
fi
exit "$rc"
