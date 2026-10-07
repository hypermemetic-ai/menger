#!/usr/bin/env bash
# WAVE-3: what keeping every placement (WAVE-6) costs — the spike's runner.
#
#   spike/WAVE-2/run.sh 100 10000 1000000
#
# Builds the program (wave3.rs) twice — with WAVE-6's pass, and with
# `--features wave3-no-replace` (without it) — prepares one store per
# magnitude, and measures both builds on byte-identical copies of it: the
# open three times (startup), then the writes (wave3.rs says which). One JSON
# line per measurement on stdout; progress on stderr. Files under
# target/wave3 (WAVE3_OUT); WAVE3_SAMPLES writes per kind (default 1000);
# WAVE3_ROUNDS rounds of without-then-with per magnitude (default 2), since a
# write's latency is mostly its fsync and that is noisy.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=${WAVE3_OUT:-$root/target/wave3}
mkdir -p "$out"
cd "$root"
cargo build --release -p menger-tickets-api --example wave3 >&2
cp target/release/examples/wave3 "$out/wave3-with"
cargo build --release -p menger-tickets-api --example wave3 --features wave3-no-replace >&2
cp target/release/examples/wave3 "$out/wave3-without"
for n in "$@"; do
  db="$out/n$n.db"
  rm -f "$db" "$db-wal" "$db-shm"
  echo "== $n: prepare" >&2
  t0=$(date +%s)
  "$out/wave3-with" prepare "$n" "$db" >&2
  echo "   prepared in $(( $(date +%s) - t0 )) s" >&2
  for round in $(seq 1 "${WAVE3_ROUNDS:-2}"); do
    for v in without with; do
      rm -f "$out/run.db" "$out/run.db-wal" "$out/run.db-shm"
      cp "$db" "$out/run.db"
      echo "== $n: $v (round $round)" >&2
      "$out/wave3-$v" open "$out/run.db" 3
      "$out/wave3-$v" measure "$out/run.db" "${WAVE3_SAMPLES:-1000}"
    done
  done
done
rm -f "$out/run.db" "$out/run.db-wal" "$out/run.db-shm"
