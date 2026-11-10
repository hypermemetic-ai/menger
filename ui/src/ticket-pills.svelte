<!-- EDITOR-20: a ticket id written in a body is a pill that links to the ticket. The owner, 2026-11-10: "we
     need the ticket surface to parse tickets for shortcodes and then give me links to them (rendered nicely
     in a pill)".
     - **Where an id is found**: in the text the host's `markdown()` drew (after its sanitizer), text node by
       text node — never in code (a span, a fence, a code card), a link's text or target, a URL, a diagram,
       a drawn tag, a frame or a house mark's attribute. An id is a shortcode and a number (`KEEL-41`), a
       subtask (`KILN-2.1`), or an address (`PLEXUS/KEEL-41`). `<m-ticket ref="ID">` is the same pill
       (`full` — kept by the sanitizer as `data-prop-full` — adds the title).
     - **A pill is made only for a ticket the reader may see**: every id on the page is read in one batched
       `tickets.list` (`ticket A or ticket B …`, at most 64 a filter: the store's cap) — a `live` collection
       whose params follow the ids asked, the bus's `debounce_ms` gathering the ids named together (EDITOR-21,
       FRAME-25: no timer of ours) — so a table of sixty, or sixty blocks of the editor, is one read. An id
       the list does not answer (no such ticket, one out of the reader's reach, `UTF-8`) stays its plain
       text, and nothing says it was asked. Up to four collections: 256 ids a page.
     - **Who draws them**: ticket-view stays a drawing from props alone (SURFACE-16: no call of menger's, it
       sits in any bundle), so it does not import this file. A component that draws a body and may read
       tickets provides the pass — `providePills()` while it initialises (a context, `PILLS`, which ticket-view
       reads and the editor hands to each block it mounts) and `<PillTable/>` placed once: ticket-body,
       editor-body (a ticket, a doc, its history), doc-margin, ticket-revisions, ticket-edit, ticket-create.
     - **A pill is live**: the same collection folds `tickets.watch` — one stream a collection, shared by
       every body the bundle draws, never one a pill — so a status, a title or a delete it hears moves every
       pill of that ticket (a deleted one goes back to its words).
     - **It is drawn as an element made here, never as HTML**: an `<a>` whose href is the one link helper's
       (`ticketHref`), its text the id as written, its stage the pill's colour (`data-stage`, painted with the `--st-*` tokens) and its
       title on hover. Its look is the sheet's that draws the body (ticket-view's `.md`, doc-margin's): a
       component's sheet styles only its own (FRAME-30). The stored body is never touched. -->
<script module>
  import { setContext, tick } from "svelte";
  import { ticketHref } from "./ticket-links.svelte";

  export const uses = ["tickets.list", "tickets.watch"];

  /** The most `ticket` predicates one filter may hold (the store's `UNDERS_MAX`). */
  export const BATCH = 64;
  /** How many BATCHes a page reads (BATCH × BATCHES ids; an id past them stays its words). */
  export const BATCHES = 4;
  /** How long the ids bodies name are gathered before they are read (the bus's debounce). */
  export const DEBOUNCE = 30;

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
  // FRAME-25: the bus keeps the time and the face's `live` is the store. Each id a body names joins `asked`
  // (this module's state, shared by every body this bundle draws); this file's component reads the tickets
  // of `asked` through `live` collections whose params follow it — `ticket A or ticket B …`, BATCH ids a
  // collection, the ids that came within `debounce_ms` of each other asked as one read — and folds
  // `tickets.watch` over them (a stage, a title, a delete). The table is made again from their rows on every
  // move: an id whose ticket no row is (none such, out of reach, `UTF-8`) stays its plain words.
  /** Every id asked on the page, in the order first asked (the collections' params are made of it). */
  const asked = $state([]);
  const known = new Set();
  /** `table`: ask → the ticket's row (`{ id, uid, title, status }`) while one of the collections holds it. */
  export const pills = { table: new Map() };
  const listeners = new Set();
  let saying = false;
  // Told after Svelte's flush (tick), never inside the effect that moved the table (COMPS-5's loop).
  const say = () => {
    if (saying) return;
    saying = true;
    tick().then(() => { saying = false; for (const f of [...listeners]) f(); });
  };
  const listen = (fn) => { listeners.add(fn); return () => listeners.delete(fn); };

  /** `ids` wanted: each one not asked before joins `asked`. */
  export function want(ids) {
    for (const raw of ids) {
      const id = askOf(raw);
      if (known.has(id)) continue;
      known.add(id);
      asked.push(id);
    }
  }

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

  const FIELDS = ["id", "uid", "title", "status", "address"];
  /** Collection `k`'s params: its BATCH of `asked` as one filter (a ticket shown or hidden, archived or not). */
  export function batchParams(k) {
    const part = asked.slice(k * BATCH, (k + 1) * BATCH);
    const filter = part.length ? `(${part.map((a) => `ticket ${a}`).join(" or ")}) and (hidden or not hidden) and (archived or not archived)` : "ticket 00000000";
    return { filter, summary: true, fields: FIELDS, limit: 1000 };
  }
  export const batchWanted = (k) => asked.length > k * BATCH;
  const pick = (row) => ({ id: row.id, uid: row.uid ?? null, title: row.title ?? "", status: row.status ?? "" });
  const same = (x, y) => x.id === y.id && x.uid === y.uid && x.title === y.title && x.status === y.status;

  /** The table made again from the collections' rows (the snapshot's and what the watch folded since). */
  export function took(rowLists) {
    const byId = new Map();
    for (const rows of rowLists) for (const r of rows ?? []) if (r && typeof r.id === "string" && r.title != null) {
      const k = r.id.toUpperCase();
      byId.set(k, [...(byId.get(k) ?? []), r]);
    }
    const now = new Map();
    for (const a of known) {
      const hits = (byId.get(a.split("/").pop().toUpperCase()) ?? []).filter((r) => names(a, r));
      if (hits.length === 1) now.set(a, pick(hits[0]));
    }
    const moved = now.size !== pills.table.size || [...now].some(([a, v]) => !pills.table.has(a) || !same(pills.table.get(a), v));
    if (!moved) return;
    pills.table = now;
    say();
  }

  // ── the drawing ──────────────────────────────────────────────────────────
  const STATUS = (s) => String(s ?? "").toLowerCase().replace(/\s+/g, "-");
  /** A pill's attributes as `row` says them now. */
  function dress(a, row, full) {
    a.setAttribute("href", ticketHref(row));
    a.setAttribute("data-status", row.status);
    a.setAttribute("data-stage", STATUS(row.status));
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
        else { if (mark) { mark.removeAttribute("data-pilled"); a.remove(); } else a.replaceWith(document.createTextNode(a.textContent)); }
      }
      // `<m-ticket ref>`: the same pill inside the mark, which then shows no `ref` of its own.
      for (const m of node.querySelectorAll("m-ticket[ref]")) {
        if (m.querySelector(":scope > a.tpill")) continue;
        const ref = (m.getAttribute("ref") ?? "").trim();
        if (!ref) continue;
        const ask = askOf(ref), row = pills.table.get(ask);
        if (row) { m.setAttribute("data-pilled", ""); m.prepend(pill(ref, ask, row, full(m))); }
        else wanted.push(ask);
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
          if (!row) { wanted.push(ask); continue; }
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
  // The page's reads of its pills: BATCHES collections (BATCH ids each), their params following `asked`, each
  // held only once it has ids, the ids named within DEBOUNCE ms of each other one read; their changes the one
  // `tickets.watch` a collection holds. No markup. Every copy of this component in a bundle asks the same
  // specs, which the host shares.
  import { untrack } from "svelte";
  import { live } from "plexus-ui/host";
  const on = { status_changed: "upsert:ticket", amended: "upsert:ticket", updated: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "remove:ids" };
  const batches = Array.from({ length: BATCHES }, (_, k) => live({ snapshot: "tickets.list", params: () => batchParams(k), changes: "tickets.watch", key: "id", on, when: () => batchWanted(k), debounce_ms: DEBOUNCE }));
  $effect(() => {
    const lists = batches.map((c) => c.rows);
    untrack(() => took(lists));
  });
</script>
