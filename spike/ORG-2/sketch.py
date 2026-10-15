#!/usr/bin/env python3
"""ORG-2: two shortcodes of the same name, stored and fetched by full address.

A sketch, not production code (ORG-3 is). It works on a COPY of a menger
database (it never opens the original for writing), adds a scratch identity
schema beside today's tables, migrates every shortcode and ticket into it,
then mints same-named shortcodes in other namespaces and resolves addresses:

    spike/ORG-2/run.sh <a menger tickets.db>

The schema it proves (names are the sketch's; ORG-3 settles them):

    orgs        (id, name UNIQUE)
    shortcodes  (id, org, host_sc, host, code)   -- host_sc: the shortcode the
                                                 -- root hangs in; host: the ticket
    UNIQUE INDEX (org, ifnull(host_sc, 0), code) -- the namespace rule
    tickets.shortcode, tickets.key               -- key: opaque and immutable;
    UNIQUE INDEX (shortcode, seq)                -- today's id IS its key
    legacy      (text PRIMARY KEY, key)          -- every id that exists today

Each check prints PASS or FAIL; the exit status is the number of FAILs.
"""
import re
import sqlite3
import sys

FAILS = 0


def check(name, ok, detail=""):
    global FAILS
    FAILS += 0 if ok else 1
    print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f" — {detail}" if detail else ""))


db = sqlite3.connect(sys.argv[1])
db.execute("PRAGMA foreign_keys = ON")
q = lambda sql, *a: db.execute(sql, a).fetchall()
one = lambda sql, *a: db.execute(sql, a).fetchone()

before = {
    t: one(f"SELECT count(*) FROM {t}")[0]
    for t in ("tickets", "transitions", "inputs", "answers", "comments", "theme_refs")
}

# ---------------------------------------------------------------- schema
db.executescript(
    """
    CREATE TABLE orgs (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE
                       CHECK (name GLOB '[a-z0-9]*' AND name NOT GLOB '*[^a-z0-9-]*'));
    CREATE TABLE shortcodes (
        id      INTEGER PRIMARY KEY,
        org     INTEGER NOT NULL REFERENCES orgs(id),
        host_sc INTEGER REFERENCES shortcodes(id),
        host    TEXT,               -- the ticket key the root hangs under
        code    TEXT NOT NULL CHECK (code GLOB '[A-Z0-9]*' AND code NOT GLOB '*[^A-Z0-9]*')
    );
    -- SQLite (like Postgres) treats NULLs as distinct in a UNIQUE: a plain
    -- UNIQUE (org, host_sc, code) would admit two root MENGERs in one org.
    CREATE UNIQUE INDEX shortcodes_ns ON shortcodes (org, ifnull(host_sc, 0), code);
    -- `UNIQUE (epic, seq)` is an inline table constraint, which SQLite cannot
    -- drop: the migration rebuilds `tickets` (and its three indexes on epic).
    CREATE TABLE tickets_new AS SELECT * FROM tickets WHERE 0;
    INSERT INTO tickets_new SELECT * FROM tickets;
    DROP TABLE tickets;
    ALTER TABLE tickets_new RENAME TO tickets;
    ALTER TABLE tickets ADD COLUMN shortcode INTEGER REFERENCES shortcodes(id);
    ALTER TABLE tickets ADD COLUMN key TEXT;
    CREATE TABLE legacy (text TEXT PRIMARY KEY, key TEXT NOT NULL) WITHOUT ROWID;
    INSERT INTO orgs (name) VALUES ('hypermemetic');
    """
)
HYP = one("SELECT id FROM orgs WHERE name = 'hypermemetic'")[0]

# ---------------------------------------------------------------- migrate
# Every shortcode today: its root and the root's host from the kept roll-ups
# (rollup_groups.root / .host — derived, so a migration may also recompute
# them). Inserted hosts first, so host_sc can name an inserted row.
groups = {e: (r, h) for e, r, h in q("SELECT epic, root, host FROM rollup_groups")}
epic_of = dict(q("SELECT id, epic FROM tickets"))
sc_id = {}
pending = dict(groups)
while pending:
    progressed = False
    for epic, (root, host) in list(pending.items()):
        host_epic = epic_of.get(host) if host else None
        if host_epic is not None and host_epic not in sc_id:
            continue
        cur = db.execute(
            "INSERT INTO shortcodes (org, host_sc, host, code) VALUES (?, ?, ?, ?)",
            (HYP, sc_id.get(host_epic), host, epic),
        )
        sc_id[epic] = cur.lastrowid
        del pending[epic]
        progressed = True
    assert progressed, f"a host cycle among {sorted(pending)}"
for epic, sid in sc_id.items():
    db.execute("UPDATE tickets SET shortcode = ?, key = id WHERE epic = ?", (sid, epic))
db.executescript(
    """
    CREATE UNIQUE INDEX tickets_key ON tickets (key);
    CREATE UNIQUE INDEX tickets_place ON tickets (shortcode, seq);
    CREATE UNIQUE INDEX tickets_path ON tickets (shortcode, path) WHERE path IS NOT NULL;
    INSERT INTO legacy (text, key) SELECT id, key FROM tickets;
    INSERT OR IGNORE INTO legacy (text, key) SELECT id, id FROM tombstones;
    """
)
n_sc, n_t = one("SELECT count(*) FROM shortcodes")[0], one("SELECT count(*) FROM tickets")[0]
check("every shortcode migrated into a namespace", n_sc == len(groups), f"{n_sc} shortcodes")
check("every ticket has a shortcode and a key", one("SELECT count(*) FROM tickets WHERE shortcode IS NULL OR key IS NULL")[0] == 0, f"{n_t} tickets")

# The full address of every shortcode: org:ROOT/…/SC, walking host_sc up.
ADDRESSES = """
    WITH RECURSIVE chain(id, org, path) AS (
        SELECT id, org, code FROM shortcodes WHERE host_sc IS NULL
        UNION ALL
        SELECT s.id, s.org, chain.path || '/' || s.code
        FROM shortcodes s JOIN chain ON s.host_sc = chain.id
    )
    SELECT chain.id, o.name || ':' || chain.path FROM chain JOIN orgs o ON o.id = chain.org
"""


def addresses():
    return dict(q(ADDRESSES))


def full(key):
    sid, seq = one("SELECT shortcode, seq FROM tickets WHERE key = ?", key)
    return f"{addresses()[sid]}-{seq}"


a = addresses()
check("every shortcode has a full address, all distinct", len(set(a.values())) == len(a) == n_sc, a[sc_id["THEME"]] if "THEME" in sc_id else "")
check("today's ids move by themselves: key = id for every row", one("SELECT count(*) FROM tickets WHERE key != id")[0] == 0)


# ---------------------------------------------------------------- writes
def mint(org, host_key, code, title):
    """A new shortcode, its root a scope under host_key (None: a root) —
    refused when (org, host shortcode, code) is taken."""
    host_sc = one("SELECT shortcode FROM tickets WHERE key = ?", host_key)[0] if host_key else None
    try:
        sid = db.execute(
            "INSERT INTO shortcodes (org, host_sc, host, code) VALUES (?, ?, ?, ?)", (org, host_sc, host_key, code)
        ).lastrowid
    except sqlite3.IntegrityError:
        taken = one(
            "SELECT id FROM shortcodes WHERE org = ? AND ifnull(host_sc, 0) = ifnull(?, 0) AND code = ?", org, host_sc, code
        )[0]
        return ("refused", addresses()[taken])
    file(sid, "scope", title, host_key)
    return ("minted", sid)


def file(sid, kind, title, parent):
    seq = one("SELECT ifnull(max(seq), 0) + 1 FROM tickets WHERE shortcode = ?", sid)[0]
    code = one("SELECT code FROM shortcodes WHERE id = ?", sid)[0]
    # A new ticket's key: opaque, never parsed, never shown. Today's rows keep
    # theirs ('THEME-3'); a newborn takes one that cannot collide with any.
    key = f"~{sid}.{seq}"
    db.execute(
        """INSERT INTO tickets (id, key, epic, seq, shortcode, title, status, kind, blocked_by, unlocks, body,
                                created_unix, updated_unix, parent)
           VALUES (?, ?, ?, ?, ?, ?, 'Pending', ?, '[]', '[]', '', 0, 0, ?)""",
        (key, key, code, seq, sid, title, kind, parent),
    )
    return key


ok, theme2 = mint(HYP, "PLEXUS-1", "THEME", "a second THEME, under PLEXUS")
check("a second THEME under PLEXUS-1 is stored", ok == "minted", a.get(theme2, addresses().get(theme2)))
root2 = one("SELECT key FROM tickets WHERE shortcode = ? AND seq = 1", theme2)[0]
for i in range(2, 4):
    file(theme2, "build", f"new THEME-{i}", root2)

ok, where = mint(HYP, "PLEXUS-5", "THEME", "THEME again, under another PLEXUS ticket")
check("THEME again under PLEXUS-5 (same host shortcode) is refused", ok == "refused", f"names {where}")
ok, where = mint(HYP, "DESIGN-3", "THEME", "THEME again, beside the first")
check("THEME again under DESIGN-3 (the first THEME's namespace) is refused", ok == "refused", f"names {where}")
ok, where = mint(HYP, None, "MENGER", "a second root MENGER")
check("a second root MENGER in hypermemetic is refused (the NULL-host index)", ok == "refused", f"names {where}")

ACME = db.execute("INSERT INTO orgs (name) VALUES ('acme')").lastrowid
ok, acme_menger = mint(ACME, None, "MENGER", "acme's own MENGER")
check("a root MENGER in org acme is stored", ok == "minted")
ok, acme_theme = mint(ACME, one("SELECT key FROM tickets WHERE shortcode = ? AND seq = 1", acme_menger)[0], "THEME", "acme's THEME")
check("a THEME under acme:MENGER-1 is stored", ok == "minted")

# Two LOOKs, neither of which has a legacy id: one under each hypermemetic THEME.
ok1, look_a = mint(HYP, "THEME-1", "LOOK", "LOOK under the first THEME")
ok2, look_b = mint(HYP, root2, "LOOK", "LOOK under the second THEME")
check("LOOK under each THEME: both stored", ok1 == ok2 == "minted")


# ---------------------------------------------------------------- reads
SEG = re.compile(r"^[A-Z0-9]+$")


def resolve(text, reach):
    """An address to one ticket key, in the reader's orgs (`reach`, names).

    Full:  org:ROOT/…/SC-N — exact.
    Short: [org:][SEG/…/]SC-N — SC is the shortcode's own code, the SEGs a
           subsequence of the shortcodes hosting it, in order (so
           MENGER/THEME-3 skips DESIGN). A path address ends …/SC/_MENGER/x.
    One candidate: it. None: not_found (another org's included — never
    forbidden). Several: a bare SC-N that names a ticket today (`legacy`)
    is pinned to it — so every id written before ORG-3 keeps its meaning —
    else ambiguous, with every candidate's full address.
    """
    org, _, rest = text.rpartition(":")
    orgs = [org] if org else reach
    if org and org not in reach:
        return ("not_found", [])
    segs = rest.split("/")
    if any(s.startswith("_") for s in segs):  # a path address — WAVE-10's, scoped
        i = next(i for i, s in enumerate(segs) if s.startswith("_"))
        segs, path, seq = segs[:i], "/".join(segs[i:]), None
    else:
        m = re.match(r"^(.*)-(\d+)$", segs[-1])
        if not m:
            return ("malformed", [])
        segs, path, seq = segs[:-1] + [m.group(1)], None, int(m.group(2))
    segs = [s.upper() for s in segs]  # shortcodes are case-blind; org names are not
    if not all(SEG.match(s) for s in segs):
        return ("malformed", [])
    found = []
    for sid, addr in addresses().items():
        o, _, chain = addr.partition(":")
        if o not in orgs:
            continue
        chain = chain.split("/")
        if chain[-1] != segs[-1]:
            continue
        it = iter(chain[:-1])
        if not all(s in it for s in segs[:-1]):  # a subsequence, in order
            continue
        if seq is not None:
            r = one("SELECT key FROM tickets WHERE shortcode = ? AND seq = ?", sid, seq)
        else:
            r = one("SELECT key FROM tickets WHERE shortcode = ? AND path = ?", sid, path)
        if r:
            found.append(r[0])
    if len(found) == 1:
        return ("ticket", found)
    if not found:
        return ("not_found", [])
    pinned = one("SELECT key FROM legacy WHERE text = ?", rest.upper()) if not org and len(segs) == 1 else None
    if pinned and pinned[0] in found:
        return ("ticket", [pinned[0]])
    return ("ambiguous", sorted(full(k) for k in found))


ME = ["hypermemetic"]
BOTH = ["hypermemetic", "acme"]
cases = [
    # text, reach, expected (kind, the full addresses answered)
    ("hypermemetic:MENGER/DESIGN/THEME-3", ME, ("ticket", ["hypermemetic:MENGER/DESIGN/THEME-3"])),
    ("hypermemetic:PLEXUS/THEME-3", ME, ("ticket", ["hypermemetic:PLEXUS/THEME-3"])),
    ("PLEXUS/THEME-3", ME, ("ticket", ["hypermemetic:PLEXUS/THEME-3"])),
    ("MENGER/THEME-3", ME, ("ticket", ["hypermemetic:MENGER/DESIGN/THEME-3"])),
    ("THEME-3", ME, ("ticket", ["hypermemetic:MENGER/DESIGN/THEME-3"])),  # legacy-pinned
    ("theme-3", ME, ("ticket", ["hypermemetic:MENGER/DESIGN/THEME-3"])),
    ("LOOK-1", ME, ("ambiguous", ["hypermemetic:MENGER/DESIGN/THEME/LOOK-1", "hypermemetic:PLEXUS/THEME/LOOK-1"])),
    ("PLEXUS/LOOK-1", ME, ("ticket", ["hypermemetic:PLEXUS/THEME/LOOK-1"])),
    ("DESIGN/LOOK-1", ME, ("ticket", ["hypermemetic:MENGER/DESIGN/THEME/LOOK-1"])),
    ("MENGER-1", ["acme"], ("ticket", ["acme:MENGER-1"])),
    ("MENGER-1", BOTH, ("ticket", ["hypermemetic:MENGER-1"])),  # legacy-pinned
    ("acme:MENGER-1", BOTH, ("ticket", ["acme:MENGER-1"])),
    ("acme:MENGER-1", ME, ("not_found", [])),  # another org's: not found, never forbidden
    ("acme:MENGER/THEME-1", BOTH, ("ticket", ["acme:MENGER/THEME-1"])),
    ("THEME-1", BOTH, ("ticket", ["hypermemetic:MENGER/DESIGN/THEME-1"])),  # three THEME-1s; legacy wins
    ("PLEXUS/_MENGER/board", ME, ("ticket", ["hypermemetic:PLEXUS-2"])),
    ("hypermemetic:PLEXUS/_MENGER/board", ME, ("ticket", ["hypermemetic:PLEXUS-2"])),
]
for text, reach, (kind, want) in cases:
    got_kind, got = resolve(text, reach)
    shown = [full(k) for k in got] if got_kind == "ticket" else got
    check(f"resolve {text!r} in {reach}", (got_kind, shown) == (kind, want), f"{got_kind} {shown}")

# Every id that exists today still resolves, to itself, after the second
# THEME, acme's MENGER and THEME, and the two LOOKs.
legacy = q("SELECT text, key FROM legacy WHERE key IN (SELECT key FROM tickets)")
bad = [t for t, k in legacy if resolve(t, BOTH) != ("ticket", [k])]
check("every id that exists today resolves to its own ticket", not bad, f"{len(legacy)} ids, {len(bad)} wrong {bad[:5]}")

# ---------------------------------------------------------------- moves
# Rehoming a root (set_parent of the second THEME's root) moves its namespace:
# into the first THEME's namespace — refused; under MENGER-2 — its address
# changes, its key and every row keyed by it do not.
def rehome(sid, host_key):
    host_sc = one("SELECT shortcode FROM tickets WHERE key = ?", host_key)[0]
    try:
        db.execute("UPDATE shortcodes SET host_sc = ?, host = ? WHERE id = ?", (host_sc, host_key, sid))
        db.execute("UPDATE tickets SET parent = ? WHERE shortcode = ? AND seq = 1", (host_key, sid))
        return "moved"
    except sqlite3.IntegrityError:
        return "refused"


check("rehoming the second THEME under DESIGN-7 (a clash) is refused", rehome(theme2, "DESIGN-7") == "refused")
before_addr = full(root2)
check("rehoming it under MENGER-2 is allowed", rehome(theme2, "MENGER-2") == "moved", f"{before_addr} → {full(root2)}")
check("its LOOK's address follows, its key does not move", full(f"~{look_b}.1") == "hypermemetic:MENGER/THEME/LOOK-1", full(f"~{look_b}.1"))
check("THEME-3 is still the first THEME's (pinned)", resolve("THEME-3", ME) == ("ticket", ["THEME-3"]))

# The side tables hold today's ids as text; with key = id they join as they did.
for table, col in (("transitions", "ticket"), ("inputs", "ticket"), ("answers", "ticket"), ("comments", "ticket"), ("theme_refs", "ticket")):
    joined = one(f"SELECT count(*) FROM {table} x WHERE x.{col} IS NULL OR x.{col} IN (SELECT key FROM tickets) OR x.{col} IN (SELECT id FROM tombstones)")[0]
    check(f"{table}: every row still names its ticket, unrewritten", joined == before[table], f"{joined}/{before[table]}")

# What the other namespace rule (host = the ticket, not its shortcode) would
# admit: two THEMEs under DESIGN-1 and DESIGN-7 — one full address for both.
db.execute("DROP INDEX shortcodes_ns")
db.execute("CREATE UNIQUE INDEX shortcodes_ns ON shortcodes (org, ifnull(host, ''), code)")
ok, _ = mint(HYP, "DESIGN-7", "THEME", "THEME under DESIGN-7, host = a ticket")
addr_counts = {}
for _, addr in q(ADDRESSES):
    addr_counts[addr] = addr_counts.get(addr, 0) + 1
clash = [x for x, n in addr_counts.items() if n > 1]
check("(host = a ticket) admits a THEME under DESIGN-7 whose full address is the first THEME's", ok == "minted" and clash == ["hypermemetic:MENGER/DESIGN/THEME"], f"{clash}")

db.rollback()
print(f"\n{FAILS} failed")
sys.exit(FAILS)
