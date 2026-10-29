#!/usr/bin/env bash
# V2-8, acceptance 2: every command in AGENTS.md runs as written against a
# throwaway v2 server.
#
#   scripts/agents-commands.sh PORT [FILE]     run every synapse line of FILE's ```sh blocks on -P PORT
#   scripts/agents-commands.sh --list [FILE]   print them, rewritten, and run nothing
#
# FILE defaults to AGENTS.md at the repository's root. A command is a line
# of a fenced ```sh block that begins with `synapse` (a line ending `\`
# joins the next); comment lines and every other line (`printf … > file`,
# `base64 …`, `cargo …`) are skipped. Its address — `-P N`, `--url URL` —
# becomes `-P PORT` (`--as X` stays), and it runs as `synapse -j …` (once) under
# `timeout 20`, from a temp directory holding a stand-in for every
# `@file` the commands name (`{}` for a .json, a line of text otherwise).
#
# ok    the server took the call: exit 0, and no line of the output is a
#       JSON-RPC error -32601 (no such method) or -32602 (bad params) —
#       by its code, or as synapse prints it (`Method not found`,
#       `Activation not found`, `Invalid params`) — nor says unknown /
#       unrecognized / cannot have subcommands / command not found. An in-band
#       domain answer (not_found, forbidden, an error about a missing ticket)
#       is ok: the server ran the method. A stream that is still open at the
#       timeout (exit 124: `watch`) is ok if it printed an event and no error.
# FAIL  anything else.
#
# Sign in to the throwaway once before: synapse -P PORT _login menger --key builder --as builder
set -uo pipefail

usage() { echo "usage: $0 PORT [FILE] | $0 --list [FILE]" >&2; exit 2; }

list=0 port="" file=""
for arg in "$@"; do
  case "$arg" in
    --list) list=1 ;;
    -h|--help) usage ;;
    *) if [[ $list -eq 0 && -z $port ]]; then port=$arg
       elif [[ -z $file ]]; then file=$arg
       else usage; fi ;;
  esac
done
[[ $list -eq 1 || $port =~ ^[0-9]+$ ]] || usage
[[ $list -eq 1 ]] && port=${port:-PORT}
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
file=${file:-$root/AGENTS.md}
[[ -r $file ]] || { echo "$0: cannot read $file" >&2; exit 2; }

# The commands, one per element: the synapse lines of the ```sh blocks,
# continuations joined.
commands=()
in_sh=0 pending=""
while IFS= read -r line || [[ -n $line ]]; do
  if [[ $in_sh -eq 0 ]]; then
    [[ $line =~ ^[[:space:]]*\`\`\`sh[[:space:]]*$ ]] && in_sh=1
    continue
  fi
  if [[ -z $pending && $line =~ ^[[:space:]]*\`\`\` ]]; then in_sh=0; continue; fi
  if [[ -n $pending ]]; then
    line="$pending ${line#"${line%%[![:space:]]*}"}"
    pending=""
  else
    line="${line#"${line%%[![:space:]]*}"}"
    [[ $line == synapse\ * ]] || continue
  fi
  if [[ $line == *\\ ]]; then pending="${line%\\}"; continue; fi
  commands+=("$line")
done < "$file"

# The address rewritten to the throwaway, and -j added before the rest.
rewrite() {
  local c=$1
  c=$(sed -E \
    -e 's/(^|[[:space:]])-P[[:space:]]+[0-9]+/\1-P '"$port"'/g' \
    -e 's/(^|[[:space:]])--url[[:space:]]+[^[:space:]]+/\1-P '"$port"'/g' \
    -e 's/(^|[[:space:]])--url=[^[:space:]]+/\1-P '"$port"'/g' <<<"$c")
  # -j once: synapse refuses a global option given twice.
  if [[ $c =~ ^synapse([[:space:]]+-[^[:space:]]+([[:space:]]+[^-][^[:space:]]*)?)*[[:space:]]+-j[[:space:]] ]]; then
    printf '%s\n' "$c"
  else
    printf 'synapse -j%s\n' "${c#synapse}"
  fi
}

if [[ $list -eq 1 ]]; then
  for c in "${commands[@]}"; do rewrite "$c"; done
  exit 0
fi

command -v synapse >/dev/null || { echo "$0: synapse is not on PATH" >&2; exit 2; }
work=$(mktemp -d "${TMPDIR:-/tmp}/agents-commands.XXXXXX")
standins=()
for c in "${commands[@]}"; do
  while read -r f; do
    [[ -n $f && $f != */* && ! -e $work/$f ]] || continue
    case "$f" in *.json) echo '{}' ;; *) echo 'a stand-in body (V2-8)' ;; esac > "$work/$f"
    standins+=("$work/$f")
  done < <(grep -oE '@[A-Za-z0-9._-]+\.[A-Za-z0-9]+' <<<"$c" | sed 's/^@//')
done
out=$(mktemp "${TMPDIR:-/tmp}/agents-commands-out.XXXXXX")
cleanup() { rm -f -- "$out" "${standins[@]}"; rmdir -- "$work" 2>/dev/null; }
trap cleanup EXIT

refused() {
  # A JSON-RPC refusal of the method or its params — by its code, or as the
  # error item synapse prints for it under -j (`Method not found: a.m`,
  # `Activation not found: a`, `Invalid params: …`) — or synapse's own.
  grep -qE '"code"[[:space:]]*:[[:space:]]*-3260[12]([^0-9]|$)' "$1" && return 0
  grep -qE '"message"[[:space:]]*:[[:space:]]*"(Invalid params|Method not found|Activation not found)' "$1" && return 0
  grep -qiE 'unknown|unrecognized|cannot have subcommands|command not found' "$1"
}

ok=0 failed=0
for c in "${commands[@]}"; do
  run=$(rewrite "$c")
  (cd "$work" && timeout 20 bash -c "$run") >"$out" 2>&1
  status=$?
  verdict=FAIL
  if [[ $status -eq 0 ]] && ! refused "$out"; then
    verdict=ok
  elif [[ $status -eq 124 ]] && grep -q '^{' "$out" && ! refused "$out" && ! grep -qE '"type"[[:space:]]*:[[:space:]]*"error"' "$out"; then
    verdict=ok
  fi
  if [[ $verdict == ok ]]; then
    ok=$((ok + 1)); printf 'ok    %s\n' "$c"
  else
    failed=$((failed + 1)); printf 'FAIL  %s  (exit %d: %s)\n' "$c" "$status" "$(grep -m1 -v '^[[:space:]]*$' "$out" | cut -c1-160)"
  fi
done
echo "$ok ok, $failed failed"
[[ $failed -eq 0 ]]
