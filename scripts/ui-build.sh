#!/usr/bin/env bash
# UI-6: ui/ → dist/ui.tar, the bundle of UI-4's contract (what V2-7's
# `deploy/pipeline build` puts in a release and pushes through `ui.push`):
#
#   index.html                 ui/index.html (the shell stamps it; nothing is stamped here)
#   engine.js  engine.wasm     the engine (ui-engine/, abi engine/1)
#   manifest.json              abi, contract, commit, routes (ui/routes.json),
#                              slots (ui/slots.json), components (hashes, uses)
#   components/<name>.json     each ui/src/<name>.svelte compiled by UI-5's
#   components/<name>.rhai     `ui-compiler` (ui/golden/<name>.* only when no compiler can be had)
#   src/<name>.svelte          the sources, kept
#
# Two halves, so the pipeline can run the second in its plexus-codegen
# container (node, tar; no cargo):
#
#   --prebuild   what needs cargo, on a machine with it: the engine
#                (`cargo build --target wasm32-unknown-unknown --features dom`,
#                then wasm-bindgen-cli `--target web` at the version Cargo.lock
#                pins — from PATH, else installed once with cargo into
#                ~/.cache/menger-tools) and the components compiled
#                (`cargo build -p menger-ui-compiler`), into the staging
#                directory beside the tar (dist/ui.d); then it stops.
#   (default)    the bundle: whatever the staging directory lacks is built
#                (cargo when there is one, else refused naming --prebuild),
#                the manifest written (node), the tar made — the same inputs,
#                the same bytes (names in the contract's order, mtime 0,
#                owner 0).
#
# A checkout with no ui/ (before UI-6's sources) gets V2-7's placeholder
# bundle: src/shell.rs's PLACEHOLDER as index.html beside an empty manifest.
#
# Usage: scripts/ui-build.sh [--prebuild] [--ui DIR] [-o|--out FILE]
#                            [--contract-from PORT] [--engine DIR]
#   --engine DIR   take engine.js / engine.wasm from DIR (built already)
#   --contract-from PORT   manifest.contract from the service on PORT: the tickets
#                          service's hash (tickets.schema's), what ui.push compares
#                          (UI-11); without it, "unknown", which the push answers null
# Everything cargo builds goes to $CARGO_TARGET_DIR (else ./target).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
ui=$root/ui
out=$root/dist/ui.tar
contract_from=
engine=
prebuild=
while [ $# -gt 0 ]; do
  case "$1" in
    --prebuild) prebuild=1; shift ;;
    --ui) ui=$(cd "$2" && pwd); shift 2 ;;
    -o|--out) out=$2; shift 2 ;;
    --contract-from) contract_from=$2; shift 2 ;;
    --engine) engine=$(cd "$2" && pwd); shift 2 ;;
    -h|--help) sed -n '2,38p' "$0"; exit 0 ;;
    *) echo "ui-build: unknown argument $1" >&2; exit 2 ;;
  esac
done

mkdir -p "$(dirname "$out")"
out=$(cd "$(dirname "$out")" && pwd)/$(basename "$out")

# ── no ui/: V2-7's placeholder bundle ─────────────────────────────────────
if [ ! -d "$ui/src" ]; then
  [ -n "$prebuild" ] && { echo "ui-build: no $ui/src — nothing to prebuild" >&2; exit 0; }
  page=$(sed -n '/^pub const PLACEHOLDER: &str = concat!(/,/^);/p' "$root/src/shell.rs" \
    | sed -n 's/^ *"\(.*\)",$/\1/p' | sed 's/\\"/"/g' | tr -d '\n')
  [[ $page == "<!doctype html>"* ]] || { echo "ui-build: no PLACEHOLDER page in src/shell.rs" >&2; exit 1; }
  stage=$(mktemp -d); trap 'rm -rf "$stage"' EXIT
  printf '%s\n' "$page" >"$stage/index.html"
  printf '%s\n' '{"abi":"engine/1","contract":"unknown","routes":[],"components":[],"slots":{}}' >"$stage/manifest.json"
  tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu -C "$stage" -cf "$out.part" index.html manifest.json
  mv "$out.part" "$out"
  echo "ui-build: $out (the placeholder page: no ui/src here)" >&2
  exit 0
fi

# The staging directory beside the tar; overwritten file by file, and the
# tar takes only the files listed below, so nothing stale rides along.
stage=${out%.tar}.d
mkdir -p "$stage/components" "$stage/src"
target=${CARGO_TARGET_DIR:-$root/target}
have_cargo() { command -v cargo >/dev/null 2>&1; }
names=()
for src in "$ui"/src/*.svelte; do names+=("$(basename "$src" .svelte)"); done

# ── the engine ────────────────────────────────────────────────────────────
build_engine() {
  # Small: opt-level z, LTO, one codegen unit, stripped — for this build
  # alone (the workspace's release profile is the server's).
  (cd "$root" && cargo build -p menger-ui-engine --lib --release --target wasm32-unknown-unknown --features dom \
    --config 'profile.release.opt-level="z"' --config 'profile.release.lto=true' \
    --config 'profile.release.codegen-units=1' --config 'profile.release.strip=true')
  local want bindgen tools
  want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$root/Cargo.lock")
  if command -v wasm-bindgen >/dev/null && wasm-bindgen --version | grep -q " $want\$"; then
    bindgen=$(command -v wasm-bindgen)
  else
    tools=$HOME/.cache/menger-tools/wasm-bindgen-$want
    [ -x "$tools/bin/wasm-bindgen" ] || cargo install wasm-bindgen-cli --version "$want" --locked --root "$tools"
    bindgen=$tools/bin/wasm-bindgen
  fi
  "$bindgen" --target web --no-typescript --out-name engine --out-dir "$stage" \
    "$target/wasm32-unknown-unknown/release/menger_ui_engine.wasm"
  mv -f "$stage/engine_bg.wasm" "$stage/engine.wasm"
}
if [ -n "$engine" ]; then
  cp "$engine/engine.js" "$engine/engine.wasm" "$stage/"
elif have_cargo; then
  build_engine
elif [ ! -s "$stage/engine.js" ] || [ ! -s "$stage/engine.wasm" ]; then
  echo "ui-build: no cargo here to build the engine — run scripts/ui-build.sh --prebuild where there is one" >&2
  exit 1
fi

# ── the components ────────────────────────────────────────────────────────
compiler=${UI_COMPILER:-$(command -v ui-compiler || true)}
if [ -z "$compiler" ] && have_cargo && [ -d "$root/ui-compiler" ]; then
  (cd "$root" && cargo build -p menger-ui-compiler --release --bin ui-compiler)
  compiler=$target/release/ui-compiler
fi
for name in "${names[@]}"; do
  if [ -n "$compiler" ]; then
    "$compiler" "$ui/src/$name.svelte" -o "$stage/components"
  elif [ -s "$stage/components/$name.json" ] && [ "$stage/components/$name.json" -nt "$ui/src/$name.svelte" ]; then
    : # compiled by --prebuild
  elif [ -f "$ui/golden/$name.json" ]; then
    echo "ui-build: no ui-compiler: $name from ui/golden/" >&2
    cp "$ui/golden/$name.json" "$stage/components/$name.json"
    cp "$ui/golden/$name.rhai" "$stage/components/$name.rhai"
  else
    echo "ui-build: no ui-compiler and no ui/golden/$name.json" >&2
    exit 1
  fi
done
if [ -n "$prebuild" ]; then
  # The container that writes the tar has no git: the commit rides along.
  git -C "$root" rev-parse --short HEAD >"$stage/commit" 2>/dev/null || true
  echo "ui-build: prebuilt into $stage" >&2
  exit 0
fi

# ── the manifest ──────────────────────────────────────────────────────────
for name in "${names[@]}"; do cp "$ui/src/$name.svelte" "$stage/src/$name.svelte"; done
cp "$ui/index.html" "$stage/index.html"
contract=unknown
if [ -n "$contract_from" ]; then
  contract=$(synapse -P "$contract_from" -j menger tickets schema | node -e '
    let t = ""; process.stdin.on("data", (d) => (t += d)).on("end", () => {
      for (const l of t.split("\n").filter(Boolean)) {
        const h = JSON.parse(l).content?.hash;
        if (h) { process.stdout.write(String(h)); return; }
      }
    });')
  [ -n "$contract" ] || { echo "ui-build: no tickets.schema hash from the service on :$contract_from" >&2; exit 1; }
fi
commit="menger@$(git -C "$root" rev-parse --short HEAD 2>/dev/null || cat "$stage/commit" 2>/dev/null || echo unknown)"
UI_DIR=$ui STAGE=$stage CONTRACT=$contract COMMIT=$commit NAMES="${names[*]}" node -e '
  const fs = require("fs"), crypto = require("crypto"), path = require("path");
  const { UI_DIR, STAGE, CONTRACT, COMMIT, NAMES } = process.env;
  const sha = (f) => crypto.createHash("sha256").update(fs.readFileSync(f)).digest("hex");
  const read = (f, dflt) => (fs.existsSync(f) ? JSON.parse(fs.readFileSync(f, "utf8")) : dflt);
  const components = {};
  for (const name of NAMES.split(" ").filter(Boolean).sort()) {
    const tree = path.join(STAGE, "components", name + ".json");
    components[name] = { tree: sha(tree), handlers: sha(path.join(STAGE, "components", name + ".rhai")), uses: JSON.parse(fs.readFileSync(tree, "utf8")).uses ?? [] };
  }
  const manifest = { abi: "engine/1", contract: CONTRACT, commit: COMMIT, routes: read(path.join(UI_DIR, "routes.json"), {}), slots: read(path.join(UI_DIR, "slots.json"), {}), components };
  fs.writeFileSync(path.join(STAGE, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
'

# ── the tar ───────────────────────────────────────────────────────────────
files=(index.html engine.js engine.wasm manifest.json)
for name in "${names[@]}"; do
  files+=("components/$name.json" "components/$name.rhai" "src/$name.svelte")
done
tar --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu -C "$stage" -cf "$out.part" "${files[@]}"
mv "$out.part" "$out"
echo "$out"
