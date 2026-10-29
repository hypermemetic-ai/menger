#!/usr/bin/env bash
# V2-8, acceptance 2: every synapse command in AGENTS.md runs as written
# against a throwaway v2 server.
#
#   scripts/agents-commands.sh PORT [FILE]     run each command of FILE on -P PORT
#   scripts/agents-commands.sh --list [FILE]   print them, rewritten, and the skips; run nothing
#
# FILE defaults to AGENTS.md at the repository's root. A command is
#   - a line of a fenced ```sh block that begins with `synapse` (a line
#     ending `\` joins the next; comment lines and every other line —
#     `printf … > file`, `base64 …`, `cargo …`, `deploy/pipeline …` — are
#     skipped), or
#   - an inline `synapse … menger …` span in the prose (outside the fences;
#     a span wrapped over two lines is joined).
# synapse's own commands (`_login`, `_key`, `_hold`, `_self`, `_find`, any
# `_name`) are not run: they are listed under "skipped (synapse's own)".
#
# Its address — `-P N`, `--url URL` — becomes `-P PORT` (`--as X` stays),
# and it runs as `synapse -j …` (once) under `timeout 20`, from a temp
# directory holding a stand-in for every `@file` the commands name (`{}` for
# a .json, a line of text otherwise).
#
# ok    the server took the call: synapse exited 0 (an in-band answer —
#       `not_found`, `forbidden`, an error about a missing ticket — exits 0:
#       the server ran the method), or 124 (a stream still open at the
#       timeout, output or not); and no line of the output is a refusal:
#       a JSON-RPC error -32601 (no such method) or -32602 (bad params) by
#       its code or as synapse prints it (`Invalid params`, `Method not
#       found`, `Activation not found`), or synapse's own `command not
#       found` / `cannot have subcommands`.
# FAIL  anything else (synapse refuses an unknown flag or command with exit 1).
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

trim() { local s=$1; s="${s#"${s%%[![:space:]]*}"}"; printf '%s' "${s%"${s##*[![:space:]]}"}"; }

# The candidates, one per element: the synapse lines of the ```sh blocks,
# continuations joined, then the inline spans of the prose.
found=()
in_fence=0 fence_sh=0 pending="" prose=""
while IFS= read -r line || [[ -n $line ]]; do
  if [[ $in_fence -eq 0 ]]; then
    if [[ $line =~ ^[[:space:]]*\`\`\` ]]; then
      in_fence=1
      [[ $line =~ ^[[:space:]]*\`\`\`sh[[:space:]]*$ ]] && fence_sh=1 || fence_sh=0
      prose+=$'\n'
      continue
    fi
    # Prose: a line joins its paragraph (a blockquote's `>` dropped); a
    # blank line ends it.
    line=$(trim "$line"); line=${line#>}; line=$(trim "$line")
    if [[ -z $line ]]; then prose+=$'\n'; else prose+=" $line"; fi
    continue
  fi
  if [[ -z $pending && $line =~ ^[[:space:]]*\`\`\`[[:space:]]*$ ]]; then in_fence=0; continue; fi
  [[ $fence_sh -eq 1 ]] || continue
  if [[ -n $pending ]]; then
    line="$pending $(trim "$line")"
    pending=""
  else
    line=$(trim "$line")
    [[ $line == synapse\ * ]] || continue
  fi
  if [[ $line == *\\ ]]; then pending="${line%\\}"; continue; fi
  found+=("$line")
done < "$file"
while IFS= read -r span; do
  span=${span#\`}; span=${span%\`}
  [[ $span == *" menger "* || $span == *" menger" ]] && found+=("$span")
done < <(grep -oE '`synapse [^`]*`' <<<"$prose")

# synapse's own commands are listed, never run.
commands=() skipped=()
for c in "${found[@]}"; do
  if [[ $c =~ (^|[[:space:]])_[a-z][a-z_]*([[:space:]]|$) ]]; then skipped+=("$c"); else commands+=("$c"); fi
done

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

print_skipped() {
  [[ ${#skipped[@]} -gt 0 ]] || return 0
  echo "skipped (synapse's own):"
  for c in "${skipped[@]}"; do printf '  %s\n' "$c"; done
}

if [[ $list -eq 1 ]]; then
  for c in "${commands[@]}"; do rewrite "$c"; done
  print_skipped
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
  grep -qE '"code"[[:space:]]*:[[:space:]]*-3260[12]([^0-9]|$)' "$1" && return 0
  grep -qE '"message"[[:space:]]*:[[:space:]]*"(Invalid params|Method not found|Activation not found)' "$1" && return 0
  grep -qiE 'command not found|cannot have subcommands' "$1"
}

ok=0 failed=0
for c in "${commands[@]}"; do
  run=$(rewrite "$c")
  (cd "$work" && timeout 20 bash -c "$run" </dev/null) >"$out" 2>&1
  status=$?
  if [[ $status -eq 0 || $status -eq 124 ]] && ! refused "$out"; then
    ok=$((ok + 1)); printf 'ok    %s\n' "$c"
  else
    failed=$((failed + 1))
    printf 'FAIL  %s  (exit %d: %s)\n' "$c" "$status" "$(grep -m1 -v '^[[:space:]]*$' "$out" | cut -c1-160)"
  fi
done
print_skipped
echo "$ok ok, $failed failed"
[[ $failed -eq 0 ]]
