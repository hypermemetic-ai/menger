#!/usr/bin/env bash
# DIST-3: publish menger to crates.io — `menger-tickets-api`, then `menger`
# with the app's build inside it (the `embedded-app` feature embeds it).
#
#   scripts/publish.sh stage     build the app, stage the package, list it —
#                                publishes nothing (a dirty tree is allowed:
#                                it stages the working tree)
#   scripts/publish.sh dry-run   stage, then `cargo publish --dry-run` both
#   scripts/publish.sh publish   stage, then `cargo publish` both — for real;
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
# 3. `cargo publish -p menger-tickets-api`, then `cargo publish -p menger
#    --features embedded-app` — the feature only changes what cargo's
#    verification builds, so the build it checks is the one with the app
#    embedded from the packaged dist.
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
  *) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2 ;;
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

case $mode in
  stage)
    say "staged; nothing published (scripts/publish.sh publish publishes it)"
    ;;
  dry-run)
    step "cargo publish --dry-run -p menger-tickets-api"
    cargo publish --dry-run -p menger-tickets-api
    step "cargo publish --dry-run -p menger --features embedded-app"
    cargo publish --dry-run -p menger --features embedded-app
    ;;
  publish)
    step "cargo publish -p menger-tickets-api"
    cargo publish -p menger-tickets-api
    step "cargo publish -p menger --features embedded-app"
    cargo publish -p menger --features embedded-app
    say "published menger-tickets-api and menger from $commit"
    ;;
esac
