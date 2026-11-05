<!-- COMPS-4: components defined in tickets, and any tag in any body drawn by its component. The owner,
     2026-11-05: "we should be able to refer to any component inside a ticket and define them in tickets,
     which would trigger a save to plexus ui!"; frames "serve as foundation for rendering other peoples ui
     elements inside your tickets".
     - **A definition** is a fence, ```component <name> (rhx, profile rhx/1) or ```component <name> html
       (a custom element's HTML and script, which only ever runs framed) — `definitions(body)`. A name is
       a tag's: lower case, with a hyphen (`x-counter`).
     - **On save** (`authorChanged(b, before, after)`, called by the editor after its amend, ticket-edit
       after its body's, a doc after a pause) each definition whose text changed is `ui.author`ed: into
       the live bundle when the saver holds ui.author on `*`, else — refused `forbidden` — into their own
       namespace (`own: true`, plexus-ui's COMPS-4); an html one into the namespace always. The answer is
       kept by name (`registry.results`) and said under the block by this file's component (`DefLine`,
       `[data-surface=component-def]`), in the editor too.
     - **A tag** (`<x-name …>` — the sanitizer's `m-el data-tag` — or an `m-*` the house draws no card
       for) is looked up (`drawTags`): `ui.components` resolves it, the live bundle first, then the newest
       authored. The bundle's (ours, and a `*`-editor's: it went into the bundle) is mounted natively, a
       bound mark the engine fills (FRAME-29); anyone else's in a frame (`bus.frame`, COMPS-2): an rhx one
       from its kept version, an html one as a page. Its attributes are its props (`data-prop-*`, the
       sanitizer's), its children its `body`. A name nothing holds keeps its words and says "no component
       named x" on hover. The table follows `ui.watch`: a namespace's write (`component`) and the bundle's
       move (resolved again), so every ticket using a definition redraws with its new version. -->
<script module>
  export const uses = ["ui.components", "ui.watch", "ui.history", "ui.author"];

  // ── definitions ────────────────────────────────────────────────────────
  const FENCE = /^ {0,3}(`{3,}|~{3,})(.*)$/;
  const DEF = /^\s*component\s+([a-z][a-z0-9]*(?:-[a-z0-9]+)+)(?:\s+(html))?\s*$/;

  /** A fence's info string as a definition's head: `{ name, html }`, or null. */
  export function defHead(info) {
    const m = DEF.exec(String(info ?? ""));
    return m ? { name: m[1], html: m[2] === "html" } : null;
  }

  /** The definitions in `body`, in order: `{ name, html, source }` (the fence's lines, a newline after). */
  export function definitions(body) {
    const lines = String(body ?? "").split("\n");
    const out = [];
    for (let i = 0; i < lines.length; i++) {
      const open = FENCE.exec(lines[i]);
      if (!open) continue;
      const ch = open[1][0], len = open[1].length;
      let k = i + 1;
      for (; k < lines.length; k++) {
        const c = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(lines[k]);
        if (c && c[1][0] === ch && c[1].length >= len) break;
      }
      const head = defHead(open[2]);
      if (head) out.push({ ...head, source: lines.slice(i + 1, k).join("\n") + "\n" });
      i = k;
    }
    return out;
  }

  /** The definitions of `after` that `before` does not hold as they are (new, or their text changed). */
  export function changed(before, after) {
    const was = new Map(definitions(before).map((d) => [d.name, d]));
    return definitions(after).filter((d) => was.get(d.name)?.source !== d.source || was.get(d.name)?.html !== d.html);
  }

  // ── the table: what a name resolves to, and what each save answered ─────
  const listeners = new Set();
  const say = () => { for (const f of [...listeners]) f(); };
  /** `table`: name → `ui.components`' row; `results`: name → the last save's answer on this page
   *  (`{ state: saving | authored | refused, source, version, own, reason, message, line }`). */
  export const registry = { table: new Map(), results: new Map(), ready: false, wanted: false, bus: null };
  export function listen(fn) { listeners.add(fn); return () => listeners.delete(fn); }

  /** Asked for by a tag or a definition on screen: the table (this file's component) reads only then. */
  export function want() {
    if (registry.wanted) return;
    registry.wanted = true;
    say();
  }
  /** The table's rows, from this file's component: `ui.components` whole (its answer, or its read again
   *  after the bundle moved), then each namespace write the watch says. */
  export function took(rows, whole) {
    if (whole) registry.table = new Map();
    for (const row of rows) if (row?.name) registry.table.set(row.name, row);
    registry.ready = true;
    say();
  }

  /** What `name` draws as now: `{ mode: native | framed | unknown | pending, row }`. */
  export function lookup(name) {
    const row = registry.table.get(name);
    if (row) return { mode: row.native ? "native" : "framed", row };
    return { mode: registry.ready ? "unknown" : "pending", row: null };
  }

  // ── on save ────────────────────────────────────────────────────────────
  const first = (r) => (r?.events ?? []).find((e) => e && (e.type === "authored" || e.type === "refused" || e.type === "error")) ?? null;

  /** Each definition `after` changed from `before`, `ui.author`ed in turn (one at a time: a live author
   *  moves the pointer the next is built on); answered by name in `registry.results`. */
  export async function authorChanged(b, before, after) {
    const defs = changed(before, after);
    if (!defs.length || !b) return [];
    want();
    for (const d of defs) registry.results.set(d.name, { state: "saving", source: d.source });
    say();
    const out = [];
    for (const d of defs) {
      const src = { name: d.name, source: d.source, ...(d.html ? { profile: "html/1" } : {}) };
      let e = null;
      try {
        if (!d.html) e = first(await b.call("ui.author", { sources: [src] }));
        // Not held on `*`: the saver's own namespace (an html one goes there always).
        if (d.html || (e?.type === "refused" && e.reason === "forbidden")) e = first(await b.call("ui.author", { sources: [src], own: true }));
      } catch (err) {
        e = { type: "error", message: String(err?.message ?? err) };
      }
      const res = e?.type === "authored"
        ? { state: "authored", source: d.source, version: e.version, own: !!e.own }
        : { state: "refused", source: d.source, reason: e?.reason ?? "error", message: e?.message ?? "no answer", line: e?.line ?? null };
      registry.results.set(d.name, res);
      out.push({ name: d.name, ...res });
      say();
    }
    return out;
  }

  // ── tags ───────────────────────────────────────────────────────────────
  /** The house's marks ticket-view draws as cards or inline (never looked up). */
  let houseMarks = new Set();
  export function houseOwns(set) { houseMarks = new Set(set); }

  /** The section's own names and the engine's (FRAME-29's `MARK_NAMES`): never a prop through a mark. */
  const MARK_NAMES = new Set(["slot", "component", "version", "layout", "failed", "css", "refused"]);
  /** A tag's props: its attributes (`data-prop-x` → `x`, `data-id` → `id`, another `data-x` → `x`, the
   *  house's words as they are), and its children's words as `body` unless an attribute says one. */
  function propsOf(e, inner) {
    const props = {};
    for (const a of e.attributes) {
      const n = a.name;
      if (n === "data-tag" || n === "data-caption" || n === "class" || n === "title" && e.dataset.unknown != null) continue;
      const k = n.startsWith("data-prop-") ? n.slice(10) : n.startsWith("data-") ? n.slice(5) : n;
      if (k && !(k in props)) props[k] = a.value;
    }
    if (!("body" in props) && inner.trim()) props.body = inner;
    return props;
  }
  const esc = (t) => String(t).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  /** An html component's frame document: its props as `window.props` (and `plexus.props`), its source. */
  function htmlDoc(name, source, props) {
    const json = JSON.stringify(props).replace(/</g, "\\u003c");
    return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${esc(name)}</title>`
      + `<script>window.props = ${json};<\/script></head><body>\n${source}\n</body></html>`;
  }
  let slotSeq = 0;

  /** Every tag under `node` drawn by its component (framed through the page's bus, the one this file's
   *  component holds), kept so as the table moves; `{ destroy }`. */
  export function drawTags(node) {
    const found = [];
    for (const e of node.querySelectorAll("*")) {
      const tag = e.localName === "m-el" ? e.getAttribute("data-tag") : e.localName;
      if (!tag || !tag.includes("-") || houseMarks.has(tag) || e.closest("[data-tag-host]")) continue;
      if (e.parentElement?.closest("m-el, [data-tag-name]") && found.some((f) => f.e.contains(e))) continue; // a tag inside a drawn tag is its body
      found.push({ e, tag, inner: e.textContent ?? "", drawn: null });
    }
    if (!found.length) return { destroy() {} };
    for (const f of found) f.e.setAttribute("data-tag-name", f.tag);
    want();
    const clear = (f) => {
      if (!f.drawn) return;
      f.drawn.frame?.destroy();
      f.drawn.host.remove();
      f.drawn = null;
      f.e.removeAttribute("data-tag-drawn");
    };
    const draw = () => {
      for (const f of found) {
        const { mode, row } = lookup(f.tag);
        const want = row ? `${mode}:${row.version}:${row.author ?? ""}` : mode;
        if (f.drawn?.want === want) continue;
        clear(f);
        f.e.removeAttribute("data-unknown");
        if (mode === "pending") continue;
        if (mode === "unknown") {
          f.e.setAttribute("data-unknown", "");
          f.e.setAttribute("title", `no component named ${f.tag}`);
          continue;
        }
        if (f.e.getAttribute("title") === `no component named ${f.tag}`) f.e.removeAttribute("title");
        const props = propsOf(f.e, f.inner);
        const host = document.createElement("div");
        host.setAttribute("data-tag-host", "");
        host.setAttribute("data-surface", "tag-component");
        host.setAttribute("data-name", f.tag);
        host.setAttribute("data-mode", mode);
        let frame = null;
        if (mode === "native") {
          // Ours: a bound mark the engine fills with the live bundle's component, its props as data-*.
          const mark = document.createElement("div");
          mark.setAttribute("data-slot", `tag-${++slotSeq}`);
          mark.setAttribute("data-component", f.tag);
          for (const [k, v] of Object.entries(props)) {
            const n = k.toLowerCase();
            if (!MARK_NAMES.has(n) && /^[a-z][a-z0-9-]*$/.test(n)) mark.setAttribute(`data-${n}`, typeof v === "string" ? v : JSON.stringify(v));
          }
          host.appendChild(mark);
        } else {
          // Someone else's: framed — never in the page — its calls the viewer's, held to its uses.
          host.style.position = "relative";
          host.setAttribute("data-author", row.by ?? row.author ?? "");
          const opts = row.profile === "html/1"
            ? { html: htmlDoc(f.tag, row.source ?? "", props), theme: true, title: f.tag }
            : { component: { version: row.version, name: f.tag }, props, theme: true, title: f.tag };
          try {
            if (!registry.bus) throw new Error("no bus on this page to frame it with");
            frame = registry.bus.frame(host, opts);
          } catch (err) {
            host.textContent = `${f.tag}: ${err?.message ?? err}`;
          }
        }
        f.e.replaceChildren(host);
        f.e.setAttribute("data-tag-drawn", mode);
        f.e.classList.remove("any", "any-inline");
        f.e.removeAttribute("data-caption");
        f.drawn = { want, host, frame };
      }
    };
    draw();
    const off = listen(draw);
    return { destroy() { off(); for (const f of found) clear(f); } };
  }

  /** Each ```component fence's code under `node` (in the order of `src`'s definitions) given its line. */
  export function defLines(node, src, mountLine) {
    const defs = definitions(src);
    const codes = [...node.querySelectorAll("pre > code.language-component")];
    const placed = [];
    codes.forEach((code, k) => {
      const d = defs[k];
      if (!d) return;
      const at = document.createElement("div");
      at.className = "def-line";
      code.parentElement.after(at);
      placed.push({ at, app: mountLine(at, d) });
    });
    return placed;
  }
</script>

<script>
  // The table: what every name resolves to (`ui.components`), read again when the bundle moves (the
  // history's top move) and kept by each namespace write (`component` on `ui.watch`) — asked only once a
  // tag or a definition is on screen (`want`). Placed once by ticket-view; no markup of its own.
  import { onDestroy } from "svelte";
  import { bus, live } from "plexus-ui/host";
  const b = bus();
  registry.bus ??= b;
  let wanted = $state(registry.wanted);
  onDestroy(listen(() => (wanted = registry.wanted)));
  const all = live({ snapshot: "ui.components", params: {}, key: "name", when: () => wanted });
  const writes = live({ snapshot: "ui.components", params: { names: ["x-none"] }, changes: "ui.watch", key: "name", on: { component: "upsert:." }, when: () => wanted });
  const moves = live({ snapshot: "ui.history", params: { limit: 1 }, changes: "ui.watch", key: "seq", on: { live: "upsert:." }, when: () => wanted });
  let top = null;
  $effect(() => {
    const rows = all.rows, status = all.status;
    if (status === "live") took(rows, true);
  });
  // Each write once (a row kept from an earlier write never undoes a read since).
  const heard = new Map();
  $effect(() => {
    const fresh = writes.rows.filter((r) => r?.name && heard.get(r.name) !== `${r.version}:${r.at}`);
    for (const r of fresh) heard.set(r.name, `${r.version}:${r.at}`);
    if (fresh.length) took(fresh, false);
  });
  $effect(() => {
    const seq = Math.max(0, ...moves.rows.map((r) => Number(r.seq) || 0));
    if (top !== null && seq > top) b.call("ui.components", {}).then((r) => r?.ok && took((r.events ?? []).filter((e) => e.type === "component"), true));
    top = Math.max(top ?? 0, seq);
  });
</script>
