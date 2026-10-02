#!/usr/bin/env python3
"""SCALE-32: a store where one ticket blocks k others, k = 1, 10^3 and 10^5.

usage: python3 perf/fan-out.py PATH       (then: node perf/ready-probe.mjs fan-out PORT RUNS)

The synthetic seed gives few edges per ticket; this writes the shape a seed
never makes, straight into the tickets table (the schema as of 4f80dbb, the
commit before SCALE-32), with `unlocks` mirrored as `create` keeps it. No
derived state: whichever binary opens it builds its own on first open, by a
scan, so one file measures a binary from before and one from after. FAN-1
is the scope root; FAN-2, FAN-3 and FAN-4 block 1, 1,000 and 100,000
tickets, each `Ready` and blocked by nothing else — so completing a blocker
moves every one of their counts to 0 and into the ready set: the most a
crossing can move."""
import json, os, sqlite3, sys

path = sys.argv[1]
for suf in ("", "-wal", "-shm"):
    try: os.remove(path + suf)
    except FileNotFoundError: pass
conn = sqlite3.connect(path)
conn.executescript("""
CREATE TABLE tickets (
    id           TEXT PRIMARY KEY,
    epic         TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    title        TEXT NOT NULL,
    status       TEXT NOT NULL,
    kind         TEXT NOT NULL,
    severity     TEXT,
    blocked_by   TEXT NOT NULL,
    unlocks      TEXT NOT NULL,
    parent       TEXT,
    body         TEXT NOT NULL,
    created_unix INTEGER NOT NULL,
    updated_unix INTEGER NOT NULL,
    commits      TEXT NOT NULL DEFAULT '[]',
    child_count  INTEGER NOT NULL DEFAULT 0,
    UNIQUE (epic, seq)
);
""")
ids = lambda seqs: json.dumps([f"FAN-{s}" for s in seqs], separators=(",", ":"))
rows = []
def row(seq, status, kind, blocked_by, unlocks, parent):
    rows.append((f"FAN-{seq}", "FAN", seq, f"fan-out {seq}", status, kind, None, ids(blocked_by),
                 ids(unlocks), parent, f"body of FAN-{seq}", 1_750_000_000, 1_750_000_000, "[]", 0))
blocks = {2: 1, 3: 1_000, 4: 100_000}
referrers, seq = {}, 5
for blocker, k in blocks.items():
    referrers[blocker] = range(seq, seq + k)
    seq += k
row(1, "Epic", "scope", [], [], None)
for blocker in blocks:
    row(blocker, "Ready", "build", [], referrers[blocker], "FAN-1")
for blocker in blocks:
    for s in referrers[blocker]:
        row(s, "Ready", "build", [blocker], [], "FAN-1")
conn.executemany("INSERT INTO tickets VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", rows)
conn.commit()
conn.close()
print(f"{path}: {len(rows)} tickets; FAN-2 blocks {blocks[2]}, FAN-3 {blocks[3]}, FAN-4 {blocks[4]}")
