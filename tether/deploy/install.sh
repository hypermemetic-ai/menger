#!/bin/sh
# TETHER-9: install menger tether as a systemd --user unit.
#   tether/deploy/install.sh            build (release), install the binary and the unit, daemon-reload
#   tether/deploy/install.sh --start    … and enable --now (or restart, when running)
# The unit runs the installed binary (~/.local/bin/tether), so a checkout's
# branch never changes what is running; run this again from main to update
# it. The settings file is made once and never rewritten.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
crate=$(cd "$here/.." && pwd)
target="${CARGO_TARGET_DIR:-$HOME/.cache/cargo-target/menger-tether}"
CARGO_TARGET_DIR="$target" cargo build --release --manifest-path "$crate/Cargo.toml" --bin tether
mkdir -p "$HOME/.local/bin" "$HOME/.config/systemd/user" "$HOME/.config/menger"
install -m 0755 "$target/release/tether" "$HOME/.local/bin/tether"
git -C "$crate" rev-parse --short HEAD > "$HOME/.local/bin/.tether-revision" 2>/dev/null || true
install -m 0644 "$here/tether.service" "$HOME/.config/systemd/user/tether.service"
env="$HOME/.config/menger/tether.env"
if [ ! -e "$env" ]; then
  admin=$(synapse _key show builder 2>/dev/null | grep -o 'key:[0-9a-f]*' | head -1 || true)
  sed -e "s|/home/hypermemetic|$HOME|g" -e "s|key:REPLACE-WITH-synapse-_key-show-builder|${admin:-key:REPLACE-WITH-synapse-_key-show-builder}|" \
    "$here/tether.env.example" | install -m 0600 /dev/stdin "$env"
fi
systemctl --user daemon-reload
echo "installed tether $(cat "$HOME/.local/bin/.tether-revision" 2>/dev/null || echo '?') as ~/.local/bin/tether; unit tether.service; settings $env"
if [ "${1:-}" = "--start" ]; then
  systemctl --user enable tether.service
  systemctl --user restart tether.service
  systemctl --user --no-pager status tether.service | head -5
fi
