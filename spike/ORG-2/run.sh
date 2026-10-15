#!/usr/bin/env bash
# ORG-2: the sketch's runner — two shortcodes of the same name, stored and
# fetched by full address, on a scratch copy of a menger database.
#
#   spike/ORG-2/run.sh /path/to/a/copy/of/tickets.db
#
# Takes a consistent copy with sqlite3's .backup (safe against a running
# server), runs sketch.py on the copy inside one transaction it rolls back,
# and deletes the copy. The database named is only read. Exit status: the
# number of failed checks.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
src=${1:?usage: run.sh <menger tickets.db>}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
sqlite3 "file:$src?mode=ro" ".backup $work/t.db"
python3 -I "$here/sketch.py" "$work/t.db"
