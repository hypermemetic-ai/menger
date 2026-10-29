#!/usr/bin/env bash
# V2-7: the UI's bundle — dist/ui.tar, what `deploy/pipeline build` puts in a
# release and the image, and what a deploy pushes through `ui.push` (UI-3).
#
# A STUB until UI-6 lands the engine's build: the placeholder page
# (src/shell.rs's PLACEHOLDER, read from the source so the two never drift) as
# the tar's index.html, and beside it the bundle's manifest.json (UI-4's
# contract: abi "engine/1", contract "unknown" — no tickets service was asked —
# and no routes, components or slots). The pipeline pushes that manifest as
# `ui.push`'s `manifest`. The tar is reproducible — names
# sorted, mtime 0, owner 0 — so the same tree gives the same hash, the same
# version to `ui.push`. Runs in the plexus-codegen container (the pipeline's
# build) or on any machine with bash, sed and GNU tar.
#
#   scripts/ui-build.sh      writes dist/ui.tar at the repository's root
set -euo pipefail
cd "$(dirname "$0")/.."

page=$(sed -n '/^pub const PLACEHOLDER: &str = concat!(/,/^);/p' src/shell.rs \
  | sed -n 's/^ *"\(.*\)",$/\1/p' | sed 's/\\"/"/g' | tr -d '\n')
[[ $page == "<!doctype html>"* ]] || { echo "ui-build: no PLACEHOLDER page in src/shell.rs" >&2; exit 1; }

stage=$(mktemp -d); trap 'rm -rf "$stage"' EXIT
printf '%s\n' "$page" >"$stage/index.html"
printf '%s\n' '{"abi":"engine/1","contract":"unknown","routes":[],"components":[],"slots":{}}' >"$stage/manifest.json"
mkdir -p dist
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu \
  -C "$stage" -cf dist/ui.tar.part index.html manifest.json
mv dist/ui.tar.part dist/ui.tar
echo "ui-build: dist/ui.tar (the placeholder page — a stub until UI-6)" >&2
