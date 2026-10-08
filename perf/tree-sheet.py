#!/usr/bin/env python3
"""FILTER-13: the before/after sheet at 10^6 in FILTER-3 §6.5's nested shape.

usage: python3 perf/tree-sheet.py BEFORE_BIN AFTER_BIN FORM4_DB WORK_DIR [ROUNDS]

FORM4_DB is a file the commit before wrote — its own `--seed-synthetic
1000000 --seed-shape nested` — in form 4. WORK_DIR gets the copies. Every
server runs on ephemeral ports (never 4450-4462), and the load average is
printed beside every number.

1. The migration: AFTER_BIN's first open of a copy of FORM4_DB (form 4 -> 5),
   timed from spawn to serving, with the store's own log lines.
2. Load and memory, ROUNDS rounds, the two binaries alternating, each on its
   own file (BEFORE on the form-4 copy, AFTER on the migrated one): spawn to
   serving, and RSS once it serves.
3. Writes, ROUNDS rounds, alternating, each on a fresh copy of its file:
   `create` under the largest root, under one of its executions and under a
   build of it (three deep); `set_status` -> Complete of each; `set_parent`
   of a build from one execution to another and back, and of an execution
   with children under another execution and back — medians, over REST.
4. Reads: both serving at once, perf/tree-probe.mjs against each."""
import json, os, shutil, socket, statistics, subprocess, sys, time, urllib.request

BEFORE, AFTER, SRC, WORK = sys.argv[1:5]
ROUNDS = int(sys.argv[5]) if len(sys.argv) > 5 else 3
HERE = os.path.dirname(os.path.abspath(__file__))
os.makedirs(WORK, exist_ok=True)
LOGS = os.path.join(WORK, "logs")
os.makedirs(LOGS, exist_ok=True)

def load():
    return open("/proc/loadavg").read().split()[:3]

def free(n):
    socks = [socket.socket() for _ in range(n)]
    for s in socks: s.bind(("127.0.0.1", 0))
    ports = [s.getsockname()[1] for s in socks]
    for s in socks: s.close()
    for p in ports: assert not 4450 <= p <= 4462, p
    return ports

def copy(src, dst):
    for suf in ("", "-wal", "-shm"):
        try: os.remove(dst + suf)
        except FileNotFoundError: pass
    shutil.copyfile(src, dst)

def rest(port, method, params, timeout=600):
    req = urllib.request.Request(f"http://127.0.0.1:{port}/rest/tickets/{method}", data=json.dumps(params).encode(),
                                 headers={"content-type": "application/json"})
    t = time.perf_counter()
    raw = urllib.request.urlopen(req, timeout=timeout).read()
    return (time.perf_counter() - t) * 1000, json.loads(raw)["data"]

def start(binary, db, tag):
    ws, rp = free(2)
    log = os.path.join(LOGS, f"{tag}.log")
    t = time.perf_counter()
    p = subprocess.Popen([binary, "--db-path", db, "--port", str(ws), "--rest-port", str(rp), "--no-shell"],
                         stdout=open(log, "w"), stderr=subprocess.STDOUT, env={**os.environ, "PLEXUS_NO_REGISTRY": "1", "NO_COLOR": "1"})
    while True:
        if p.poll() is not None:
            raise SystemExit(f"{tag} exited; see {log}")
        try:
            rest(rp, "health", {}, timeout=5)
            break
        except Exception:
            time.sleep(0.05)
    serving = time.perf_counter() - t
    rss = int(open(f"/proc/{p.pid}/status").read().split("VmRSS:")[1].split()[0]) / 1024
    return p, ws, rp, {"serving_s": round(serving, 2), "rss_mib": round(rss, 1), "log": log}

def stop(p):
    p.terminate()
    p.wait()

def med(v):
    return round(statistics.median(v), 3)

print(json.dumps({"loadavg_at_start": load()}))
BDB, ADB = os.path.join(WORK, "before.db"), os.path.join(WORK, "after.db")
copy(SRC, BDB)
copy(SRC, ADB)

# 1. The migration.
la = load()
p, _, _, out = start(AFTER, ADB, "after-migrate")
stop(p)
text = open(out["log"]).read()
lines = [l.split("  ")[-1] for l in text.splitlines() if "stored the derived state" in l or "kept the tree" in l or "interned" in l]
print(json.dumps({"migration": "form 4 -> 5, first open", "spawn_to_serving_s": out["serving_s"], "loadavg": la, "log": lines}))

# 2. Load and memory.
loads = {"before": [], "after": []}
for r in range(ROUNDS):
    for tag, binary, db in (("before", BEFORE, BDB), ("after", AFTER, ADB)):
        la = load()
        p, _, _, out = start(binary, db, f"{tag}-load-{r}")
        stop(p)
        out["loadavg"] = la
        loads[tag].append(out)
        print(json.dumps({"round": r, "binary": tag, "serving_s": out["serving_s"], "rss_mib": out["rss_mib"], "loadavg": la}))
for tag in loads:
    v = loads[tag]
    print(json.dumps({"binary": tag, "median_serving_s": statistics.median(x["serving_s"] for x in v),
                      "median_rss_mib": statistics.median(x["rss_mib"] for x in v)}))

# 3. Writes, each round on fresh copies.
def writes(binary, src, tag):
    db = os.path.join(WORK, f"{tag}-writes.db")
    copy(src, db)
    p, _, rp, _ = start(binary, db, tag)
    try:
        _, kids = rest(rp, "list", {"parent": "MAP-1", "summary": True, "limit": 2000})
        execs = [e["summary"] for e in kids if e.get("type") == "summary" and e["summary"]["kind"] == "execution" and e["summary"]["child_count"] > 2]
        x, y, z = execs[0]["id"], execs[1]["id"], execs[2]["id"]
        _, under_x = rest(rp, "list", {"parent": x, "summary": True, "limit": 200})
        b = [e["summary"]["id"] for e in under_x if e.get("type") == "summary" and e["summary"]["kind"] == "build"][0]
        out = {"loadavg": load(), "exec": x, "build": b}
        ts = {"create root": [], "create exec": [], "create build": [], "set_status root": [], "set_status exec": [],
              "set_status build": [], "set_parent leaf": [], "set_parent subtree": []}
        made = {"root": [], "exec": [], "build": []}
        for i in range(20):
            for where, parent in (("root", None), ("exec", x), ("build", b)):
                params = {"epic": "MAP", "title": f"probe {i}", "kind": "doc" if where == "build" else "build"}
                if parent: params["parent"] = parent
                t, ev = rest(rp, "create", params)
                assert ev[0]["type"] == "created", ev
                ts[f"create {where}"].append(t)
                made[where].append(ev[0]["ticket"]["id"])
        for where in ("root", "exec"):
            for i in made[where]:
                t, ev = rest(rp, "set_status", {"id": i, "status": "Complete"})
                assert ev[0]["type"] == "status_changed", ev
                ts[f"set_status {where}"].append(t)
        # A build of x's: Complete, then back (its doc children stay).
        for i in range(10):
            for status in ("Complete", "Pending"):
                t, ev = rest(rp, "set_status", {"id": b, "status": status})
                assert ev[0]["type"] == "status_changed", ev
                ts["set_status build"].append(t)
        # A leaf, from x to y and back; an execution with children, under z and back.
        for i in made["exec"][:10]:
            for parent in (y, x):
                t, ev = rest(rp, "set_parent", {"id": i, "parent": parent})
                assert ev[0]["type"] == "parent_changed", ev
                ts["set_parent leaf"].append(t)
        for _ in range(10):
            for parent in (z, "MAP-1"):
                t, ev = rest(rp, "set_parent", {"id": y, "parent": parent})
                assert ev[0]["type"] == "parent_changed", ev
                ts["set_parent subtree"].append(t)
        out.update({k: med(v) for k, v in ts.items()})
        out["rss_mib_after"] = round(int(open(f"/proc/{p.pid}/status").read().split("VmRSS:")[1].split()[0]) / 1024, 1)
        return out
    finally:
        stop(p)

for r in range(ROUNDS):
    for tag, binary, src in (("before", BEFORE, BDB), ("after", AFTER, ADB)):
        print(json.dumps({"writes_round": r, "binary": tag, **writes(binary, src, f"{tag}-w{r}")}))

# 4. Reads, both serving.
pb, wb, _, _ = start(BEFORE, BDB, "before-probe")
pa, wa, _, _ = start(AFTER, ADB, "after-probe")
try:
    for tag, port in (("before", wb), ("after", wa)):
        la = load()
        out = subprocess.run(["node", os.path.join(HERE, "tree-probe.mjs"), tag, str(port), "21"], capture_output=True, text=True)
        print(json.dumps({"probe": tag, "loadavg": la}))
        print(out.stdout, end="")
        if out.returncode:
            print(out.stderr)
    rss = {t: int(open(f"/proc/{p.pid}/status").read().split("VmRSS:")[1].split()[0]) / 1024 for t, p in (("before", pb), ("after", pa))}
    print(json.dumps({"rss_mib_after_probes": rss, "loadavg": load()}))
finally:
    stop(pb)
    stop(pa)
