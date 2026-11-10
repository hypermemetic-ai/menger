<!-- EDITOR-20: a ticket id written in a body is a pill that links to the ticket. The owner, 2026-11-10: "we
     need the ticket surface to parse tickets for shortcodes and then give me links to them (rendered nicely
     in a pill)".
     - **Where an id is found**: in the text the host's `markdown()` drew (after its sanitizer), text node by
       text node — never in code (a span, a fence, a code card), a link's text or target, a URL, a diagram,
       a drawn tag, a frame or a house mark's attribute. An id is a shortcode and a number (`KEEL-41`), a
       subtask (`KILN-2.1`), or an address (`PLEXUS/KEEL-41`). `<m-ticket ref="ID">` is the same pill
       (`full` — kept by the sanitizer as `data-prop-full` — adds the title).
     - **A pill is made only for a ticket the reader may see**: every id on the page is looked up in one
       batched `tickets.list` (`ticket A or ticket B …`, at most 64 a filter: the store's cap), asked a tick
       after the bodies drawn in it named them — a table of sixty, or sixty blocks of the editor, is one call.
       An id the list does not answer (no such ticket, one out of the reader's reach, `UTF-8`) stays its
       plain text, and nothing says it was asked; a filter the service refuses is halved until the id it
       stumbles on is alone, which then stays plain.
     - **Who draws them**: ticket-view stays a drawing from props alone (SURFACE-16: no call of menger's, it
       sits in any bundle), so it does not import this file. A component that draws a body and may read
       tickets provides the pass — `providePills()` while it initialises (a context, `PILLS`, which ticket-view
       reads and the editor hands to each block it mounts) and `<PillTable/>` placed once: ticket-body,
       editor-body (a ticket, a doc, its history), doc-margin, ticket-revisions, ticket-edit, ticket-create.
     - **A pill is live**: this file's component (no markup), placed by each provider,
       holds one `tickets.watch` for the page — one spec, shared by every body drawn on it, never one a pill
       — and a status or a title it hears moves every pill of that ticket. A delete is heard only for a
       ticket the watch has said something of on this page (its fold removes rows it holds, and the lookup's
       rows are not in it): another ticket deleted keeps its pill until the body is drawn again.
     - **It is drawn as an element made here, never as HTML**: an `<a>` whose href is the one link helper's
       (`ticketHref`), its text the id as written, its stage the pill's colour (the `--st-*` tokens) and its
       title on hover. Its look is the sheet's that draws the body (ticket-view's `.md`, doc-margin's): a
       component's sheet styles only its own (FRAME-30). The stored body is never touched. -->
<script module>
  import { setContext } from "svelte";
  import { ticketHref } from "./ticket-links.svelte";

  export const uses = ["tickets.list", "tickets.watch"];

  /** The most `ticket` predicates one filter may hold (the store's `UNDERS_MAX`). */
  export const BATCH = 64;

  // An id: an optional org (`hypermemetic:`), any shortcodes of its address (`PLEXUS/`), the shortcode (a
  // namesake's `PERF.115` too), a number, and a subtask's `.N`s. Not after a letter, a digit or anything a
  // URL, a path or an address joins words with; not before them either (a sentence's `.` is the end).
  const ID = /(?<![\p{L}\p{N}_/.:@#=&?%+~$\\-])((?:[a-z][a-z0-9-]*:)?(?:[A-Z][A-Z0-9]*\/)*[A-Z][A-Z0-9]*(?:\.[0-9]+)?-[0-9]+(?:\.[0-9]+)*)(?![\p{L}\p{N}_/@\\-]|\.[\p{L}\p{N}])/gu;
  // A URL in the text, whose words are never ids.
  const URL_RUN = /\b(?:https?|ftp|mailto|file):\S+/gi;
  /** What a text node's ids are never looked for under. */
  const SKIP = "a, code, pre, kbd, samp, script, style, textarea, button, input, select, svg, .code, .code-card, .diagram, .def-line, .mx, [data-media-live], [data-tag-host], [data-tag-name], m-el, m-code, m-commit, m-ticket, m-mockup, m-live, m-page, .tpill";

  /** The ids written in `text`, in order: `{ at, end, id }` — none inside a URL. */
  export function idsIn(text) {
    const s = String(text ?? "");
    const urls = [...s.matchAll(URL_RUN)].map((m) => [m.index, m.index + m[0].length]);
    const out = [];
    for (const m of s.matchAll(ID)) {
      const at = m.index, end = at + m[0].length;
      if (urls.some(([a, z]) => at < z && end > a)) continue;
      out.push({ at, end, id: m[1] });
    }
    return out;
  }

  /** An id as it is asked and kept: the address's case kept, the org (`x:`) dropped. */
  const askOf = (id) => String(id).replace(/^[a-z][a-z0-9-]*:/, "");

  // ── the table: what each id names, for this reader ──────────────────────
  /** `table`: ask → the ticket's row (`{ id, uid, title, status }`), or null for none the reader may see. */
  export const pills = { table: new Map(), buses: [] };
  const asking = new Set(), pending = new Set();
  let timer = null;
  const listeners = new Set();
  let saying = false;
  // Told on the next microtask, never inside the effect or the fold that changed the table.
  const say = () => {
    if (saying) return;
    saying = true;
    queueMicrotask(() => { saying = false; for (const f of [...listeners]) f(); });
  };
  const listen = (fn) => { listeners.add(fn); return () => listeners.delete(fn); };

  /** `ids` wanted: each not yet known asked, all of them a tick from now, together. */
  export function want(ids) {
    for (const raw of ids) {
      const id = askOf(raw);
      if (!pills.table.has(id) && !asking.has(id)) pending.add(id);
    }
    if (pending.size && !timer) { timer = setTimeout(flush, 0); say(); }
  }
  /** Whether anything was asked on this page (the watch is held from then on). */
  export const anyAsked = () => pills.table.size > 0 || pending.size > 0 || asking.size > 0;

  /** Whether `row` is the ticket `ask` names: its id, or an address whose shortcodes are in its address, in order. */
  function names(ask, row) {
    const parts = ask.toUpperCase().split("/");
    const last = parts.pop();
    if (String(row.id).toUpperCase() !== last) return false;
    if (!parts.length) return true;
    const path = String(row.address ?? "").replace(/^[^:]*:/, "").toUpperCase().split("/");
    let k = 0;
    for (const seg of path.slice(0, -1)) if (seg === parts[k]) k++;
    return k === parts.length;
  }

  /** The rows a list's answer carries (a `rows` page, or one event a row). */
  const rowsOf = (r) => (r?.events ?? []).flatMap((e) => (e?.type === "rows" ? e.items ?? [] : [e])).map((e) => e?.summary ?? e?.ticket ?? e).filter((x) => x && typeof x.id === "string" && x.title != null);

  const FIELDS = ["id", "uid", "title", "status", "address"];
  /** One filter's worth of asks: answered, or — refused — halved until the one it stumbles on stands alone. */
  async function look(b, asks) {
    const filter = `(${asks.map((a) => `ticket ${a}`).join(" or ")}) and (hidden or not hidden) and (archived or not archived)`;
    const r = await b.call("tickets.list", { filter, summary: true, fields: FIELDS, limit: 1000 });
    const refused = !r.ok || (r.events ?? []).some((e) => e?.type === "error" || e?.type === "refused");
    if (refused && asks.length > 1) {
      const half = Math.ceil(asks.length / 2);
      await Promise.all([look(b, asks.slice(0, half)), look(b, asks.slice(half))]);
      return;
    }
    const rows = refused ? [] : rowsOf(r);
    for (const a of asks) {
      const hits = rows.filter((row) => names(a, row));
      pills.table.set(a, hits.length === 1 ? pick(hits[0]) : null);
      asking.delete(a);
    }
    say();
  }
  const pick = (row) => ({ id: row.id, uid: row.uid ?? null, title: row.title ?? "", status: row.status ?? "" });

  async function flush() {
    timer = null;
    const b = pills.buses[pills.buses.length - 1];
    if (!b || !pending.size) return; // asked again when a table mounts
    const asks = [...pending];
    pending.clear();
    for (const a of asks) asking.add(a);
    const parts = [];
    for (let i = 0; i < asks.length; i += BATCH) parts.push(asks.slice(i, i + BATCH));
    await Promise.all(parts.map((p) => look(b, p).catch(() => { for (const a of p) { asking.delete(a); pills.table.set(a, null); } say(); })));
  }

  /** What the watch says, kept: a ticket's row moved (`rows`), or tickets gone (`removed`, their ids). */
  export function heard(rows, removed = []) {
    let moved = false;
    const gone = new Set(removed.map((x) => String(x)));
    if (gone.size) for (const [a, v] of pills.table) if (v && gone.has(v.id)) { pills.table.set(a, null); moved = true; }
    for (const row of rows ?? []) {
      if (!row || typeof row.id !== "string" || row.title == null) continue;
      for (const [a, v] of pills.table) {
        if (!v || v.id !== row.id) continue;
        const now = pick({ ...v, ...row });
        if (now.title !== v.title || now.status !== v.status || now.uid !== v.uid) { pills.table.set(a, now); moved = true; }
      }
    }
    if (moved) say();
  }

  // ── the drawing ──────────────────────────────────────────────────────────
  const STATUS = (s) => String(s ?? "").toLowerCase().replace(/\s+/g, "-");
  /** A pill's attributes as `row` says them now. */
  function dress(a, row, full) {
    a.setAttribute("href", ticketHref(row));
    a.setAttribute("data-status", row.status);
    const st = STATUS(row.status);
    a.setAttribute("data-stage", st);
    // Its stage's colour, the status tokens', handed to the sheet that draws it (ticket-view's, doc-margin's)
    // as two properties — set through the CSSOM, which the page policy allows (no style attribute is written).
    a.style.setProperty("--pill-bg", `var(--st-${st}-bg)`);
    a.style.setProperty("--pill-fg", `var(--st-${st}-fg)`);
    a.setAttribute("title", `${row.id} · ${row.status} — ${row.title}`);
    if (full) {
      let t = a.querySelector(".tpill-title");
      if (!t) { t = document.createElement("span"); t.className = "tpill-title"; a.append(" ", t); }
      if (t.textContent !== row.title) t.textContent = row.title;
    }
  }
  /** A pill for `text` (the id as written), naming the ticket `ask`. */
  function pill(text, ask, row, full = false) {
    const a = document.createElement("a");
    a.className = "tpill";
    a.setAttribute("data-pill", ask);
    a.textContent = text;
    dress(a, row, full);
    return a;
  }

  /** Every id in `node`'s drawn text a pill, once the table knows the reader may see its ticket; kept so as
   *  the table moves (a status, a title, a delete). `{ destroy }`. A Svelte action. */
  export function drawPills(node) {
    const draw = () => {
      const wanted = [];
      // The pills already drawn: dressed again, or back to their words when the ticket went.
      for (const a of node.querySelectorAll("a.tpill[data-pill]")) {
        const row = pills.table.get(a.getAttribute("data-pill"));
        const mark = a.parentElement?.localName === "m-ticket" ? a.parentElement : null;
        if (row) dress(a, row, mark ? full(mark) : false);
        else if (row === null) { if (mark) { mark.removeAttribute("data-pilled"); a.remove(); } else a.replaceWith(document.createTextNode(a.textContent)); }
      }
      // `<m-ticket ref>`: the same pill inside the mark, which then shows no `ref` of its own.
      for (const m of node.querySelectorAll("m-ticket[ref]")) {
        if (m.querySelector(":scope > a.tpill")) continue;
        const ref = (m.getAttribute("ref") ?? "").trim();
        if (!ref) continue;
        const ask = askOf(ref), row = pills.table.get(ask);
        if (row) { m.setAttribute("data-pilled", ""); m.prepend(pill(ref, ask, row, full(m))); }
        else if (row === undefined) wanted.push(ask);
      }
      // The text: each id a pill, the rest of its words as they were.
      const walk = document.createTreeWalker(node, NodeFilter.SHOW_TEXT, {
        // Only what is inside the drawn body decides (the editor's own lines around it are editable).
        acceptNode: (n) => { const no = n.parentElement?.closest?.(SKIP); return !/[A-Z]/.test(n.data) || (no && node.contains(no)) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT; },
      });
      const texts = [];
      for (let n = walk.nextNode(); n; n = walk.nextNode()) texts.push(n);
      for (const n of texts) {
        const found = idsIn(n.data);
        if (!found.length) continue;
        let at = 0, made = null;
        for (const f of found) {
          const ask = askOf(f.id), row = pills.table.get(ask);
          if (row === undefined) wanted.push(ask);
          if (!row) continue;
          made ??= document.createDocumentFragment();
          if (f.at > at) made.append(n.data.slice(at, f.at));
          made.append(pill(f.id, ask, row));
          at = f.end;
        }
        if (!made) continue;
        if (at < n.data.length) made.append(n.data.slice(at));
        n.replaceWith(made);
      }
      if (wanted.length) want(wanted);
    };
    draw();
    const off = listen(draw);
    return { update: draw, destroy: off };
  }
  const full = (m) => m.hasAttribute("full") || m.hasAttribute("data-prop-full") || m.hasAttribute("data-full");

  /** The context ticket-view reads its pill pass from (the key spelled there too: it imports nothing of this file). */
  export const PILLS = "menger:ticket-pills";
  /** This component's descendants draw ticket ids as pills: call while it initialises, and place `<PillTable/>`. */
  export function providePills() { setContext(PILLS, drawPills); }
</script>

<script>
  // The page's one watch for its pills: a constant spec (the host shares it between every body drawn on the
  // page), held only once an id was asked; what it hears moves the table. No markup.
  import { onDestroy } from "svelte";
  import { bus, live } from "plexus-ui/host";
  const b = bus();
  pills.buses.push(b);
  if (pending.size && !timer) timer = setTimeout(flush, 0);
  let asked = $state(anyAsked());
  const offAsked = listen(() => { if (!asked && anyAsked()) asked = true; });
  const seen = live({ snapshot: "tickets.list", params: { filter: "ticket 00000000", summary: true, limit: 1 }, changes: "tickets.watch", key: "id",
                      on: { status_changed: "upsert:ticket", amended: "upsert:ticket", updated: "upsert:ticket", revision_restored: "upsert:ticket",
                            deleted: "remove:ids" },
                      when: () => asked });
  const off = seen.subscribe((_v, change) => { if (change?.upsert?.length || change?.remove?.length) heard(change.upsert, change.remove ?? []); });
  onDestroy(() => {
    off();
    offAsked();
    const i = pills.buses.lastIndexOf(b);
    if (i >= 0) pills.buses.splice(i, 1);
  });
</script>

