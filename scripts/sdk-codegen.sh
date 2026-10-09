#!/usr/bin/env bash
# COMP-10: generate the SDK's typed client — `menger.tickets.*` — from the
# `tickets` activation's schema with the house TypeScript generator
# (synapse-cc → hub-codegen), in the plexus-codegen container. The one
# definition of how it is done; ../plexus-starter/scripts/codegen.sh is the
# pattern it follows.
#
#  1. Builds menger and starts a throwaway server on its own ports over a
#     temporary database — SDK_CODEGEN_PORT (default 4466) for the
#     WebSocket, +1 the app, +2 REST; never 4450–4462 (the deployed
#     environments and the demo server) — stopped and deleted when done.
#  2. In the image: `synapse-cc build` reads the schema and hub-codegen
#     writes the TypeScript client, installs its dependencies, compiles it
#     and smoke-tests it against that server; tsc emits the client's
#     declarations (.d.ts) and typechecks the bridge transport
#     (frontend/sdk/bridge.ts) and the SDK's own types
#     (frontend/sdk/menger.d.ts) against it; bun bundles the bridge
#     transport with the client (IIFE).
#  3. Writes the snapshot, frontend/sdk/generated/ — the client's sources
#     and declarations as generated, a directory per activation on the hub
#     (`tickets/`, and since COMMENT-2 `comments/`), `manifest.json` (each
#     activation's methods and their parameter names, from the IR: what the
#     drift guard, tickets-api/tests/sdk_manifest.rs, holds the activations
#     to) and `sdk-bundle.js` — and splices the bundle into
#     frontend/sdk/menger-sdk.js between its `@generated-begin` /
#     `@generated-end` lines. The bundle is the bridge's: `menger.tickets.*`
#     (a frame's SDK reaches tickets; comments are the app's own).
#
# All of it is committed, so `cargo build`, `trunk build` and the tests need
# neither Docker nor Node. Run it whenever the activation's surface changes
# (a method, a parameter, an event or a type); the drift guard fails
# `cargo test --workspace` until it is. A second run with nothing changed
# changes nothing.
#
#   PLEXUS_CODEGEN_IMAGE   the image (default plexus-codegen:latest — built
#                          by ../dockerfiles/plexus-codegen/build.sh)
#   SDK_CODEGEN_PORT       the throwaway server's WebSocket port (default 4466)
#   CARGO_TARGET_DIR       honoured when locating the built server
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
image="${PLEXUS_CODEGEN_IMAGE:-plexus-codegen:latest}"
port="${SDK_CODEGEN_PORT:-4466}"
sdk="$root/frontend/sdk"
out="$sdk/generated"

say() { printf 'sdk-codegen: %s\n' "$*" >&2; }
die() { say "$*"; exit 1; }

# --- the ports -----------------------------------------------------------
case "$port" in ''|*[!0-9]*) die "SDK_CODEGEN_PORT must be a port number, not '$port'" ;; esac
if [ "$port" -le 4462 ] && [ $((port + 2)) -ge 4450 ]; then
  die "ports $port–$((port + 2)) overlap 4450–4462 (menger's deployed environments and the demo server) — pick others with SDK_CODEGEN_PORT"
fi
answers() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
for p in "$port" $((port + 1)) $((port + 2)); do
  answers "$p" && die "something already listens on :$p — this script starts its own server; pick other ports with SDK_CODEGEN_PORT"
done

# --- the image -----------------------------------------------------------
command -v docker >/dev/null || die "docker is not installed — codegen runs in a container"
docker image inspect "$image" >/dev/null 2>&1 || die "image $image not found. Build it from the hypermemetic checkouts:
    $root/../dockerfiles/plexus-codegen/build.sh
  or point PLEXUS_CODEGEN_IMAGE at one you have."

# --- the throwaway server ------------------------------------------------
work="$(mktemp -d "${TMPDIR:-/tmp}/menger-sdk-codegen.XXXXXX")"
pid=""
cleanup() {
  if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
  rm -rf "$work"
}
trap cleanup EXIT
mkdir -p "$work/db" "$work/gen"

say "building menger"
cargo build --quiet --manifest-path "$root/Cargo.toml" --bin menger
bin="${CARGO_TARGET_DIR:-$root/target}/debug/menger"
say "starting a throwaway menger on :$port (database in $work/db)"
PLEXUS_NO_REGISTRY=1 "$bin" --port "$port" --shell-port $((port + 1)) --rest-port $((port + 2)) \
  --db-path "$work/db/menger.db" --app-dist "$work/no-dist" >"$work/server.log" 2>&1 &
pid=$!
for _ in $(seq 1 100); do
  answers "$port" && break
  kill -0 "$pid" 2>/dev/null || { cat "$work/server.log" >&2; die "the server exited before it listened"; }
  sleep 0.1
done
answers "$port" || { cat "$work/server.log" >&2; die "the server did not listen on :$port"; }

# --- what the container works on -----------------------------------------
gen="$work/gen"
cp "$sdk/bridge.ts" "$sdk/menger.d.ts" "$sdk/menger-sdk.js" "$gen/"
cat >"$gen/synapse.config.json" <<EOF
{
  "schema": "1.0",
  "language": "typescript",
  "backend": "menger",
  "url": "ws://127.0.0.1:$port",
  "packageManager": "npm",
  "targets": {
    "sdk": {
      "generate": ["transport", "rpc", "plugins"],
      "transport": "browser",
      "outputDir": "generated"
    }
  }
}
EOF
# The SDK's own types, checked against the generated ones: menger.d.ts
# resolves, and a component script written against it typechecks.
cat >"$gen/check.ts" <<'EOF'
import type { Answer, Input, MengerElement, WireEvent } from './menger';
export async function script(this: MengerElement): Promise<number> {
  let n = 0;
  for await (const ev of this.menger.tickets.waves(['RETURNS'])) {
    if (ev.type === 'wave_counts') n = ev.waves[0] ?? 0;
  }
  const raw: WireEvent[] = await menger.call('waves', { epics: ['RETURNS'] });
  const off: () => void = menger.on('change', (e: WireEvent) => void e.type);
  off();
  menger.navigate('#/components/m-wave-summary');
  const v: string = window.menger.version;
  // INPUT-4: this ticket's inputs, read typed, and the first write.
  const ticket: string | null = this.menger.input.ticket;
  // (epic, filter, state, ticket): the generated client takes them in name order.
  for await (const ev of menger.tickets.inputs(undefined, undefined, 'open', ticket)) {
    if (ev.type === 'input') n += ev.input.history.length + (ev.input.answer?.atUnix ?? 0);
  }
  const answer: Answer = await menger.input.submit('landing', { type: 'choice', option: 'archive' }, 'holds are real');
  const multi: Answer = await this.menger.input.submit('checks', { type: 'multi', options: ['unit'] });
  n += answer.atUnix + multi.atUnix + (answer.source === 'app' ? 1 : 0);
  // INPUT-9: the second write — the input as the clear left it.
  const cleared: Input = await this.menger.input.clear('landing');
  n += cleared.history.length + (cleared.history[0]?.atUnix ?? 0) + (cleared.state === 'open' ? 1 : 0);
  return n + raw.length + v.length + (this.attr('board') ?? '').length + this.root.childElementCount;
}
EOF
cat >"$gen/tsconfig.check.json" <<'EOF'
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "lib": ["ES2022", "DOM", "DOM.AsyncIterable"],
    "types": []
  },
  "files": ["bridge.ts", "menger.d.ts", "check.ts"]
}
EOF
cat >"$gen/finish.mjs" <<'EOF'
// The manifest (from the IR the generator read) and the splice.
import { readFileSync, writeFileSync } from 'node:fs';
const ir = JSON.parse(readFileSync('generated/ir.json', 'utf8'));
// Each activation's methods (the hub's own, namespace '', are not one).
const activations = {};
for (const m of Object.values(ir.irMethods)) {
  if (!m.mdNamespace) continue;
  (activations[m.mdNamespace] ??= {})[m.mdName] = m.mdParams.map((p) => p.pdName).sort();
}
const sorted = (o) => Object.fromEntries(Object.keys(o).sort().map((k) => [k, o[k]]));
const manifest = {
  comment:
    'COMP-10: written by scripts/sdk-codegen.sh from the IR hub-codegen generated this client from — each ' +
    'activation on the hub (COMMENT-2: comments beside tickets), its methods and their parameter names. ' +
    'tickets-api/tests/sdk_manifest.rs holds the activations to it. Do not edit: run the script.',
  activations: sorted(Object.fromEntries(Object.entries(activations).map(([ns, ms]) => [ns, sorted(ms)]))),
};
if (!manifest.activations.tickets) throw new Error('the IR holds no tickets activation');
writeFileSync('manifest.json', JSON.stringify(manifest, null, 2) + '\n');

const bundle = readFileSync('bundle.js', 'utf8');
const lower = bundle.toLowerCase();
if (lower.includes('</script') || lower.includes('<!--')) throw new Error('the bundle cannot be inlined into a <script>');
if (!bundle.includes('mengerTyped =')) throw new Error('the bundle does not assign mengerTyped');
const BEGIN = '  // @generated-begin scripts/sdk-codegen.sh\n';
const END = '  // @generated-end\n';
const sdk = readFileSync('menger-sdk.js', 'utf8');
const a = sdk.indexOf(BEGIN);
const b = sdk.indexOf(END);
if (a < 0 || b < a || sdk.indexOf(BEGIN, a + 1) >= 0) throw new Error('menger-sdk.js: one @generated-begin … @generated-end region expected');
writeFileSync('menger-sdk.js', sdk.slice(0, a + BEGIN.length) + bundle + sdk.slice(b));
EOF

# --- generate, in the container ------------------------------------------
# --network host: the container reaches the throwaway server on loopback.
# --user: what it writes is yours (HOME is /tmp inside). label=disable: on
# an SELinux host a bind mount is unreadable to the container otherwise.
say "synapse-cc build, tsc, bun build (in $image)"
started=$(date +%s)
docker run --rm --network host \
  --security-opt label=disable \
  --user "$(id -u):$(id -g)" \
  -v "$gen:/work" -w /work \
  "$image" sh -euc '
    synapse-cc build >&2
    tsc=/work/generated/node_modules/.bin/tsc
    (cd generated && "$tsc" -p tsconfig.json --noEmit false --declaration --emitDeclarationOnly --outDir ../decl)
    "$tsc" -p tsconfig.check.json
    bun build bridge.ts --format=iife --target=browser --outfile=bundle.js >&2
    node finish.mjs
  '
say "generated in $(( $(date +%s) - started ))s"

# --- the snapshot --------------------------------------------------------
rm -rf "$out"
mkdir -p "$out"
for f in index.ts rpc.ts types.ts transport.ts package.json; do
  cp "$gen/generated/$f" "$out/$f"
done
# COMMENT-2: every activation's module — a directory holding a client.
for dir in "$gen"/generated/*/; do
  ns="$(basename "$dir")"
  [ -f "$dir/client.ts" ] || continue
  mkdir -p "$out/$ns"
  for f in index.ts client.ts types.ts; do
    cp "$dir/$f" "$out/$ns/$f"
  done
done
(cd "$gen/decl" && find . -name '*.d.ts' -print) | while read -r f; do
  mkdir -p "$out/$(dirname "$f")"
  cp "$gen/decl/$f" "$out/$f"
done
cp "$gen/manifest.json" "$out/manifest.json"
cp "$gen/bundle.js" "$out/sdk-bundle.js"
cp "$gen/menger-sdk.js" "$sdk/menger-sdk.js"
say "wrote frontend/sdk/generated/ ($(find "$out" -type f | wc -l) files) and the GENERATED region of frontend/sdk/menger-sdk.js"
