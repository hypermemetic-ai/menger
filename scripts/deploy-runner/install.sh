#!/bin/sh
# DEPLOY-37: install the deploy runner as a systemd --user unit.
#   scripts/deploy-runner/install.sh            copy the runner and the unit, daemon-reload
#   scripts/deploy-runner/install.sh --start    … and enable --now (or restart, when running)
# The runner runs from an installed copy (~/.local/share/menger-deploy-runner),
# so a checkout's branch never changes what is running; run this again from
# main to update it. The settings file and the webhook's secret are made once
# (the secret 0600, random, never printed) and never rewritten.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
dest="$HOME/.local/share/menger-deploy-runner"
mkdir -p "$dest/deploy" "$HOME/.config/systemd/user" "$HOME/.config/menger"
install -m 0644 "$repo/deploy/runner.mjs" "$dest/deploy/runner.mjs"
install -m 0644 "$repo/deploy/menger-call.mjs" "$dest/deploy/menger-call.mjs"
git -C "$repo" rev-parse --short HEAD > "$dest/REVISION" 2>/dev/null || true
install -m 0644 "$here/menger-deploy-runner.service" "$HOME/.config/systemd/user/menger-deploy-runner.service"
env="$HOME/.config/menger/deploy-runner.env"
[ -e "$env" ] || sed "s|/home/hypermemetic|$HOME|g" "$here/deploy-runner.env.example" | install -m 0600 /dev/stdin "$env"
secret="$HOME/.config/menger/deploy-runner.secret"
[ -e "$secret" ] || (umask 077 && head -c 32 /dev/urandom | base64 | tr -d '\n=' > "$secret")
systemctl --user daemon-reload
echo "installed $(cat "$dest/REVISION" 2>/dev/null || echo '?') into $dest; unit menger-deploy-runner.service; settings $env; webhook secret $secret (0600)"
if [ "${1:-}" = "--start" ]; then
  systemctl --user enable menger-deploy-runner.service
  systemctl --user restart menger-deploy-runner.service
  systemctl --user --no-pager status menger-deploy-runner.service | head -5
fi
