#!/usr/bin/env bash
# MENGER-10: the landing page's product visuals — regenerated from the real
# app whenever it changes.
#
#   scripts/landing-media.sh            seed a throwaway menger, capture, encode
#   scripts/landing-media.sh seed       only start the server and seed it (left running)
#   scripts/landing-media.sh capture    only capture + encode (on a server `seed` just made: the clips write)
#   scripts/landing-media.sh encode     only encode what capture left in $WORK/raw
#
# It runs a THROWAWAY menger — its own ports (4470–4472) and a scratch
# database under $WORK — seeded with a sample project (a small product team
# shipping offline mode for a mobile app) through synapse, the same calls
# anyone makes. Never prod's or test's ports (4450–4458), never
# ~/.local/share/menger/tickets.db, never the house tracker's tickets (its
# one read is the UI components' source, for KIT-1: see below).
#
# Needs: a release build of menger with the app embedded, or MENGER_BIN
# pointing at one (cargo build --release --features embedded-app, after
# `cd frontend && trunk build --release`); synapse >= 3.14.2; Playwright in
# perf/node_modules (cd perf && npm ci; or PLAYWRIGHT_FROM, a directory
# whose node_modules has it); ffmpeg (libx264, libvpx-vp9),
# avifenc and cwebp.
#
# Output: landing/media/ — stills light and dark, AVIF + WebP at 1200 px (the
# hero's AVIF at 2400 too), the nav crop at 720; clips as MP4 (H.264) + WebM
# (VP9) with a WebP poster. The raw captures stay in $WORK/raw.
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
WORK=${WORK:-$HOME/.cache/menger-landing-media}
PORT=${PORT:-4470}          # WebSocket (synapse -P)
SHELL_PORT=${SHELL_PORT:-4471}  # the app
REST_PORT=${REST_PORT:-4472}
MENGER_BIN=${MENGER_BIN:-$here/target/release/menger}
OUT=$here/landing/media

for p in "$PORT" "$SHELL_PORT" "$REST_PORT"; do
  if (( p >= 4450 && p <= 4458 )); then echo "landing-media: port $p is prod's or test's — refused" >&2; exit 2; fi
done

mkdir -p "$WORK"
syn() { synapse -P "$PORT" menger "$@" >/dev/null; }
say() { printf 'landing-media: %s\n' "$*" >&2; }

start_server() {
  [[ -x $MENGER_BIN ]] || { say "no binary at $MENGER_BIN (cargo build --release --features embedded-app, or set MENGER_BIN)"; exit 1; }
  rm -f "$WORK"/demo.db*
  PLEXUS_NO_REGISTRY=1 "$MENGER_BIN" --port "$PORT" --shell-port "$SHELL_PORT" --rest-port "$REST_PORT" \
    --db-path "$WORK/demo.db" > "$WORK/server.log" 2>&1 &
  echo $! > "$WORK/server.pid"
  for _ in $(seq 1 100); do
    if synapse -P "$PORT" menger tickets health >/dev/null 2>&1; then say "server up (pid $(cat "$WORK/server.pid"))"; return; fi
    sleep 0.2
  done
  say "the server did not come up — $WORK/server.log"; exit 1
}
stop_server() {
  if [[ -f $WORK/server.pid ]]; then kill "$(cat "$WORK/server.pid")" 2>/dev/null || true; rm -f "$WORK/server.pid"; fi
}

# ── the components ────────────────────────────────────────────────────────
# A decision renders through the house components (<m-decision>, <m-option>,
# …; ART-9's ```component blocks). They are UI code, not tickets, and the
# repository keeps no copy of the tracker: the blocks are read at run time
# from a menger that has them (COMPONENTS_FROM, a WebSocket port; read-only,
# one `get`) and written into the sample's own doc, KIT-1.
COMPONENTS_FROM=${COMPONENTS_FROM:-4455}
COMPONENTS_TICKET=${COMPONENTS_TICKET:-ART-9}
kit_body() {
  synapse -j -P "$COMPONENTS_FROM" menger tickets get --id "$COMPONENTS_TICKET" | python3 -c '
import json, re, sys
body = next(json.loads(l)["content"]["ticket"]["body"] for l in sys.stdin
            if l.strip().startswith("{") and "\"ticket\"" in l)
blocks = [m.group(0) for m in re.finditer(r"^(`{3,}|~{3,})component [^\n]*\n.*?^\1[ \t]*$", body, re.S | re.M)]
if not blocks: sys.exit("no component blocks")
print("The components our tickets use: decisions, inputs, checklists, status steps.\n")
print("\n\n".join(blocks))
'
}

# ── the sample project ─────────────────────────────────────────────────────
# Fieldnote, a note-taking app for field workers; its mobile team ships
# offline mode. OFFLINE is the plan; SYNC (the server side) is hosted under it.
seed() {
  local T="tickets"
  syn $T create --epic KIT --kind doc --title 'Ticket components: decisions, inputs, checklists' --body "$(kit_body)"
  syn $T create --epic OFFLINE --kind scope --title 'Offline mode for the Fieldnote mobile app' --body "$(cat <<'EOF'
Field crews lose signal in basements, tunnels and on site. Today the app shows a spinner and their notes are gone.

**Goal:** every screen works offline; edits sync when the phone is back online, without losing anything and without asking twice.

**Out of scope:** offline search across the whole team's notes (later).

Owner: Priya (mobile lead) · Target: the 4.2 release
EOF
)"
  # 2 — the rollout, waiting on everything below
  syn $T create --epic OFFLINE --kind execution --title 'Beta rollout to 5% of Android and iOS users' \
    --blocked-by OFFLINE-8,OFFLINE-9,OFFLINE-10,OFFLINE-11,OFFLINE-12 --body "$(cat <<'EOF'
1. Ship 4.2-beta behind the `offline_mode` flag.
2. Turn it on for 5% of each platform.
3. Watch sync errors and the outbox depth for 72 hours.
4. Go to 25% if sync errors stay under 0.1% of sessions.
EOF
)"
  # 3 — the storage spike (done), its finding as a doc beneath it
  syn $T create --epic OFFLINE --kind spike --title 'Which local store: SQLite or the platform key-value store?' --body "$(cat <<'EOF'
**Question:** can the platform key-value stores hold 20,000 notes with attachments, or do we need SQLite on both platforms?

**Answer:** SQLite. The key-value stores slowed to 1.8 s per list query at 20k notes on a Moto G7; SQLite answered in 40 ms. Numbers in the findings below.
EOF
)"
  syn $T create --epic OFFLINE --kind build --title 'Local SQLite cache for notes, projects and photos' --blocked-by OFFLINE-3 --body "$(cat <<'EOF'
One SQLite file per signed-in user. Notes, projects and photo metadata are cached on first open and kept current by the sync engine. Photos are stored as files beside it.

- [x] schema + migrations
- [x] read path: every list screen reads the cache first
- [x] eviction: photos older than 30 days, least recently opened first
EOF
)"
  syn $T create --epic OFFLINE --kind build --title 'Outbox: queue every write made offline' --blocked-by OFFLINE-4 --body "$(cat <<'EOF'
Every create, edit and delete goes to an outbox table first, then to the server. A write is removed from the outbox only when the server acknowledges it, so a crash or a dead battery loses nothing.
EOF
)"
  syn $T create --epic OFFLINE --kind build --title 'Sync engine: replay the outbox on reconnect' --blocked-by OFFLINE-5,SYNC-2 --body "$(cat <<'EOF'
On reconnect the engine sends the outbox in order, then pulls everything changed since the last cursor.

```mermaid
sequenceDiagram
  participant App
  participant Outbox
  participant Server
  App->>Outbox: write (offline)
  Note over App,Server: connection comes back
  Outbox->>Server: replay writes in order
  Server-->>Outbox: ack each write
  App->>Server: changes since cursor
  Server-->>App: deltas + new cursor
```

Retries back off from 2 s to 5 min. A write the server rejects is kept and shown to the author, never dropped.
EOF
)"
  syn $T create --epic OFFLINE --kind spike --title 'Conflict policy for a note edited on two devices' --body "$(cat <<'EOF'
Two people edit the same note while offline. When both sync, what happens?

<m-decision data-field="decision" id="policy">
<m-question data-field="question">When two offline edits collide, which one wins?</m-question>
<m-option data-field="option" name="Merge per field" value="merge" recommended cost="a field-level change log" benefit="both edits survive in most cases"></m-option>
<m-option data-field="option" name="Last writer wins" value="lww" cost="silent data loss" benefit="nothing new to build"></m-option>
<m-option data-field="option" name="Ask the user" value="ask" cost="a prompt on every collision" benefit="no surprises"></m-option>
<m-recommendation data-field="recommendation">Merge per field: crews edit different fields of the same note (status, photos, text) far more often than the same field.</m-recommendation>
</m-decision>
EOF
)"
  syn $T create --epic OFFLINE --kind build --title 'Conflict screen: show both versions when a merge cannot decide' --blocked-by OFFLINE-6,OFFLINE-7 --body 'When the same field changed on two devices, show both values side by side and let the author pick. Never pick silently.'
  syn $T create --epic OFFLINE --kind build --title 'Offline banner and sync status in the header' --body "$(cat <<'EOF'
A thin banner when the phone is offline: *Working offline — 3 changes will sync*. It turns into a check mark for two seconds once everything has synced.
EOF
)"
  syn $T create --epic OFFLINE --kind gap --title 'Photos attached offline are lost if the app is killed before sync' --body "$(cat <<'EOF'
**Steps:** airplane mode → attach two photos to a note → swipe the app away → reopen.

**Expected:** both photos on the note, waiting to sync. **Actual:** the note is there, the photos are not.

The photo is written to a temp directory the OS clears. Move it into the cache directory before the outbox entry is written.
EOF
)"
  syn $T create --epic OFFLINE --kind build --title 'Background sync on iOS (BGTaskScheduler)' --blocked-by OFFLINE-6 --body 'Register a processing task that drains the outbox when the phone is charging or on Wi-Fi. iOS decides when it runs; the foreground sync stays the guarantee.'
  syn $T create --epic OFFLINE --kind build --title 'Background sync on Android (WorkManager)' --blocked-by OFFLINE-6 --body 'A unique periodic work request with a network constraint drains the outbox. Expedited when the outbox holds more than 20 writes.'

  # SYNC — the server side, hosted under OFFLINE
  syn $T create --epic SYNC --kind scope --title 'Sync API on the server' --parent OFFLINE-1 --body 'What the server needs for the mobile app to sync: changes since a cursor, deletes that sync, and a cursor that survives deploys.'
  syn $T create --epic SYNC --kind build --title 'Delta endpoint: everything changed since a cursor' --body '`GET /sync/changes?cursor=…` answers the changes after the cursor, oldest first, 500 at a time, with the next cursor.'
  syn $T create --epic SYNC --kind build --title 'Tombstones, so deletes sync too' --blocked-by SYNC-2 --body 'A deleted note leaves a tombstone for 90 days, so a phone that was offline for a month still learns about the delete.'
  syn $T create --epic SYNC --kind gap --title 'Cursor resets to zero after a server deploy' --body 'The cursor is an in-memory counter. After a deploy every phone downloads everything again. Store it with the change log.'
  syn $T create --epic SYNC --kind build --title 'Rate-limit the delta endpoint per device' --body 'A phone that reconnects in a loop must not starve the others: 60 requests a minute per device, 429 with Retry-After past that.'

  # 17 — the spike's finding, filed beneath it (a doc: no lifecycle)
  syn $T create --epic OFFLINE --kind doc --title 'Findings: local store benchmarks on a low-end Android phone' --parent OFFLINE-3 --body "$(cat <<'EOF'
Moto G7, Android 10, 20,000 notes, 3,000 photos (metadata only).

| store | open | list 50 notes | search 1 word |
|---|---|---|---|
| key-value | 2.4 s | 1.8 s | 6.1 s |
| SQLite | 0.2 s | 40 ms | 120 ms |

SQLite with an FTS index wins on every query. Recommendation: SQLite on both platforms.
EOF
)"

  # where the work is
  local s
  for s in OFFLINE-3 OFFLINE-4 OFFLINE-5 SYNC-2; do syn $T set_status --id "$s" --status Complete; done
  for s in OFFLINE-6; do syn $T set_status --id "$s" --status 'In Review'; done
  for s in OFFLINE-9 SYNC-3; do syn $T set_status --id "$s" --status Coding; done
  for s in OFFLINE-7 OFFLINE-8 OFFLINE-10 SYNC-4 SYNC-5; do syn $T set_status --id "$s" --status Ready; done
  syn $T set_status --id OFFLINE-1 --status Epic
  syn $T set_status --id SYNC-1 --status Epic
  # the commits that finished the work (the sample repository's)
  syn $T link_commits --id OFFLINE-3 --commits 3f9c2a1
  syn $T link_commits --id OFFLINE-4 --commits 8b41d07,c2e9f55
  syn $T link_commits --id OFFLINE-5 --commits 51aa0e3
  syn $T link_commits --id OFFLINE-6 --commits 9d7e214
  syn $T link_commits --id SYNC-2 --commits fieldnote-server@4c0b8e2

  # a reader's comments on the app, and notes not yet tickets
  syn comments add --anchor.route '#/ticket/OFFLINE-10' --anchor.path '#ticket-page h2' --anchor.label 'Title' \
    --anchor.excerpt 'Photos attached offline are lost' --anchor.ticket OFFLINE-10 --author priya \
    --body 'Seen twice on the Galaxy A12 too. Raising to the 4.2 blockers.'
  syn comments add --anchor.route '#/ticket/OFFLINE-2' --anchor.path '#ticket-page h2' --anchor.label 'Title' \
    --anchor.excerpt 'Beta rollout' --anchor.ticket OFFLINE-2 --author sam \
    --body 'Can we start with 1% on iOS? TestFlight users are our most active.'
  syn notes add --author priya --body 'Offline search across the team: worth a spike after 4.2?'
  syn notes add --author sam --body 'Show the outbox depth in the debug menu so support can ask for it.'
  say "seeded: KIT-1, OFFLINE-1…13, SYNC-1…5, 2 comments, 2 notes"
}

capture() {
  node "$here/scripts/landing-media.mjs" --port "$SHELL_PORT" --ws-port "$PORT" --raw "$WORK/raw"
}

# ── encode ───────────────────────────────────────────────────────────────
# Every still at 1200 and 2400 px, AVIF and WebP, so a 2× screen gets 2×
# pixels at any slot up to 1200 CSS px (the hero), and at quality high enough
# that the app's small UI text stays crisp (owner, 2026-10-12: "the
# screenshot is also blurry"). The browser picks by srcset/sizes.
#   raw/stills/<name>.<theme>.png → <name>.<theme>.{1200,2400}.{avif,webp}
# Clips: H.264 (every browser) and VP9 WebM, muted; the poster is the last
# frame (the change made), WebP.
#   raw/clips/<clip>.mp4 → <clip>.{mp4,webm} + <clip>.poster.webp
HERO=board
avif() { avifenc -q "$2" -s 4 -y 444 -a tune=ssim "$1" "$3" >/dev/null; }
encode() {
  mkdir -p "$OUT"
  local f base w
  for f in "$WORK"/raw/stills/*.png; do
    base=$(basename "$f" .png)
    for w in 1200 2400; do
      ffmpeg -loglevel error -y -i "$f" -vf "scale=$w:-2:flags=lanczos" "$WORK/tmp.png"
      avif "$WORK/tmp.png" 80 "$OUT/$base.$w.avif"
      cwebp -quiet -q 90 -m 6 "$WORK/tmp.png" -o "$OUT/$base.$w.webp"
    done
  done
  # the nav tree alone, cropped from the board (its first 360 × 480 CSS px), for "a plan is a tree"
  local theme
  for theme in light dark; do
    ffmpeg -loglevel error -y -i "$WORK/raw/stills/board.$theme.png" -vf "crop=720:960:0:0" "$WORK/tmp.png"
    avif "$WORK/tmp.png" 80 "$OUT/nav.$theme.720.avif"
    cwebp -quiet -q 90 -m 6 "$WORK/tmp.png" -o "$OUT/nav.$theme.720.webp"
  done
  local clip
  for clip in "$WORK"/raw/clips/*.mp4; do
    base=$(basename "$clip" .mp4)
    ffmpeg -loglevel error -y -i "$clip" -an -c:v libx264 -preset veryslow -tune animation -crf 30 \
      -pix_fmt yuv420p -movflags +faststart "$OUT/$base.mp4"
    ffmpeg -loglevel error -y -i "$clip" -an -c:v libvpx-vp9 -b:v 0 -crf 56 -row-mt 1 -deadline good -cpu-used 1 \
      "$OUT/$base.webm"
    ffmpeg -loglevel error -y -sseof -0.1 -i "$clip" -frames:v 1 -update 1 "$WORK/tmp.png"
    cwebp -quiet -q 72 -m 6 "$WORK/tmp.png" -o "$OUT/$base.poster.webp"
  done
  rm -f "$WORK/tmp.png"
  du -cb "$OUT"/* | tail -1 >&2
}

case "${1:-all}" in
  seed) start_server; seed ;;
  capture) capture; encode ;;
  encode) encode ;;
  all) trap stop_server EXIT; start_server; seed; capture; encode ;;
  stop) stop_server ;;
  *) echo "usage: $0 [all|seed|capture|encode|stop]" >&2; exit 2 ;;
esac
