#!/usr/bin/env bash
# PERF-24: the decades — each a reflink copy of the one before it, extended
# by `menger-scale gen`, its bytes split by `sizes`, then measured by `bench`
# on the real server. Pinned to cores 12–23 (another agent holds 0–11).
#
#   scripts/scale/run.sh EXP...          e.g. 3 4 5 6
#
#   SCALE_DIR   the datasets (default ~/.cache/menger-scale; never /tmp, never
#               ~/.local/share/menger)
#   SCALE_BIN   the server: the perf build (default $CARGO_TARGET_DIR/perf/menger)
#   SCALE_TOOL  the harness (default $CARGO_TARGET_DIR/release/menger-scale)
#   SCALE_TEMPLATES  a JSON array of real tickets to draw from (never committed)
#   SCALE_MAX_USED   stop before the disk passes this % used (default 75)
#
# Each decade's directory holds tickets.db, gen.json, sizes.json, bench.json
# and server-0.log, server-1.log (two starts: the first and every later one). A decade is skipped when the disk would pass SCALE_MAX_USED
# with it: the size is projected from the decade before it, ×10.
set -euo pipefail
cd "$(dirname "$0")/../.."
dir=${SCALE_DIR:-$HOME/.cache/menger-scale}
target=${CARGO_TARGET_DIR:-target}
bin=${SCALE_BIN:-$target/perf/menger}
tool=${SCALE_TOOL:-$target/release/menger-scale}
templates=${SCALE_TEMPLATES:-$dir/src/templates.json}
max_used=${SCALE_MAX_USED:-75}
cpus=${SCALE_CPUS:-12-23}
prev=""
for e in "$@"; do
  # The decade before it, when this run did not just make it.
  [[ -z "$prev" && -f "$dir/1e$((e-1))/gen.json" ]] && prev=$((e-1))
  n=$((10**e)); d="$dir/1e$e"
  mkdir -p "$d"
  scripts/disk-check.sh --path "$dir" --max-used-pct "$max_used" || { echo "scale: disk low before 1e$e — stopping"; exit 3; }
  if [[ ! -s "$d/gen.json" ]]; then
    if [[ -n "$prev" && -f "$dir/1e$prev/tickets.db" ]]; then
      bytes=$(stat -c %s "$dir/1e$prev/tickets.db")
      read -r size used <<<"$(df -B1 --output=size,used "$dir" | tail -1)"
      proj=$(( used + bytes * (10**(e-prev)) ))
      if (( proj * 100 / size >= max_used )); then
        echo "scale: 1e$e would take the disk to $(( proj * 100 / size ))% (> $max_used%) — stopping; extrapolate"
        exit 4
      fi
      # The WAL the last server left folded in first: the copy is the main file alone.
      sqlite3 "$dir/1e$prev/tickets.db" 'PRAGMA wal_checkpoint(TRUNCATE)' > /dev/null
      # Over whatever a stopped generation left: a decade starts from the one before.
      cp --reflink=always -f "$dir/1e$prev/tickets.db" "$d/tickets.db"
    fi
    taskset -c "$cpus" "$tool" gen --db "$d/tickets.db" --to "$n" --templates "$templates" > "$d/gen.json.part" 2> "$d/gen.log"
    mv "$d/gen.json.part" "$d/gen.json"
  fi
  [[ -s "$d/sizes.json" ]] || taskset -c "$cpus" "$tool" sizes --db "$d/tickets.db" > "$d/sizes.json"
  taskset -c "$cpus" "$tool" bench --db "$d/tickets.db" --bin "$bin" --out "$d/bench.json" --port $((4740 + e)) \
    ${SCALE_SAMPLES:+--samples $SCALE_SAMPLES} ${SCALE_WATCHERS:+--watchers $SCALE_WATCHERS} > /dev/null
  prev=$e
done
