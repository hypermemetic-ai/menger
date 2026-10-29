#!/usr/bin/env bash
# UI-6: ui/ → dist/ui.tar, the bundle of UI-4's contract:
#
#   index.html                 ui/index.html (the shell stamps it; nothing is stamped here)
#   engine.js  engine.wasm     the engine (ui-engine/, abi engine/1)
#   manifest.json              abi, contract, commit, routes (ui/routes.json),
#                              slots (ui/slots.json), components (hashes, uses)
#   components/<name>.json     each ui/src/<name>.svelte compiled by `ui-compiler`
#   components/<name>.rhai     (UI-5); until it lands, ui/golden/<name>.* as checked in
#   src/<name>.svelte          the sources, kept
#
# The engine: `cargo build --target wasm32-unknown-unknown --features dom`,
# then wasm-bindgen-cli (`--target web`) at the version Cargo.lock pins —
# from PATH, else trunk's cache (~/.cache/trunk/wasm-bindgen-<v>), else
# installed once with cargo into ~/.cache/menger-tools.
#
# Usage: scripts/ui-build.sh [--ui DIR] [-o|--out FILE] [--contract-from PORT]
#                            [--engine DIR]   (reuse a built engine.js / engine.wasm)
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
ui=$root/ui
out=$root/dist/ui.tar
contract_from=
engine=
while [ $# -gt 0 ]; do
  case "$1" in
    --ui) ui=$(cd "$2" && pwd); shift 2 ;;
    -o|--out) out=$2; shift 2 ;;
    --contract-from) contract_from=$2; shift 2 ;;
    --engine) engine=$(cd "$2" && pwd); shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "ui-build: unknown argument $1" >&2; exit 2 ;;
  esac
done

mkdir -p "$(dirname "$out")"
out=$(cd "$(dirname "$out")" && pwd)/$(basename "$out")
# The staging directory beside the tar; overwritten file by file, and the
# tar takes only the files listed below, so nothing stale rides along.
stage=${out%.tar}.d
mkdir -p "$stage/components" "$stage/src"

# ── the engine ────────────────────────────────────────────────────────────
if [ -n "$engine" ]; then
  cp "$engine/engine.js" "$engine/engine.wasm" "$stage/"
else
  target=${CARGO_TARGET_DIR:-$root/target}
  (cd "$root" && cargo build -p menger-ui-engine --lib --release --target wasm32-unknown-unknown --features dom)
  want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$root/Cargo.lock")
  bindgen=
  if command -v wasm-bindgen >/dev/null && wasm-bindgen --version | grep -q " $want\$"; then
    bindgen=$(command -v wasm-bindgen)
  elif [ -x "$HOME/.cache/trunk/wasm-bindgen-$want/wasm-bindgen" ]; then
    bindgen=$HOME/.cache/trunk/wasm-bindgen-$want/wasm-bindgen
  else
    tools=$HOME/.cache/menger-tools/wasm-bindgen-$want
    [ -x "$tools/bin/wasm-bindgen" ] || cargo install wasm-bindgen-cli --version "$want" --locked --root "$tools"
    bindgen=$tools/bin/wasm-bindgen
  fi
  "$bindgen" --target web --no-typescript --out-name engine --out-dir "$stage" \
    "$target/wasm32-unknown-unknown/release/menger_ui_engine.wasm"
  mv -f "$stage/engine_bg.wasm" "$stage/engine.wasm"
fi

# ── the components ────────────────────────────────────────────────────────
compiler=${UI_COMPILER:-$(command -v ui-compiler || true)}
names=()
for src in "$ui"/src/*.svelte; do
  name=$(basename "$src" .svelte)
  names+=("$name")
  cp "$src" "$stage/src/$name.svelte"
  if [ -n "$compiler" ]; then
    "$compiler" "$src" -o "$stage/components"
  else
    [ -f "$ui/golden/$name.json" ] || { echo "ui-build: no ui-compiler and no golden tree for $name (ui/golden/$name.json)" >&2; exit 1; }
    cp "$ui/golden/$name.json" "$stage/components/$name.json"
    cp "$ui/golden/$name.rhai" "$stage/components/$name.rhai"
  fi
done

# ── the manifest ──────────────────────────────────────────────────────────
contract=unknown
if [ -n "$contract_from" ]; then
  contract=$(synapse -P "$contract_from" -j menger hash | jq -r 'select(.type == "data") | .content | (.hash // .value // .) | strings' | head -1)
  [ -n "$contract" ] || { echo "ui-build: no hash from the service on :$contract_from" >&2; exit 1; }
fi
commit="menger@$(git -C "$root" rev-parse --short HEAD 2>/dev/null || echo unknown)"
components='{}'
for name in "${names[@]}"; do
  tree=$(sha256sum < "$stage/components/$name.json" | cut -d' ' -f1)
  handlers=$(sha256sum < "$stage/components/$name.rhai" | cut -d' ' -f1)
  uses=$(jq -c '.uses' "$stage/components/$name.json")
  components=$(jq -c --arg n "$name" --arg t "$tree" --arg h "$handlers" --argjson u "$uses" '. + {($n): {tree: $t, handlers: $h, uses: $u}}' <<<"$components")
done
slots='{}'
[ -f "$ui/slots.json" ] && slots=$(jq -c . "$ui/slots.json")
jq -n --arg abi "engine/1" --arg contract "$contract" --arg commit "$commit" \
  --argjson routes "$(jq -c . "$ui/routes.json")" --argjson slots "$slots" --argjson components "$components" \
  '{abi: $abi, contract: $contract, commit: $commit, routes: $routes, slots: $slots, components: $components}' \
  > "$stage/manifest.json"
cp "$ui/index.html" "$stage/index.html"

# ── the tar: the same inputs give the same bytes ─────────────────────────
files=(index.html engine.js engine.wasm manifest.json)
for name in "${names[@]}"; do
  files+=("components/$name.json" "components/$name.rhai" "src/$name.svelte")
done
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu -C "$stage" -cf "$out" "${files[@]}"
echo "$out"
