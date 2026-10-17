#!/usr/bin/env bash
# RUN-21: the one way an agent removes what a gate left behind — its
# worktree and branch, its cargo target dir, its scratch databases, its
# session scratch. No agent hand-writes `rm -r` (owner, 2026-10-16: "we need
# to make sure to not hit spooky rms").
#
#   scripts/cleanup-gate.sh NAME...            what the gate NAME left (dry run)
#   scripts/cleanup-gate.sh --path PATH...     these paths, each by its shape (dry run)
#   scripts/cleanup-gate.sh --list             every leftover it recognises (never removes)
#   … --yes                                    remove for real, and log it
#
# A dry run is the default: it says what it would remove and the space it
# would free. With --yes it removes, prints and appends each removal to
# ~/.local/state/menger/cleanup-gate.log (CLEANUP_GATE_LOG moves it).
#
# Safe by construction: a path is removed only if, its symlinks resolved, it
# has one of these shapes —
#
#   worktree    a linked git worktree under ~/dev/controlflow/hypermemetic,
#               removed with `git worktree remove` (never forced: a dirty or
#               locked one is refused), then its branch with `git branch -d`
#               (an unmerged branch is kept and said)
#   ws-scratch  ~/dev/controlflow/hypermemetic/.NAME that is no checkout and
#               holds no .git anywhere (a gate's logs and scratch files)
#   target      ~/.cache/cargo-target/NAME — a direct child, never a shared
#               target (`menger`, or any NAME that is a repository beside menger)
#   scratch-db  /tmp/menger-*.db, .db-shm, .db-wal, .log — a throwaway
#               server's database and log
#   session     anything below /tmp/claude-UID/ (a session's scratch)
#   sweep       /tmp/theme-sweep (scripts/theme-sweep.mjs's default out)
#
# — and it is refused if it is /, $HOME, any of those roots itself, under
# ~/.local/share/menger, a repository's main checkout, publish.sh's stage
# (.menger-publish), in use by a process (its cwd or an open file), the
# directory you are in, or named with a `..`. A refusal is said and the rest
# go on; the exit is 2 if anything was refused, 1 on a usage error.
#
# CLEANUP_GATE_ROOT=DIR (tests only; DIR must lie below /tmp/) moves every
# root into DIR — DIR/home is $HOME, DIR/tmp is /tmp — so the tests run on a
# throwaway tree (scripts/tests/cleanup-gate).
set -uo pipefail

usage() { sed -n '/^#   scripts/p' "$0" | sed 's/^# *//' >&2; exit 1; }

yes=0 list=0 mode=name
args=()
while [[ $# -gt 0 ]]; do
  case $1 in
    --yes) yes=1 ;;
    --dry-run) yes=0 ;;
    --list) list=1 ;;
    --path) mode=path ;;
    -h|--help) usage ;;
    --) shift; args+=("$@"); break ;;
    -*) printf 'cleanup-gate: unknown flag %s\n' "$1" >&2; usage ;;
    *) args+=("$1") ;;
  esac
  shift
done
if [[ $list == 1 ]]; then
  [[ $yes == 1 ]] && { echo 'cleanup-gate: --list never removes; name what to remove (NAME or --path)' >&2; exit 1; }
  [[ ${#args[@]} -gt 0 ]] && { echo 'cleanup-gate: --list takes no arguments' >&2; exit 1; }
elif [[ ${#args[@]} -eq 0 ]]; then usage; fi

# ── the roots ───────────────────────────────────────────────────────────────
if [[ -n ${CLEANUP_GATE_ROOT:-} ]]; then
  R0=$(readlink -f -- "$CLEANUP_GATE_ROOT") || { echo 'cleanup-gate: CLEANUP_GATE_ROOT does not resolve' >&2; exit 1; }
  TMP0=$(readlink -f /tmp)
  [[ $R0 == "$TMP0"/?*/?* ]] || { echo "cleanup-gate: CLEANUP_GATE_ROOT must lie at least two levels below /tmp (got $R0)" >&2; exit 1; }
  H=$R0/home TMPR=$R0/tmp
else
  H=$(readlink -f -- "$HOME") TMPR=$(readlink -f /tmp)
fi
WS=$H/dev/controlflow/hypermemetic
CT=$H/.cache/cargo-target
SCR=$TMPR/claude-$(id -u)
DATA=$H/.local/share/menger
LOG=${CLEANUP_GATE_LOG:-$H/.local/state/menger/cleanup-gate.log}
HERE_PWD=$(readlink -f -- "$PWD" 2>/dev/null || echo "$PWD")
# a root that does not exist resolves to itself; one that does, to its real path
real() { readlink -f -- "$1" 2>/dev/null || echo "$1"; }
WS=$(real "$WS") CT=$(real "$CT") SCR=$(real "$SCR") DATA=$(real "$DATA")

refused=0 total=0
human() { numfmt --to=iec --suffix=B "$1" 2>/dev/null || echo "${1}B"; }
say()  { printf '%s\n' "$*"; }
refuse() { refused=1; printf 'refused  %s — %s\n' "$1" "$2"; [[ $yes == 1 ]] && logline "refused $1 — $2"; }
logline() { mkdir -p "$(dirname "$LOG")" && printf '%s %s\n' "$(date -u +%FT%TZ)" "$*" >>"$LOG"; }
size_of() { du -sb -- "$@" 2>/dev/null | awk '{s += $1} END {print s + 0}'; }
under() { [[ $1 == "$2"/* ]]; }   # strictly below

# Everything any process holds open, or works in: "path<TAB>pid", read once.
OPEN=$(find /proc/[0-9]*/cwd /proc/[0-9]*/fd/ -maxdepth 1 -type l -printf '%l\t%p\n' 2>/dev/null \
  | awk -F'\t' -v me="$$" '{ split($2, a, "/"); if (a[3] != me) print $1 "\t" a[3] }')
in_use() { # PATH → the pid holding it (or something below it), if any
  awk -F'\t' -v r="$1" '$1 == r || index($1, r "/") == 1 { print $2; exit }' <<<"$OPEN"
}

git_q() { GIT_CEILING_DIRECTORIES=$(dirname "$1") git -C "$1" "${@:2}" 2>/dev/null; }

# A shared target is one named for a repository beside menger (MENGER-41's
# CARGO_TARGET_DIR=~/.cache/cargo-target/<repo>), or menger's own.
shared_target() { [[ $1 == menger ]] || [[ -d $WS/$1 && ! $1 == .* ]]; }

# ── one path: classify, refuse or remove ───────────────────────────────────
remove_path() {
  local p=$1 r shape="" pid gd cd top main br sz extra=""
  [[ $p == /* ]] || p=$PWD/$p
  case "/$p/" in */../*) refuse "$p" 'named with ..'; return ;; esac
  if [[ ! -e $p && ! -L $p ]]; then say "absent   $p"; return; fi
  r=$(readlink -f -- "$p") || { refuse "$p" 'does not resolve'; return; }
  local shown=$r; [[ $r != "$p" ]] && shown="$p → $r"

  # the never-list, before any shape
  case $r in
    /|"$H"|"$WS"|"$CT"|"$SCR"|"$TMPR"|"$H/.cache"|"$H/dev"|"$H/dev/controlflow") refuse "$shown" 'a root, never removed'; return ;;
  esac
  if [[ $r == "$DATA" ]] || under "$r" "$DATA"; then refuse "$shown" "menger's data (~/.local/share/menger)"; return; fi
  if [[ $r == "$WS/.menger-publish" ]]; then refuse "$shown" "scripts/publish.sh's stage"; return; fi
  if [[ $HERE_PWD == "$r" ]] || under "$HERE_PWD" "$r"; then refuse "$shown" 'you are inside it'; return; fi
  if [[ -d $r && ! -L $r ]] && top=$(git_q "$r" rev-parse --show-toplevel) && [[ $(real "$top") == "$r" ]]; then
    gd=$(real "$(git_q "$r" rev-parse --path-format=absolute --git-dir)")
    cd=$(real "$(git_q "$r" rev-parse --path-format=absolute --git-common-dir)")
    if [[ $gd == "$cd" ]]; then refuse "$shown" "a repository's main checkout"; return; fi
    if ! under "$r" "$WS"; then refuse "$shown" 'a git worktree outside ~/dev/controlflow/hypermemetic'; return; fi
    shape=worktree
  elif [[ -d $r && $(dirname "$r") == "$WS" && $(basename "$r") == .?* ]]; then
    if [[ -n $(find "$r" -name .git -print -quit 2>/dev/null) ]]; then refuse "$shown" 'holds a git checkout'; return; fi
    shape=ws-scratch
  elif [[ $(dirname "$r") == "$CT" ]]; then
    if shared_target "$(basename "$r")"; then refuse "$shown" 'a shared cargo target'; return; fi
    shape=target
  elif [[ -f $r && $(dirname "$r") == "$TMPR" && $(basename "$r") =~ ^menger-.+\.(db|db-shm|db-wal|log)$ ]]; then
    shape=scratch-db
  elif under "$r" "$SCR"; then
    shape=session
  elif [[ -d $r && $r == "$TMPR/theme-sweep" ]]; then
    shape=sweep
  else
    refuse "$shown" 'not a leftover shape (worktree, ws-scratch, target, scratch-db, session, sweep)'; return
  fi
  if pid=$(in_use "$r") && [[ -n $pid ]]; then
    refuse "$shown" "in use by pid $pid ($({ tr '\0\n' '  ' </proc/"$pid"/cmdline; } 2>/dev/null | cut -c1-60))"; return
  fi

  sz=$(size_of "$r")
  if [[ $shape == worktree ]]; then
    main=$(dirname "$cd")
    br=$(git_q "$r" symbolic-ref --quiet --short HEAD)
    if [[ -n $(git_q "$r" status --porcelain) ]]; then refuse "$shown" 'a worktree with uncommitted or untracked files (git worktree remove would refuse; commit or drop them first)'; return; fi
    if [[ -n $br ]]; then
      if git_q "$main" merge-base --is-ancestor "$br" HEAD; then extra=", branch $br (merged)"
      else extra=", branch $br kept (not merged into $(git_q "$main" symbolic-ref --short HEAD))"; fi
    fi
  fi

  if [[ $yes == 0 ]]; then
    say "would remove  $(printf '%-10s' "$shape") $shown  $(human "$sz")$extra"
    total=$((total + sz)); return
  fi
  local out rc
  case $shape in
    worktree)
      out=$(git -C "$main" worktree remove -- "$r" 2>&1); rc=$?
      if [[ $rc != 0 ]]; then refuse "$shown" "git worktree remove: ${out//$'\n'/ }"; return; fi
      if [[ -n $br ]]; then
        if out=$(git -C "$main" branch -d -- "$br" 2>&1); then extra=", branch $br deleted"
        else extra=", branch $br kept (${out//$'\n'/ })"; fi
      fi ;;
    target|session|ws-scratch|sweep)
      if [[ -d $r ]]; then out=$(rm -r --one-file-system -- "$r" 2>&1); rc=$?
      else out=$(rm -f -- "$r" 2>&1); rc=$?; fi
      [[ $rc == 0 ]] || { refuse "$shown" "rm: ${out//$'\n'/ }"; return; } ;;
    scratch-db)
      out=$(rm -f -- "$r" 2>&1) || { refuse "$shown" "rm: $out"; return; } ;;
  esac
  say "removed  $(printf '%-10s' "$shape") $shown  $(human "$sz")$extra"
  logline "removed $shape $shown $sz bytes$extra"
  total=$((total + sz))
}

# ── NAME → its leftovers ───────────────────────────────────────────────────
paths_for_name() {
  local n=$1 f repo
  if [[ ! $n =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ || $n == *..* ]]; then
    refuse "$n" 'not a gate name (letters, digits, . _ -; no ..)'; return
  fi
  for f in "$WS/.$n" "$WS/$n" "$CT/$n" "$SCR/$n" \
           "$TMPR/menger-$n.db" "$TMPR/menger-$n.db-shm" "$TMPR/menger-$n.db-wal" "$TMPR/menger-$n.log"; do
    [[ -e $f || -L $f ]] && printf '%s\n' "$f"
  done
  for repo in "$WS"/*/; do
    f=${repo}.claude/worktrees/$n
    [[ -e $f ]] && printf '%s\n' "$f"
  done
}

# ── --list: every candidate, each judged as a dry run would ────────────────
candidates() {
  local repo wt f
  for repo in "$WS"/*/; do
    [[ -d $repo ]] || continue
    # the first entry is the main checkout; the rest are its linked worktrees
    while IFS= read -r wt; do
      printf '%s\n' "$(real "${wt#worktree }")"
    done < <(git_q "${repo%/}" worktree list --porcelain | grep '^worktree ' | tail -n +2)
  done
  for f in "$WS"/.[!.]* "$CT"/* "$TMPR"/menger-*.db "$TMPR"/menger-*.db-shm "$TMPR"/menger-*.db-wal \
           "$TMPR"/menger-*.log "$SCR"/* "$TMPR/theme-sweep"; do
    [[ -e $f || -L $f ]] && printf '%s\n' "$f"
  done
}

declare -A seen=()
run_all() {
  local p
  while IFS= read -r p; do
    [[ -z $p || -n ${seen[$p]:-} ]] && continue
    seen[$p]=1; remove_path "$p"
  done
}
if [[ $list == 1 ]]; then
  run_all < <(candidates | sort -u)
elif [[ $mode == path ]]; then
  run_all < <(printf '%s\n' "${args[@]}")
else
  for n in "${args[@]}"; do
    found=$(paths_for_name "$n")
    if [[ $found == refused* ]]; then printf '%s\n' "$found"; refused=1; continue; fi
    [[ -z $found ]] && { say "nothing  left by $n"; continue; }
    run_all <<<"$found"
  done
fi

if [[ $yes == 1 ]]; then say "freed $(human "$total")"
else say "would free $(human "$total") — a dry run; --yes removes"; fi
[[ $refused == 0 ]] || exit 2
