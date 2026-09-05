#!/usr/bin/env bash
# MNG-2 acceptance demonstration. The heavy lifting is in two hermetic
# test binaries that spawn the real `menger` executable, SIGKILL it,
# restart it on the same database, and assert over the wire:
#
#   criterion 1,2,4 — tests/durability.rs::acknowledged_mutations_survive_sigkill
#   criterion 3     — tests/durability.rs::fresh_location_starts_empty_and_writable
#   criterion 5     — tests/wire.rs::reads_survive_db_location_unavailable
#   criterion 6     — tests/durability.rs::default_location_is_under_the_user_data_dir
#   criterion 7     — tests/wire.rs::concurrent_reads_complete_under_mutation_stream
#   criterion 8     — run this script (or `cargo test`) twice in a row;
#                     every test isolates its own storage under target/tmp.
#
# Usage: scripts/mng2-durability.sh
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test --test durability --test wire -- --nocapture
