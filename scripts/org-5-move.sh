#!/usr/bin/env bash
# ORG-5: a scope under a scope roots its own shortcode — the scopes that sat
# inside their parent's shortcode moved out, each into a shortcode it roots
# (the owner, 2026-10-15: `existing` = migrate, `names` = yes), through
# ACT-6's `tickets.reshortcode`:
#
#   reshortcode --ids MENGER-2 --to APP --with-children true
#
# MENGER-2 becomes APP-1, hosted where it hangs (under MENGER-1); its tree in
# MENGER comes with it, numbered APP-2, … in MENGER's order; a shortcode
# hosted under it (a child rooting its own) follows into APP's namespace.
# Every old id keeps naming its ticket (`get MENGER-2` answers APP-1 with
# `moved_from`), so links, chips, commit messages and blockers keep working.
#
#   scripts/org-5-move.sh PORT              the moves, against the menger on PORT
#   scripts/org-5-move.sh PORT --dry-run    what it would move, nothing written
#   scripts/org-5-move.sh PORT --only-ten   the ten the owner named, not the
#                                           four that appeared since (input `more`)
#
# Idempotent: a move already made (the old id names a ticket in its new
# shortcode) is skipped and said. Prints each old → new id and how many
# tickets came with it. Run against prod only after the release that has
# ACT-6 and ORG-5 is promoted — never before: an older server has no
# `reshortcode`. Needs synapse >= 3.14.2 and jq.
set -euo pipefail

port="${1:?usage: scripts/org-5-move.sh PORT [--dry-run] [--only-ten]}"
shift
dry=0
only_ten=0
for a in "$@"; do
  case "$a" in
    --dry-run) dry=1 ;;
    --only-ten) only_ten=1 ;;
    *) echo "org-5-move: unknown option $a" >&2; exit 2 ;;
  esac
done

# old id → the shortcode it roots. Inner scopes before the scopes around them
# in one shortcode (HYPERFORGE-7 and -13 sit under HYPERFORGE-17): a move
# with its children carries only its own shortcode's tickets, and a scope
# among them would land inside the new one.
moves=(
  "MENGER-2 APP"
  "MENGER-3 STORE"
  "MENGER-4 TICKETS"
  "MENGER-5 RUN"
  "DEPLOY-9 SHIPS"
  "AUTH-14 WORKOS"
  "AUTH-23 ACCOUNT"
  "IDENTITY-33 LINKED"
  "IDENTITY-40 ATTEST"
  "RETURNS-37 PRECISE"
)
# The four that appeared after ORG-5 was filed (its input `more`, on the
# recommended answer).
if [ "$only_ten" = 0 ]; then
  moves+=(
    "DESIGN-32 WALL"
    "HYPERFORGE-7 DRIVERS"
    "HYPERFORGE-13 LOGIN"
    "HYPERFORGE-17 SURFACE"
  )
fi

syn() { synapse -P "$port" -j menger tickets "$@"; }

moved=0
skipped=0
for m in "${moves[@]}"; do
  read -r old code <<<"$m"
  now=$(syn get --id "$old" | jq -c 'select(.content.ticket) | .content.ticket | {id, epic, kind}' | head -1)
  if [ -z "$now" ]; then
    echo "org-5-move: $old names no ticket on :$port — stopping" >&2
    exit 1
  fi
  id=$(jq -r .id <<<"$now")
  epic=$(jq -r .epic <<<"$now")
  if [ "$epic" = "$code" ]; then
    echo "$old → $id (already moved)"
    skipped=$((skipped + 1))
    continue
  fi
  if [ "$id" != "$old" ] || [ "$(jq -r .kind <<<"$now")" != "scope" ]; then
    echo "org-5-move: $old is $id, a $(jq -r .kind <<<"$now") in $epic — not the scope this script moves; stopping" >&2
    exit 1
  fi
  if [ "$dry" = 1 ]; then
    echo "$old → $code-1 (dry run: with its tree in $epic)"
    continue
  fi
  out=$(syn reshortcode --ids "$old" --to "$code" --with-children true --message "ORG-5: a scope under a scope roots its own shortcode")
  if ! jq -e 'select(.content.type == "reshortcoded" or .type == "reshortcoded")' <<<"$out" >/dev/null 2>&1 \
     && ! jq -e 'select(.content.moves)' <<<"$out" >/dev/null 2>&1; then
    echo "org-5-move: $old → $code refused:" >&2
    echo "$out" | jq -r '.content.message // .' >&2
    exit 1
  fi
  n=$(jq -s '[.[] | .content.moves // empty | length] | add' <<<"$out")
  root=$(jq -rs '[.[] | .content.moves // empty | .[0].to] | first' <<<"$out")
  echo "$old → $root ($n tickets moved)"
  moved=$((moved + 1))
done
echo "org-5-move: $moved moved, $skipped already moved, on :$port"
