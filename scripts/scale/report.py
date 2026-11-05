#!/usr/bin/env python3
"""PERF-24: the decades as results — m-results tables and inline SVG curves.

    python3 -I scripts/scale/report.py [SCALE_DIR] [--to 9] > results.md

Reads each `1eN/` the harness wrote (gen.json, sizes.json, bench.json and the
two starts' logs) and prints Markdown for the ticket: the measured decades,
then each decade above the largest measured one, extrapolated by a power law
fitted to the last three measured decades (labelled as extrapolation).
"""
import json
import math
import os
import re
import sys
from datetime import datetime

args = [a for a in sys.argv[1:] if not a.startswith("--")]
root = os.path.expanduser(args[0] if args else "~/.cache/menger-scale")
to = 9
if "--to" in sys.argv:
    to = int(sys.argv[sys.argv.index("--to") + 1])

CALLS = [
    ("get_by_id", "get by id"),
    ("get_by_uid", "get by uid"),
    ("list_first_page", "list, first page"),
    ("list_deep_after", "list, deep page (after)"),
    ("list_deep_at", "list, deep page (at N/2)"),
    ("list_filtered", "list, filtered"),
    ("list_in_shortcode", "list, in a shortcode"),
    ("search_common", "search, a common word"),
    ("search_two_words", "search, two words"),
    ("search_rare", "search, a rare word"),
    ("search_typo", "search, a typo (corrected)"),
    ("search_prefix", "search, a prefix"),
    ("search_filtered", "search, filtered"),
    ("nav_counts", "nav.counts, filtered"),
    ("nav_counts_all", "nav.counts, all"),
    ("nav_facets", "nav.facets"),
    ("create", "create"),
    ("update", "update"),
    ("set_status", "set_status"),
]

ANSI = re.compile(r"\x1b\[[0-9;]*m")
STAMP = re.compile(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d+)Z")


def stamp(line):
    m = STAMP.search(line)
    return datetime.fromisoformat(m.group(1)) if m else None


def phases(log_path):
    """The start's phases from its log: the store's load, the backfill, the rest."""
    try:
        lines = [ANSI.sub("", l) for l in open(log_path, errors="replace")]
    except OSError:
        return {}
    first = next((stamp(l) for l in lines if stamp(l)), None)
    loaded = next((l for l in lines if "loaded" in l and "tickets from the database" in l), None)
    back = next((l for l in lines if "backfill:" in l and ("unchanged" in l or "skipped" in l)), None)
    ws = next((l for l in lines if "WebSocket:" in l), None)
    out = {}
    if loaded:
        m = re.search(r"in ([\d.]+) ms", loaded)
        if m:
            out["load_s"] = float(m.group(1)) / 1000
    if loaded and back:
        out["backfill_s"] = (stamp(back) - stamp(loaded)).total_seconds()
    if back and ws:
        out["after_backfill_s"] = (stamp(ws) - stamp(back)).total_seconds()
    if first and ws:
        out["logged_s"] = (stamp(ws) - first).total_seconds()
    return out


def tail(c):
    c = c.get("last_watcher", c)
    return (c.get("errors", 0), c["p99_ms"])


def merge(benches):
    """Several passes over one decade (run while other work shared the
    machine): each call from the pass whose tail was quieter — fewer errors,
    then the lower p99 — and the start and memory from the faster start."""
    best = min(benches, key=lambda b: b["startup_s"])
    out = dict(best)
    out["calls"] = {k: min((b["calls"][k] for b in benches if k in b["calls"]), key=tail) for k in best["calls"]}
    out["passes"] = len(benches)
    return out


def load(e):
    d = os.path.join(root, f"1e{e}")
    passes = sorted(f for f in os.listdir(d) if re.fullmatch(r"bench-pass\d+\.json", f)) if os.path.isdir(d) else []
    try:
        benches = [json.load(open(os.path.join(d, f))) for f in passes] or [json.load(open(os.path.join(d, "bench.json")))]
    except (OSError, ValueError):
        return None
    bench = merge(benches)
    gen = json.load(open(os.path.join(d, "gen.json")))
    sizes = json.load(open(os.path.join(d, "sizes.json")))
    return {"e": e, "n": bench["tickets"], "gen": gen, "sizes": sizes, "bench": bench, "phases": phases(os.path.join(d, "server-1.log"))}


rows = [r for r in (load(e) for e in range(3, 10)) if r]
if not rows:
    sys.exit("no decades measured under " + root)


def fit(xs, ys):
    """log-log least squares: y = a·x^b; returns (a, b)."""
    pts = [(math.log10(x), math.log10(y)) for x, y in zip(xs, ys) if x > 0 and y > 0]
    if len(pts) < 2:
        return None
    n = len(pts)
    mx = sum(p[0] for p in pts) / n
    my = sum(p[1] for p in pts) / n
    sxx = sum((p[0] - mx) ** 2 for p in pts)
    b = sum((p[0] - mx) * (p[1] - my) for p in pts) / sxx if sxx else 0.0
    return 10 ** (my - b * mx), b


def series(get):
    return [get(r) for r in rows]


def extrap(get, n, floor_slope=0.0):
    tail = rows[-3:]
    f = fit([r["n"] for r in tail], [get(r) for r in tail])
    if not f:
        return None
    a, b = f
    return a * n ** max(b, floor_slope), b


def ms(v):
    if v is None:
        return "—"
    if v >= 10000:
        return f"{v / 1000:.0f} s"
    if v >= 1000:
        return f"{v / 1000:.1f} s"
    if v >= 10:
        return f"{v:.0f}"
    return f"{v:.2f}"


def gb(b):
    if b >= 1e12:
        return f"{b / 1e12:.2f} TB"
    if b >= 1e9:
        return f"{b / 1e9:.1f} GB"
    return f"{b / 1e6:.0f} MB"


def dur(s):
    if s is None:
        return "—"
    if s >= 3600:
        return f"{s / 3600:.1f} h"
    if s >= 60:
        return f"{s / 60:.1f} min"
    return f"{s:.1f} s"


def sup(e):
    return "10" + "⁰¹²³⁴⁵⁶⁷⁸⁹"[e] if e < 10 else f"10^{e}"


measured = {r["e"] for r in rows}
top = max(measured)
out = []
w = out.append

# ---- the dataset and the start
passes = max(r["bench"].get("passes", 1) for r in rows)
w(f'<m-results label="By decade — the dataset, the start, memory (measured; the faster of {passes} starts)">\n')
w("| N | generated in (insert + first open) | file | bytes/ticket: table · indexes · FTS · derived · other | start (every later start) | of it: load · backfill · rest | RSS after start | peak RSS |")
w("| -- | -- | -- | -- | -- | -- | -- | -- |")
for r in rows:
    g, s, b, p = r["gen"], r["sizes"], r["bench"], r["phases"]
    pt = s["per_ticket"]
    w(
        f"| {sup(r['e'])} | {dur(g['insert_s'])} + {dur(g['derive_s'])} | {gb(s['bytes']['total'])} | "
        f"{pt['total']:,.0f}: {pt['table']:,.0f} · {pt['indexes']:,.0f} · {pt['fts']:,.0f} · {pt['derived']:,.0f} · {pt['other']:,.0f} | "
        f"{dur(b['startup_s'])} | {dur(p.get('load_s'))} · {dur(p.get('backfill_s'))} · {dur(p.get('after_backfill_s'))} | "
        f"{b['rss_mb']['after_start']:,.0f} MB | {b['rss_mb']['peak']:,.0f} MB |"
    )
w("\n</m-results>\n")

# ---- the calls
for label, keys in [("reads", CALLS[:16]), ("writes", CALLS[16:])]:
    w(f'<m-results label="Over the wire — {label}, p50 / p99 / max in ms (measured; each call the quieter of {passes} passes)">\n')
    w("| call | " + " | ".join(sup(r["e"]) for r in rows) + " |")
    w("| -- | " + " | ".join("--" for _ in rows) + " |")
    for k, name in keys:
        cells = []
        for r in rows:
            c = r["bench"]["calls"].get(k)
            if not c:
                cells.append("—")
                continue
            err = f" ({c['errors']} err)" if c["errors"] else ""
            cells.append(f"{ms(c['p50_ms'])} / {ms(c['p99_ms'])} / {ms(c['max_ms'])}{err}")
        w(f"| {name} | " + " | ".join(cells) + " |")
    if label == "writes":
        cells = []
        for r in rows:
            f = r["bench"]["calls"]["watch_fan_out"]["last_watcher"]
            cells.append(f"{ms(f['p50_ms'])} / {ms(f['p99_ms'])} / {ms(f['max_ms'])}")
        watchers = rows[-1]["bench"]["calls"]["watch_fan_out"]["watchers"]
        w(f"| watch fan-out: a set_status to the last of {watchers} watchers | " + " | ".join(cells) + " |")
    w("\n</m-results>\n")

# ---- extrapolation
# Quantities that are a fixed cost plus a cost per ticket (the file, the
# generation, the start, memory) are extrapolated as a line through the last
# two decades, a + b·N; latencies as a power law through the last three.
EX = [
    ("file", lambda r: r["sizes"]["bytes"]["total"], gb, "line"),
    ("generation (insert + first open)", lambda r: r["gen"]["insert_s"] + r["gen"]["derive_s"], dur, "line"),
    ("start", lambda r: r["bench"]["startup_s"], dur, "line"),
    ("of it: the store's load", lambda r: r["phases"].get("load_s") or 0, dur, "line"),
    ("of it: COMPS-5's backfill", lambda r: r["phases"].get("backfill_s") or 0, dur, "line"),
    ("RSS after start", lambda r: r["bench"]["rss_mb"]["after_start"] * 1048576, gb, "line"),
    ("search, a common word p50", lambda r: r["bench"]["calls"]["search_common"]["p50_ms"], ms, "power"),
    ("search, filtered p50", lambda r: r["bench"]["calls"]["search_filtered"]["p50_ms"], ms, "power"),
    ("nav.facets p50", lambda r: r["bench"]["calls"]["nav_facets"]["p50_ms"], ms, "power"),
    ("nav.counts, all p50", lambda r: r["bench"]["calls"]["nav_counts_all"]["p50_ms"], ms, "power"),
    ("list, deep page (at N/2) p50", lambda r: r["bench"]["calls"]["list_deep_at"]["p50_ms"], ms, "power"),
    ("create p50", lambda r: r["bench"]["calls"]["create"]["p50_ms"], ms, "power"),
    ("get by id p50", lambda r: r["bench"]["calls"]["get_by_id"]["p50_ms"], ms, "power"),
]


def line(get, n):
    (x0, y0), (x1, y1) = [(r["n"], get(r)) for r in rows[-2:]]
    b = (y1 - y0) / (x1 - x0)
    return y1 + b * (n - x1), b


if top < to:
    w(f'<m-results label="Extrapolation above {sup(top)} — not measured: a line a + b·N through the last two measured decades for sizes, times and memory; a power law through the last three for latencies">\n')
    w("| quantity | fit | " + " | ".join(sup(e) for e in range(top + 1, to + 1)) + " |")
    w("| -- | -- | " + " | ".join("--" for _ in range(top + 1, to + 1)) + " |")
    for name, get, f, how in EX:
        if how == "line":
            res = [line(get, 10 ** e) for e in range(top + 1, to + 1)]
            b = res[0][1]
            per = f"{gb(b)} per ticket" if f is gb else f"{b * 1e6:.0f} µs per ticket"
            if f is gb:
                per = f"{b:,.0f} B per ticket"
            w(f"| {name} | line, {per} | " + " | ".join(f(v[0]) for v in res) + " |")
        else:
            res = [extrap(get, 10 ** e) for e in range(top + 1, to + 1)]
            slope = res[0][1] if res and res[0] else 0
            w(f"| {name} | N^{slope:.2f} | " + " | ".join(f(v[0]) if v else "—" for v in res) + " |")
    w("\n</m-results>\n")


# ---- curves: inline SVG, log-log
def svg(title, lines, unit, fmt):
    W, H, L, R, T, B = 640, 300, 64, 150, 28, 36
    xs = [r["n"] for r in rows]
    ys = [v for _, vals in lines for v in vals if v and v > 0]
    if not ys:
        return ""
    x0, x1 = math.log10(min(xs)), math.log10(max(xs))
    y0, y1 = math.floor(math.log10(min(ys))), math.ceil(math.log10(max(ys)))
    if y1 == y0:
        y1 += 1
    X = lambda x: L + (math.log10(x) - x0) / (x1 - x0 or 1) * (W - L - R)
    Y = lambda y: T + (1 - (math.log10(y) - y0) / (y1 - y0)) * (H - T - B)
    palette = ["#2563eb", "#dc2626", "#16a34a", "#9333ea", "#ea580c", "#0891b2", "#4b5563", "#ca8a04"]
    s = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" font-family="sans-serif" font-size="11">']
    s.append(f'<text x="{L}" y="16" font-size="13" font-weight="600">{title}</text>')
    for e in range(y0, y1 + 1):
        yy = Y(10 ** e)
        s.append(f'<line x1="{L}" x2="{W - R}" y1="{yy:.1f}" y2="{yy:.1f}" stroke="#ddd"/>')
        s.append(f'<text x="{L - 6}" y="{yy + 4:.1f}" text-anchor="end" fill="#555">{fmt(10 ** e)}</text>')
    for r in rows:
        xx = X(r["n"])
        s.append(f'<line x1="{xx:.1f}" x2="{xx:.1f}" y1="{T}" y2="{H - B}" stroke="#eee"/>')
        s.append(f'<text x="{xx:.1f}" y="{H - B + 16}" text-anchor="middle" fill="#555">{sup(r["e"])}</text>')
    s.append(f'<text x="{(L + W - R) / 2:.0f}" y="{H - 4}" text-anchor="middle" fill="#555">tickets (log)</text>')
    s.append(f'<text x="12" y="{(T + H - B) / 2:.0f}" fill="#555" transform="rotate(-90 12 {(T + H - B) / 2:.0f})" text-anchor="middle">{unit} (log)</text>')
    for i, (name, vals) in enumerate(lines):
        c = palette[i % len(palette)]
        pts = [(X(x), Y(v)) for x, v in zip(xs, vals) if v and v > 0]
        if not pts:
            continue
        s.append(f'<polyline fill="none" stroke="{c}" stroke-width="2" points="{" ".join(f"{a:.1f},{b:.1f}" for a, b in pts)}"/>')
        for a, b in pts:
            s.append(f'<circle cx="{a:.1f}" cy="{b:.1f}" r="3" fill="{c}"/>')
        s.append(f'<rect x="{W - R + 10}" y="{T + i * 16}" width="10" height="10" fill="{c}"/>')
        s.append(f'<text x="{W - R + 24}" y="{T + i * 16 + 9}">{name}</text>')
    s.append("</svg>")
    return "".join(s)


def calls(k, q="p50_ms"):
    return [r["bench"]["calls"].get(k, {}).get(q) for r in rows]


lin = lambda v: (f"{v:g}" if v < 1000 else f"{v / 1000:g}k") if v < 1e6 else f"{v / 1e6:g}M"
w("**Curves** (log-log; a line with slope 1 grows ×10 per decade — O(N)):\n")
w(svg("p50 per call, ms", [
    ("get by id", calls("get_by_id")),
    ("list deep (at)", calls("list_deep_at")),
    ("search common", calls("search_common")),
    ("search rare", calls("search_rare")),
    ("nav.facets", calls("nav_facets")),
    ("nav.counts all", calls("nav_counts_all")),
    ("create", calls("create")),
    ("set_status", calls("set_status")),
], "ms", lin))
w("")
w(svg("The start and memory", [
    ("start, s", series(lambda r: r["bench"]["startup_s"])),
    ("store load, s", series(lambda r: r["phases"].get("load_s"))),
    ("backfill, s", series(lambda r: r["phases"].get("backfill_s"))),
    ("RSS after start, MB", series(lambda r: r["bench"]["rss_mb"]["after_start"])),
    ("peak RSS, MB", series(lambda r: r["bench"]["rss_mb"]["peak"])),
], "s · MB", lin))
w("")
w(svg("The file, MB", [
    ("total", series(lambda r: r["sizes"]["bytes"]["total"] / 1e6)),
    ("tickets table", series(lambda r: r["sizes"]["bytes"]["table"] / 1e6)),
    ("FTS", series(lambda r: r["sizes"]["bytes"]["fts"] / 1e6)),
    ("indexes", series(lambda r: r["sizes"]["bytes"]["indexes"] / 1e6)),
    ("derived", series(lambda r: r["sizes"]["bytes"]["derived"] / 1e6)),
], "MB", lin))
print("\n".join(out))
