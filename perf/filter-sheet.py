#!/usr/bin/env python3
"""FILTER-4: the before/after sheet's server half, at 10^6.

usage: python3 perf/filter-sheet.py BEFORE_BIN AFTER_BIN BEFORE_DB AFTER_DB [ROUNDS]

BEFORE_DB is a file the commit before wrote (form 3, e.g. its own
--seed-synthetic 1000000); AFTER_DB the same file once AFTER_BIN has opened
it (form 4). Each round starts BEFORE_BIN on BEFORE_DB, then AFTER_BIN on
AFTER_DB, back to back on ephemeral ports (never 4450-4462): the time from
spawn to `health`, the store's own "read the derived state" time, and the
RSS once it serves. Then both serve at once — perf/filter-probe.mjs runs
against each, and perf/filter-identical.mjs compares them — and the load
average is printed beside every number."""
import json, os, re, socket, statistics, subprocess, sys, time, urllib.request

BEFORE, AFTER, BDB, ADB = sys.argv[1:5]
ROUNDS = int(sys.argv[5]) if len(sys.argv) > 5 else 3
HERE = os.path.dirname(os.path.abspath(__file__))
LOGS = os.path.join(os.path.dirname(os.path.abspath(ADB)), "sheet-logs")
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

def start(binary, db, tag):
    ws, rest = free(2)
    log = os.path.join(LOGS, f"{tag}.log")
    t = time.perf_counter()
    p = subprocess.Popen([binary, "--db-path", db, "--port", str(ws), "--rest-port", str(rest), "--no-shell"],
                         stdout=open(log, "w"), stderr=subprocess.STDOUT, env={**os.environ, "PLEXUS_NO_REGISTRY": "1", "NO_COLOR": "1"})
    while True:
        if p.poll() is not None:
            raise SystemExit(f"{tag} exited; see {log}")
        try:
            urllib.request.urlopen(urllib.request.Request(f"http://127.0.0.1:{rest}/rest/tickets/health", data=b"{}",
                                   headers={"content-type": "application/json"}), timeout=5).read()
            break
        except Exception:
            time.sleep(0.05)
    serving = time.perf_counter() - t
    rss = int(open(f"/proc/{p.pid}/status").read().split("VmRSS:")[1].split()[0]) / 1024
    text = re.sub(r"\x1b\[[0-9;]*m", "", open(log).read())
    m = re.search(r"read the derived state from the database in ([0-9.]+) ms", text)
    rows = re.search(r"loaded (\d+) tickets.*? in ([0-9.]+) ms", text)
    return p, ws, {"serving_s": round(serving, 2), "rss_mib": round(rss, 1), "derived_read_ms": float(m.group(1)) if m else None,
                   "log": log}

def stop(p):
    p.terminate()
    p.wait()

print(json.dumps({"loadavg_at_start": load()}))
loads = {"before": [], "after": []}
for r in range(ROUNDS):
    for tag, binary, db in (("before", BEFORE, BDB), ("after", AFTER, ADB)):
        la = load()
        p, _, out = start(binary, db, f"{tag}-load-{r}")
        stop(p)
        out["loadavg"] = la
        loads[tag].append(out)
        print(json.dumps({"round": r, "binary": tag, **out}))
for tag in loads:
    v = loads[tag]
    print(json.dumps({"binary": tag, "median_serving_s": statistics.median(x["serving_s"] for x in v),
                      "median_rss_mib": statistics.median(x["rss_mib"] for x in v),
                      "median_derived_read_ms": statistics.median(x["derived_read_ms"] or 0 for x in v)}))

# Both serving: the probes, then the byte comparison.
pb, wb, _ = start(BEFORE, BDB, "before-probe")
pa, wa, _ = start(AFTER, ADB, "after-probe")
try:
    for tag, port in (("before", wb), ("after", wa)):
        la = load()
        out = subprocess.run(["node", os.path.join(HERE, "filter-probe.mjs"), tag, str(port), "21"], capture_output=True, text=True)
        print(json.dumps({"probe": tag, "loadavg": la}))
        print(out.stdout, end="")
        if out.returncode:
            print(out.stderr)
    rss = {t: int(open(f"/proc/{p.pid}/status").read().split("VmRSS:")[1].split()[0]) / 1024 for t, p in (("before", pb), ("after", pa))}
    print(json.dumps({"rss_mib_after_probes": rss}))
    la = load()
    out = subprocess.run(["node", os.path.join(HERE, "filter-identical.mjs"), str(wb), str(wa), "33"], capture_output=True, text=True)
    print(json.dumps({"identical": "perf/filter-identical.mjs", "loadavg": la}))
    print(out.stdout, end="")
    if out.returncode:
        print(out.stderr)
finally:
    stop(pb)
    stop(pa)
