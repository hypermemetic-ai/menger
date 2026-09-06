#!/usr/bin/env python3
"""One-shot import of plans/<EPIC>/<EPIC>-N.md into a running menger.

NOT a sync and NOT a storage backend (the owner rejected plans/-directory
integration): this reads the files once and creates tickets over REST so
the allocated ids match the file ids exactly. Idempotent — existing ids
are skipped. Seq gaps (a deleted file) get a Superseded placeholder so
later ids still line up. A file's `parent:` frontmatter (AGS-19) becomes
the create param, so the parent tree is seeded with the tickets.

  ./scripts/seed-from-plans.py [--rest http://127.0.0.1:4457] [--dry-run]
"""
import argparse, glob, json, os, re, sys, urllib.request

EPIC_ORDER = ["MNG", "MFE", "HLX", "AGS", "PERF", "SDLC", "PLEXUS", "TRANSPORT", "MACRO"]  # cross-epic blocked_by/parent refs point backward in this order

# AGS-13: a file's frontmatter `type:` (the house ticketing vocabulary) maps
# onto the server's kind — the same pinned table the store's migration uses.
# Anything else is a hard stop, not a guess.
KIND_OF = {
    "epic": "scope",
    "implementation": "build",
    "analysis": "spike",
    "checkpoint": "execution",
    "spike": "spike",
}
KINDS = ("scope", "execution", "build", "gap", "spike")


def kind_of(file_type, path):
    if file_type in KINDS:  # a file already speaking kinds passes through
        return file_type
    try:
        return KIND_OF[file_type]
    except KeyError:
        sys.exit(f"{path}: frontmatter type {file_type!r} has no kind mapping "
                 f"(legacy {', '.join(KIND_OF)}; kinds {', '.join(KINDS)})")


def rest(base, method, params):
    req = urllib.request.Request(
        f"{base}/rest/tickets/{method}",
        data=json.dumps(params).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.load(r)["data"]


def frontmatter(text):
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        return None
    fm = {}
    for line in m.group(1).splitlines():
        k, _, v = line.partition(":")
        if not _:
            continue
        v = v.strip()
        if v.startswith("[") and v.endswith("]"):
            v = [x.strip().strip('"') for x in v[1:-1].split(",") if x.strip()]
        else:
            v = v.strip('"')
        fm[k.strip()] = v
    return fm


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rest", default="http://127.0.0.1:4457")
    ap.add_argument("--plans", default=os.path.join(os.path.dirname(__file__), "..", "plans"))
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()

    existing = {ev["ticket"]["id"] for ev in rest(a.rest, "list", {}) if ev.get("type") == "ticket"}

    for epic in EPIC_ORDER:
        files = {}
        for path in glob.glob(os.path.join(a.plans, epic, f"{epic}-*.md")):
            n = int(re.search(rf"{epic}-(\d+)\.md$", path).group(1))
            files[n] = path
        if not files:
            continue
        for n in range(1, max(files) + 1):
            tid = f"{epic}-{n}"
            if tid in existing:
                print(f"skip   {tid} (exists)")
                continue
            if n not in files:
                # Hold the seq so later ids match their files.
                params = {"epic": epic, "title": f"(deleted from plans; seq placeholder)", "kind": "build"}
                status = "Superseded"
                print(f"gap    {tid} -> Superseded placeholder")
            else:
                text = open(files[n], encoding="utf-8").read()
                fm = frontmatter(text) or {}
                if fm.get("id") != tid:
                    sys.exit(f"{files[n]}: frontmatter id {fm.get('id')!r} != {tid}")
                params = {
                    "epic": epic,
                    "title": fm.get("title", tid),
                    "kind": kind_of(fm.get("type", "implementation"), files[n]),
                    "body": text,
                }
                if fm.get("severity"):
                    params["severity"] = fm["severity"]
                if fm.get("blocked_by"):
                    params["blocked_by"] = fm["blocked_by"]
                # AGS-19: a file's `parent:` is the create param. The
                # parent must already exist, which EPIC_ORDER (and seq
                # order within an epic) guarantees for backward refs.
                if fm.get("parent"):
                    params["parent"] = fm["parent"]
                status = fm.get("status", "Pending")
                parent = f" parent:{params['parent']}" if params.get("parent") else ""
                print(f"create {tid} [{params['kind']:9}] {status:10} {params['title'][:60]}{parent}")
            if a.dry_run:
                continue
            ev = rest(a.rest, "create", params)[0]
            if ev.get("type") != "created":
                sys.exit(f"{tid}: create failed: {ev}")
            got = ev["ticket"]["id"]
            if got != tid:
                sys.exit(f"id mismatch: wanted {tid}, server allocated {got} — aborting")
            if status != "Pending":
                ev = rest(a.rest, "set_status", {"id": tid, "status": status})[0]
                if ev.get("type") != "status_changed":
                    sys.exit(f"{tid}: set_status {status} failed: {ev}")
    print("done")


if __name__ == "__main__":
    main()
