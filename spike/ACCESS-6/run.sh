#!/usr/bin/env bash
# ACCESS-6: the spike's evidence, end to end. Builds the server (and the
# wasm32 client half), starts it on 4670 (ws only — 4671/4672 stay free),
# drives it with synapse and raw frames, runs the Rust client tests against
# it, and stops it. `SCHEMA=anyof|examples` builds Status's open schemas
# instead of the closed enum. `CODEGEN=1` also runs synapse-cc in the
# plexus-codegen container into $OUT/gen.
#
#   CARGO_TARGET_DIR  honoured (default ~/.cache/cargo-target/access6-spike)
#   OUT               where the transcript and generated code go (default ./out, ignored)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/cargo-target/access6-spike}"
out="${OUT:-$here/out}"
mkdir -p "$out"
port=4670
features=()
case "${SCHEMA:-enum}" in
  enum) ;;
  anyof) features=(--features schema-anyof) ;;
  examples) features=(--features schema-examples) ;;
  *) echo "SCHEMA must be enum, anyof or examples" >&2; exit 2 ;;
esac
(exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && { echo "something listens on :$port" >&2; exit 1; }

cargo build -q "${features[@]}" || exit 1
cargo build -q --lib --target wasm32-unknown-unknown --no-default-features && echo "wasm32 client half: built"
"$CARGO_TARGET_DIR/debug/access6-spike" >"$out/server.log" 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null' EXIT
for _ in $(seq 50); do (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.1; done

run() { printf '\n$ synapse %s\n' "$*"; synapse "$@" 2>&1; }
A=(--cookie access_token=spike-alice)
M=(--cookie access_token=spike-mallory)
{
  run -P $port -s spike tickets
  run -P $port spike tickets
  run -P $port spike tickets set_status --help
  run -P $port spike tickets inputs answer --help
  # good calls
  run -P $port -j "${A[@]}" spike tickets get --id mng-1
  run -P $port -j "${A[@]}" spike tickets inputs answer --ticket ACCESS-6 --value Coding
  # the lenient enum, as a flag and through -p
  run -P $port -j "${A[@]}" spike tickets set_status --id MNG-1 --status active
  run -P $port -j "${A[@]}" spike tickets set_status --id MNG-1 --status ready
  run -P $port -j "${A[@]}" -p '{"id":"mng-1","status":"active"}' spike tickets set_status
  # bad params
  run -P $port -j "${A[@]}" spike tickets set_status --id not_an_id --status Ready
  run -P $port -j "${A[@]}" -p '{"id":"MNG-1","status":"wobbly"}' spike tickets set_status
  run -P $port -j "${A[@]}" -p '{"id":"MNG-1"}' spike tickets set_status
  # anonymous; anonymous with a bad param; authenticated but refused
  run -P $port -j spike tickets get --id MNG-1
  run -P $port -j spike tickets set_status --id not_an_id --status Ready
  run -P $port -j spike tickets inputs list --ticket MNG-1
  run -P $port -j "${M[@]}" spike tickets get --id MNG-1
  run -P $port -j "${M[@]}" spike tickets inputs list --ticket MNG-1
  run -P $port -j "${M[@]}" spike tickets set_status --id not_an_id --status Ready
  # the raw frames
  printf '\n$ node raw.mjs …\n'
  node raw.mjs - tickets.get '{"id":"MNG-1"}'
  node raw.mjs spike-mallory tickets.inputs.list '{"ticket":"MNG-1"}'
  node raw.mjs spike-alice tickets.set_status '{"id":"x","status":"Ready"}'
  node raw.mjs spike-alice tickets.set_status '{"id":"MNG-1","status":"active"}'
  printf '\n$ cargo test --test client -- --include-ignored --nocapture\n'
  cargo test -q "${features[@]}" --test client -- --include-ignored --nocapture 2>&1 | grep -Ev '^(warning|\s*[|=]|\s*-->|\s*[0-9]+ \||$)'
} | tee "$out/transcript-${SCHEMA:-enum}.txt"

if [ "${CODEGEN:-0}" = 1 ]; then
  gen="$out/gen-${SCHEMA:-enum}"
  rm -rf "$gen"; mkdir -p "$gen"
  cat >"$gen/synapse.config.json" <<JSON
{ "schema": "1.0", "language": "typescript", "backend": "spike", "url": "ws://127.0.0.1:$port",
  "packageManager": "npm",
  "targets": { "sdk": { "generate": ["transport", "rpc", "plugins"], "transport": "browser", "outputDir": "generated" } } }
JSON
  docker run --rm --network host --security-opt label=disable --user "$(id -u):$(id -g)" \
    -v "$gen:/work" -w /work "${PLEXUS_CODEGEN_IMAGE:-plexus-codegen:latest}" \
    sh -euc 'synapse-cc --version; synapse-cc build' 2>&1 | tee "$out/codegen-${SCHEMA:-enum}.txt"
  rm -rf "$gen/generated/node_modules"
fi
