#!/bin/sh
# CI-2: install the build-output sweep as a systemd --user timer.
#   scripts/sweep/install.sh            copy the scripts and the units, daemon-reload
#   scripts/sweep/install.sh --start    … and enable --now the timer, and run one sweep
# It runs from an installed copy (~/.local/share/menger-sweep), so a
# checkout's branch never changes what runs; run this again from main to
# update it (its output names the revision installed). `systemctl --user list-timers menger-sweep.timer` says when it
# runs next; `journalctl --user -u menger-sweep` what it did.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
dest="$HOME/.local/share/menger-sweep"
units="$HOME/.config/systemd/user"
mkdir -p "$dest" "$units"
install -m 0755 "$repo/scripts/cleanup-gate.sh" "$dest/cleanup-gate.sh"
install -m 0755 "$repo/scripts/disk-check.sh" "$dest/disk-check.sh"
install -m 0755 "$here/run" "$dest/run"
git -C "$repo" rev-parse --short HEAD > "$dest/REVISION" 2>/dev/null || true
install -m 0644 "$here/menger-sweep.service" "$units/menger-sweep.service"
install -m 0644 "$here/menger-sweep.timer" "$units/menger-sweep.timer"
systemctl --user daemon-reload
rev=$(cat "$dest/REVISION" 2>/dev/null || echo '?')
echo "installed $rev into $dest; units menger-sweep.service, menger-sweep.timer"
# RUN-34: a merged gate is not the running one until this runs again
echo "the timer runs this installed copy ($dest, revision $rev), not a checkout: a change merged after $rev runs only once install.sh runs again from main"
echo "its settings: ~/.config/menger/sweep.env (optional; MENGER_SCRATCH_DIRS=DIR:DIR, scratch roots beyond ~/.cache/menger-perf-scratch)"
if [ "${1:-}" = "--start" ]; then
  systemctl --user enable --now menger-sweep.timer
  systemctl --user start menger-sweep.service
  systemctl --user --no-pager list-timers menger-sweep.timer | head -3
fi
