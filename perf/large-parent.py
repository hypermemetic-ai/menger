#!/usr/bin/env python3
"""SCALE-3 / SCALE-4: what a create costs as the store, and its parent, grow.

usage: python3 perf/large-parent.py <N> [EPIC]   (needs target/release/menger)

Seeds N synthetic tickets on a temp DB / ephemeral ports, then times `create`
under the largest epic's root vs. under a tiny epic's root, sizes the
`children_changed` watch event and the root's summary row, and times a page
of the root's children (`list { parent, summary, limit: 200 }`) at the top of
the list and deep in it. EPIC defaults to MAP, the largest epic
--seed-value 1 generates at every N.

SCALE-4: a row carries `child_count` where it carried `children`, the list
of every child id; the probe reads either, so the same command measures a
binary from before the change and one from after. MENGER_BIN runs another
binary (default: this checkout's release build); LARGE_PARENT_PORTS=WS
pins the socket's port instead of taking an ephemeral one.

ACCESS-52: the server runs `--auth-provider key` with a key of the probe's
own (target/perf-tmp/large-parent-N/operator.key) as --admin, and every
call goes over the WebSocket signed in with it (perf/menger_ws.py) — the
REST bridge is gone (ACCESS-24). Times are send to `done`; sizes are the
bytes of the call's messages on the socket (the REST body's before), and a
watch event's its own message's (the SSE line's before)."""
import json, os, socket, statistics, subprocess, sys, threading, time
from menger_ws import Socket, Throwaway, signed_in, wait_ready

N = int(sys.argv[1]); BIG = sys.argv[2] if len(sys.argv) > 2 else "MAP"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))   # the checkout this script lives in
BIN = os.environ.get("MENGER_BIN") or f"{ROOT}/target/release/menger"
if not os.access(BIN, os.X_OK):
    raise SystemExit(f"{BIN} not found: cargo build --release first")
TMP = f"{ROOT}/target/perf-tmp"; os.makedirs(TMP, exist_ok=True)
DB = f"{TMP}/large-parent-{N}.db"
LOG = f"{TMP}/large-parent-{N}.log"
for suf in ("", "-wal", "-shm"):
    try: os.remove(DB + suf)
    except FileNotFoundError: pass

def free(n):
    # Hold every socket until all are bound, so two calls cannot hand back
    # the same ephemeral port.
    socks = [socket.socket() for _ in range(n)]
    for s in socks: s.bind(("127.0.0.1", 0))
    ports = [s.getsockname()[1] for s in socks]
    for s in socks: s.close()
    return ports
pinned = os.environ.get("LARGE_PARENT_PORTS")
ws = int(pinned.split(",")[0]) if pinned else free(1)[0]
WHO = Throwaway(f"{TMP}/large-parent-{N}")
log = open(LOG, "w")
srv = subprocess.Popen([BIN, "--db-path", DB, "--seed-synthetic", str(N), "--port", str(ws),
                        "--no-shell", *WHO.args()], stderr=log, stdout=log,
                       env={**os.environ, "PLEXUS_NO_REGISTRY": "1"})
CONN = None
def call(method, params):
    """`tickets.<method>`: (ms, the data items, the bytes they came in)."""
    ms, data = CONN.timed(f"tickets.{method}", params)
    return ms, data, CONN.last_bytes
def child_count(t):
    # After SCALE-4 a row says how many; before it, it listed them all.
    return t["child_count"] if "child_count" in t else len(t["children"])
try:
    # The seed runs before anything listens (~13 s at 10^6 on NVMe): wait
    # for as long as the server lives, with a ceiling that grows with N.
    wait_ready(srv, ws, deadline_s=120 + N / 5000, log=LOG)
    TOKEN = signed_in(WHO, ws)
    CONN = Socket(ws, TOKEN)
    _, data, size = call("get", {"id": f"{BIG}-1"})
    ev = data[0]
    if ev.get("type") != "ticket":
        raise SystemExit(f"{BIG}-1: {ev}; name an epic the seed generated")
    root = ev["ticket"]
    kids = child_count(root)
    print(f"N={N}  root {root['id']} kind={root['kind']} children={kids}  get payload={size} B")
    _, _, size = call("list", {"epic": BIG, "summary": True, "limit": 1})
    print(f"  summary row of the root on a list page: {size} B")

    # The root's children by page: the first 200, and 200 from 90 % of the
    # way down (seeded children of a root are its shortcode's other
    # tickets, so `BIG-k` is a cursor inside the list).
    def med(v): return f"median {statistics.median(v):.3f} ms  (min {min(v):.3f}, max {max(v):.3f})"
    for label, after in (("first page", None), ("deep page", f"{BIG}-{max(2, kids * 9 // 10)}")):
        ts = []
        for _ in range(7):
            params = {"parent": root["id"], "summary": True, "limit": 200}
            if after: params["after"] = after
            ms, data, size = call("list", params)
            ts.append(ms)
        page = data[-1]
        print(f"  list {{parent: {root['id']}, limit: 200}} {label}{f' (after {after})' if after else ''}: "
              f"{len(data) - 1} rows, remaining {page.get('remaining')}, {size} B; {med(ts)}")

    # watch on a socket of its own, record the size of each event's message
    events = []
    def watch():
        try:
            w = Socket(ws, TOKEN)
            for raw, item in w.stream("tickets.watch", {}):
                if item.get("type") == "data":
                    events.append(((item.get("content") or {}).get("type"), len(raw)))
        except Exception: pass
    threading.Thread(target=watch, daemon=True).start(); time.sleep(0.5)

    def creates(epic, n=20, **kw):
        ts, ids = [], []
        for i in range(n):
            ms, data, _ = call("create", {"epic": epic, "title": f"probe {i}", **kw})
            ev = data[0]
            assert ev["type"] == "created", ev
            ts.append(ms)
            ids.append(ev["ticket"]["id"])
        return ts, ids
    # SCALE-28: and a status change of each probe into Complete — a crossing,
    # so it moves the roll-up of every group around it, the most a status
    # change writes.
    def completes(ids):
        ts = []
        for i in ids:
            ms, data, _ = call("set_status", {"id": i, "status": "Complete"})
            ev = data[0]
            assert ev["type"] == "status_changed", ev
            ts.append(ms)
        return ts
    big, big_ids = creates(BIG)
    call("create", {"epic": "TINY", "title": "tiny root", "kind": "scope"})
    small, small_ids = creates("TINY")
    big_done, small_done = completes(big_ids), completes(small_ids)
    time.sleep(0.5)
    print(f"  create under {BIG}-1 ({kids} children): {med(big)}")
    print(f"  create under TINY-1 (<=20 children):        {med(small)}")
    print(f"  ratio of medians: {statistics.median(big) / statistics.median(small):.2f}x")
    print(f"  set_status -> Complete, under {BIG}-1:      {med(big_done)}")
    print(f"  set_status -> Complete, under TINY-1:       {med(small_done)}")
    cc = [sz for (ty, sz) in events if ty == "children_changed"]
    cr = [sz for (ty, sz) in events if ty == "created"]
    print(f"  watch events seen: {len(events)}; types: {sorted(set(t for t,_ in events))}")
    if cc: print(f"  children_changed event: max {max(cc)} B, min {min(cc)} B;  created event: median {int(statistics.median(cr))} B")
    rss = int(open(f"/proc/{srv.pid}/status").read().split("VmRSS:")[1].split()[0]) / 1024
    print(f"  server RSS now: {rss:.0f} MiB")
finally:
    srv.terminate(); srv.wait()
    for suf in ("", "-wal", "-shm"):
        try: os.remove(DB + suf)
        except FileNotFoundError: pass
