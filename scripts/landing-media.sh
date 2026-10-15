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
# (VP9) with a WebP poster, and as an animated WebP (DESIGN-46: the page's
# <img>, which loads like a GIF). The raw captures stay in $WORK/raw.
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
  # DESIGN-41: ART-9's blocks are past the shell's 128 KB argument cap
  # (SYNAPSE-10) since DESIGN-40, so this one create goes over the WebSocket
  # through deploy/menger-call.mjs, whose params ride stdin.
  kit_body | python3 -c '
import json, sys
print(json.dumps({"epic": "KIT", "kind": "doc", "title": "Ticket components: decisions, inputs, checklists", "body": sys.stdin.read()}))
' | node "$here/deploy/menger-call.mjs" --url "ws://127.0.0.1:$PORT" tickets.create > /dev/null
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

  # DESIGN-48 — what the build agents put inside their tickets: the screen
  # mocked up before it is built, with the one question that decides it
  # (OFFLINE-14, a doc under the conflict-screen build); then the help site
  # (OFFLINE-15, OFFLINE-16, below).
  syn $T create --epic OFFLINE --kind doc --title 'Mockup · the conflict screen, both versions side by side' --parent OFFLINE-8 --body "$(cat <<'EOF'
Filed by the build agent on OFFLINE-8 before any code: the screen as it would look on a phone, and the one question that decides it. The layout is the only thing worth asking about; the rest follows the design system.

<style>
.mock { display: grid; grid-template-columns: 300px minmax(0, 1fr); gap: 28px; align-items: start; margin: 8px 0 20px; }
.phone { width: 300px; border: 6px solid #1c1c22; border-radius: 30px; background: #fafaf7; color: #1a1a1e; font: 14px/1.4 system-ui, sans-serif; overflow: hidden; box-shadow: 0 10px 30px rgba(0,0,0,.18); }
.phone .bar { display: flex; justify-content: space-between; padding: 8px 16px 0; font-size: 11px; color: #555; }
.phone .hd { padding: 10px 16px 8px; font-weight: 600; font-size: 15px; border-bottom: 1px solid #e6e4dc; }
.phone .hd small { display: block; font-weight: 400; font-size: 12px; color: #6b6b74; margin-top: 2px; }
.phone .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: #e6e4dc; }
.phone .col { background: #fafaf7; padding: 10px 12px 12px; min-height: 180px; }
.phone .who { font-size: 10.5px; letter-spacing: .08em; text-transform: uppercase; color: #6b6b74; margin-bottom: 6px; }
.phone .who b { color: #1a1a1e; }
.phone p { margin: 0 0 8px; font-size: 13px; }
.phone .diff { background: #fde9c8; border-radius: 3px; padding: 0 2px; }
.phone .keep { display: block; width: 100%; margin-top: 10px; padding: 8px 0; border: 1px solid #1a5fb4; border-radius: 8px; background: transparent; color: #1a5fb4; font: inherit; font-size: 13px; font-weight: 600; }
.phone .keep.mine { background: #1a5fb4; color: #fff; }
.phone .ft { padding: 10px 16px 14px; font-size: 12px; color: #6b6b74; }
.mock .notes { font-size: 14px; color: #55555e; }
.mock .notes li { margin: 0 0 8px; }
@media (max-width: 720px) { .mock { grid-template-columns: 1fr; } }
</style>
<div class="mock">
<div class="phone" role="img" aria-label="A phone showing the conflict screen: two versions of one field side by side, Keep mine and Keep theirs under each">
<div class="bar"><span>9:41</span><span>offline · 3 to sync</span></div>
<div class="hd">This note changed on two devices<small>Site 14 · pump room · field “Condition”</small></div>
<div class="cols">
<div class="col"><div class="who"><b>Yours</b> · 08:12</div><p>Seal weeping at the inlet flange; <span class="diff">torqued to 45 Nm</span>, recheck Friday.</p><button class="keep mine" type="button">Keep mine</button></div>
<div class="col"><div class="who"><b>Priya’s</b> · 08:40</div><p>Seal weeping at the inlet flange; <span class="diff">replacement ordered (PN 2231)</span>.</p><button class="keep" type="button">Keep theirs</button></div>
</div>
<div class="ft">Everything else on the note merged on its own.</div>
</div>
<ul class="notes">
<li>Only the field that collided is shown; the rest of the note merged (OFFLINE-7’s policy).</li>
<li>Each version carries who and when; the differing span is marked.</li>
<li>One tap decides; nothing is picked silently (OFFLINE-8).</li>
</ul>
</div>

<m-decision data-field="decision" id="layout">
<m-question data-field="question">Both versions side by side, or stacked with the differences marked?</m-question>
<m-option data-field="option" name="Side by side" value="side" recommended cost="two narrow columns on small phones" benefit="both texts whole, compared at a glance"></m-option>
<m-option data-field="option" name="Stacked, differences marked" value="stacked" cost="a diff to compute and explain" benefit="one column, reads top to bottom"></m-option>
<m-option data-field="option" name="Mine on top, theirs on tap" value="tap" cost="the other version is hidden behind a tap" benefit="the simplest screen"></m-option>
<m-recommendation data-field="recommendation">Side by side: crews compare two short field notes, not two pages, and the collided field is rarely more than a line.</m-recommendation>
</m-decision>
EOF
)"
  # The help site (DESIGN-48, re-cut on the owner's word): the build agent on
  # OFFLINE-9 maps the help as a graph, builds the page itself — a whole HTML
  # document, filed as a doc with `view: mockup` so the ticket shows it at
  # 390 and 1440 side by side (MENGER-15) — and asks the one question that
  # decides the rest. OFFLINE-15 is the build (the map, the link, the
  # decision); OFFLINE-16, under it, is the page.
  syn $T create --epic OFFLINE --kind build --title 'Help site for offline mode: mapped, the first page built' --parent OFFLINE-9 --body "$(cat <<'EOF'
The banner (OFFLINE-9) links to help. Here is the help as a map, then the first page as built — <m-ticket ref="OFFLINE-16"></m-ticket>, shown at phone and desktop width — and the one question worth asking before the other pages take its shape. Copy is first draft; the shape is the question.

```mermaid
graph LR
  B["the offline banner"] --> H["Working offline"]
  H --> S["What syncs, and when"]
  H --> C["When two edits collide"]
  S --> O["The outbox"]
  C --> K["Keep mine / keep theirs"]
```

<m-decision data-field="decision" id="shape">
<m-question data-field="question">One page with the three parts, or a page for each?</m-question>
<m-option data-field="option" name="One page" value="one" recommended cost="a longer scroll on a phone" benefit="everything a crew needs on one screen, found by one search"></m-option>
<m-option data-field="option" name="Three pages" value="three" cost="three places to keep in step with the app" benefit="each short, each linkable from its banner"></m-option>
<m-option data-field="option" name="One page, collisions apart" value="split" cost="two places to keep in step" benefit="the rare case explained at length, the common one kept short"></m-option>
<m-recommendation data-field="recommendation">One page: crews read help with no signal, once, in a hurry; a page that scrolls beats three that load.</m-recommendation>
</m-decision>
EOF
)"
  syn $T create --epic OFFLINE --kind doc --title 'Page · Working offline, the help page as built' --parent OFFLINE-15 --body "$(cat <<'EOF'
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light dark">
<meta name="menger-view" content="mockup">
<meta name="menger-widths" content="390,1440">
<title>Working offline — Fieldnote help</title>
<style>
:root { --bg: #fbfaf6; --ink: #1b1b20; --dim: #666670; --line: #e3e1d8; --card: #fff; --acc: #1a5fb4; --acc-ink: #fff; --warn: #fde9c8; --warn-ink: #6b4a10; }
@media (prefers-color-scheme: dark) { :root { --bg: #15151a; --ink: #ececf0; --dim: #a0a0aa; --line: #2c2c34; --card: #1d1d24; --acc: #6ea2e8; --acc-ink: #0d1a2e; --warn: #4a3a1a; --warn-ink: #f3dcae; } }
* { box-sizing: border-box; }
html { background: var(--bg); color: var(--ink); font: 16px/1.55 system-ui, -apple-system, "Segoe UI", sans-serif; }
body { margin: 0; }
a { color: var(--acc); }
.wrap { width: min(1040px, 100% - 40px); margin: 0 auto; }
header { border-bottom: 1px solid var(--line); background: var(--card); position: sticky; top: 0; }
.top { display: flex; align-items: center; gap: 18px; height: 58px; }
.brand { display: inline-flex; align-items: center; gap: 9px; font-weight: 700; color: var(--ink); text-decoration: none; }
.brand i { width: 22px; height: 22px; border-radius: 6px; background: var(--acc); display: inline-block; position: relative; }
.brand i::after { content: ""; position: absolute; left: 6px; top: 6px; width: 10px; height: 10px; border-radius: 2px; background: var(--acc-ink); }
.top nav { margin-left: auto; display: flex; gap: 22px; font-size: 14.5px; }
.top nav a { color: var(--dim); text-decoration: none; }
.top nav a[aria-current] { color: var(--ink); font-weight: 600; }
.hero { padding: 56px 0 36px; }
.crumb { font-size: 13px; color: var(--dim); text-transform: uppercase; letter-spacing: .08em; margin: 0 0 10px; }
h1 { font-size: clamp(32px, 5vw, 48px); line-height: 1.08; margin: 0 0 14px; letter-spacing: -.02em; }
.lede { font-size: 19px; color: var(--dim); max-width: 36em; margin: 0; }
.banner { display: inline-flex; align-items: center; gap: 10px; margin: 22px 0 0; padding: 9px 14px; border-radius: 999px; background: var(--warn); color: var(--warn-ink); font-size: 14px; font-weight: 600; }
.banner b { width: 8px; height: 8px; border-radius: 50%; background: currentColor; }
section { padding: 36px 0; border-top: 1px solid var(--line); }
h2 { font-size: 26px; margin: 0 0 8px; letter-spacing: -.01em; }
.sub { color: var(--dim); margin: 0 0 22px; max-width: 40em; }
.steps { display: grid; grid-template-columns: repeat(3, 1fr); gap: 18px; }
.step { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 18px 18px 16px; }
.step b { display: inline-grid; place-items: center; width: 28px; height: 28px; border-radius: 50%; background: var(--acc); color: var(--acc-ink); font-size: 13px; margin-bottom: 10px; }
.step h3 { margin: 0 0 6px; font-size: 17px; }
.step p { margin: 0; color: var(--dim); font-size: 15px; }
.collide { display: grid; grid-template-columns: 1fr 1fr; gap: 18px; }
.ver { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 16px 18px; }
.ver .who { font-size: 12px; letter-spacing: .08em; text-transform: uppercase; color: var(--dim); margin-bottom: 8px; }
.ver p { margin: 0 0 12px; }
.ver mark { background: var(--warn); color: var(--warn-ink); border-radius: 3px; padding: 0 3px; }
.keep { display: inline-block; padding: 8px 14px; border-radius: 8px; border: 1px solid var(--acc); color: var(--acc); font: inherit; font-size: 14px; font-weight: 600; background: transparent; }
.keep.mine { background: var(--acc); color: var(--acc-ink); }
.faq dt { font-weight: 600; margin-top: 14px; }
.faq dd { margin: 4px 0 0; color: var(--dim); }
footer { border-top: 1px solid var(--line); padding: 26px 0 40px; font-size: 13.5px; color: var(--dim); display: flex; gap: 18px; flex-wrap: wrap; }
@media (max-width: 700px) {
  .top nav { gap: 14px; font-size: 13.5px; }
  .top nav a:nth-child(n+3) { display: none; }
  .hero { padding: 36px 0 28px; }
  .steps, .collide { grid-template-columns: 1fr; }
  section { padding: 28px 0; }
}
</style>
</head>
<body>
<header><div class="wrap top"><a class="brand" href="#"><i></i>Fieldnote</a><nav><a href="#" aria-current="page">Help</a><a href="#">Guides</a><a href="#">Status</a><a href="#">Contact</a></nav></div></header>
<main class="wrap">
<div class="hero">
<p class="crumb">Help · the mobile app</p>
<h1>Working offline</h1>
<p class="lede">No signal? Keep going. Every screen works, and every change you make is kept on this phone until it can be sent.</p>
<p class="banner"><b></b>Working offline — 3 changes will sync</p>
</div>
<section id="sync">
<h2>What syncs, and when</h2>
<p class="sub">The banner at the top says how many changes are waiting. It turns into a check mark once everything has synced.</p>
<div class="steps">
<div class="step"><b>1</b><h3>Into the outbox</h3><p>Each change goes into the outbox the moment you make it — notes, photos, project edits.</p></div>
<div class="step"><b>2</b><h3>Sent in order</h3><p>When the phone is back online, the outbox is sent oldest first. A change the server refuses is kept and shown to you, never dropped.</p></div>
<div class="step"><b>3</b><h3>Then the rest</h3><p>The phone fetches what changed on the server while you were away, so the team's notes are current again.</p></div>
</div>
</section>
<section id="collide">
<h2>When two edits collide</h2>
<p class="sub">If you and a colleague changed the same field of the same note while offline, the app shows both versions and asks which to keep. Everything else on the note merges on its own. Nothing is picked for you.</p>
<div class="collide">
<div class="ver"><div class="who">Yours · 08:12</div><p>Seal weeping at the inlet flange; <mark>torqued to 45 Nm</mark>, recheck Friday.</p><span class="keep mine">Keep mine</span></div>
<div class="ver"><div class="who">Priya's · 08:40</div><p>Seal weeping at the inlet flange; <mark>replacement ordered (PN 2231)</mark>.</p><span class="keep">Keep theirs</span></div>
</div>
</section>
<section id="faq">
<h2>Common questions</h2>
<dl class="faq">
<dt>How long can I stay offline?</dt><dd>As long as you like. The outbox has no limit; a week underground is fine.</dd>
<dt>What about photos?</dt><dd>They are kept on the phone and sent with the note they belong to, on Wi-Fi first.</dd>
<dt>Can I see what is waiting?</dt><dd>Tap the banner: every change, oldest first, and why one could not be sent.</dd>
</dl>
</section>
</main>
<footer class="wrap"><span>Fieldnote · help</span><span>Updated for 4.2</span><a href="#">Working offline</a><a href="#">Sync</a><a href="#">Conflicts</a></footer>
</body>
</html>
EOF
)"

  # DESIGN-48 (the owner, 2026-10-15: "show that we can build custom
  # components … the look and feel of components we want to build"): the
  # team's own components, defined in a doc as ```component blocks (COMP-1)
  # — the header banner and the conflict card — and used right there in
  # every state, so the look and feel is agreed before the app has them.
  # OFFLINE-17, under the banner build. `#/components/fn-banner` is its help
  # page, with the example live.
  syn $T create --epic OFFLINE --kind doc --title 'Design · the banner and the conflict card, as components' --parent OFFLINE-9 --body "$(cat <<'EOF'
The banner (OFFLINE-9) and the conflict card (OFFLINE-8) as components of our own: defined here, used in any ticket, skinned by the project's theme. The look and feel is agreed on this page before the app has a line of either.

```component fn-banner state=offline count=3
<template><span class="b" part="banner"><i part="dot"></i><span class="t" part="text"></span></span></template>
<style>
:host{display:inline-block;font:600 13px/1.2 system-ui,sans-serif}
.b{display:inline-flex;align-items:center;gap:.5em;padding:.45em .9em;border-radius:999px;background:var(--bg);color:var(--fg)}
i{width:.5em;height:.5em;border-radius:50%;background:currentColor}
.offline{--bg:#fde9c8;--fg:#6b4a10}.syncing{--bg:#dbe7fb;--fg:#1a3f78}.synced{--bg:#dcefdc;--fg:#1f5c2a}
</style>
<docs>
  <summary>The header banner: offline with the number of changes waiting, syncing with the number left, or synced.</summary>
  <attr name="state" type="enum(offline|syncing|synced)">Default offline.</attr>
  <attr name="count" type="number">Changes waiting (offline) or left (syncing).</attr>
  <part name="banner">The pill.</part>
  <part name="dot">Its dot.</part>
  <part name="text">Its words.</part>
  <example><fn-banner state="offline" count="3"></fn-banner> <fn-banner state="syncing" count="1"></fn-banner> <fn-banner state="synced"></fn-banner></example>
  Words always, never colour alone: a crew in sunlight reads the text.
</docs>
<script>
const $ = (q) => this.root.querySelector(q);
const states = ['offline', 'syncing', 'synced'];
const s = states.includes(this.attr('state')) ? this.attr('state') : 'offline';
const n = Number(this.attr('count') || 0);
$('.b').className = 'b ' + s;
$('.t').textContent = s === 'offline' ? `Working offline — ${n} change${n === 1 ? '' : 's'} will sync` : s === 'syncing' ? `Syncing — ${n} left` : 'Everything synced';
</script>
```

```component fn-conflict field="Condition" mine="torqued to 45 Nm" theirs="replacement ordered (PN 2231)" who="Priya"
<template><div class="c" part="card"><div class="h" part="field"></div><div class="cols"><div class="v"><div class="w">Yours</div><p class="m"></p><button class="k mine" part="keep" type="button">Keep mine</button></div><div class="v"><div class="w o"></div><p class="t"></p><button class="k" part="keep" type="button">Keep theirs</button></div></div></div></template>
<style>
:host{display:block;max-width:440px;font:14px/1.4 system-ui,sans-serif;color:#1a1a1e}
.c{border:1px solid #e3e1d8;border-radius:14px;background:#fff;overflow:hidden}
.h{padding:10px 14px;font-weight:600;border-bottom:1px solid #e3e1d8;font-size:13px}
.cols{display:grid;grid-template-columns:1fr 1fr;gap:1px;background:#e3e1d8}
.v{background:#fff;padding:10px 12px 12px}
.w{font-size:10.5px;letter-spacing:.08em;text-transform:uppercase;color:#6b6b74;margin-bottom:6px}
p{margin:0 0 10px;min-height:2.8em}
p span{background:#fde9c8;border-radius:3px;padding:0 3px}
.k{width:100%;padding:8px 0;border:1px solid #1a5fb4;border-radius:8px;background:transparent;color:#1a5fb4;font:inherit;font-size:13px;font-weight:600}
.k.mine{background:#1a5fb4;color:#fff}
</style>
<docs>
  <summary>The conflict card: one field changed on two devices, both values side by side, one tap to keep either.</summary>
  <attr name="field" type="string">The field that collided.</attr>
  <attr name="mine" type="string">This device's value.</attr>
  <attr name="theirs" type="string">The other device's value.</attr>
  <attr name="who" type="string">Whose the other value is.</attr>
  <part name="card">The card.</part>
  <part name="field">Its heading.</part>
  <part name="keep">Either button.</part>
  <example><fn-conflict field="Condition" mine="torqued to 45 Nm" theirs="replacement ordered (PN 2231)" who="Priya"></fn-conflict></example>
  Nothing is picked silently (OFFLINE-8): the card stays until a tap.
</docs>
<script>
const $ = (q) => this.root.querySelector(q);
const span = (el, text) => { el.textContent = ''; const s = this.ownerDocument.createElement('span'); s.textContent = text; el.append(s); };
$('.h').textContent = `This field changed on two devices · ${this.attr('field') || ''}`;
span($('.m'), this.attr('mine') || '');
span($('.t'), this.attr('theirs') || '');
$('.o').textContent = `${this.attr('who') || 'Theirs'}’s`;
</script>
```

## In every state

The banner, as the header will show it:

<p><fn-banner state="offline" count="3"></fn-banner> <fn-banner state="syncing" count="1"></fn-banner> <fn-banner state="synced"></fn-banner></p>

The card, with OFFLINE-14's example:

<fn-conflict field="Condition" mine="torqued to 45 Nm" theirs="replacement ordered (PN 2231)" who="Priya"></fn-conflict>
EOF
)"

  # where the work is
  local s
  for s in OFFLINE-3 OFFLINE-4 OFFLINE-5 SYNC-2; do syn $T set_status --id "$s" --status Complete; done
  for s in OFFLINE-6; do syn $T set_status --id "$s" --status 'In Review'; done
  for s in OFFLINE-9 OFFLINE-15 SYNC-3; do syn $T set_status --id "$s" --status Coding; done
  for s in OFFLINE-7 OFFLINE-8 OFFLINE-10 SYNC-4 SYNC-5; do syn $T set_status --id "$s" --status Ready; done
  syn $T set_status --id OFFLINE-1 --status Epic
  syn $T set_status --id SYNC-1 --status Epic
  # the commits that finished the work (the sample repository's)
  syn $T link_commits --id OFFLINE-3 --commits 3f9c2a1
  syn $T link_commits --id OFFLINE-4 --commits 8b41d07,c2e9f55
  syn $T link_commits --id OFFLINE-5 --commits 51aa0e3
  syn $T link_commits --id OFFLINE-6 --commits 9d7e214
  syn $T link_commits --id SYNC-2 --commits fieldnote-server@4c0b8e2
  # DESIGN-48 — the app's CI records its deploys (DEPLOY-11): prod runs 4.1.2
  # with the store and the cache; the beta channel runs 4.2-beta.3 with the
  # outbox and the sync engine too, so OFFLINE-5 waits for prod and OFFLINE-6
  # runs on beta while in review.
  syn deployments record --project OFFLINE --environment prod --state succeeded --revision c2e9f55 \
    --commits 3f9c2a1,8b41d07,c2e9f55 --release 4.1.2 --at 2026-10-12T15:40:00Z
  syn deployments record --project OFFLINE --environment beta --state succeeded --revision 9d7e214 \
    --commits 3f9c2a1,8b41d07,c2e9f55,51aa0e3,9d7e214 --previous c2e9f55 --release 4.2-beta.3 --at 2026-10-14T09:05:00Z

  # a reader's comments on the app, and notes not yet tickets
  syn comments add --anchor.route '#/ticket/OFFLINE-10' --anchor.path '#ticket-page h2' --anchor.label 'Title' \
    --anchor.excerpt 'Photos attached offline are lost' --anchor.ticket OFFLINE-10 --author priya \
    --body 'Seen twice on the Galaxy A12 too. Raising to the 4.2 blockers.'
  syn comments add --anchor.route '#/ticket/OFFLINE-2' --anchor.path '#ticket-page h2' --anchor.label 'Title' \
    --anchor.excerpt 'Beta rollout' --anchor.ticket OFFLINE-2 --author sam \
    --body 'Can we start with 1% on iOS? TestFlight users are our most active.'
  # DESIGN-48: the team on the agent's plan — two notes on OFFLINE-15, both
  # pinned at its title (an anchor inside the framed body is placed only once
  # the frame has answered the probe, too late for a still)
  syn comments add --anchor.route '#/ticket/OFFLINE-15' --anchor.path '#ticket-page h2' --anchor.label 'Title' \
    --anchor.excerpt 'Help site for offline mode' --anchor.ticket OFFLINE-15 --author sam \
    --body 'One page — but crews read this underground on a 5-inch phone: put the collide part first?'
  syn comments add --anchor.route '#/ticket/OFFLINE-15' --anchor.path '#ticket-page h2' --anchor.label 'Title' \
    --anchor.excerpt 'Help site for offline mode' --anchor.ticket OFFLINE-15 --author priya \
    --body 'Agent: the banner copy should count photos too — see OFFLINE-10.'
  syn notes add --author priya --body 'Offline search across the team: worth a spike after 4.2?'
  syn notes add --author sam --body 'Show the outbox depth in the debug menu so support can ask for it.'
  say "seeded: KIT-1, OFFLINE-1…17, SYNC-1…5, 2 deploys, 4 comments, 2 notes"
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
    case $base in collab.*|kit.*) continue ;; esac   # cut to their subject below (collab-main, kit-main); whole, they are on no page
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
  # DESIGN-48: the landing's figures, cut to their subject — hero: the whole
  # app, nav and all, down to the bottom of the page's two frames (from
  # page; owner: "show the nav"); the rest the main column alone (the 1080
  # CSS px right of the nav): collab-main: the map, the decision and the
  # team's notes listed at their pin (from collab); kit-main: the components
  # in every state (from kit); deploys-main: the environments (from deploys)
  # — at 1200 and at their native width.
  local crop
  for crop in "collab-main collab 2160:1580:720:0" "kit-main kit 2160:960:720:640" "deploys-main deploys 2160:1100:720:0"; do
    set -- $crop
    for theme in light dark; do
      for w in 1200 ${3%%:*}; do
        ffmpeg -loglevel error -y -i "$WORK/raw/stills/$2.$theme.png" -vf "crop=$3,scale=$w:-2:flags=lanczos" "$WORK/tmp.png"
        avif "$WORK/tmp.png" 80 "$OUT/$1.$theme.$w.avif"
        cwebp -quiet -q 90 -m 6 "$WORK/tmp.png" -o "$OUT/$1.$theme.$w.webp"
      done
    done
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
    # DESIGN-46: the clip as an animated image too — an <img> that loads like
    # a GIF (no player, no poster, playing as it appears) at a fraction of a
    # GIF's bytes, lossy WebP.
    # DESIGN-48 (owner: "all videos should be higher quality gifs"): 20 fps,
    # up to 1200 px wide (the page's widest slot), quality 88.
    ffmpeg -loglevel error -y -i "$clip" -vf "fps=20,scale='min(1200,iw)':-2:flags=lanczos" -loop 0 \
      -c:v libwebp_anim -lossless 0 -q:v 88 -compression_level 6 "$OUT/$base.anim.webp"
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
