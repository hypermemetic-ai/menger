#!/usr/bin/env bash
# UI-5: the compiler as a wasm module with no imports → dist/ui-compiler.wasm.
# Needs only the wasm32-unknown-unknown target (rustup target add
# wasm32-unknown-unknown). The crate stays an rlib in its manifest; the
# cdylib is asked for here, so a native build makes no shared library.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo="${CARGO:-cargo}"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
"$cargo" rustc --manifest-path "$root/ui-compiler/Cargo.toml" --lib --release \
  --target wasm32-unknown-unknown --features wasm --crate-type cdylib
mkdir -p "$root/dist"
cp "$target_dir/wasm32-unknown-unknown/release/menger_ui_compiler.wasm" "$root/dist/ui-compiler.wasm"
echo "$root/dist/ui-compiler.wasm"
