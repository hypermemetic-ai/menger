#!/usr/bin/env bash
# UI-5: the compiler as a wasm module with no imports → dist/ui-compiler.wasm.
# Needs only the wasm32-unknown-unknown target (rustup target add
# wasm32-unknown-unknown). The crate stays an rlib in its manifest; the
# cdylib is asked for here, so a native build makes no shared library.
# `--stack-first` puts the stack below the data, so an overflow traps
# instead of writing over it. The build fails if the module imports anything.
# FRAME-3: the compiler is ../plexus-ui's (plexus-ui-compiler), built by
# --manifest-path into this checkout's target ($CARGO_TARGET_DIR, else ./target).
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo="${CARGO:-cargo}"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
"$cargo" rustc --manifest-path "$root/../plexus-ui/compiler/Cargo.toml" --target-dir "$target_dir" --lib --release \
  --target wasm32-unknown-unknown --features wasm --crate-type cdylib \
  -- -C link-arg=--stack-first
mkdir -p "$root/dist"
cp "$target_dir/wasm32-unknown-unknown/release/plexus_ui_compiler.wasm" "$root/dist/ui-compiler.wasm"
python3 -I - "$root/dist/ui-compiler.wasm" <<'PY'
import sys
b = open(sys.argv[1], "rb").read()
assert b[:8] == b"\0asm\x01\0\0\0", "not a wasm module"
def leb(at):
    v = s = 0
    while True:
        x = b[at]; at += 1; v |= (x & 0x7f) << s
        if x < 0x80: return v, at
        s += 7
at = 8
while at < len(b):
    sid = b[at]; size, at = leb(at + 1)
    if sid == 2 and leb(at)[0] != 0:
        sys.exit(f"{sys.argv[1]}: the module imports {leb(at)[0]} item(s); it must import nothing")
    at += size
PY
echo "$root/dist/ui-compiler.wasm"
