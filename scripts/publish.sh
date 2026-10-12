#!/usr/bin/env bash
# DIST-3: publish menger to crates.io — `plexus-shell` (SCAFFOLD-5),
# `menger-tickets-api`, `plexus-login` (SCAFFOLD-9), then `menger` with the app's build inside it (the
# `embedded-app` feature embeds it). A crate whose version is already on
# crates.io is skipped, saying so .
#
#   scripts/publish.sh stage     build the app, stage the package, list it —
#                                publishes nothing (a dirty tree is allowed:
#                                it stages the working tree)
#   scripts/publish.sh dry-run   stage, then `cargo publish --dry-run` of the
#                                unpublished crates, verified together
#   scripts/publish.sh publish   stage, then `cargo publish` them — for real;
#                                refuses a dirty tree
#
# 1. `trunk build --release` in frontend/ (README "Frontend" has the
#    toolchain). The app's build is frontend/dist; no build, no publish.
# 2. The package is staged in a directory beside the repository (a sibling,
#    so the plexus crates' relative paths — ../plexus-core … — resolve):
#    the files git keeps (tracked, and untracked but not ignored), plus
#    frontend/dist, minus frontend/Cargo.toml. Why a stage: cargo never
#    packages a file under a directory holding another package's
#    Cargo.toml (frontend/ is the app's own crate), and in a git repository
#    never a gitignored one (frontend/dist is), whatever `include` says. In
#    the stage frontend/ is a plain directory and there is no repository,
#    so the package carries what `include` names. The cost: the published
#    crate has no .cargo_vcs_info.json (the commit is in this script's
#    output instead).
# 3. One `cargo publish -p plexus-shell -p menger-tickets-api -p plexus-login -p menger
#    --features menger/embedded-app` over the crates not yet on crates.io.
#    Cargo publishes them in dependency order and, in a dry run, verifies
#    each against the others' packages, so menger's verification resolves a
#    plexus-shell that is not on crates.io yet. The feature only changes what
#    cargo's verification builds, so the build it checks is the one with the
#    app embedded from the packaged dist.
#
# Publishing needs the plexus 0.6 line (plexus-core, plexus-macros,
# plexus-client, plexus-auth-core, plexus-transport) on crates.io first
# (DIST-1 step 1): until then `dry-run` stops at resolving them. A crates.io
# token comes from `cargo login` or CARGO_REGISTRY_TOKEN.
set -euo pipefail

REPO=$(git -C "$(dirname "$(readlink -f "$0")")" rev-parse --show-toplevel)
STAGE=$(dirname "$REPO")/.menger-publish

say()  { printf '%s\n' "$*" >&2; }
die()  { say "publish: $*"; exit 1; }
step() { say "── $*"; }
need() { command -v "$1" >/dev/null || die "needs $1 on PATH"; }

mode=${1:-}
case $mode in
  stage|dry-run|publish) ;;
  *) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2 ;;
esac
need cargo; need trunk; need git

if [[ $mode == publish && -n $(git -C "$REPO" status --porcelain) ]]; then
  die "the working tree is dirty — publish a commit (commit or stash first)"
fi
commit=$(git -C "$REPO" rev-parse --short HEAD)

step "app: trunk build --release (frontend/)"
(cd "$REPO/frontend" && trunk build --release) || die "trunk build failed"
[[ -f $REPO/frontend/dist/index.html ]] \
  || die "no app build at frontend/dist (index.html missing) — menger is not published without it"

step "stage: $STAGE ($commit)"
rm -rf "$STAGE"
mkdir -p "$STAGE"
(
  cd "$REPO"
  git ls-files -z --cached --others --exclude-standard | while IFS= read -r -d '' f; do
    # A tracked file deleted in the working tree is not staged.
    if [[ -e $f || -L $f ]]; then cp --parents -d -- "$f" "$STAGE"; fi
  done
)
mkdir -p "$STAGE/frontend"
cp -R "$REPO/frontend/dist" "$STAGE/frontend/dist"
# The app's crate is not built from the stage; without its manifest,
# frontend/ is a directory cargo packages from.
rm -f "$STAGE/frontend/Cargo.toml"

cd "$STAGE"
# The checkout's build directory, so verification reuses what it compiled.
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target}
list=$(cargo package -p menger --list --no-verify --allow-dirty 2>/dev/null)
grep -qx 'frontend/dist/index.html' <<<"$list" \
  || die "the staged package does not carry frontend/dist (is $STAGE inside a git repository that does not ignore it?)"
if grep -E '^(tests|deploy|perf|diagrams|spike)/' <<<"$list" >/dev/null; then
  die "the staged package carries files it should not: $(grep -E '^(tests|deploy|perf|diagrams|spike)/' <<<"$list" | head -3)"
fi
say "menger's package: $(wc -l <<<"$list") files, $(grep -c '^frontend/dist/' <<<"$list") of them the app's build"
# The .crate's size, near enough (cargo's own `package` resolves the plexus
# line from crates.io, so it can only run once that is published).
size=$(grep -vx -e 'Cargo.toml.orig' -e '.cargo_vcs_info.json' <<<"$list" | tar -czf - -T - | wc -c)
say "menger's package: about $((size / 1024)) KiB compressed (crates.io takes up to 10 MiB)"
(( size < 10 * 1024 * 1024 )) || die "too big for crates.io"

shell_list=$(cargo package -p plexus-shell --list --no-verify --allow-dirty 2>/dev/null)
if grep -E '^(toy|target)/' <<<"$shell_list" >/dev/null; then
  die "plexus-shell's package carries files it should not: $(grep -E '^(toy|target)/' <<<"$shell_list" | head -3)"
fi
say "plexus-shell's package: $(wc -l <<<"$shell_list") files"

# A crate version crates.io already holds is skipped (crates.io refuses a
# second upload of a version). Its sparse index answers which it holds.
version_of() { sed -n 's/^version = "\(.*\)"/\1/p' "$1/Cargo.toml" | head -1; }
published() {
  local name=$1 version=$2 path
  case ${#name} in
    1) path=1/$name ;;
    2) path=2/$name ;;
    3) path=3/${name:0:1}/$name ;;
    *) path=${name:0:2}/${name:2:2}/$name ;;
  esac
  curl -sf "https://index.crates.io/$path" | grep -q "\"vers\":\"$version\""
}
need curl
crates=()
for pair in plexus-shell:plexus-shell menger-tickets-api:tickets-api plexus-login:plexus-login menger:.; do
  name=${pair%%:*}
  version=$(version_of "$STAGE/${pair#*:}")
  if published "$name" "$version"; then
    say "$name $version is on crates.io already — skipped"
  else
    crates+=(-p "$name")
  fi
done
(( ${#crates[@]} )) || die "every crate's version is on crates.io already — bump a version first"
features=()
[[ " ${crates[*]} " == *" menger "* ]] && features=(--features menger/embedded-app)

case $mode in
  stage)
    say "staged; nothing published (scripts/publish.sh publish publishes ${crates[*]})"
    ;;
  dry-run)
    step "cargo publish --dry-run ${crates[*]} ${features[*]}"
    cargo publish --dry-run "${crates[@]}" "${features[@]}"
    ;;
  publish)
    step "cargo publish ${crates[*]} ${features[*]}"
    cargo publish "${crates[@]}" "${features[@]}"
    say "published ${crates[*]} from $commit"
    ;;
esac
