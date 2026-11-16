<!-- CODE-3: code-ide — a public GitHub repository read through menger's code service (CODE-2), drawn in a
     body by its tag and at #/code/… by page-code: a head (the repository, its ref, the commit, refresh, pin,
     GitHub, fill the window), the tree with its finder (code-ide-tree), the tabs (code-ide-tabs), a path
     line, the lines (code-ide-view: CodeMirror, read-only, painted by language) and a foot (what happened,
     and the copies). svelte/1. The owner, 2026-11-15: "a nice fast … ide into menger that we can point at gh
     and navigate the codebase from. it should allow us to start inside particular dirs open and file tabs.
     specify the commit or branch latest".

     **The tag** says what is shown first: `repo` (owner/name, or a github.com address of the repository,
     of a tree `/tree/<ref>/<dir>` or of a blob `/blob/<ref>/<path>#L<a>-L<b>`, filling what the tag does
     not say — its ref matched against `code.refs`, the longest first, since a branch's name may hold a
     slash), `ref` (a branch or tag, resolved on every mount; a hash the listing lacks is read as a commit),
     `commit` (a hash: pinned, winning over `ref`), `dir` (directories, comma-separated, each expanded, the
     first scrolled into the tree's view), `open` (paths, comma-separated, each a tab in order, the first
     shown, each with `#L<a>` or `#L<a>-L<b>`), `height` (a CSS length; 70vh, at least 320 px). page-code
     gives `view` (`dir=…;open=…`, the address's last segment) and `page`.

     **Fast**: the tree is one `code.tree` for a commit, and a file one `code.read` for a commit and a path,
     each kept for the page (this module's maps, shared by every code-ide on it); a tab shown again asks
     nothing; a file under the pointer 150 ms is read ahead. A commit that is not kept is fetched
     (`code.fetch`) when the reader holds it (`auth.requires` and `auth.whoami`, asked once a page); else the
     view says someone who may must open it once, and nothing that would be refused is called.

     **The address** (`addressOf`, from the same state as the tag, `tagOf`): `#/code/<owner>/<name>/<ref or
     commit>/<view>`, each segment percent-encoded (`/` `#` `?` `%` and the like; `=` `,` `;` `:` `@` left
     readable), which the engine's router decodes. On the page it follows the view in place
     (`history.replaceState`: no history entry, no route change); "copy link" puts it on the clipboard from
     a body too, "copy permanent link" with the commit for the ref.

     Keys: ctrl+p / ⌘+p find a file (while this view has the focus, or is the page); Escape leaves the
     finder, closes the drawer, then puts a filled window back. Under 641 px the tree is a drawer.
     Marks: `[data-surface=code-ide]` (`data-state` loading · refused · unfetched · fetching · ready,
     `data-commit`, `data-pinned`, `data-drawer`, `data-full`), `[data-field=ref|commit|path|status]`,
     `[data-action=files|refresh|pin|github|full|copy-tag|copy-link|copy-permalink]`, `[data-notice=<kind>]`
     (`data-reason` for a refusal), `input[data-goto]`, and the siblings'. `sheet`: "loading" draws the
     skeleton and asks nothing; "sample" draws a repository of its own, with no call. -->
<script module>
  import { FOLLOW, SESSION } from "./code-ide-follow.svelte";
  export const uses = ["code.open", "code.refs", "code.fetch", "code.tree", "code.read", "auth.requires", "auth.whoami",
    "ui.versions", "ui.watch", "ui.here", "ui.follow"];

  const HEX = /^[0-9a-f]{7,40}$/i;
  /** `L5` → [5, 5], `L5-L10` (or `L5-10`) → [5, 10]; null when it is no mark. */
  export function markOf(frag) {
    const m = /^L(\d+)(?:-L?(\d+))?$/i.exec(String(frag ?? "").trim());
    if (!m) return null;
    const a = Number(m[1]), z = m[2] ? Number(m[2]) : a;
    return a > 0 && z > 0 ? [Math.min(a, z), Math.max(a, z)] : null;
  }
  export const markText = (m) => (m ? `#L${m[0]}${m[1] > m[0] ? `-L${m[1]}` : ""}` : "");
  const clean = (p) => String(p ?? "").trim().replace(/^\/+|\/+$/g, "");
  /** A tag's `dir`: directories, comma-separated. */
  export const dirsOf = (text) => String(text ?? "").split(",").map(clean).filter(Boolean);
  /** A tag's `open`: `[{ path, mark }]`. */
  export const opensOf = (text) => String(text ?? "").split(",").map((s) => s.trim()).filter(Boolean).map((s) => {
    const at = s.indexOf("#");
    return at < 0 ? { path: clean(s), mark: null } : { path: clean(s.slice(0, at)), mark: markOf(s.slice(at + 1)) };
  }).filter((t) => t.path);
  /** The address's last segment, decoded: `dir=a,b;open=x#L1-L2,y` → `{ dir, open }`. */
  export function parseView(text) {
    const out = { dir: "", open: "" };
    for (const part of String(text ?? "").split(";")) {
      const at = part.indexOf("=");
      const k = at < 0 ? "" : part.slice(0, at).trim();
      if (k === "dir" || k === "open") out[k] = part.slice(at + 1);
    }
    return out;
  }
  /** A tag's `repo`: `owner/name`, or a github.com address → `{ repo, kind: tree | blob | null, rest, mark }`
   *  (`rest` the ref and the path after `/tree/` or `/blob/`, the two told apart against the listing). */
  export function parseRepo(text) {
    let t = String(text ?? "").trim();
    const hash = t.indexOf("#");
    const frag = hash < 0 ? "" : t.slice(hash + 1);
    if (hash >= 0) t = t.slice(0, hash);
    const g = /^(?:https?:\/\/)?(?:www\.)?github\.com\/([^/?#]+)\/([^/?#]+)(?:\/(tree|blob)\/([^?#]+))?\/?(?:\?.*)?$/i.exec(t);
    if (g) {
      let rest = g[4] ? g[4].replace(/\/+$/, "") : "";
      try { rest = decodeURIComponent(rest); } catch { /* as written */ }
      return { repo: `${g[1]}/${g[2].replace(/\.git$/i, "")}`, kind: g[3]?.toLowerCase() ?? null, rest, mark: markOf(frag) };
    }
    return { repo: t.replace(/\.git$/i, ""), kind: null, rest: "", mark: null };
  }
  /** `{ repo, ref, commit, dirs, tabs }` as a tag that reopens it. */
  const attr = (v) => String(v).replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
  export function tagOf(v) {
    const parts = [`repo="${attr(v.repo)}"`];
    if (v.commit) parts.push(`commit="${attr(v.commit)}"`);
    else if (v.ref) parts.push(`ref="${attr(v.ref)}"`);
    if (v.dirs.length) parts.push(`dir="${attr(v.dirs.join(", "))}"`);
    if (v.tabs.length) parts.push(`open="${attr(v.tabs.map((t) => t.path + markText(t.mark)).join(", "))}"`);
    return `<code-ide ${parts.join(" ")}></code-ide>`;
  }
  /** The view's segment as the tag writes `dir` and `open`, undecoded: `dir=src;open=src/a.rs#L5-L10,src/b.rs`. */
  export function viewText(v) {
    const parts = [];
    if (v.dirs.length) parts.push(`dir=${v.dirs.join(",")}`);
    if (v.tabs.length) parts.push(`open=${v.tabs.map((t) => t.path + markText(t.mark)).join(",")}`);
    return parts.join(";");
  }
  /** One segment of the address, percent-encoded, its `= , ; : @` left readable (the router decodes it whole). */
  export const segment = (s) => encodeURIComponent(s).replace(/%3D/gi, "=").replace(/%2C/gi, ",").replace(/%3B/gi, ";").replace(/%3A/gi, ":").replace(/%40/gi, "@");
  /** The same view as an address: `#/code/<owner>/<name>/<ref or commit>/<view>`. */
  export function addressOf(v) {
    const at = v.commit || v.ref, vt = viewText(v);
    return `#/code/${v.repo}${at ? `/${segment(at)}${vt ? `/${segment(vt)}` : ""}` : ""}`;
  }

  /** The tree from `code.tree`'s entries: `{ kids: { dir: [entry] }, kinds: Map(path → kind), files: [path] }`,
   *  a directory's entries directories first, then by name; a directory the entries only imply is made. */
  export function build(entries, truncated = false) {
    const kids = {}, kinds = new Map(), files = [];
    const parentOf = (p) => (p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : "");
    const add = (d, e) => (kids[d] ??= []).push(e);
    const dir = (d, kind = "dir") => {
      if (!d || kinds.has(d)) return;
      dir(parentOf(d));
      kinds.set(d, kind);
      add(parentOf(d), { path: d, name: d.slice(d.lastIndexOf("/") + 1), kind, bytes: 0 });
    };
    for (const e of entries ?? []) {
      if (!e?.path) continue;
      if (e.kind === "dir" || e.kind === "module") { dir(e.path, e.kind); continue; }
      dir(parentOf(e.path));
      if (kinds.has(e.path)) continue;
      kinds.set(e.path, e.kind);
      add(parentOf(e.path), { path: e.path, name: e.path.slice(e.path.lastIndexOf("/") + 1), kind: e.kind, bytes: e.bytes ?? 0, target: e.target });
      files.push(e.path);
    }
    const isDir = (e) => e.kind === "dir" || e.kind === "module";
    for (const d of Object.keys(kids)) kids[d].sort((a, b) => (isDir(a) !== isDir(b) ? (isDir(a) ? -1 : 1) : a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    return { kids, kinds, files, truncated: !!truncated };
  }

  /** The finder: every path holding `q` as a subsequence, best first (runs, a name's start, the file's own name,
   *  a short path), at most `limit`. */
  export function find(paths, q, limit = 50) {
    const s = String(q ?? "").toLowerCase().replace(/\s+/g, "");
    if (!s) return [];
    const out = [];
    for (const p of paths) {
      const l = p.toLowerCase(), base = l.lastIndexOf("/") + 1;
      let i = 0, score = 0, last = -2;
      for (let k = 0; k < l.length && i < s.length; k++) {
        if (l[k] !== s[i]) continue;
        score += 1 + (k === last + 1 ? 3 : 0) + (k >= base ? 2 : 0) + (k === 0 || "/-_.".includes(l[k - 1]) ? 2 : 0);
        last = k;
        i++;
      }
      if (i === s.length) out.push({ p, score: score * 1000 - p.length });
    }
    out.sort((a, b) => b.score - a.score || (a.p < b.p ? -1 : 1));
    return out.slice(0, limit).map((o) => o.p);
  }

  export const short = (c) => String(c ?? "").slice(0, 8);
  export function size(n) {
    const b = Number(n) || 0;
    return b < 1024 ? `${b} B` : b < 1024 * 1024 ? `${(b / 1024).toFixed(1)} KiB` : `${(b / 1024 / 1024).toFixed(1)} MiB`;
  }
  /** When a commit was made, as a person says it. */
  export function when(unix, now = Date.now() / 1000) {
    if (!unix) return "";
    const s = Math.max(0, now - unix), n = (v, w) => `${v} ${w}${v === 1 ? "" : "s"} ago`;
    if (s < 90) return "just now";
    if (s < 3600) return n(Math.round(s / 60), "minute");
    if (s < 2 * 86400) return n(Math.round(s / 3600), "hour");
    if (s < 14 * 86400) return n(Math.round(s / 86400), "day");
    return new Date(unix * 1000).toISOString().slice(0, 10);
  }
  const ghPath = (p) => p.split("/").map(encodeURIComponent).join("/");

  /** The first answer of a call among `types`, or its refusal, or its failure in words. */
  function answer(r, types) {
    const events = Array.isArray(r) ? r : (r?.events ?? []);
    const hit = events.find((e) => e && (types.includes(e.type) || e.type === "refused" || e.type === "error"));
    if (hit) return hit;
    return { type: "error", message: r?.message || "no answer" };
  }

  // What the page holds, shared by every code-ide on it: a tree per commit, a file per commit and path, a
  // fetch per commit, the listing's refs per repository, and whether the reader may fetch.
  const TREES = new Map(), FILES = new Map(), FETCHES = new Map(), REFS = new Map();
  let MAY = null;
  function kept(map, key, ask) {
    let p = map.get(key);
    if (!p) {
      p = ask();
      map.set(key, p);
      p.then((a) => { if (a.type === "error") map.delete(key); });
    }
    return p;
  }

  // ── the sheet's repository: drawn with no call ─────────────────────────
  const S_REPO = "hypermemetic-ai/kiln", S_COMMIT = `9c41d07e${"5".repeat(32)}`;
  const S_FILES = {
    "src/cones.rs": "//! The cones a firing may name.\n\npub const CONES: [&str; 5] = [\"06\", \"04\", \"5\", \"6\", \"10\"];\n\npub fn known(cone: &str) -> Result<(), Refusal> {\n    if CONES.contains(&cone) {\n        return Ok(());\n    }\n    Err(Refusal::unknown_cone(cone, &CONES))\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn a_cone_nobody_fires() {\n        assert!(known(\"7\").is_err());\n    }\n}\n",
    "src/log.rs": "use crate::{cones, store, Firing, Refusal};\n\npub fn log(f: Firing) -> Result<(), Refusal> {\n    cones::known(&f.cone)?;\n    store::append(f)\n}\n",
    "src/store.rs": "use crate::Firing;\n\npub fn append(f: Firing) -> Result<(), crate::Refusal> {\n    // one row a firing\n    Ok(())\n}\n",
    "src/lib.rs": "pub mod cones;\npub mod log;\npub mod store;\n",
    "ui/routes.json": "{\n  \"#/\": \"firing-log\"\n}\n",
    "Cargo.toml": "[package]\nname = \"kiln\"\nversion = \"1.3.0\"\n",
    "README.md": "# kiln\n\nA firing log. Cones 06 to 10.\n",
  };
  const said = (e) => Promise.resolve({ ok: true, events: [e], message: null });
  const SAMPLE = {
    open: () => said({ type: "opened", repo: S_REPO, commit: S_COMMIT, ref: "main", tip: true, committed_unix: 1_790_000_000, kept: true, files: 7, bytes: 900 }),
    refs: () => said({ type: "refs", repo: S_REPO, default_branch: "main", branches: [{ name: "main", commit: S_COMMIT }], tags: [{ name: "v1.2.0", commit: `3f2a91c8${"4".repeat(32)}` }] }),
    fetch: () => said({ type: "fetched", repo: S_REPO, commit: S_COMMIT, files: 7, bytes: 900, new_bytes: 0 }),
    tree: () => said({ type: "tree", repo: S_REPO, commit: S_COMMIT, path: "", truncated: false, entries: Object.keys(S_FILES).sort().map((path) => ({ path, kind: "file", bytes: S_FILES[path].length })) }),
    read: (p) => said(S_FILES[p.file] != null ? { type: "file", path: p.file, bytes: S_FILES[p.file].length, lines: S_FILES[p.file].split("\n").length - 1, text: S_FILES[p.file] } : { type: "not_found", path: p.file }),
  };

  // ── CODE-6: follow — an agent the reader follows opens files in the reader's view ──
  /** A code address as the route reads it — `/app/#/code/<owner>/<name>[/<ref>[/<view>]]` (the origin, `/app/`
   *  and a `?query` may be left out or given), each segment percent-decoded as the router decodes it → `{ repo,
   *  ref, view }`; a view written with its slashes unencoded is read whole. Null when it is no code address. */
  export function codeAddress(to) {
    const m = /^(?:https?:\/\/[^/#]+)?(?:\/app\/?)?#\/code\/([^?]*)/.exec(String(to ?? "").trim());
    if (!m) return null;
    const segs = m[1].split("/");
    const dec = (s) => { try { return decodeURIComponent(s); } catch { return s; } };
    const [owner, name, ref = "", ...rest] = segs.map(dec);
    if (!owner || !name) return null;
    return { repo: `${owner}/${name}`, ref, view: rest.join("/") };
  }
  // The reader's choice and the page's one session are code-ide-follow's (FOLLOW, SESSION). The views on the
  // page (each `{ root, repo, take, bus }`), the shows heard (by `seq`: one applied once), and the bus that says
  // where the page is.
  const VIEWS = new Set(), HEARD = new Set();
  const F = { bus: null, said: null, timer: null, tries: 0, listening: false };
  /** `ui.here`: where the page is, said when the session's watch opens and whenever the address moves; once
   *  more a moment on while the watch is not open yet (`unknown_session`). */
  function sayHere(again = false) {
    clearTimeout(F.timer);
    F.timer = setTimeout(() => {
      const b = F.bus;
      if (!b || !FOLLOW.on) return;
      const route = `${location.pathname}${location.hash || "#/"}`;
      if (route === F.said && !again) return;
      F.said = route;
      b.call("ui.here", { session: SESSION, route }).then((a) => {
        if (a?.ok) { F.tries = 0; return; }
        F.said = null;
        if (++F.tries <= 5) setTimeout(() => sayHere(true), 600);
      }, () => {});
    }, 40);
  }
  /** A `shown` heard on the session's watch (a row: the fold keeps its type off; a knock has no `everyone`). */
  function heard(row) {
    if (!row || typeof row.to !== "string" || !("everyone" in row)) return;
    if (row.seq != null) {
      if (HEARD.has(row.seq)) return;
      HEARD.add(row.seq);
    }
    if (!FOLLOW.on || !(row.everyone || row.by === FOLLOW.who || row.principal === FOLLOW.who)) return;
    const addr = codeAddress(row.to);
    const views = [...VIEWS].filter((v) => v.root()?.isConnected)
      .sort((x, y) => (x.root().compareDocumentPosition(y.root()) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));
    const target = (addr && views.find((v) => v.repo() === addr.repo.toLowerCase())) ?? views[0];
    target?.take(row, addr);
  }

  /** A refusal in words, and what to do. */
  export function refusalOf(e, repo, want) {
    const reason = e?.reason ?? (/bad_repo/.test(e?.message ?? "") ? "bad_repo" : e?.error_kind ?? "error");
    const message = String(e?.message ?? "").replace(/^repo: bad_repo: /, "");
    const todo = {
      bad_repo: "Name a repository as owner/name, or paste its github.com address.",
      not_public: `${repo} is not a public repository on GitHub: only public repositories are read. Check its name, or make it public.`,
      no_such_ref: `GitHub lists no branch or tag ${want ?? ""} in ${repo}. Pick one from the list, or give a commit's hash.`,
      no_such_commit: "GitHub has no such commit in this repository. Check the hash, or open a branch instead.",
      rate_limited: `GitHub is limiting requests${e?.retry_after_secs ? `; try again in ${e.retry_after_secs} s` : "; try again in a minute"}.`,
      too_large: `This commit is past what is kept${e?.max ? ` (${size(e.max)})` : ""}: it cannot be read here. Open it on GitHub.`,
      full: "The code store is full with commits read in the last hour. Try again later, or ask an admin to forget one (code.forget).",
      over_allowance: "Your allowance cannot hold this commit's new bytes (see #/usage). Ask an admin for more.",
      unreachable: "GitHub could not be reached from menger. Try again in a moment.",
      bad_archive: "GitHub's archive of this commit could not be read. Try again, or open it on GitHub.",
      bad_path: "This commit holds a path that leaves its tree, so nothing of it is kept. Open it on GitHub.",
      forbidden: "You may not read code here. Ask for a viewer's grant.",
    }[reason] ?? "Try again; if it stays, the service's words above say why.";
    return { reason, message, todo };
  }
</script>

<script>
  import { onDestroy, tick, untrack } from "svelte";
  import { bus, markdown } from "plexus-ui/host";
  import Tree from "./code-ide-tree.svelte";
  import Tabs from "./code-ide-tabs.svelte";
  import Follow from "./code-ide-follow.svelte";
  import View from "./code-ide-view.svelte";

  let { repo = "", ref = "", commit = "", dir = "", open = "", height = "", view = "", page = "", sheet = null } = $props();
  const b = bus();
  const sample = $derived(sheet === "sample");
  const onPage = $derived(page === true || page === "true");
  const call = {
    open: (p) => (sample ? SAMPLE.open(p) : b.call("code.open", p)),
    refs: (p) => (sample ? SAMPLE.refs(p) : b.call("code.refs", p)),
    fetch: (p) => (sample ? SAMPLE.fetch(p) : b.call("code.fetch", p)),
    tree: (p) => (sample ? SAMPLE.tree(p) : b.call("code.tree", p)),
    read: (p) => (sample ? SAMPLE.read(p) : b.call("code.read", p)),
  };

  let name = $state("");          // the repository, owner/name
  let at = $state.raw(null);      // what code.open answered: { commit, ref, tip, committed_unix, kept, files }
  let pinned = $state(false);     // a commit, not a branch: the view never moves
  let refs = $state.raw(null);    // code.refs' answer
  let phase = $state("loading");
  let busy = $state(false);       // a ref being resolved again, the old view still drawn
  let refusal = $state.raw(null);
  let tree = $state.raw(null);
  let dirs = $state([]);          // the open directories, in the order opened
  let tabs = $state([]);          // [{ path, mark }], in order
  let shown = $state("");
  let crumb = $state("");
  let status = $state("");
  let query = $state("");
  let full = $state(false);
  let drawer = $state(false);
  let reveal = $state.raw(null);
  let jump = $state.raw(null);
  let got = $state.raw({});       // `${repo}@${commit}:${path}` → code.read's answer
  let finder = $state(null);
  let root = $state(null);
  let otherRef = $state(false);
  let engaged = false;            // the last press was inside: ctrl+p is this view's
  let seq = 0;

  let asked = null;               // the ref asked for last, as a refusal names it
  const fail = (e) => {
    refusal = refusalOf(e, name, asked ?? at?.ref);
    phase = "refused";
    busy = false;
  };

  function askRefs(again = false) {
    if (!name) return Promise.resolve(null);
    if (again) REFS.delete(name);
    const p = kept(REFS, name, () => call.refs({ repo: name }).then((r) => answer(r, ["refs"])));
    p.then((e) => { if (e?.type === "refs" && e.repo === name) refs = e; });
    return p;
  }
  function mayFetch() {
    if (sample) return Promise.resolve(true);
    MAY ??= Promise.all([b.call("auth.requires", { method: "code.fetch" }), b.call("auth.whoami", {})]).then(([need, me]) => {
      const roles = (need?.events ?? []).find((e) => Array.isArray(e?.roles))?.roles ?? [];
      const held = (me?.events ?? []).find((e) => Array.isArray(e?.memberships))?.memberships ?? [];
      return held.some((m) => m?.role === "admin" || roles.includes(m?.role));
    }, () => false);
    return MAY;
  }
  const treeOf = (repo, c) => kept(TREES, `${repo}@${c}`, () => call.tree({ repo, commit: c }).then((r) => answer(r, ["tree", "not_kept", "not_found"])));
  const fetchOf = (repo, c) => kept(FETCHES, `${repo}@${c}`, () => call.fetch({ repo, commit: c }).then((r) => answer(r, ["fetched"])));

  const isDir = (k) => k === "dir" || k === "module";
  const keyOf = (path) => (at ? `${name}@${at.commit}:${path}` : "");
  /** A file read once for this commit (the page's map), its answer kept for the view. */
  function readFile(path) {
    if (!at || !tree || !path || !tree.kinds.has(path) || isDir(tree.kinds.get(path))) return;
    const k = keyOf(path), c = at.commit, repo = name;
    if (k in got) return;
    kept(FILES, k, () => call.read({ repo, commit: c, file: path }).then((r) => answer(r, ["file", "binary", "too_large", "link", "not_found", "not_kept"])))
      .then((a) => { if (!(k in got)) got = { ...got, [k]: a }; });
  }

  // ── what the tag says, then the commit, then the tree ──────────────────
  let named = [];                 // the directories the tag (or a show) named, the first scrolled to
  /** What the tag says — or, from a show of another repository (CODE-6), what its address says. */
  async function start(from = { repo, ref, commit, dir, open, view }) {
    const n = ++seq;
    phase = "loading"; refusal = null; tree = null; at = null; refs = null; got = {}; status = ""; crumb = ""; query = ""; busy = false; otherRef = false;
    if (sheet === "loading") return;
    const p = parseRepo(sample ? from.repo || S_REPO : from.repo);
    const v = parseView(from.view);
    name = p.repo.toLowerCase(); // GitHub reads a name without regard to case; the service answers it lower-cased
    named = dirsOf(from.dir || v.dir);
    let ds = named, os = opensOf(from.open || v.open);
    let refname = String(from.ref ?? "").trim() || null, pin = String(from.commit ?? "").trim() || null;
    if (!name) return fail({ reason: "bad_repo", message: "no repository named: repo=\"owner/name\", or a github.com address" });
    if (p.kind && p.rest) {
      const e = await askRefs();
      if (n !== seq) return;
      if (e?.type !== "refs") return fail(e);
      const names = [...e.branches, ...e.tags].map((r) => r.name).sort((x, y) => y.length - x.length);
      const hit = names.find((nm) => p.rest === nm || p.rest.startsWith(`${nm}/`));
      let urlRef = hit ?? null, urlCommit = null, rest = hit ? p.rest.slice(hit.length + 1) : "";
      if (!hit) {
        const [h, ...more] = p.rest.split("/");
        rest = more.join("/");
        if (HEX.test(h)) urlCommit = h; else urlRef = h;
      }
      if (!refname && !pin) { pin = urlCommit; refname = urlCommit ? null : urlRef; }
      if (p.kind === "tree" && rest && !ds.length) ds = [rest];
      if (p.kind === "blob" && rest) {
        if (!os.length) os = [{ path: rest, mark: p.mark }];
        if (!ds.length && rest.includes("/")) ds = [rest.slice(0, rest.lastIndexOf("/"))];
      }
    }
    tabs = os;
    shown = os[0]?.path ?? "";
    dirs = ds;
    pinned = !!pin;
    if (!pin) askRefs();
    await go({ refname, commit: pin }, n, "start");
  }

  async function go(want, n, why) {
    const before = at;
    asked = want.refname ?? want.commit ?? null;
    busy = true;
    if (!tree) phase = "loading";
    let e = answer(await call.open({ repo: name, ...(want.commit ? { commit: want.commit } : want.refname ? { refname: want.refname } : {}) }), ["opened"]);
    if (n !== seq) return;
    // A route's ref may be a commit's hash: what the listing lacks and is hex is read as one.
    if (e.type === "refused" && e.reason === "no_such_ref" && want.refname && HEX.test(want.refname)) {
      e = answer(await call.open({ repo: name, commit: want.refname }), ["opened"]);
      if (n !== seq) return;
      pinned = true;
    }
    if (e.type !== "opened") return fail(e);
    name = e.repo;
    let opened = e;
    if (!e.kept) {
      at = e;
      if (!(await mayFetch())) {
        if (n !== seq) return;
        tree = null; busy = false; phase = "unfetched";
        status = `${short(e.commit)} is not kept yet; nothing was asked of GitHub`;
        return;
      }
      if (n !== seq) return;
      tree = null; phase = "fetching";
      status = `fetching ${name} at ${short(e.commit)} from GitHub, once; it is kept after`;
      const f = await fetchOf(name, e.commit);
      if (n !== seq) return;
      if (f.type !== "fetched") return fail(f);
      const again = answer(await call.open({ repo: name, commit: f.commit }), ["opened"]);
      if (n !== seq) return;
      opened = again.type === "opened" ? { ...again, ref: e.ref, tip: e.tip } : { ...e, commit: f.commit, kept: true, files: f.files, bytes: f.bytes };
    }
    const t = await treeOf(name, opened.commit);
    if (n !== seq) return;
    if (t.type !== "tree") return fail(t.type === "not_kept" ? { reason: "not_kept", message: `${short(opened.commit)} is not kept` } : t);
    at = opened;
    tree = build(t.entries, t.truncated);
    busy = false;
    phase = "ready";
    settle(why, before);
  }

  /** The tree drawn: the named directories open (each with its parents), what it lacks said in the crumb. */
  function settle(why, before) {
    const lacks = [];
    const open = [];
    for (const d of dirs) {
      if (!isDir(tree.kinds.get(d))) { lacks.push(d); continue; }
      const parts = d.split("/");
      for (let i = 1; i <= parts.length; i++) { const p = parts.slice(0, i).join("/"); if (!open.includes(p)) open.push(p); }
    }
    if (why === "start") {
      dirs = open;
      const first = named.find((d) => isDir(tree.kinds.get(d))) ?? open.at(-1);
      if (first) reveal = { dir: first, n: (reveal?.n ?? 0) + 1 };
    }
    crumb = lacks.length ? `${lacks.join(", ")} ${lacks.length > 1 ? "are" : "is"} not in the tree at ${short(at.commit)}` : why === "start" && named.length ? `starts in ${named[0]}/` : "";
    const files = `${tree.files.length} file${tree.files.length === 1 ? "" : "s"}`;
    if (why === "refresh") status = before?.commit === at.commit ? `asked GitHub: ${at.ref} is still at ${short(at.commit)} (its answer is kept a minute). Nothing fetched.` : `${at.ref} moved from ${short(before?.commit)} to ${short(at.commit)}; the tabs stay, and a file the tree lacks says so.`;
    else if (pinned) status = `pinned at ${short(at.commit)}: this view never moves. ${files}, kept by the service.`;
    else status = `${at.ref} resolved to its tip, ${short(at.commit)}. ${files}, kept by the service; opening a file asks GitHub nothing.`;
  }

  // The props: a new tag or address starts again — but not the address this view wrote itself.
  let wrote = null;
  $effect(() => {
    const k = JSON.stringify([repo, ref, commit, dir, open, view, sheet, page]);
    untrack(() => {
      if (wrote && onPage && wrote.key === JSON.stringify([repo, String(ref ?? ""), String(view ?? "")])) return;
      if (wrote?.props === k) return;
      wrote = { props: k, key: null };
      start();
    });
  });

  // The shown file read when it can be (once: the page's map), and a file under the pointer read ahead.
  $effect(() => {
    const s = shown, t = tree, a = at;
    if (s && t && a) untrack(() => readFile(s));
  });
  let hoverAt = null;
  function hovered(path) {
    if (hoverAt != null) clearTimeout(hoverAt);
    hoverAt = path ? setTimeout(() => { hoverAt = null; readFile(path); }, 150) : null;
  }

  // ── what a person does ────────────────────────────────────────────────
  function toggle(d) {
    dirs = dirs.includes(d) ? dirs.filter((x) => x !== d) : [...dirs, d];
  }
  function openFile(path) {
    if (!tabs.some((t) => t.path === path)) tabs = [...tabs, { path, mark: null }];
    shown = path;
    drawer = false;
    if (query) query = "";
    status = `opened ${path} at ${short(at?.commit)}`;
  }
  function close(path) {
    const i = tabs.findIndex((t) => t.path === path);
    if (i < 0) return;
    const rest = tabs.filter((t) => t.path !== path);
    if (shown === path) shown = (rest[i] ?? rest[i - 1])?.path ?? "";
    tabs = rest;
  }
  function mark(m) {
    if (!shown) return;
    tabs = tabs.map((t) => (t.path === shown ? { path: t.path, mark: m } : t));
    status = m ? `line${m[1] > m[0] ? `s ${m[0]} to ${m[1]}` : ` ${m[0]}`} marked; shift-click another to mark a range` : "marks cleared";
  }
  function goto(ev) {
    if (ev.key !== "Enter") return;
    ev.preventDefault();
    const m = markOf(`L${ev.currentTarget.value.trim().replace(/^L/i, "")}`);
    if (!m) { status = "a line is a number, or two: 12, or 12-20"; return; }
    mark(m);
    jump = { mark: m, n: (jump?.n ?? 0) + 1 };
    ev.currentTarget.value = "";
  }
  function refresh() {
    if (!at?.ref || pinned) return;
    status = `asking GitHub for ${at.ref}'s tip…`;
    askRefs(true);
    go({ refname: at.ref }, ++seq, "refresh");
  }
  function pin() {
    if (!at) return;
    pinned = true;
    status = `pinned at ${short(at.commit)}: this view never moves`;
  }
  function pick(ev) {
    const v = ev.currentTarget.value;
    if (v === "other") {
      ev.currentTarget.value = refValue;
      otherRef = true;
      tick().then(() => root?.querySelector("[data-field=commit-input]")?.focus());
      return;
    }
    const [kind, ...rest] = v.split(":");
    const val = rest.join(":");
    if (kind === "c") { pinned = true; go({ commit: val }, ++seq, "pin"); }
    else { pinned = false; status = `asking GitHub for ${val}…`; go({ refname: val }, ++seq, "ref"); }
  }
  function otherKey(ev) {
    if (ev.key === "Escape") { ev.preventDefault(); otherRef = false; return; }
    if (ev.key !== "Enter") return;
    ev.preventDefault();
    const h = ev.currentTarget.value.trim();
    if (!HEX.test(h)) { status = "a commit is its hash: 7 to 40 hex digits"; return; }
    otherRef = false;
    pinned = true;
    go({ commit: h }, ++seq, "pin");
  }

  // ── the view as a tag and as an address, from one state ────────────────
  function viewNow() {
    const open = new Set(dirs);
    const visible = (d) => { for (let p = d; p.includes("/"); ) { p = p.slice(0, p.lastIndexOf("/")); if (!open.has(p)) return false; } return true; };
    const shownDirs = dirs.filter((d) => (!tree || isDir(tree.kinds.get(d))) && visible(d));
    return {
      repo: name,
      ref: pinned ? null : (at?.ref ?? (String(ref ?? "").trim() || null)),
      commit: pinned ? (at?.commit ?? (String(commit ?? "").trim() || null)) : null,
      dirs: shownDirs.filter((d) => !shownDirs.some((o) => o !== d && o.startsWith(`${d}/`))),
      tabs: tabs.map((t) => ({ path: t.path, mark: t.mark ? [t.mark[0], t.mark[1]] : null })),
    };
  }
  async function copy(what) {
    const v = viewNow();
    const text = what === "tag" ? tagOf(v) : `${location.origin}${location.pathname}${addressOf(what === "permalink" ? { ...v, ref: null, commit: at?.commit } : v)}`;
    try {
      await navigator.clipboard.writeText(text);
      status = `copied ${what === "tag" ? "this view as a tag" : what === "link" ? "a link to this view" : "a link that never moves"}: ${text}`;
    } catch {
      status = `could not copy; here it is: ${text}`;
    }
  }
  // On the page, the address follows the view — in place, no history entry, no route change.
  $effect(() => {
    if (!onPage || sample || phase !== "ready" || !at) return;
    const v = viewNow();
    untrack(() => {
      const h = addressOf(v);
      if (location.hash !== h) history.replaceState(history.state, "", h);
      wrote = { ...(wrote ?? {}), key: JSON.stringify([repo, v.commit || v.ref || "", viewText(v)]) };
    });
  });

  // ── keys ──────────────────────────────────────────────────────────────
  function findKey() {
    if (!root || phase !== "ready") return;
    if (!onPage && !engaged && !root.contains(document.activeElement)) return;
    drawer = true;
    tick().then(() => { finder?.focus(); finder?.select(); });
  }
  if (sheet == null) b.keys({ "ctrl+p": { to: findKey, label: "find a file in the code" }, "meta+p": { to: findKey, label: "find a file in the code" } });
  $effect(() => {
    if (!root) return;
    const away = (ev) => { engaged = root.contains(ev.target); };
    addEventListener("pointerdown", away, true);
    return () => removeEventListener("pointerdown", away, true);
  });
  function leave() {
    query = "";
    drawer = false;
    tick().then(() => root?.querySelector("[data-surface=code-ide-view] .cm-content")?.focus());
  }
  let offEscape = null;
  $effect(() => {
    const on = full;
    untrack(() => {
      if (on && !offEscape) offEscape = b.keys({ escape: { to: () => { if (query || drawer) leave(); else full = false; }, label: "put the code back in its place", in_inputs: true, only: true } });
      if (!on && offEscape) { offEscape(); offEscape = null; }
    });
  });

  // ── CODE-6: follow ────────────────────────────────────────────────────
  // With follow on, the session's watch: `ui.watch` with the page's id and whom it follows (one stream for
  // every code-ide on the page: the same spec is one collection), its shows folded by their number. Off, it
  // is let go and nothing is asked. The snapshot is the live version alone, a row with no `seq`, kept by
  // nobody: the collection holds the shows.
  let note = $state.raw(null);    // { by, principal, say, to, link, point, n }
  let pend = $state.raw(null);    // { path, mark }: a shown file's lines, scrolled to once its text is drawn
  // The session's watch is code-ide-follow's (one collection for every code-ide on the page); its shows come
  // here (`heard`, a `seq` applied once, by the view that should take it), and its opening says where it is.
  if (sheet == null) {
    const me = { root: () => root, repo: () => name, take, bus: b };
    VIEWS.add(me);
    F.bus ??= b;
    if (!F.listening) { F.listening = true; addEventListener("hashchange", () => sayHere()); }
    onDestroy(() => {
      VIEWS.delete(me);
      if (F.bus === b) F.bus = [...VIEWS][0]?.bus ?? null;
    });
  }
  /** The reader turned follow on or off, or named another agent (code-ide-follow said `ui.follow`). */
  function followed(on, who, refusal) {
    if (refusal) { status = refusal; return; }
    if (!on) { note = null; status = "following nobody: nothing moves this view but you"; return; }
    status = `following ${who}: what ${who} shows opens here (ui.show). Turn it off at any time.`;
    sayHere(true);
  }

  /** A show applied here, in place: its files tabs beside the reader's (the first shown, its lines marked and
   *  scrolled to), its directories open, its ref or repository taken as the controls take them; its words
   *  the note. Nothing navigates; on the page the address follows the view as it always does. */
  function take(row, addr) {
    const to = String(row.to);
    const link = !addr && /^(\/app\/)?#\//.test(to) ? (to.startsWith("#") ? `/app/${to}` : to) : null;
    note = { by: row.by, principal: row.principal, say: row.say ?? "", to, link, point: row.point ?? null, n: (note?.n ?? 0) + 1 };
    // What it shows is put before the reader: in a body, the view scrolled onto the screen (never the page's address).
    if (!onPage && !full) tick().then(() => root?.scrollIntoView({ block: "nearest" }));
    if (!addr) { status = `${row.by} shows you ${to}; this view stays as it is`; return; }
    const v = parseView(addr.view);
    const os = opensOf(v.open), ds = dirsOf(v.dir);
    const first = os[0] ?? null;
    if (addr.repo.toLowerCase() !== name) {
      // Another repository: it is opened as a tag naming it would be; the reader's tabs of the old one go.
      pend = first?.mark ? { path: first.path, mark: first.mark } : null;
      start({ repo: addr.repo, ref: addr.ref, commit: "", dir: v.dir, open: v.open, view: "" });
      status = `${row.by} opened ${addr.repo}${first ? ` at ${first.path}` : ""}; your tabs of ${name} were closed`;
      return;
    }
    // The same repository: the reader's tabs and marks kept, the show's added (a mark it names replaces).
    let next = tabs.slice();
    for (const t of os) {
      const i = next.findIndex((x) => x.path === t.path);
      if (i < 0) next.push({ path: t.path, mark: t.mark });
      else if (t.mark) next[i] = { path: t.path, mark: t.mark };
    }
    tabs = next;
    const opened = [...dirs];
    const add = (d) => { const parts = d.split("/"); for (let i = 1; i <= parts.length; i++) { const p = parts.slice(0, i).join("/"); if (p && !opened.includes(p)) opened.push(p); } };
    for (const d of ds) add(d);
    if (first?.path.includes("/")) add(first.path.slice(0, first.path.lastIndexOf("/")));
    dirs = opened;
    if (ds[0]) reveal = { dir: ds[0], n: (reveal?.n ?? 0) + 1 };
    if (first) { shown = first.path; pend = first.mark ? { path: first.path, mark: first.mark } : null; }
    drawer = false;
    // Another ref or commit: taken as the ref control takes one (a hash the listing lacks is read as a commit).
    const want = String(addr.ref ?? "").trim();
    const same = !want || (pinned ? at?.commit?.startsWith(want.toLowerCase()) : want === at?.ref);
    if (!same) {
      pinned = false;
      go({ refname: want }, ++seq, "ref");
    }
    status = `${row.by} opened ${first ? `${first.path}${first.mark ? ` at L${first.mark[0]}${first.mark[1] > first.mark[0] ? `–L${first.mark[1]}` : ""}` : ""}` : addr.repo}${same ? "" : ` on ${want}`} here; your other tabs are kept`;
  }
  // A shown file's lines scrolled to once its text is drawn (the view scrolls a file it draws first itself).
  $effect(() => {
    const p = pend, s = shown, t = text, k = shownKey;
    if (!p || s !== p.path || t == null || !k) return;
    untrack(() => { pend = null; tick().then(() => { jump = { mark: p.mark, n: (jump?.n ?? 0) + 1 }; }); });
  });
  // `point`: what it names inside this view ringed until the note is dismissed (again when the lines redraw).
  $effect(() => {
    const n = note, _t = text, _s = shown, _p = phase;
    untrack(() => tick().then(() => {
      for (const el of root?.querySelectorAll("[data-code-ide-ring]") ?? []) el.removeAttribute("data-code-ide-ring");
      if (!n?.point || !root) return;
      let el = null;
      try { el = root.querySelector(n.point); } catch { /* not a selector */ }
      el?.setAttribute("data-code-ide-ring", "");
    }));
  });
  function dismiss() {
    note = null;
    tick().then(() => root?.querySelector("[data-action=follow]")?.focus());
  }
  // On the page, where it is follows the address (written in place, which no hashchange says).
  $effect(() => {
    if (!FOLLOW.on || !onPage || phase !== "ready") return;
    void viewNow();
    untrack(() => tick().then(() => sayHere()));
  });

  // ── what is drawn ─────────────────────────────────────────────────────
  const matches = $derived(tree && query ? find(tree.files, query) : []);
  const shownTab = $derived(tabs.find((t) => t.path === shown) ?? null);
  const inTree = $derived(!!(tree && shown && tree.kinds.has(shown) && !isDir(tree.kinds.get(shown))));
  const shownKey = $derived(at && shown ? keyOf(shown) : "");
  const shownAnswer = $derived(shownKey ? (got[shownKey] ?? null) : null);
  const gone = $derived(tree ? tabs.filter((t) => !tree.kinds.has(t.path) || isDir(tree.kinds.get(t.path))).map((t) => t.path) : []);
  const keep = $derived(at ? tabs.map((t) => keyOf(t.path)) : []);
  const ghAt = $derived(at?.commit ?? (pinned ? commit : at?.ref) ?? "");
  const ghHref = $derived(!name ? "https://github.com/" : !at ? `https://github.com/${name}`
    : inTree ? `https://github.com/${name}/blob/${ghAt}/${ghPath(shown)}${shownTab?.mark ? `#L${shownTab.mark[0]}${shownTab.mark[1] > shownTab.mark[0] ? `-L${shownTab.mark[1]}` : ""}` : ""}`
    : `https://github.com/${name}/tree/${ghAt}${dirs.length ? `/${ghPath(dirs.at(-1))}` : ""}`);
  const notice = $derived.by(() => {
    if (sheet === "loading" || phase === "loading") return { kind: "loading", text: `asking for ${name || "the repository"}…` };
    if (phase === "refused") return { kind: "refused", reason: refusal?.reason, text: refusal?.message, todo: refusal?.todo };
    if (phase === "unfetched") return { kind: "unfetched", text: `${name} at ${short(at?.commit)} is not kept by menger yet.`, todo: "Someone who may fetch code (an editor) must open this view once; then it is kept, and anyone who reads here sees it. Nothing was asked of GitHub." };
    if (phase === "fetching") return { kind: "fetching", text: `fetching ${name} at ${short(at?.commit)} from GitHub — once; every later read is menger's own.` };
    if (!shown) return { kind: "pick", text: "Pick a file in the tree, or find one by name (Ctrl P)." };
    if (!inTree) return { kind: "gone", text: `${shown} is not in the tree at ${short(at?.commit)}. The tab is kept; another branch or commit may hold it.` };
    const a = shownAnswer;
    if (!a) return { kind: "reading", text: `reading ${shown}…` };
    if (a.type === "file") return null;
    if (a.type === "binary") return { kind: "binary", text: `${shown} is a binary file, ${size(a.bytes)}: it is not drawn here.`, link: ghHref };
    if (a.type === "too_large") return { kind: "too-large", text: `${shown} is ${size(a.bytes)}, past the ${size(a.max)} drawn here.`, link: ghHref };
    if (a.type === "link") return { kind: "link", text: `${shown} is a link to ${a.target} (never followed).` };
    if (a.type === "not_kept") return { kind: "unfetched", text: `${short(at?.commit)} is not kept any more; open the view again to fetch it.` };
    if (a.type === "not_found") return { kind: "gone", text: `${shown} is not in the tree at ${short(at?.commit)}.` };
    return { kind: "refused", reason: a.reason ?? "error", text: a.message ?? "the file could not be read", todo: refusalOf(a, name).todo };
  });
  const text = $derived(shownAnswer?.type === "file" ? shownAnswer.text : null);
  const refValue = $derived(!at ? "" : pinned || !at.ref ? `c:${at.commit}` : (refs?.tags ?? []).some((t) => t.name === at.ref) ? `t:${at.ref}` : `b:${at.ref}`);
  const heightOf = $derived(/^\d+(\.\d+)?(px|vh|dvh|svh|lvh|em|rem|%)$/.test(String(height ?? "").trim()) ? String(height).trim() : "");
  const lines = $derived(shownAnswer?.type === "file" ? shownAnswer.lines : null);
</script>

<div class="ide" class:full class:on-page={onPage} data-surface="code-ide" data-state={sheet === "loading" ? "loading" : phase} data-repo={name}
  data-commit={at?.commit ?? ""} data-pinned={pinned ? "true" : "false"} data-drawer={drawer ? "open" : "closed"} data-full={full ? "true" : "false"}
  style={heightOf && !onPage && !full ? `height: max(320px, ${heightOf})` : undefined} bind:this={root}>
  <div class="head">
    <button type="button" class="ctl treebtn" data-action="files" aria-label="files" aria-expanded={drawer ? "true" : "false"} onclick={() => (drawer = !drawer)}>☰</button>
    <a class="repo" href={`https://github.com/${name}`} target="_blank" rel="noopener noreferrer" data-field="repo">{name || "…"}</a>
    <select class="ref" data-field="ref" aria-label="branch, tag or commit" value={refValue} onfocus={() => askRefs()} onpointerdown={() => askRefs()} onchange={pick} disabled={!at || sample}>
      {#if at && (pinned || !at.ref)}<option value={`c:${at.commit}`}>{short(at.commit)} · pinned</option>{/if}
      {#if at && !pinned && at.ref && !(refs?.branches ?? []).some((r) => r.name === at.ref) && !(refs?.tags ?? []).some((r) => r.name === at.ref)}<option value={`b:${at.ref}`}>{at.ref} · {at.tip ? "latest" : "tag"}</option>{/if}
      {#each refs?.branches ?? [] as r (r.name)}<option value={`b:${r.name}`}>{r.name} · latest</option>{/each}
      {#each refs?.tags ?? [] as r (r.name)}<option value={`t:${r.name}`}>{r.name} · tag</option>{/each}
      <option value="other">a commit…</option>
    </select>
    {#if otherRef}<input class="hash" data-field="commit-input" placeholder="a commit's hash, Enter" aria-label="a commit's hash" size="14" spellcheck="false" onkeydown={otherKey} onblur={() => (otherRef = false)} />{/if}
    {#if at}<span class="pill" class:tip={!pinned && at.tip} class:pin={pinned || !at.tip} data-field="commit" title={at.commit}>{short(at.commit)}{#if at.committed_unix}{` · ${when(at.committed_unix)}`}{/if}{#if pinned}{" · pinned"}{/if}</span>{/if}
    {#if at && !pinned && at.tip && phase === "ready"}
      <button type="button" class="ctl" data-action="refresh" title="ask GitHub for the branch's tip again" disabled={busy} onclick={refresh}>{busy ? "asking…" : "refresh"}</button>
      <button type="button" class="ctl" data-action="pin" title="pin this commit: the view never moves" onclick={pin}>pin</button>
    {/if}
    {#if sheet == null || sample}<Follow {sheet} disabled={sample} onheard={heard} onopen={() => sayHere(true)} onchange={followed} />{/if}
    <span class="grow"></span>
    <a class="gh" data-action="github" href={ghHref} target="_blank" rel="noopener noreferrer">on GitHub ↗</a>
    {#if !onPage}<button type="button" class="ctl" data-action="full" aria-pressed={full ? "true" : "false"} title={full ? "put it back in its place (Escape)" : "fill the window with this view"} onclick={() => (full = !full)}>{full ? "back" : "fill"}</button>{/if}
  </div>
  <div class="notes">
    {#if note}
      <div class="note" data-surface="code-ide-note" data-by={note.by} data-code={note.link || !codeAddress(note.to) ? "false" : "true"} role="status" title={note.principal}>
        <b class="by">{note.by} shows you</b>
        <span class="words">
          {#if note.say}<span class="say">{@html markdown(note.say)}</span>{/if}
          {#if !codeAddress(note.to)}
            {#if note.link}<a class="to" data-field="note-to" href={note.link}>{note.to}</a>{:else}<span class="to" data-field="note-to">{note.to}</span>{/if}
          {/if}
        </span>
        <button type="button" class="ctl" data-action="dismiss" aria-label="dismiss the note" onclick={dismiss}>dismiss</button>
      </div>
    {/if}
  </div>
  <div class="main">
    <div class="side">
      <Tree {tree} open={dirs} {shown} tabs={tabs.map((t) => t.path)} {query} {matches} {crumb} {reveal} bind:finder
        sheet={sheet === "loading" || phase === "loading" || phase === "fetching" ? "loading" : null}
        ontoggle={toggle} onopen={openFile} onhover={hovered} onquery={(q) => (query = q)} onfind={() => matches[0] && openFile(matches[0])} onleave={leave} />
    </div>
    {#if drawer}<button type="button" class="scrim" aria-label="close the files" onclick={() => (drawer = false)}></button>{/if}
    <div class="pane">
      <Tabs {tabs} {shown} {gone} onshow={(p) => (shown = p)} onclose={close} />
      <div class="path" data-field="path">
        {#if shown}
          <span class="where">{shown.split("/").join(" / ")}</span>
          {#if lines != null}<span>{lines} line{lines === 1 ? "" : "s"}</span>{/if}
          {#if shownTab?.mark}<span data-field="marked">L{shownTab.mark[0]}{#if shownTab.mark[1] > shownTab.mark[0]}–L{shownTab.mark[1]}{/if} marked</span>
            <button type="button" class="ctl small" data-action="unmark" onclick={() => mark(null)}>clear</button>{/if}
          {#if text != null}<input class="goto" data-goto="" placeholder="line" aria-label="mark a line or a range: 12, or 12-20, then Enter" size="7" spellcheck="false" onkeydown={goto} />{/if}
        {:else}
          <span>{at ? `${short(at.commit)}${at.ref && !pinned ? ` · ${at.ref}` : ""}` : ""}</span>
        {/if}
      </div>
      <div class="code">
        <View {text} key={shownKey} path={shown} mark={shownTab?.mark ?? null} {keep} {jump} onmark={mark} />
        {#if notice}
          <div class="notice" data-notice={notice.kind} data-reason={notice.reason ?? undefined} role={notice.kind === "refused" ? "alert" : undefined}>
            <p class="say">{notice.text}</p>
            {#if notice.todo}<p class="todo">{notice.todo}</p>{/if}
            {#if notice.link}<p><a href={notice.link} target="_blank" rel="noopener noreferrer" data-action="github-file">open it on GitHub ↗</a></p>{/if}
          </div>
        {/if}
      </div>
    </div>
  </div>
  <div class="foot">
    <span class="said" role="status" data-field="status">{status}</span>
    {#if phase === "ready"}
      <span class="copies">
        <button type="button" class="ctl" data-action="copy-tag" onclick={() => copy("tag")}>copy this view as a tag</button>
        <button type="button" class="ctl" data-action="copy-link" onclick={() => copy("link")}>copy link</button>
        <button type="button" class="ctl" data-action="copy-permalink" title="a link at this commit: it never moves" onclick={() => copy("permalink")}>copy permanent link</button>
      </span>
    {/if}
  </div>
</div>

<style>
  .ide { position: relative; display: grid; grid-template-rows: auto auto minmax(0, 1fr) auto; grid-template-columns: minmax(0, 1fr); box-sizing: border-box; height: max(320px, 70vh); min-width: 0; margin: 6px 0; overflow: hidden; color: var(--ink); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-xl); font-family: var(--font-text); font-size: 12.5px; line-height: 1.5; }
  .ide.on-page { height: 100vh; height: 100dvh; margin: 0; border: 0; border-radius: 0; }
  .ide.full { position: fixed; inset: 0; z-index: 900; height: auto; margin: 0; border-radius: 0; }
  .head { display: flex; flex-wrap: wrap; gap: 4px 8px; align-items: center; min-width: 0; padding: 5px 8px; border-bottom: var(--border) solid var(--line); }
  .ide a { font: inherit; }
  .ide .repo { font-weight: var(--weight-strong); color: var(--ink); text-decoration: none; overflow-wrap: anywhere; }
  .ide .repo:hover, .ide .repo:global([data-sheet~=hover]) { text-decoration: underline; }
  .ref { max-width: 16em; }
  .ref, .hash, .goto { min-width: 0; padding: 1px 6px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .ref:disabled, .ref:global([data-sheet~=disabled]) { color: var(--muted); }
  .pill { padding: 0 7px; font-size: 11px; line-height: 18px; white-space: nowrap; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .pill.tip { color: var(--live); }
  .pill.pin { color: var(--code-type); }
  .grow { flex: 1; }
  .ide .gh { color: var(--accent); text-decoration: none; white-space: nowrap; }
  .ide .gh:hover, .ide .gh:global([data-sheet~=hover]) { text-decoration: underline; }
  .ctl { padding: 1px 7px; font: inherit; color: var(--ink); background: none; border: 0; border-radius: var(--radius); cursor: pointer; white-space: nowrap; }
  .ctl:hover, .ctl:global([data-sheet~=hover]) { background: var(--hover); }
  .ctl:disabled, .ctl:global([data-sheet~=disabled]) { color: var(--disabled); cursor: default; }
  .ctl[aria-pressed=true], .ctl:global([data-sheet~=checked]) { color: var(--accent); }
  .ctl.small { padding: 0 5px; font-size: 11px; color: var(--muted); }
  .ctl:focus-visible, .ref:focus-visible, .hash:focus-visible, .goto:focus-visible, .repo:focus-visible, .gh:focus-visible, .notice a:focus-visible,
  .ctl:global([data-sheet~=focus]), .ref:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .treebtn { display: none; }
  /* CODE-6: the note of what an agent showed (the follow control is code-ide-follow's). */
  .notes { min-width: 0; }
  .note { display: flex; gap: 8px; align-items: baseline; min-width: 0; padding: 5px 10px; background: var(--sel); border-bottom: var(--border) solid var(--line); }
  .note .by { flex: none; font-weight: var(--weight-strong); color: var(--accent); }
  .note .words { display: flex; flex: 1; flex-wrap: wrap; gap: 2px 10px; align-items: baseline; min-width: 0; color: var(--ink); overflow-wrap: anywhere; }
  .note .say :global(p) { display: inline; margin: 0; }
  .note .say :global(a), .note .to { color: var(--accent); }
  .note .to { font-size: 11px; }
  .note a:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ide :global([data-code-ide-ring]) { outline: 2px solid var(--accent); outline-offset: 1px; border-radius: var(--radius); }
  .main { position: relative; display: grid; grid-template-columns: clamp(150px, 28%, 240px) minmax(0, 1fr); min-height: 0; min-width: 0; }
  .side { min-height: 0; min-width: 0; border-right: var(--border) solid var(--line); }
  .scrim { display: none; }
  .pane { display: grid; grid-template-rows: auto auto 1fr; min-width: 0; min-height: 0; }
  .path { display: flex; flex-wrap: wrap; gap: 2px 10px; align-items: center; min-height: 20px; padding: 3px 10px; font-size: 11px; color: var(--muted); border-bottom: var(--border) solid var(--line); }
  .where { overflow-wrap: anywhere; }
  .goto { margin-left: auto; padding: 0 5px; font-size: 11px; }
  .code { position: relative; min-height: 0; min-width: 0; }
  .notice { position: absolute; inset: 0; z-index: 2; padding: 24px 28px; overflow: auto; color: var(--muted); background: var(--panel); }
  .notice .say { margin: 0 0 6px; color: var(--ink); overflow-wrap: anywhere; }
  .notice[data-notice=refused] .say { color: var(--warn); }
  .notice[data-notice=loading] .say, .notice[data-notice=reading] .say, .notice[data-notice=pick] .say { color: var(--muted); }
  .notice .todo { margin: 0 0 6px; max-width: 72ch; }
  .notice p { margin: 0 0 6px; }
  .notice a { color: var(--accent); }
  .foot { display: flex; flex-wrap: wrap; gap: 2px 12px; align-items: center; min-height: 23px; padding: 2px 8px; font-size: 11px; color: var(--muted); border-top: var(--border) solid var(--line); }
  .said { flex: 1 1 320px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .copies { display: flex; flex: 0 1 auto; flex-wrap: wrap; justify-content: flex-end; gap: 0 2px; min-width: 0; margin-left: auto; }
  .copies .ctl { font-size: 11px; }
  /* Under 641 px the tree is a drawer over the lines. */
  @media (max-width: 640px) {
    .main { grid-template-columns: minmax(0, 1fr); }
    .side { position: absolute; top: 0; bottom: 0; left: 0; z-index: 4; display: none; width: 84%; background: var(--panel); }
    .ide[data-drawer=open] .side { display: block; }
    .ide[data-drawer=open] .scrim { position: absolute; inset: 0; z-index: 3; display: block; padding: 0; background: var(--shadow); border: 0; border-radius: 0; cursor: default; }
    .treebtn { display: inline-block; }
    .ide .gh { display: none; }
    .said { flex-basis: 100%; white-space: normal; }
  }
</style>
