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
binary (default: this checkout's release build); LARGE_PARENT_PORTS=WS,REST
pins the two ports instead of taking ephemeral ones."""
import json, os, socket, statistics, subprocess, sys, threading, time, urllib.request

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
ws, rest = [int(p) for p in pinned.split(",")] if pinned else free(2)
log = open(LOG, "w")
srv = subprocess.Popen([BIN, "--db-path", DB, "--seed-synthetic", str(N), "--port", str(ws),
                        "--rest-port", str(rest), "--no-shell"], stderr=log, stdout=log,
                       env={**os.environ, "PLEXUS_NO_REGISTRY": "1"})
def call(method, params, timeout=120):
    req = urllib.request.Request(f"http://127.0.0.1:{rest}/rest/tickets/{method}",
        data=json.dumps(params).encode(), headers={"content-type": "application/json"})
    t = time.perf_counter()
    raw = urllib.request.urlopen(req, timeout=timeout).read()
    return (time.perf_counter() - t) * 1000, raw
def child_count(t):
    # After SCALE-4 a row says how many; before it, it listed them all.
    return t["child_count"] if "child_count" in t else len(t["children"])
try:
    # The seed runs before anything listens (~13 s at 10^6 on NVMe): wait
    # for as long as the server lives, with a ceiling that grows with N.
    deadline = time.monotonic() + 120 + N / 5000
    while True:
        if srv.poll() is not None:
            raise SystemExit(f"server exited ({srv.returncode}) before serving; see {LOG}")
        if time.monotonic() > deadline:
            raise SystemExit(f"server never came up; see {LOG}")
        try: call("health", {}, timeout=5); break
        except Exception: time.sleep(0.2)
    _, raw = call("get", {"id": f"{BIG}-1"})
    ev = json.loads(raw)["data"][0]
    if ev.get("type") != "ticket":
        raise SystemExit(f"{BIG}-1: {ev}; name an epic the seed generated")
    root = ev["ticket"]
    kids = child_count(root)
    print(f"N={N}  root {root['id']} kind={root['kind']} children={kids}  get payload={len(raw)} B")
    _, raw = call("list", {"epic": BIG, "summary": True, "limit": 1})
    print(f"  summary row of the root on a list page: {len(raw)} B")

    # The root's children by page: the first 200, and 200 from 90 % of the
    # way down (seeded children of a root are its shortcode's other
    # tickets, so `BIG-k` is a cursor inside the list).
    def med(v): return f"median {statistics.median(v):.1f} ms  (min {min(v):.1f}, max {max(v):.1f})"
    for label, after in (("first page", None), ("deep page", f"{BIG}-{max(2, kids * 9 // 10)}")):
        ts = []
        for _ in range(7):
            params = {"parent": root["id"], "summary": True, "limit": 200}
            if after: params["after"] = after
            ms, raw = call("list", params)
            ts.append(ms)
        data = json.loads(raw)["data"]
        page = data[-1]
        print(f"  list {{parent: {root['id']}, limit: 200}} {label}{f' (after {after})' if after else ''}: "
              f"{len(data) - 1} rows, remaining {page.get('remaining')}, {len(raw)} B; {med(ts)}")

    # watch over SSE, record the size of each event line
    events = []
    def watch():
        req = urllib.request.Request(f"http://127.0.0.1:{rest}/rest/tickets/watch", data=b"{}",
                                     headers={"content-type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=600) as r:
                for line in r:
                    if line.startswith(b"data:"):
                        try: ev = json.loads(line[5:])
                        except Exception: continue
                        events.append((ev.get("type") or ev.get("content", {}).get("type"), len(line)))
        except Exception: pass
    threading.Thread(target=watch, daemon=True).start(); time.sleep(0.5)

    def creates(epic, n=20, **kw):
        ts = []
        for i in range(n):
            ms, raw = call("create", {"epic": epic, "title": f"probe {i}", **kw})
            ev = json.loads(raw)["data"][0]
            assert ev["type"] == "created", ev
            ts.append(ms)
        return ts
    big = creates(BIG)
    call("create", {"epic": "TINY", "title": "tiny root", "kind": "scope"})
    small = creates("TINY")
    time.sleep(0.5)
    print(f"  create under {BIG}-1 ({kids} children): {med(big)}")
    print(f"  create under TINY-1 (<=20 children):        {med(small)}")
    print(f"  ratio of medians: {statistics.median(big) / statistics.median(small):.2f}x")
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
