#!/bin/sh
# CHAT-11: install the chat bridge as a systemd --user unit.
#   scripts/chat-bridge/install.sh            copy the bridge and the unit, daemon-reload
#   scripts/chat-bridge/install.sh --start    … and enable --now (or restart, when running)
# The bridge runs from an installed copy (~/.local/share/menger-chat-bridge),
# so a checkout's branch or rebase never changes what is running; run this
# again from main to update it. The settings file and the allow list are
# made once and never rewritten.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
dest="$HOME/.local/share/menger-chat-bridge"
mkdir -p "$dest/scripts/chat-bridge" "$dest/deploy" "$HOME/.config/systemd/user" "$HOME/.config/menger"
install -m 0644 "$repo/scripts/chat-bridge/bridge.mjs" "$dest/scripts/chat-bridge/bridge.mjs"
install -m 0644 "$repo/scripts/chat-bridge/live.mjs" "$dest/scripts/chat-bridge/live.mjs"
install -m 0644 "$repo/scripts/chat-bridge/models.mjs" "$dest/scripts/chat-bridge/models.mjs"
install -m 0644 "$repo/scripts/chat-bridge/reply.mjs" "$dest/scripts/chat-bridge/reply.mjs"
install -m 0644 "$repo/deploy/menger-call.mjs" "$dest/deploy/menger-call.mjs"
git -C "$repo" rev-parse --short HEAD > "$dest/REVISION" 2>/dev/null || true
install -m 0644 "$here/menger-chat-bridge.service" "$HOME/.config/systemd/user/menger-chat-bridge.service"
env="$HOME/.config/menger/chat-bridge.env"
[ -e "$env" ] || sed "s|/home/hypermemetic|$HOME|g" "$here/chat-bridge.env.example" | install -m 0600 /dev/stdin "$env"
arch="$HOME/.config/menger/archetypes.env"
[ -e "$arch" ] || install -m 0644 "$here/archetypes.env.example" "$arch"
allow="$HOME/.config/menger/chat-bridge.allow"
[ -e "$allow" ] || printf '# CHAT-11: the principals the chat bridge answers, one a line. # comments.\n# key:/agent: lines are ignored; +key:… admits a key explicitly (tests only, removed after).\n' | install -m 0600 /dev/stdin "$allow"
systemctl --user daemon-reload
echo "installed $(cat "$dest/REVISION" 2>/dev/null || echo '?') into $dest; unit menger-chat-bridge.service; settings $env; archetypes $arch; allow list $allow"
if [ "${1:-}" = "--start" ]; then
  systemctl --user enable menger-chat-bridge.service
  systemctl --user restart menger-chat-bridge.service
  systemctl --user --no-pager status menger-chat-bridge.service | head -5
fi
