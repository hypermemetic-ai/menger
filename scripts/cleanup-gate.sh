#!/usr/bin/env bash
# RUN-21: the one way an agent removes what a gate left behind — its
# worktree and branch, its cargo target dir, its scratch databases, its
# session scratch. No agent hand-writes `rm -r` (owner, 2026-10-16: "we need
# to make sure to not hit spooky rms").
#
#   scripts/cleanup-gate.sh NAME...            what the gate NAME left (dry run)
#   scripts/cleanup-gate.sh --path PATH...     these paths, each by its shape (dry run)
#   scripts/cleanup-gate.sh --list             every leftover it recognises (never removes)
#   scripts/cleanup-gate.sh --sweep            CI-2: build output nothing has touched for a while (dry run)
#   … --yes                                    remove for real, and log it
#   … --log-days N                             the sweep's pipeline-log age (default 60; CLEANUP_GATE_LOG_DAYS)
#   … --idle-hours N                           the sweep's idle threshold (default 6; CLEANUP_GATE_IDLE_HOURS)
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
#   target-part a shared target's intermediate output — ~/.cache/cargo-target/
#               SHARED/[TRIPLE/]PROFILE/{deps,incremental,build,.fingerprint}
#               (since the build-dir moved, CI-2, only stale copies live there)
#   cargo-build ~/.cache/cargo-build/XX/HASH — one workspace's build-dir
#               (~/.cargo/config.toml's `build-dir = …/{workspace-path-hash}`);
#               a worktree removed takes its own with it (`cargo metadata`'s
#               build_directory, read from each workspace in it before it goes)
#   scratch-db  /tmp/menger-*.db, .db-shm, .db-wal, .sessions.jsonl(.lock),
#               .log — a throwaway server's database, sessions and log; and
#               (SCRATCH-NAMES) its other services' files, named after it —
#               menger-NAME.{ui,media,docs,quota}.db(-shm, -wal) and the
#               directory menger-NAME.media.blobs
#   pages-tmp   V2-38: /tmp/claude-UID/ui8-pages-… — a browser page-test run's work
#               directory (tests/pages_browser.mjs, which removes its own since
#               V2-38; these are what a killed run left). By --path at once, as a
#               session's scratch; the sweep takes one nothing in has touched for
#               an hour (whatever --idle-hours says) and nothing holds
#   session     anything else below /tmp/claude-UID/ (a session's scratch)
#   sweep       /tmp/theme-sweep (scripts/theme-sweep.mjs's default out)
#   pipeline-log ~/.local/state/menger-deploy/logs/<stamp>-….log (CI-6): one
#               run's log; the sweep takes those older than 60 days
#               (--log-days N, CLEANUP_GATE_LOG_DAYS) and never a red run's —
#               `<stamp>.red` beside them marks it, and a path names none of those
#   ui8-work    RUN-34: scripts/ui-throwaway.mjs's work directory, ui8-<prefix>-…
#               (tests/pages_browser.mjs's ui8-pages-, perf/pages-probe.mjs's
#               ui8-probe-) directly in a scratch root — ~/.cache/menger-perf-scratch,
#               or a directory MENGER_SCRATCH_DIRS names (colon-separated; the
#               timer reads it from ~/.config/menger/sweep.env) — holding the
#               throwaway's marks (tickets.db, ui.db, admin.key), nothing in it
#               modified for an hour (whatever --idle-hours says); the sweep takes
#               them too. A scratch root's other files are no shape. (Below
#               /tmp/claude-UID/ such a directory is a session's scratch.)
#
# — and it is refused if it is /, $HOME, any of those roots itself (a scratch
# root too: one MENGER_SCRATCH_DIRS names that is such a root refuses the run,
# or is skipped by --sweep), under
# ~/.local/share/menger, a repository's main checkout, publish.sh's stage
# (.menger-publish), in use by a process (its cwd or an open file) or by a
# cargo holding its lock, the directory you are in, or named with a `..`. A
# refusal is said and the rest go on; the exit is 2 if anything was refused,
# 1 on a usage error.
#
# CI-2 — the sweep (`--sweep`; the hourly timer runs `--sweep --yes`, see
# scripts/sweep/install.sh) takes only build output: every cargo-build dir,
# every non-shared target and every target-part, each only when nothing in it
# was modified for the idle threshold and nothing holds it — and RUN-34's
# ui8-work directories in the scratch roots and V2-38's pages-tmp ones below
# /tmp/claude-UID/, each idle an hour. The threshold
# drops as the disk fills: 6 h (--idle-hours), 1 h at 75% used, 15 min at
# 90%. What it keeps is said (`kept`), not refused; a candidate of any other
# shape (never expected: a symlink into a worktree, say) is refused and the
# sweep exits 2, else 0. A bad scratch root is said on stderr and skipped, so
# the sweep still runs (--path, --list and NAME exit 1 on one instead).
#
# CLEANUP_GATE_ROOT=DIR (tests only; DIR must lie below /tmp/) moves every
# root into DIR — DIR/home is $HOME, DIR/tmp is /tmp — so the tests run on a
# throwaway tree (scripts/tests/cleanup-gate). CLEANUP_GATE_USED_PCT fakes the
# disk's use and CLEANUP_GATE_CARGO names the cargo that reads a workspace's
# build_directory (both for the tests).
set -uo pipefail

usage() { sed -n '/^#   scripts/p' "$0" | sed 's/^# *//' >&2; exit 1; }

LOG_DAYS=${CLEANUP_GATE_LOG_DAYS:-60}
yes=0 list=0 sweep=0 mode=name idle_hours=${CLEANUP_GATE_IDLE_HOURS:-6}
args=()
while [[ $# -gt 0 ]]; do
  case $1 in
    --yes) yes=1 ;;
    --dry-run) yes=0 ;;
    --list) list=1 ;;
    --sweep) sweep=1 ;;
    --idle-hours) shift; idle_hours=${1:-} ;;
    --idle-hours=*) idle_hours=${1#*=} ;;
    --log-days) shift; LOG_DAYS=${1:-} ;;
    --log-days=*) LOG_DAYS=${1#*=} ;;
    --path) mode=path ;;
    -h|--help) usage ;;
    --) shift; args+=("$@"); break ;;
    -*) printf 'cleanup-gate: unknown flag %s\n' "$1" >&2; usage ;;
    *) args+=("$1") ;;
  esac
  shift
done
[[ ${LOG_DAYS:-60} =~ ^[0-9]+$ ]] || { echo 'cleanup-gate: --log-days takes a whole number of days' >&2; exit 1; }
[[ $idle_hours =~ ^[0-9]+$ ]] || { echo 'cleanup-gate: --idle-hours takes a whole number of hours' >&2; exit 1; }
if [[ $sweep == 1 ]]; then
  [[ $list == 1 || ${#args[@]} -gt 0 || $mode == path ]] && { echo 'cleanup-gate: --sweep takes no names, paths or --list' >&2; exit 1; }
elif [[ $list == 1 ]]; then
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
  R0="" H=$(readlink -f -- "$HOME") TMPR=$(readlink -f /tmp)
fi
WS=$H/dev/controlflow/hypermemetic
CT=$H/.cache/cargo-target
CB=$H/.cache/cargo-build
SCR=$TMPR/claude-$(id -u)
DATA=$H/.local/share/menger
PLOGS=$H/.local/state/menger-deploy/logs
LOG=${CLEANUP_GATE_LOG:-$H/.local/state/menger/cleanup-gate.log}
HERE_PWD=$(readlink -f -- "$PWD" 2>/dev/null || echo "$PWD")
# a root that does not exist resolves to itself; one that does, to its real path
real() { readlink -f -- "$1" 2>/dev/null || echo "$1"; }
PLOGS=$(real "$PLOGS")
WS=$(real "$WS") CT=$(real "$CT") CB=$(real "$CB") SCR=$(real "$SCR") DATA=$(real "$DATA")
# a root is never removed, nor a scratch root
is_root() { case $1 in /|"$H"|"$WS"|"$CT"|"$CB"|"$SCR"|"$TMPR"|"$H/.cache"|"$H/dev"|"$H/dev/controlflow"|"$DATA") return 0 ;; esac; return 1; }

# RUN-34: the scratch roots a ui8-work directory lives in — menger-perf-scratch
# in ~/.cache by name, and each of MENGER_SCRATCH_DIRS (absolute, colon-separated).
# Each, its symlinks resolved, must be no root, no ancestor of $HOME, not in
# the workspace or menger's data (and under a test root, under it). A bad
# default root is dropped with a line on stderr; a bad MENGER_SCRATCH_DIRS
# entry refuses the run (exit 1), except under --sweep, which says it and
# goes on without it — the hourly timer must not stop sweeping over one line.
scratch_root_bad() { # PATH → why it may not be a scratch root (empty: it may)
  local d=$1 r
  [[ $d == *$'\n'* ]] && { echo 'a newline in it'; return; }
  [[ $d == /* ]] || { echo 'not an absolute path'; return; }
  case "/$d/" in */../*|*/./*) echo 'named with . or ..'; return ;; esac
  r=$(real "$d")
  if is_root "$r" || [[ $H == "$r"/* || $r == "$DATA"/* ]]; then echo 'a root, never a scratch root'; return; fi
  if [[ $r == "$WS"/* ]]; then echo 'in the workspace, never a scratch root'; return; fi
  if [[ -n ${R0:-} && $r != "$R0"/* ]]; then echo "outside the test root $R0"; return; fi
}
SCRATCH_ROOTS=()
_d=$H/.cache/menger-perf-scratch
if _why=$(scratch_root_bad "$_d") && [[ -n $_why ]]; then
  printf 'cleanup-gate: %s → %s — %s; not a scratch root\n' "$_d" "$(real "$_d")" "$_why" >&2
else SCRATCH_ROOTS+=("$(real "$_d")"); fi
_sd=()
[[ -n ${MENGER_SCRATCH_DIRS:-} ]] && mapfile -d: -t _sd < <(printf '%s' "$MENGER_SCRATCH_DIRS")
for _d in "${_sd[@]}"; do
  [[ -n $_d ]] || continue
  _why=$(scratch_root_bad "$_d")
  if [[ -n $_why ]]; then
    printf 'cleanup-gate: MENGER_SCRATCH_DIRS names %s — %s\n' "${_d//$'\n'/\\n}" "$_why" >&2
    [[ $sweep == 1 ]] && continue
    exit 1
  fi
  SCRATCH_ROOTS+=("$(real "$_d")")
done
unset _sd _d _why
# a work directory's own name: mkdtemp's ui8-<prefix>-XXXXXX, nothing else (no newline, no space)
ui8_name() { [[ $1 =~ ^ui8-[A-Za-z0-9._-]+$ ]]; }
is_scratch_root() { local s; for s in "${SCRATCH_ROOTS[@]}"; do [[ $1 == "$s" ]] && return 0; done; return 1; }
UI8_MARKS=(tickets.db ui.db admin.key)
has_ui8_marks() { local m; for m in "${UI8_MARKS[@]}"; do [[ -f $1/$m ]] || return 1; done; }

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

# CI-2: a workspace's build-dir is $CB/XX/HASH (cargo's {workspace-path-hash},
# 16 hex digits split 2/14); a shared target's intermediate output is
# SHARED/[TRIPLE/]PROFILE/PART.
cargo_build_shape() { [[ $(dirname "$(dirname "$1")") == "$CB" && $(basename "$(dirname "$1")") =~ ^[0-9a-f]{2}$ && $(basename "$1") =~ ^[0-9a-f]{6,}$ ]]; }
target_part_shape() {
  local rel=${1#"$CT"/} s
  [[ $1 == "$CT"/* ]] || return 1
  s=${rel%%/*}; shared_target "$s" || return 1
  [[ $rel =~ ^[^/]+/([^/]+/)?[^/]+/(deps|incremental|build|\.fingerprint)$ ]]
}
# cargo holds a lock file in each profile while it builds: a lock we cannot
# share is a build under way, whatever /proc says
lock_held() { local f
  while IFS= read -r f; do flock -n -s "$f" true 2>/dev/null || { printf '%s\n' "$f"; return 0; }
  done < <(find "$1" -maxdepth 3 -name '.cargo*lock' -type f 2>/dev/null)
  return 1
}
# the first thing in PATH (itself included) modified within MINUTES: empty when idle that long
busy_since() { find "$1" -mmin "-$2" -print -quit 2>/dev/null; }

# A worktree's build-dirs: each workspace in it, asked of cargo (it hashes the
# canonical path), kept only when they have the cargo-build shape.
CARGO=${CLEANUP_GATE_CARGO:-cargo}
build_dirs_of() { local m d
  command -v "$CARGO" >/dev/null 2>&1 || return 0
  while IFS= read -r m; do
    d=$(cd "$(dirname "$m")" && env -u CARGO_TARGET_DIR -u CARGO_BUILD_BUILD_DIR timeout 30 "$CARGO" metadata --offline --no-deps \
          --format-version 1 --manifest-path "$m" 2>/dev/null | jq -r '.build_directory // empty' 2>/dev/null)
    [[ -n $d ]] || continue
    d=$(real "$d"); cargo_build_shape "$d" && [[ -d $d ]] && printf '%s\n' "$d"
  done < <(find "$1" -maxdepth 3 -name Cargo.toml -not -path '*/target/*' -not -path '*/node_modules/*' 2>/dev/null) | sort -u
}

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
    /|"$H"|"$WS"|"$CT"|"$CB"|"$SCR"|"$TMPR"|"$H/.cache"|"$H/dev"|"$H/dev/controlflow") refuse "$shown" 'a root, never removed'; return ;;
  esac
  if is_scratch_root "$r"; then refuse "$shown" 'a scratch root — a root, never removed'; return; fi
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
  elif [[ -d $r ]] && cargo_build_shape "$r"; then
    shape=cargo-build
  elif [[ -d $r ]] && target_part_shape "$r"; then
    shape=target-part
  elif [[ $(dirname "$r") == "$CT" ]]; then
    if shared_target "$(basename "$r")"; then refuse "$shown" 'a shared cargo target'; return; fi
    shape=target
  elif [[ -f $r && $(dirname "$r") == "$TMPR" && $(basename "$r") =~ ^menger-.+\.(db|db-shm|db-wal|sessions\.jsonl|sessions\.jsonl\.lock|log)$ ]]; then
    shape=scratch-db
  elif [[ -d $r && ! -L $p && $(dirname "$r") == "$TMPR" && $(basename "$r") =~ ^menger-.+\.media\.blobs$ ]]; then
    shape=scratch-db
  elif [[ -d $r && ! -L $p && $(dirname "$r") == "$SCR" && $(basename "$r") =~ ^ui8-pages-[A-Za-z0-9._-]+$ ]]; then
    shape=pages-tmp
  elif under "$r" "$SCR"; then
    shape=session
  elif [[ -d $r && $r == "$TMPR/theme-sweep" ]]; then
    shape=sweep
  elif [[ -f $r && $(dirname "$r") == "$PLOGS" && $(basename "$r") =~ ^([0-9]{8}T[0-9]{6}Z)-.+\.log$ ]]; then
    if [[ -e $PLOGS/${BASH_REMATCH[1]}.red ]]; then refuse "$shown" "a red run's log (kept: $PLOGS/${BASH_REMATCH[1]}.red)"; return; fi
    shape=pipeline-log
  elif [[ -d $r ]] && ui8_name "$(basename "$r")" && is_scratch_root "$(dirname "$r")"; then
    if ! has_ui8_marks "$r"; then refuse "$shown" "a ui8 directory without the throwaway's marks (${UI8_MARKS[*]})"; return; fi
    shape=ui8-work
  else
    refuse "$shown" 'not a leftover shape (worktree, ws-scratch, target, target-part, cargo-build, scratch-db, pages-tmp, session, sweep, pipeline-log, ui8-work)'; return
  fi
  # the sweep takes build output and work directories, nothing else, whatever named it
  if [[ $sweep == 1 ]]; then
    case $shape in cargo-build|target|target-part|pipeline-log|ui8-work|pages-tmp) ;;
      *) refuse "$shown" "a $shape, and the sweep takes only build output, pipeline logs, ui8-work and pages-tmp"; return ;;
    esac
  fi
  # what rm -r takes must not be, or cross, a mount
  case $shape in target|target-part|cargo-build|session|ws-scratch|sweep|ui8-work|pages-tmp|scratch-db)
    if [[ -d $r ]] && { mountpoint -q -- "$r" 2>/dev/null || [[ $(stat -c %d -- "$r" 2>/dev/null) != "$(stat -c %d -- "$(dirname "$r")" 2>/dev/null)" ]]; }; then
      refuse "$shown" 'a mount point, never removed'; return
    fi ;;
  esac
  local busy="" idle=$idle_min
  # RUN-34: a work directory waits an hour, by path or by the sweep, whatever --idle-hours says
  [[ $shape == ui8-work ]] && idle=60
  # V2-38: a page-test run's, swept an hour after it was last written
  [[ $shape == pages-tmp && $sweep == 1 ]] && idle=60
  if [[ $sweep == 1 || $shape == ui8-work ]] && busy=$(busy_since "$r" "$idle") && [[ -n $busy ]]; then
    [[ $busy == "$r" ]] && busy=itself || busy=${busy#"$r"/}
    if [[ $sweep == 1 ]]; then say "kept     $(printf '%-11s' "$shape") $shown — modified within $(mins "$idle") ($busy)"
    else refuse "$shown" "modified within $(mins "$idle") ($busy)"; fi
    return
  fi
  if pid=$(in_use "$r") && [[ -n $pid ]]; then
    busy="in use by pid $pid ($({ tr '\0\n' '  ' </proc/"$pid"/cmdline; } 2>/dev/null | cut -c1-60))"
  elif [[ $shape == cargo-build || $shape == target || $shape == target-part ]] && [[ -d $r ]] && pid=$(lock_held "$r"); then
    busy="in use: cargo holds ${pid#"$r"/}"
  fi
  if [[ -n $busy ]]; then
    if [[ $sweep == 1 ]]; then say "kept     $(printf '%-11s' "$shape") $shown — $busy"; else refuse "$shown" "$busy"; fi
    return
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
  local bds=""; [[ $shape == worktree ]] && bds=$(build_dirs_of "$r")

  if [[ $yes == 0 ]]; then
    say "would remove  $(printf '%-10s' "$shape") $shown  $(human "$sz")$extra"
    total=$((total + sz)); after_worktree "$bds"; return
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
    target|target-part|cargo-build|session|ws-scratch|sweep|ui8-work|pages-tmp|scratch-db)
      if [[ -d $r ]]; then out=$(rm -r --one-file-system -- "$r" 2>&1); rc=$?
      else out=$(rm -f -- "$r" 2>&1); rc=$?; fi
      [[ $rc == 0 ]] || { refuse "$shown" "rm: ${out//$'\n'/ }"; return; } ;;
    pipeline-log)
      out=$(rm -f -- "$r" 2>&1) || { refuse "$shown" "rm: $out"; return; } ;;
  esac
  # a hash's XX/ parent goes when its last build-dir does (rmdir: only if empty)
  [[ $shape == cargo-build ]] && rmdir -- "$(dirname "$r")" 2>/dev/null
  say "removed  $(printf '%-10s' "$shape") $shown  $(human "$sz")$extra"
  logline "removed $shape $shown $sz bytes$extra"
  total=$((total + sz))
  after_worktree "$bds"
}
# CI-2: a worktree's build-dirs go with it (each judged as its own path)
after_worktree() { local d
  [[ -n $1 ]] || return 0
  while IFS= read -r d; do [[ -n ${seen[$d]:-} ]] && continue; seen[$d]=1; remove_path "$d"; done <<<"$1"
}
mins() { if (( $1 > 0 && $1 % 60 == 0 )); then echo "$(( $1 / 60 ))h"; else echo "${1}m"; fi; }

# ── NAME → its leftovers ───────────────────────────────────────────────────
paths_for_name() {
  local n=$1 f repo s db=()
  if [[ ! $n =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ || $n == *..* ]]; then
    refuse "$n" 'not a gate name (letters, digits, . _ -; no ..)'; return
  fi
  # SCRATCH-NAMES: the server's database and its other services' files, named after it
  for s in "" .ui .media .docs .quota; do db+=("$TMPR/menger-$n$s.db" "$TMPR/menger-$n$s.db-shm" "$TMPR/menger-$n$s.db-wal"); done
  for f in "$WS/.$n" "$WS/$n" "$CT/$n" "$SCR/$n" "${db[@]}" "$TMPR/menger-$n.media.blobs" \
           "$TMPR/menger-$n.sessions.jsonl" "$TMPR/menger-$n.sessions.jsonl.lock" "$TMPR/menger-$n.log"; do
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
  for f in "$WS"/.[!.]* "$CT"/* "$CB"/*/* "$TMPR"/menger-*.db "$TMPR"/menger-*.db-shm "$TMPR"/menger-*.db-wal "$TMPR"/menger-*.sessions.jsonl "$TMPR"/menger-*.sessions.jsonl.lock \
           "$TMPR"/menger-*.log "$TMPR"/menger-*.media.blobs "$SCR"/* "$TMPR/theme-sweep"; do
    [[ -e $f || -L $f ]] && printf '%s\n' "$f"
  done
  ui8_dirs all
}
# RUN-34: the ui8-* entries directly in each scratch root — `all` of them (--list
# judges each), or only the work directories holding the marks (the sweep's)
ui8_dirs() { local s f
  for s in "${SCRATCH_ROOTS[@]}"; do
    for f in "$s"/ui8-?*; do
      [[ -e $f || -L $f ]] || continue
      ui8_name "$(basename "$f")" || continue   # one path per line: a name with a newline never passes
      [[ $1 == all ]] || { [[ -d $f && ! -L $f ]] && has_ui8_marks "$f"; } || continue
      printf '%s\n' "$f"
    done
  done
}

# ── --sweep: build output, each part judged by its idleness ───────────────
sweep_candidates() {
  local d s
  for d in "$CB"/*/*; do [[ -d $d ]] && cargo_build_shape "$d" && printf '%s\n' "$d"; done
  # CI-6: green pipeline logs older than LOG_DAYS (a red run's stay: <stamp>.red)
  if [[ -d $PLOGS ]]; then
    find "$PLOGS" -maxdepth 1 -type f -name '*.log' -mtime "+$LOG_DAYS" -print 2>/dev/null \
      | while IFS= read -r d; do s=$(basename "$d"); [[ -e $PLOGS/${s%%-*}.red ]] || printf '%s\n' "$d"; done
  fi
  for d in "$CT"/*; do
    [[ -e $d || -L $d ]] || continue
    s=$(basename "$d")
    if shared_target "$s"; then
      find "$d" -mindepth 2 -maxdepth 3 -type d \( -name deps -o -name incremental -o -name build -o -name .fingerprint \) \
        -prune -print 2>/dev/null
    else printf '%s\n' "$d"; fi
  done
  ui8_dirs marked
  # V2-38: a page-test run's work directory, left by a run that was killed
  for d in "$SCR"/ui8-pages-?*; do [[ -d $d && ! -L $d ]] && printf '%s\n' "$d"; done
}
# the disk's use, in percent, where the build output lives
used_pct() {
  if [[ -n ${CLEANUP_GATE_USED_PCT:-} ]]; then echo "$CLEANUP_GATE_USED_PCT"; return; fi
  df --output=pcent -- "$H" 2>/dev/null | tail -n 1 | tr -dc '0-9'
}
idle_min=$((idle_hours * 60))
if [[ $sweep == 1 ]]; then
  used=$(used_pct); used=${used:-0}
  if (( used >= 90 )); then (( idle_min > 15 )) && idle_min=15
  elif (( used >= 75 )); then (( idle_min > 60 )) && idle_min=60; fi
  say "sweep: $H ${used}% used — build output idle $(mins "$idle_min") goes"
fi

declare -A seen=()
run_all() {
  local p
  while IFS= read -r p; do
    [[ -z $p || -n ${seen[$p]:-} ]] && continue
    seen[$p]=1; remove_path "$p"
  done
}
if [[ $sweep == 1 ]]; then
  run_all < <(sweep_candidates | sort -u)
elif [[ $list == 1 ]]; then
  run_all < <(candidates | sort -u)
elif [[ $mode == path ]]; then
  # each path whole: one with a newline in it is refused, never read as two
  for p in "${args[@]}"; do
    if [[ $p == *$'\n'* ]]; then refuse "${p//$'\n'/\\n}" 'named with a newline'; continue; fi
    run_all <<<"$p"
  done
else
  for n in "${args[@]}"; do
    found=$(paths_for_name "$n")
    if [[ $found == refused* ]]; then printf '%s\n' "$found"; refused=1; continue; fi
    [[ -z $found ]] && { say "nothing  left by $n"; continue; }
    run_all <<<"$found"
  done
fi

if [[ $yes == 1 ]]; then
  say "freed $(human "$total")"
  [[ $sweep == 1 ]] && logline "sweep ${used}% used, idle $(mins "$idle_min"): freed $total bytes"
else say "would free $(human "$total") — a dry run; --yes removes"; fi
[[ $refused == 0 ]] || exit 2
