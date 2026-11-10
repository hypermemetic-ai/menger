<!-- SURFACE-16: a ticket's rendering, its input the ticket itself — `body`,
     optional `title` and `status`, drawn from props: no `live`, no call,
     `uses` empty, the markdown through the host's render (the engine's
     sanitizer), the look on the tokens. It sits in any bundle with nothing
     else of menger. A parent holding the ticket imports it as a sibling,
     `./ticket-view.svelte`: its code inlined, no section per instance
     (perf/notes.md "SURFACE-16" has what a bound mark costs instead).
     V2-24: a body names a kept image `![alt](media:<hash>)` (the media
     service's `media.put` answers the hash); the page draws it from the
     shell's `/media/<hash>`, behind the session, so the link is rewritten
     to that path on this origin before the render — the sanitizer keeps an
     image's `src` only when it is http(s), and the page policy (`img-src
     'self'`) admits the origin's own.
     EDITOR-8: one surface. The body is drawn block by block — `blocks()`
     below splits it where a blank line ends a block, keeping a fence, a
     house mark from its open line to its close (`<m-summary>` … `</m-summary>`),
     a comment and a list's continuation whole — each block through
     one `{@html markdown(…)}` (the body's references and media links with it). The editor (editor-body) draws the same blocks with the
     same component: each drawn block there is this one mounted with `block`
     (the block's text as `body`, the body's references as `defs`), drawing
     that block alone through the same `{@html markdown(…)}`, so a block the
     caret is not in is drawn there exactly as here; the caret's block shows
     its markdown. `edit` (an action given the body's element) and
     `titleEdit` (one given the title's) are how the editor sits inside. The house's marks (`m-*`, kept by
     the sanitizer since EDITOR-8) are drawn as cards by the rules below,
     one card per kind, the tokens' colours.
     EDITOR-14: a full page — `<m-mockup>…</m-mockup>`, a ```mockup / ```page /
     ```live fence, ```html with a page's attributes, `<m-live>`, or a body that
     is a whole `<!doctype html>` page — is one piece, whole (`page: true`),
     drawn by its tag's component (COMPS-3: `TAGS` — m-mockup, m-live, m-page) rather than through
     the markdown; in the editor it is one widget, its source shown when the
     caret enters it. And a body holding raw HTML the page cannot draw (m-mockup's
     `holdsRaw`, outside fences and page blocks — v1's `is_framed`; the owner: "also
     you know that v1 could just host raw html inside a frame right?") is one piece,
     `raw: true`, the whole body drawn in one frame by m-page (its markdown
     around the raw HTML, a page block in it a nested sandboxed frame).
     EDITOR-20: every block's drawn text is given its ticket ids as pills, after the markdown and its
     sanitizer, by the pass ticket-pills provides through a context (`providePills`) — so the ticket page,
     the editor's drawn blocks, a document, a revision and a preview draw them, and this file still calls
     nothing; a framed page does not. -->
<script module>
  import { PAGE_TAG, WHOLE, fencePage, pageEnd, pageSpec, holdsRaw } from "./m-mockup.svelte";
  import { openName, closeLine, tagSpans } from "./tag-source.svelte";

  export const uses = [];

  // V2-24: `](media:<64 hex>)` → `](<origin>/media/<hash>)`, the hash lower-cased.
  const MEDIA = /\]\(\s*media:([0-9a-fA-F]{64})\s*\)/g;
  const origin = () => (typeof location === "object" && location?.origin) || "";
  export const mediaLinks = (md) => String(md ?? "").replace(MEDIA, (_, hash) => `](${origin()}/media/${hash.toLowerCase()})`);

  const FENCE = /^ {0,3}(`{3,}|~{3,})/;
  // EDITOR-22: a tag's attributes, a quoted value holding a `>` too (`summary="a -> b"`).
  const ATTRS = `(?:\\s(?:[^>"']|"[^"]*"|'[^']*')*)?`;
  const HOUSE_OPEN = new RegExp(`^<(m-[a-z0-9-]+)${ATTRS}>\\s*$`);
  const LIST = /^ {0,3}(?:[-*+]|\d{1,9}[.)])(?:\s|$)/;
  const REF = /^ {0,3}\[[^\]]+\]:\s*\S/;
  const ELEMENT_LINE = new RegExp(`^\\s*<(m-[a-z0-9-]+)${ATTRS}>.*<\\/\\1>\\s*$`);
  /** EDITOR-22: a piece that is one `<m-details>` whole — its open tag, what it holds, its close. */
  const DETAILS = new RegExp(`^(<m-details${ATTRS}>)([^]*)(<\\/m-details>)\\s*$`);
  const blank = (l) => l.trim() === "";
  /** COMPS-16: the line after the close of the block-form tag opened on line `i` (tag-source), if it closes before `hi`; else -1. */
  function srcEnd(lines, i, hi) {
    const name = openName(lines[i]);
    if (!name) return -1;
    const z = closeLine(lines, i, name);
    return z > 0 && z <= hi ? z : -1;
  }

  /** The line after the fence opened at `i` closes (or the end). */
  function fenceEnd(lines, i) {
    const m = FENCE.exec(lines[i]);
    const ch = m[1][0], len = m[1].length;
    for (let k = i + 1; k < lines.length; k++) {
      const c = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(lines[k]);
      if (c && c[1][0] === ch && c[1].length >= len) return k + 1;
    }
    return lines.length;
  }
  /** The line after the close of the house mark opened at `i`, or -1 when it never closes. */
  function houseEnd(lines, i, tag) {
    const open = new RegExp(`^<${tag}${ATTRS}>\\s*$`), close = new RegExp(`^</${tag}>\\s*$`);
    let depth = 0;
    for (let k = i; k < lines.length; k++) {
      if (k > i && FENCE.test(lines[k])) { k = fenceEnd(lines, k) - 1; continue; }
      if (open.test(lines[k])) depth++;
      else if (close.test(lines[k]) && --depth === 0) return k + 1;
    }
    return -1;
  }

  /** `src` as its blocks: `[{ from, to, text, part }]`, offsets into `src`, `to` the end of the block's last
   *  line (the blank lines between blocks belong to none). A block is what a blank line ends, but a fence
   *  is whole, a house mark is whole from its open line to its close, a comment to its `-->`, and a line
   *  indented, or a list's next item, after blank lines joins the block before it — merging never
   *  changes what the markdown draws; splitting could. */
  export function blocks(src, raw = true) {
    const text = String(src ?? "");
    const lines = text.split("\n");
    const at = [0];
    for (const l of lines) at.push(at[at.length - 1] + l.length + 1);
    const out = [];
    // EDITOR-14: a body that is a whole page is one piece, drawn as one frame.
    if (WHOLE.test(text)) {
      const from = text.length - text.trimStart().length;
      return [{ from, to: text.length, text: text.slice(from), part: false, page: true }];
    }
    scan(lines, at, text, 0, lines.length, null, out);
    // COMPS-16: a tag's place among the body's tags of its name — tag-source's count, the one a save finds it by.
    if (out.some((p) => p.src)) {
      const spans = new Map(tagSpans(text).map((s) => [s.from, s]));
      for (const p of out) if (p.src) { const s = spans.get(p.from); if (s && s.name === p.src.name) p.src.n = s.n; }
    }
    // EDITOR-14: raw HTML anywhere (outside code and page blocks): the body is one piece, framed whole.
    if (raw && out.some((p) => !p.page && !p.src && !FENCE.test(p.text) && !/^(?: {4}|\t)/.test(p.text) && holdsRaw(p.text))) {
      const from = out[0].from;
      return [{ from, to: text.length, text: text.slice(from), part: false, page: true, raw: true }];
    }
    return out;
  }

  /** Whether lines [lo, hi) hold a page (EDITOR-14), outside fences. */
  function holdsPage(lines, lo, hi) {
    for (let k = lo; k < hi; k++) {
      if (PAGE_TAG.test(lines[k])) return true;
      if (FENCE.test(lines[k])) { if (fencePage(lines[k].replace(FENCE, ""))) return true; k = fenceEnd(lines, k) - 1; }
    }
    return false;
  }

  /** The pieces of lines [lo, hi) into `out`; `wrap` the house mark around them, when they are inside one
   *  (EDITOR-9: a house mark is its open line, its inner pieces and its close line, each its own piece —
   *  `h: "open" | "mid" | "close"`, the mark's lines as `wrap` — so the caret reveals one line of it and
   *  the rest stays drawn as one card). */
  function scan(lines, at, text, lo, hi, wrap, out) {
    const piece = (a, z, extra) => {
      while (z > a + 1 && blank(lines[z - 1])) z--;
      const t = text.slice(at[a], at[z] - 1);
      // EDITOR-22: a piece that is one `<m-details>` (not one inside another mark's piece) is a fold, `det`.
      const det = !wrap && DETAILS.test(t) ? { det: true } : {};
      out.push({ from: at[a], to: at[z] - 1, text: t, part: false, ...(wrap ? { h: "mid", wrap } : {}), ...det, ...extra });
    };
    let i = lo;
    while (i < hi) {
      if (blank(lines[i])) { i++; continue; }
      const start = i;
      let end;
      // EDITOR-14: a page element is one piece, from its open tag to its close (the page's blank lines inside it).
      if (PAGE_TAG.test(lines[i])) { const z = Math.min(pageEnd(lines, i), hi); piece(i, z, { page: true }); i = z; continue; }
      const house = HOUSE_OPEN.exec(lines[i]);
      const closes = house ? houseEnd(lines, i, house[1]) : -1;
      if (FENCE.test(lines[i])) { piece(i, Math.min(fenceEnd(lines, i), hi), fencePage(lines[i].replace(FENCE, "")) ? { page: true } : undefined); i = fenceEnd(lines, i); continue; }
      // COMPS-16: a tag in block form (its open line and its close line each alone, tag-source) is one piece,
      // whole, its blank lines too — `src`, so its component is handed its own source (`blocks()` gives its place).
      const tagEnd = srcEnd(lines, i, hi);
      if (tagEnd > 0) { piece(i, tagEnd, { src: { name: openName(lines[i]) } }); i = tagEnd; continue; }
      if (house && closes > 0 && closes <= hi) {
        const own = { open: lines[i], close: lines[closes - 1], tag: house[1] };
        // A mark inside a mark: one piece, drawn whole inside its parent — unless it holds a page (a mockup in a
        // decision's option), which is then its own piece, framed.
        if (wrap && !holdsPage(lines, i + 1, closes - 1)) { piece(i, closes); i = closes; continue; }
        // EDITOR-22: a fold is one piece, whole — its line and what it opens to drawn together, so opening it
        // is the drawing's own (in the editor, the caret's block shows all of its source) — unless it holds a
        // page, which is then split as any mark is.
        if (house[1] === "m-details" && !holdsPage(lines, i + 1, closes - 1)) { piece(i, closes); i = closes; continue; }
        out.push({ from: at[i], to: at[i + 1] - 1, text: lines[i], part: false, h: "open", wrap: own });
        scan(lines, at, text, i + 1, closes - 1, own, out);
        out.push({ from: at[closes - 1], to: at[closes] - 1, text: lines[closes - 1], part: false, h: "close", wrap: own });
        i = closes;
        continue;
      }
      if (/^ {0,3}<!--/.test(lines[i]) && !lines[i].includes("-->")) {
        end = i + 1;
        while (end < hi && !lines[end - 1].includes("-->")) end++;
        piece(i, end);
        i = end;
        continue;
      }
      end = i;
      while (end < hi && !blank(lines[end])) {
        if (end > i && FENCE.test(lines[end])) { end = Math.min(fenceEnd(lines, end), hi); continue; }
        if (end > i && srcEnd(lines, end, hi) > 0) break; // COMPS-16: a tag in block form starts its own piece
        end++;
      }
      // A continuation after blank lines: an indented line, or the next item of a list this block began.
      for (;;) {
        let k = end;
        while (k < hi && blank(lines[k])) k++;
        if (k === end || k >= hi) break;
        const indented = /^(?: {2,}|\t)\S/.test(lines[k]);
        const item = LIST.test(lines[start]) && LIST.test(lines[k]);
        if (!indented && !item) break;
        end = k;
        while (end < hi && !blank(lines[end])) {
          if (FENCE.test(lines[end].trimStart()) && end > k) { end = Math.min(fenceEnd(lines, end), hi); continue; }
          end++;
        }
      }
      // EDITOR-9: a list is drawn item by item — the smallest unit the caret reveals — each item its own
      // piece (the first `part: false`, the rest `part: true`, drawn without the gap between blocks).
      // EDITOR-9: and lines that are each one whole mark (`<m-option …></m-option>`, a stat, a step) are each
      // their own piece, the caret's line revealed and its neighbours drawn.
      const own = (l) => ELEMENT_LINE.exec(l);
      // (Stats stay one piece: a row of tiles side by side.)
      const lineByLine = end - start > 1 && lines.slice(start, end).every((l) => blank(l) || (own(l) && own(l)[1] !== "m-stat"));
      const items = lineByLine ? lines.slice(start, end).map((_, k) => [start + k, start + k + 1]).filter(([a]) => !blank(lines[a]))
        : LIST.test(lines[start]) ? listItems(lines, start, end) : [[start, end]];
      items.forEach(([a, z], n) => piece(a, z, { part: n > 0 }));
      i = end;
    }
  }

  /** The items of the list on lines [start, end): each `[from, to)` line range, an item from a line
   *  with a marker at the list's own indent to the next such line. */
  function listItems(lines, start, end) {
    const indent = /^ */.exec(lines[start])[0].length;
    const marker = new RegExp(`^ {${indent}}(?:[-*+]|\\d{1,9}[.)])(?:\\s|$)`);
    const out = [];
    let a = start;
    for (let k = start + 1; k < end; k++) {
      if (FENCE.test(lines[k])) { k = fenceEnd(lines, k) - 1; continue; }
      if (marker.test(lines[k])) { out.push([a, k]); a = k; }
    }
    out.push([a, end]);
    return out;
  }

  /** The markdown a piece is drawn from: a house mark's open or close line its empty mark (the card's
   *  top or bottom), a piece inside one wrapped in the mark's own lines (the card's middle). */
  export function pieceSource(p) {
    // EDITOR-22: a fold's inside is markdown however it was written (its open and close each a line of their own).
    if (p.det) { const m = DETAILS.exec(p.text); if (m) return `${m[1]}\n\n${m[2].trim()}\n\n${m[3]}`; }
    // COMPS-16: a tag in block form holding a blank line is drawn as its open and close lines alone — the
    // markdown would end its HTML at the blank line and tear the tag; its component is handed its source.
    const text = p.src && /\n[ \t]*\n/.test(p.text) ? `${p.text.slice(0, p.text.indexOf("\n"))}\n${p.text.slice(p.text.lastIndexOf("\n") + 1)}` : p.text;
    if (!p.wrap) return text;
    if (p.h === "open" || p.h === "close") return `${p.wrap.open}\n\n${p.wrap.close}`;
    return `${p.wrap.open}\n\n${text}\n\n${p.wrap.close}`;
  }
  /** COMPS-3: the tag a page piece (`page: true`) is drawn as — `m-page` for a body framed whole (`raw`),
   *  `m-live` for `<m-live>` and ```live, else `m-mockup` (`<m-mockup>`, the page fences, a whole page). */
  export const pageTag = (p) => (p.raw ? "m-page" : pageSpec(p.text)?.kind === "live" ? "m-live" : "m-mockup");
  /** COMPS-3: a raw body's pieces as m-page frames them — each piece's text, whether it is a page, and the
   *  markdown it draws as here (its mark's lines around it, the body's references and media links). */
  export function rawPieces(src) {
    const d = refDefs(src);
    return blocks(src, false).map((p) => ({ text: p.text, page: !!p.page, md: mediaLinks(pieceSource(p)) + d }));
  }
  /** A piece's box: `blk`, `part` (a list's next item), `h-open | h-mid | h-close` (a card's piece). */
  export const pieceClass = (p) => ["blk", p.part ? "part" : "", p.h ? `h-${p.h}` : "", p.det ? "det" : ""].filter(Boolean).join(" ");

  /** The house's marks that draw as cards, and the ones that draw inline; any other mark (and a custom
   *  tag, kept as `m-el`) draws its words in a neutral box, captioned by the attribute that reads as words. */
  const CARDS = new Set(["m-summary", "m-decision", "m-input", "m-cases", "m-beat", "m-status", "m-diagram", "m-mockup", "m-results", "m-walk", "m-code", "m-compare", "m-callout", "m-acceptance", "m-cards", "m-card", "m-dag", "m-details", "m-provides", "m-consumes"]);
  const INLINE = new Set(["m-field", "m-answer", "m-question", "m-recommendation", "m-option", "m-choice", "m-step", "m-stat", "m-ticket", "m-badge", "m-cite", "m-commit", "m-chip"]);
  const caption = (e) => e.getAttribute("label") || e.getAttribute("name") || e.getAttribute("title") || e.getAttribute("data-field") || "";

  /** The link reference definitions of `src` (`[name]: url`), outside fences: each block is drawn with them. */
  export function refDefs(src) {
    const lines = String(src ?? "").split("\n"), defs = [];
    for (let i = 0; i < lines.length; i++) {
      if (FENCE.test(lines[i])) { i = fenceEnd(lines, i) - 1; continue; }
      if (REF.test(lines[i])) defs.push(lines[i]);
    }
    return defs.length ? "\n\n" + defs.join("\n") + "\n" : "";
  }
</script>

<script>
  import { mount, unmount } from "svelte";
  import { markdown } from "plexus-ui/host";
  import Diagram from "plexus-ui:mermaid-diagram";
  import CodeCard from "./code-card.svelte";
  import MMockup from "./m-mockup.svelte";
  import MLive from "./m-live.svelte";
  import MPage from "./m-page.svelte";
  // EDITOR-22: `<m-details summary>` folded — the line its component, what it holds under it, closed or open.
  import MDetails, { foldDetails } from "./m-details.svelte";

  /** COMPS-3: a tag → the component that draws it, mounted where the body has the tag (props: `source`, the
   *  piece's markdown; `pieces`, m-page's). ticket-view keeps no frame code: a page is these components'.
   *  COMPS-4 extends this table. */
  const TAGS = { "m-mockup": MMockup, "m-live": MLive, "m-page": MPage };
  // COMPS-4: TAGS's general case — any other tag in a body drawn by the component it names (ours natively,
  // anyone else's framed), and a ```component definition's save said under its block; tag-components'
  // table (no markup) reads what names resolve to, once a tag or a definition asks.
  import TagTable, { drawTags, defLines, houseOwns } from "./tag-components.svelte";
  import DefLine from "./component-def.svelte";
  houseOwns([...CARDS, ...INLINE, ...Object.keys(TAGS)]);
  // EDITOR-20: each ticket id in the drawn text a pill, made from the text after the sanitizer, never as HTML —
  // the pass ticket-pills provides (`providePills`) to what a reader of tickets places this in; with none
  // (this file alone in a bundle, SURFACE-16: no call of menger's) the text is drawn as it is.
  import { getContext } from "svelte";
  const drawPills = getContext("menger:ticket-pills") ?? null;

  // EDITOR-9: what the markdown drew, finished. Each ```mermaid fence (a bare one, or one inside a
  // house mark) gets the diagram's bound mark before it — its own module, fetched the first time one is
  // on screen; the fence stays under it, hidden by the rules below once the diagram is drawn. And a mark
  // with no card of its own (an unknown `m-*`, a custom tag kept as `m-el`, a known inline mark used as
  // a box) is marked `any`: its words drawn in a neutral box, captioned by its label, name, title or
  // field — the tag's own name when it holds no words at all; inline among words, a caption beside them.
  // COMPS-16: the body this draws sits in — `{ kind: ticket | doc, id, editor }`, from the context ticket-body
  // and editor-body give (none elsewhere: a preview, a revision); handed to a drawn tag with its own source.
  const bodyOf = getContext("menger:body-of") ?? null;
  function enhance(node, given) {
    let placed = [];
    let tags = null;
    let pills = null;
    let key = null;
    const clear = () => { pills?.destroy(); pills = null; tags?.destroy(); tags = null; for (const p of placed) { unmount(p.app); p.at.remove(); } placed = []; };
    // The piece (`{ text, src? }`): drawn again only when its text or its tag's place moves.
    const place = (q) => {
      const k = `${q.src?.n ?? ""}\u0000${pieceSource(q)}\u0000${q.text}`;
      if (k === key) return;
      key = k;
      const text = q.text;
      const src = q.src ? { ...q.src, source: q.text } : null;
      clear();
      const top = !node.parentElement?.matches?.(".h-mid, .h-close"); // a card's caption once, on its top piece
      for (const e of node.querySelectorAll("*")) {
        const tag = e.localName;
        if (!tag.startsWith("m-") || CARDS.has(tag)) continue;
        const parent = e.parentElement;
        const alone = parent === node || parent?.localName?.startsWith("m-") || (parent?.localName === "p" && parent.textContent.trim() === e.textContent.trim());
        if (INLINE.has(tag) && parent !== node) continue; // a known inline mark keeps its look, unless it is the piece's box
        e.classList.add("any");
        if (!alone) e.classList.add("any-inline");
        const words = !top && parent === node ? "" : caption(e) || (e.textContent.trim() ? "" : (e.getAttribute("data-tag") || tag.replace(/^m-/, "")));
        if (words) e.setAttribute("data-caption", words); else e.removeAttribute("data-caption");
      }
      for (const code of node.querySelectorAll("pre > code.language-mermaid")) {
        const at = document.createElement("div");
        at.className = "diagram";
        code.parentElement.before(at);
        placed.push({ at, app: mount(Diagram, { target: at, props: { source: code.textContent } }) });
      }
      // EDITOR-12: each table's cells labelled by their column's header (`data-label`), so on a phone a row
      // stacks into a group, each cell under its header's words.
      for (const table of node.querySelectorAll("table")) {
        const heads = [...(table.querySelector("thead tr")?.children ?? [])].map((th) => th.textContent.trim());
        if (heads.length) for (const tr of table.querySelectorAll("tbody tr")) [...tr.children].forEach((td, k) => { if (heads[k]) td.setAttribute("data-label", heads[k]); });
      }
      // Every other fence (and an indented block) is a code card: its language from the fence, else
      // the <m-code> around it, which gives its `path` and `why` too.
      for (const code of node.querySelectorAll("pre > code:not(.language-mermaid)")) {
        const pre = code.parentElement;
        const mc = pre.closest("m-code");
        const info = [...code.classList].find((c) => c.startsWith("language-"))?.slice("language-".length) || mc?.getAttribute("lang") || "";
        const at = document.createElement("div");
        at.className = info === "component" ? "code def-src" : "code"; // COMPS-8: a definition's source, closed
        pre.before(at);
        const props = { code: code.textContent.replace(/\n$/, ""), info, path: mc?.getAttribute("path") || "", why: mc?.getAttribute("why") || "" };
        placed.push({ at, app: mount(CodeCard, { target: at, props }) });
      }
      // COMPS-4: each ```component fence's save, said under its card; each tag drawn by its component.
      // COMPS-8: the card is closed; its line names the component and opens its source.
      placed.push(...defLines(node, text, (at, d, card) => mount(DefLine, { target: at, props: { name: d.name, source: d.source, html: d.html, v1: d.v1, card } })));
      tags = drawTags(node, { src, of: bodyOf?.() ?? null });
      // EDITOR-22: each `<m-details summary>` a line that opens; before the pills, so its sentence's ids are.
      for (const d of foldDetails(node)) placed.push({ at: d.at, app: mount(MDetails, { target: d.at, props: d.props }) });
      // EDITOR-20: last, so a code card, a diagram and a drawn tag are already what they are (none holds a pill).
      pills = drawPills?.(node) ?? null;
    };
    place(given);
    return { update: place, destroy: () => { key = null; clear(); } };
  }

  let { body = "", title = null, status = null, edit = null, titleEdit = null, block = false, piece = null, defs: given = null } = $props();

  // COMPS-16: each piece keyed by what it is (its text, its mark's line, which of the same it is), not by its
  // offset — so a write elsewhere in the body (a writer above saving) leaves a piece and what is drawn in it be.
  const parts = $derived.by(() => {
    if (edit || block) return [];
    const seen = new Map();
    return blocks(body).map((p) => {
      const t = `${p.wrap?.open ?? ""}\u0000${p.h ?? ""}\u0000${p.text}`;
      const c = seen.get(t) ?? 0;
      seen.set(t, c + 1);
      return { ...p, k: `${t}\u0000${c}` };
    });
  });
  const defs = $derived(block ? given ?? "" : edit ? "" : refDefs(body));
</script>

{#snippet drawn(p)}{#if p.page}{@const Tag = TAGS[pageTag(p)]}<div class="blk-in"><Tag source={p.text} pieces={p.raw ? rawPieces(p.text) : null}/></div>{:else}<div class="blk-in" use:enhance={p}>{@html markdown(mediaLinks(pieceSource(p)) + defs)}</div>{/if}{/snippet}

<TagTable/>{#if block}{@render drawn(piece ?? { text: body })}{:else}
<article class="ticket-view" data-surface="ticket-view" data-editing={edit ? "" : undefined}>
  {#if title != null || status != null || titleEdit}
    <header class="head">
      {#if titleEdit}<h3 class="title" data-field="title" use:titleEdit></h3>
      {:else if title != null}<h3 class="title" data-field="title">{title}</h3>{/if}
      {#if status != null}<span class="status" data-field="status" data-status={status}>{status}</span>{/if}
    </header>
  {/if}
  {#if edit}
    <div class="md" data-field="body" data-ticket-body use:edit></div>
  {:else if !body}
    <p class="empty" data-field="body" data-empty>No body yet.</p>
  {:else}
    <div class="md" data-field="body" data-ticket-body>{#each parts as p (p.k)}<div class={pieceClass(p)} data-block data-tag={p.wrap?.tag} data-page={p.page ? "" : undefined}>{@render drawn(p)}</div>{/each}</div>
  {/if}
</article>{/if}

<style>
  .ticket-view { color: var(--ink); font-size: 14px; }
  .head { display: flex; align-items: baseline; gap: 8px; margin-bottom: 6px; }
  .title { margin: 0; min-width: 0; flex: 1; font-family: var(--font-display); font-size: 14px; font-weight: var(--weight-strong); overflow-wrap: anywhere; }
  [data-editing] .title { font-size: 20px; line-height: 1.3; }
  .title[contenteditable] { padding: 2px 4px; margin-left: -5px; border: var(--border) solid transparent; border-radius: var(--radius); outline: none; }
  .title[contenteditable]:hover, .title[contenteditable]:global([data-sheet~=hover]) { border-color: var(--line); }
  .title[contenteditable]:focus, .title[contenteditable]:global([data-sheet~=focus]) { border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .title[contenteditable]:empty::before { content: "Untitled"; color: var(--muted); }
  .status { flex: none; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .status[data-status=Pending] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .status[data-status=Ready] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .status[data-status=Coding] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .status[data-status="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .status[data-status=Complete] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .status[data-status=Blocked] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .status[data-status=Superseded] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .status[data-status=Canceled] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .status[data-status=Epic] { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .status[data-status=Doc] { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  /* The body: one continuous page, the same here and in the editor (where these rules reach inside the
     editor's own lines and widgets). EDITOR-9 (owner: "I want one smooth surface"): a block is a piece
     of the flow, never a box — no border, background, shadow or container around a house mark, and no
     gap but the type's own rhythm; a mark reads as typography, its labels small and muted. Only code
     keeps a tint. */
  .md { font-family: var(--sans); line-height: 1.6; overflow-wrap: anywhere; }
  .md :global(.blk) { display: flow-root; white-space: normal; }
  .md :global(.blk-in) { display: contents; }
  .md :global(.blk-in > :first-child), .md :global(.blk-in > :first-child > :first-child) { margin-top: 0; }
  .md :global(.blk-in > :last-child), .md :global(.blk-in > :last-child > :last-child) { margin-bottom: 0; }
  .md :global(.blk + .blk), .md :global(.cm-line + .blk), .md :global(.blk + .cm-line) { padding-top: 12px; }
  .md :global(.blk + .blk.part), .md :global(.cm-line + .blk.part) { padding-top: 2px; }
  .md :global(.blk + .blk:has(> .blk-in > :is(h1, h2, h3, h4))), .md :global(.cm-line + .blk:has(> .blk-in > :is(h1, h2, h3, h4))) { padding-top: 22px; }
  /* A house mark's own lines take no room (a caption aside, on its open line); the pieces between read as the flow. */
  .md :global(.blk.h-close) { padding-top: 0; }
  .md :global(.blk.h-open + .blk), .md :global(.blk.h-open + .cm-line) { padding-top: 0; }
  .md :global(.blk:is(.h-mid, .h-close) > .blk-in > :first-child)::before { display: none; }
  .md :global(h1), .md :global(h2), .md :global(h3), .md :global(h4) { margin: 0 0 4px; font-family: var(--font-display); font-weight: var(--weight-strong); line-height: 1.3; }
  .md :global(h1) { font-size: 20px; }
  .md :global(h2) { font-size: 16px; }
  .md :global(h3), .md :global(h4), .md :global(h5), .md :global(h6) { font-size: 14px; }
  .md :global(p) { margin: 6px 0; }
  .md :global(ul), .md :global(ol) { margin: 4px 0; padding-left: 20px; }
  .md :global(a) { color: var(--accent); text-decoration: underline; }
  .md :global(code) { padding: 0 3px; font-family: var(--font-text); font-size: 12.5px; background: var(--panel); border-radius: var(--radius-sm); }
  .md :global(pre:not(.plain, .painted)) { overflow-x: auto; margin: 6px 0; padding: 10px 12px; font-family: var(--font-text); font-size: 12.5px; line-height: 1.55; white-space: pre; background: var(--panel); border-radius: var(--radius); }
  .md :global(pre code) { padding: 0; font-size: inherit; background: none; }
  .md :global(.code-card code) { padding: 0; background: none; }
  .md :global(pre:has(> code.language-mermaid))::before { content: "mermaid"; display: block; margin-bottom: 4px; font-size: 11px; color: var(--muted); }
  .md :global(table) { border-collapse: collapse; margin: 6px 0; }
  .md :global(th), .md :global(td) { padding: 4px 14px 4px 0; text-align: left; vertical-align: top; border-bottom: var(--border) solid var(--line); }
  .md :global(th) { font-weight: var(--weight-strong); }
  .md :global(blockquote) { margin: 6px 0; padding-left: 14px; color: var(--muted); border-left: 2px solid var(--line); }
  .md :global(img) { max-width: 100%; height: auto; }
  .md :global(hr) { margin: 12px 0; border: 0; border-top: var(--border) solid var(--line); }
  /* The house's marks as typography in the flow. Every mark is a plain block or a run of text; what it says
     beside its words (a label, a kicker, a status, who and when) is small muted text. */
  .md :global(:is(m-summary, m-decision, m-input, m-cases, m-beat, m-status, m-diagram, m-mockup, m-results, m-walk, m-code, m-compare, m-callout, m-acceptance, m-cards, m-card, m-dag, m-details, m-provides, m-consumes)) { display: block; }
  .md :global(:is(m-cases, m-results, m-diagram, m-mockup, m-compare)[label])::before,
  .md :global(:is(m-beat, m-walk)[kicker])::before,
  .md :global(m-field[label])::before,
  .md :global(.any[data-caption])::before { display: block; margin: 0 0 2px; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .md :global(:is(m-cases, m-results, m-diagram, m-mockup, m-compare)[label])::before, .md :global(m-field[label])::before { content: attr(label); }
  .md :global(:is(m-beat, m-walk)[kicker])::before { content: attr(kicker); }
  .md :global(.any[data-caption])::before { content: attr(data-caption); }
  .md :global(:is(m-field, m-answer, m-question, m-recommendation, m-option, m-choice, m-step, m-stat, m-ticket, m-badge, m-cite, m-commit, m-chip)) { display: inline; }
  /* A summary: its standing a lead paragraph, its stats an inline row of figures. */
  .md :global(m-summary :is(m-field, m-answer):is([data-field=standing], [data-field=answer])) { display: block; font-size: 16px; line-height: 1.5; font-weight: var(--weight-medium); }
  .md :global(m-stat) { display: inline-flex; flex-direction: column; margin: 4px 22px 4px 0; vertical-align: top; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .md :global(m-stat)::before { content: attr(value); font-family: var(--font-display); font-size: 18px; font-weight: var(--weight-strong); line-height: 1.2; color: var(--ink); }
  .md :global(m-stat[tone=pass])::before { color: var(--live); }
  .md :global(m-stat[tone=warn])::before, .md :global(m-stat[tone=fail])::before { color: var(--warn); }
  /* A decision: its question, its options a simple list, the recommended (or picked) one by weight. */
  .md :global(m-question) { display: block; font-weight: var(--weight-strong); }
  .md :global(:is(m-option, m-choice)) { display: list-item; margin: 2px 0 2px 20px; list-style: disc; }
  .md :global(m-option)::before { content: attr(name); }
  .md :global(m-option[cost])::after { content: " — cost: " attr(cost) "; benefit: " attr(benefit); font-size: 12px; color: var(--muted); }
  .md :global(:is(m-option, m-choice):is([recommended], [picked])) { font-weight: var(--weight-strong); }
  .md :global(:is(m-option, m-choice):is([recommended], [picked]))::after { font-weight: normal; }
  .md :global(:is(m-recommendation, m-answer)) { display: block; margin: 6px 0 0; }
  .md :global(m-recommendation)::before { content: "recommended" " · " attr(by) "  "; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .md :global(m-answer[by])::before { content: "answered · " attr(by) "  "; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  /* A step: a dated line. */
  .md :global(m-step) { display: block; margin: 2px 0; }
  .md :global(m-step)::before { content: attr(date) " · " attr(status) " · " attr(by) "  "; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  /* EDITOR-22: a fold (m-details): one line, what it opens to indented a little under its sentence; several in
     a row a list, a hairline between and no gap but the line's own. Closed, its inside is `hidden=until-found`
     (find-in-page reaches it); printed, it is open. */
  .md :global(m-details[data-folds]) { padding: 3px 0; }
  .md :global(m-details[data-folds] > .det-body) { padding: 0 0 0 22px; }
  .md :global(m-details[data-folds] > .det-body:not([hidden])) { padding-top: 2px; padding-bottom: 4px; }
  .md :global(m-details[data-folds] > .det-body > :first-child) { margin-top: 0; }
  .md :global(m-details[data-folds] > .det-body > :last-child) { margin-bottom: 0; }
  .md :global(.blk.det + .blk.det) { padding-top: 0; border-top: var(--border) solid var(--line); }
  @media print { .md :global(m-details[data-folds] > .det-body[hidden]) { content-visibility: visible; padding-top: 2px; padding-bottom: 4px; } }
  .md :global(m-ticket)::before { content: attr(ref); font-family: var(--font-text); font-size: 12.5px; color: var(--accent); }
  /* EDITOR-20: a ticket id as a pill (ticket-pills): the chip's shape, its stage's colour (`data-stage`, the
     status tokens). */
  .md :global(a.tpill[data-pill]) { display: inline-flex; align-items: baseline; gap: 4px; max-width: 100%; padding: 0 7px; font-family: var(--font-text); font-size: 12px; line-height: 18px; white-space: nowrap; color: var(--chip-fg); text-decoration: none; background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .md :global(a.tpill[data-pill]:hover), .md :global(a.tpill[data-pill][data-sheet~=hover]) { border-color: var(--line); }
  .md :global(a.tpill[data-pill]:focus-visible) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .md :global(a.tpill[data-pill] .tpill-title) { max-width: 36ch; overflow: hidden; text-overflow: ellipsis; font-family: var(--sans); }
  .md :global(a.tpill[data-stage=draft]) { color: var(--st-draft-fg); background: var(--st-draft-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=pending]) { color: var(--st-pending-fg); background: var(--st-pending-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=ready]) { color: var(--st-ready-fg); background: var(--st-ready-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=coding]) { color: var(--st-coding-fg); background: var(--st-coding-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=in-review]) { color: var(--st-in-review-fg); background: var(--st-in-review-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=complete]) { color: var(--st-complete-fg); background: var(--st-complete-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=blocked]) { color: var(--st-blocked-fg); background: var(--st-blocked-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=superseded]) { color: var(--st-superseded-fg); background: var(--st-superseded-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=canceled]) { color: var(--st-canceled-fg); background: var(--st-canceled-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=epic]) { color: var(--st-epic-fg); background: var(--st-epic-bg); border-color: transparent; }
  .md :global(a.tpill[data-stage=doc]) { color: var(--st-doc-fg); background: var(--st-doc-bg); border-color: transparent; }
  .md :global(m-ticket[data-pilled])::before { content: none; }
  .md :global(m-badge) { font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  /* A mark with no look of its own (unknown, a custom tag, a known inline one standing alone): its words in
     the flow, captioned (above) by the attribute that reads as words. */
  .md :global(.any) { display: block; }
  .md :global(.any-inline) { display: inline; }
  .md :global(.any-inline[data-caption])::before { display: inline; margin: 0 4px 0 0; }
  /* EDITOR-9: a diagram drawn over its fence (no frame); the fence shows while there is none (loading, refused). */
  .md :global(.diagram) { margin: 6px 0; }
  .md :global(.diagram:empty) { display: none; }
  .md :global(.diagram:has([data-state=drawn]) + pre) { display: none; }
  /* EDITOR-9: a fence drawn as its code card (code-card); the fence it was drawn from goes. */
  .md :global(.code + pre) { display: none; }
  /* COMPS-8: a ```component definition's source is closed until its line's `source` opens it. */
  .md :global(.code.def-src:not(.def-open)) { display: none; }
  /* EDITOR-10: what a doc absorbed, drawn (media-draw's widgets), on EDITOR-9's one smooth page: a player,
     an image and a PDF bare, in the flow; a file card and a link card a hairline frame and nothing more. */
  .md :global(.mx) { display: block; margin: 8px 0; max-width: 100%; }
  .md :global(.mx video), .md :global(.mx iframe) { display: block; width: 100%; max-width: 720px; border: 0; }
  .md :global(.mx video) { max-height: 480px; }
  .md :global(.mx audio) { display: block; width: 100%; max-width: 520px; }
  .md :global(.mx[data-media=pdf] iframe) { height: 560px; }
  .md :global(.mx-embed iframe) { aspect-ratio: 16 / 9; height: auto; }
  .md :global(.mx-embed[data-provider=x] iframe) { aspect-ratio: auto; height: 420px; max-width: 550px; }
  .md :global(.mx-embed[data-provider=figma] iframe) { aspect-ratio: 4 / 3; }
  .md :global(.mx-cap) { display: flex; gap: 8px; align-items: baseline; margin-top: 4px; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .md :global(.mx-cap .mx-name) { color: var(--ink); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .md :global(.mx[data-media=file]) { display: flex; align-items: center; gap: 10px; max-width: 520px; padding: 8px 10px; border: var(--border) solid var(--line); border-radius: var(--radius); }
  .md :global(.mx-ext) { flex: none; font-family: var(--font-text); font-size: 11px; font-weight: var(--weight-strong); color: var(--muted); }
  .md :global(.mx-meta) { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .md :global(.mx-meta .mx-name) { color: var(--ink); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .md :global(.mx-size) { font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .md :global(.mx-dl) { flex: none; font-size: 12px; }
  .md :global(.mx-again) { flex: none; margin-left: auto; padding: 1px 8px; font: inherit; color: var(--ink); background: none; border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .md :global(.mx-again:hover) { border-color: var(--muted); }
  .md :global(.mx-again[hidden]) { display: none; }
  .md :global(.mx-card) { display: flex; max-width: 560px; overflow: hidden; color: inherit !important; text-decoration: none !important; border: var(--border) solid var(--line); border-radius: var(--radius); }
  .md :global(.mx-card:hover), .md :global(.mx-card[data-sheet~=hover]) { border-color: var(--muted); }
  .md :global(.mx-card img) { flex: none; width: 120px; height: auto; max-height: 100px; object-fit: cover; border-radius: 0 !important; }
  .md :global(.mx-text) { display: flex; flex-direction: column; gap: 2px; min-width: 0; padding: 8px 10px; }
  .md :global(.mx-site) { font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .md :global(.mx-title) { font-weight: var(--weight-strong); color: var(--ink); overflow: hidden; text-overflow: ellipsis; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
  .md :global(.mx-desc) { font-size: 12px; color: var(--muted); overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
  .md :global(.mx-src) { display: inline-block; margin-top: 4px; font-size: 12px; }
  .md :global([data-media-pending]) { min-height: 1.5em; }
  .empty { margin: 0; color: var(--muted); font-family: var(--font-text); font-size: 12px; }
  /* EDITOR-12: on a phone (under 641 px) nothing is wider than the column — the owner: "make sure for the doc
     component that there is no scroll … horizontally". A table stacks, a row a group, each cell under its
     column's header (the `data-label` enhance gave it); a bare fence wraps; media and players fit. */
  @media (max-width: 640px) {
    .md { min-width: 0; max-width: 100%; }
    .md :global(pre:not(.plain, .painted)) { overflow-x: visible; white-space: pre-wrap; overflow-wrap: anywhere; }
    .md :global(table), .md :global(tbody), .md :global(tr), .md :global(td), .md :global(th) { display: block; }
    .md :global(table) { width: 100%; }
    .md :global(thead) { display: none; }
    .md :global(tr) { padding: 6px 0; border-bottom: var(--border) solid var(--line); }
    .md :global(td), .md :global(th) { padding: 1px 0; border-bottom: 0; }
    .md :global(td[data-label]) { display: grid; grid-template-columns: minmax(0, 38%) minmax(0, 1fr); gap: 10px; }
    .md :global(td[data-label])::before { content: attr(data-label); font-family: var(--font-text); font-size: 11px; line-height: 1.9; color: var(--muted); overflow-wrap: anywhere; }
    .md :global(img), .md :global(video), .md :global(iframe), .md :global(svg) { max-width: 100%; }
    .md :global(.mx video), .md :global(.mx iframe), .md :global(.mx audio) { max-width: 100%; }
    .md :global(.mx-embed[data-provider=x] iframe) { height: auto; aspect-ratio: 550 / 420; }
    .md :global(.mx[data-media=pdf] iframe) { height: auto; aspect-ratio: 3 / 4; }
    .md :global(.mx-card img) { width: 88px; }
    .md :global(m-stat) { margin-right: 14px; }
  }
</style>
