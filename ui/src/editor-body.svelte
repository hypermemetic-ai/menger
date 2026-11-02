<!-- EDITOR-8: the rendering is the editor — one surface, Obsidian's live
     preview. The ticket is drawn by ticket-view (a sibling, inlined), and its
     body's element holds a CodeMirror 6 editor whose document is the body
     byte for byte: every block the caret is not in is replaced by a widget
     that is ticket-view itself mounted on that block (its `blocks()` split and
     its one `{@html markdown(…)}`, the host's markdown through the engine's sanitizer, the house's marks as
     cards), so a block away from the caret is drawn exactly as `#/ticket/:id`
     draws it; the caret's block shows its markdown, formed in place (headings,
     emphasis, links, lists, fences; a mark hidden off the caret's line). A
     click on a drawn block puts the caret where it was clicked, in the source.
     Unfocused, every block is drawn. The title is ticket-view's, editable in
     place. Live on the page's `get` spec (the one ticket-head reads: one get,
     one watch, shared). Writes: 600 ms after the last key, or when the focus
     leaves, one `tickets.amend { id, body }`, the body whole; the title the
     same way, `{ id, title }`. The line under the canvas says saving · saved
     HH:MM · the service's refusal (`[data-refused]`) · whose hand an outside
     amend was. An outside amend (one that is not the body this editor last
     sent): with the draft clean it is applied as changes the editor maps the
     caret and the scroll through; with the draft dirty it waits for the
     pause, then is merged line by line with ours, ours winning where they
     overlap, and the merge is sent and said — the revision the service kept
     has theirs. EDITOR-5: a URL pasted over a selection links it; an image
     pasted, dropped or picked goes to `media.put` and lands as
     `![name](media:<hash>)`. EDITOR-6: a selection and one line become a
     queued request. Keys: mod+b / mod+i bold and italic, mod+k a link,
     mod+z / mod+shift+z undo and redo, mod+s saves now, mod+enter asks an
     agent. CodeMirror comes from ../plexus-ui/package.json (the node_modules
     the svelte/1 build reads), bundled into this module by the tool. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.amend", "tickets.revisions.list", "media.put", "tickets.inputs.ask", "tickets.queue", "tickets.inputs.close"];

  /** A line-level diff of `a` → `b`: hunks `{ a0, a1, b0, b1 }`, each "replace a[a0:a1] with b[b0:b1]",
   *  by the common prefix and suffix, then an LCS over what is left (one hunk past 4 M cells). */
  export function diffLines(a, b) {
    const n = a.length, m = b.length;
    let p = 0;
    while (p < n && p < m && a[p] === b[p]) p++;
    let s = 0;
    while (s < n - p && s < m - p && a[n - 1 - s] === b[m - 1 - s]) s++;
    const A = a.slice(p, n - s), B = b.slice(p, m - s);
    if (!A.length && !B.length) return [];
    if (!A.length || !B.length || A.length * B.length > 4_000_000) return [{ a0: p, a1: n - s, b0: p, b1: m - s }];
    const L = Array.from({ length: A.length + 1 }, () => new Uint32Array(B.length + 1));
    for (let i = A.length - 1; i >= 0; i--) for (let j = B.length - 1; j >= 0; j--) L[i][j] = A[i] === B[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
    const hunks = [];
    let i = 0, j = 0, ha = -1, hb = -1;
    const open = () => { if (ha < 0) { ha = i; hb = j; } };
    const close = () => { if (ha >= 0) { hunks.push({ a0: p + ha, a1: p + i, b0: p + hb, b1: p + j }); ha = -1; } };
    while (i < A.length && j < B.length) {
      if (A[i] === B[j]) { close(); i++; j++; }
      else if (L[i + 1][j] >= L[i][j + 1]) { open(); i++; }
      else { open(); j++; }
    }
    if (i < A.length || j < B.length) { open(); i = A.length; j = B.length; }
    close();
    return hunks;
  }

  /** The hunks of `diffLines(a, b)` as CodeMirror changes over the text `a.join("\n")`. */
  export function changesOf(a, b) {
    const off = [0];
    for (const l of a) off.push(off[off.length - 1] + l.length + 1);
    const end = Math.max(0, off[a.length] - 1);
    return diffLines(a, b).map(({ a0, a1, b0, b1 }) => {
      const lines = b.slice(b0, b1);
      if (a1 < a.length) return { from: off[a0], to: off[a1], insert: lines.length ? lines.join("\n") + "\n" : "" };
      const from = a0 > 0 ? off[a0] - 1 : 0;
      return { from, to: end, insert: (a0 > 0 && lines.length ? "\n" : "") + lines.join("\n") };
    });
  }

  /** `base` with `theirs`' and `ours`' line changes both applied; where a hunk of each covers the same
   *  base lines, ours alone. Answers the merged text and how many of theirs it dropped. */
  export function merge3(base, theirs, ours) {
    const B = base.split("\n");
    const ht = diffLines(B, theirs.split("\n")), ho = diffLines(B, ours.split("\n"));
    const overlaps = (t, o) => !(t.a1 <= o.a0 || o.a1 <= t.a0);
    const kept = ht.filter((t) => !ho.some((o) => overlaps(t, o)));
    const T = theirs.split("\n"), O = ours.split("\n");
    const all = [...kept.map((h) => ({ ...h, from: T, side: 0 })), ...ho.map((h) => ({ ...h, from: O, side: 1 }))].sort((x, y) => x.a0 - y.a0 || x.side - y.side);
    const out = [];
    let pos = 0;
    for (const h of all) {
      out.push(...B.slice(pos, h.a0), ...h.from.slice(h.b0, h.b1));
      pos = h.a1;
    }
    out.push(...B.slice(pos));
    return { text: out.join("\n"), dropped: ht.length - kept.length };
  }

  /** Where in a block's markdown `src` the caret goes for a click after the drawn text `seen`: the
   *  drawn characters matched in order against the source, past its tags and its marks. */
  export function sourceOffset(src, seen) {
    const want = seen.replace(/\s+/g, "");
    let i = 0;
    for (const ch of want) {
      for (;;) {
        if (i >= src.length) return src.length;
        if (src[i] === "<") {
          const tag = /^<\/?[a-zA-Z][^>\n]*>/.exec(src.slice(i));
          if (tag) { i += tag[0].length; continue; }
        }
        if (src[i] === ch) { i++; break; }
        i++;
      }
    }
    return i;
  }
</script>

<script>
  import { mount, tick, unmount, untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";
  import TicketView, { blocks, refDefs } from "./ticket-view.svelte";
  import { Annotation, EditorSelection, EditorState, StateEffect, StateField } from "@codemirror/state";
  import { Decoration, EditorView, WidgetType, keymap, placeholder } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { markdown as markdownSupport, markdownKeymap, markdownLanguage } from "@codemirror/lang-markdown";
  import { syntaxTree } from "@codemirror/language";

  let { id, sheet = null } = $props();
  // The document this instance edits: fixed at mount (editor-screen keys the editor by it), so a
  // write still in hand when the screen moves to another document lands on its own ticket.
  const doc_id = untrack(() => id);
  const b = bus();
  const PAUSE = 600;

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id: doc_id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  // ticket-head's spec word for word: the page holds one `get` and one `watch` of the ticket, shared.
  const t = $derived(ticket.rows.find((r) => r.title != null && !ticket.rows.some((x) => Array.isArray(x.tombstones))));

  // The line under the canvas: what the last write did, and whose the last outside one was.
  let state = $state({ kind: "idle", text: "" });
  let titleDraft = "";
  let titleDirty = false;
  let titleTimer = null;
  let titleEl = null;
  let host = null;
  let wrap = $state(null);
  let view = null;
  // The write model, outside Svelte's reactivity: the body the service holds as far as we know
  // (`acked`), the one in flight (`sent`), whether the document differs from `acked` (`dirty`),
  // an outside body waiting for the pause (`pending`), and the pause's timer.
  let acked = null, sent = null, dirty = false, pending = null, timer = null;
  const Outside = Annotation.define();
  const hhmm = () => { const d = new Date(); return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`; };
  const doc = () => view.state.doc.toString();
  const said = (r) => String(r.message ?? "refused").replace(/^Invalid params: /, "");

  // ── the write model ──────────────────────────────────────────────────────
  function edited() {
    view.dispatch({ effects: upFade.of(null) }); // a refused upload goes with the next key
    dirty = doc() !== acked;
    if (timer) clearTimeout(timer);
    if (dirty || pending !== null) timer = setTimeout(flush, PAUSE);
  }

  /** Whose hand the newest revision was: its name, else its actor, else nobody known. */
  async function whose() {
    const r = await b.call("tickets.revisions.list", { id: doc_id, limit: 1 });
    // Each item is its own `revision { revision }` event (a `rows { items }` page is read the same way).
    const row = (r.events ?? []).flatMap((e) => (e.type === "rows" ? e.items ?? [] : [e])).map((e) => e.revision ?? e).find((e) => e.rev != null) ?? null;
    return row ? row.name || row.actor || "someone" : "someone";
  }

  /** `inputs ask` writes the body too: a change that only appends a question is nobody's hand on the text. */
  const onlyAsks = (was, now) => now.startsWith(was.trimEnd()) && /^\s*<m-input id="ask-\d+" kind="text">[\s\S]*<\/m-input>\s*$/.test(now.slice(was.trimEnd().length));

  /** The line says whose an outside change was; with a request open, its lines carry the hand. */
  async function heardHand(before, after, text) {
    const who = await whose();
    state = { kind: text ? "merged" : "outside", text: text ? text.replace("@who", who) : `${who} changed the body`, who };
    const open = requests.filter((r) => r.state === "asked" || r.state === "answered");
    if (!open.length || !view) return;
    const lines = diffLines(before.split("\n"), after.split("\n")).flatMap((h) => Array.from({ length: h.b1 - h.b0 }, (_, i) => h.b0 + i + 1));
    const max = view.state.doc.lines;
    view.dispatch({ effects: handAdd.of({ who, at: lines.filter((n) => n <= max).map((n) => view.state.doc.line(n).from) }) });
    for (const r of open) r.state = "answered";
    hand = who;
  }

  /** A body from the service, through `get` or the watch. */
  function incoming(body) {
    if (!view || body === acked) return;
    if (sent !== null && body === sent) { acked = body; sent = null; return; }
    const now = doc();
    if (body === now) { acked = body; dirty = false; return; }
    if (dirty) { pending = body; if (!timer) timer = setTimeout(flush, PAUSE); return; }
    view.dispatch({ changes: changesOf(now.split("\n"), body.split("\n")), annotations: Outside.of(true), scrollIntoView: false });
    acked = body;
    if (!onlyAsks(now, body)) heardHand(now, body, null);
  }

  /** The pause: a waiting outside body merged with ours, then ours sent whole. */
  async function flush() {
    if (timer) { clearTimeout(timer); timer = null; }
    if (!view) return;
    let text = doc();
    let merged = null;
    if (pending !== null) {
      const theirs = pending;
      pending = null;
      const m = merge3(acked ?? "", theirs, text);
      if (m.text !== text) view.dispatch({ changes: changesOf(text.split("\n"), m.text.split("\n")), annotations: Outside.of(true), scrollIntoView: false });
      merged = { ...m, before: text, only: onlyAsks(acked ?? "", theirs) };
      text = m.text;
      acked = theirs;
    }
    if (text === acked) { dirty = false; if (merged) state = { kind: "saved", text: `saved ${hhmm()}` }; return; }
    sent = text;
    state = { kind: "saving", text: "saving…" };
    const r = await b.call("tickets.amend", { id: doc_id, body: text });
    if (r.ok) {
      acked = text;
      if (sent === text) sent = null;
      dirty = view ? doc() !== acked : false;
      if (merged && !merged.only) await heardHand(merged.before, text, `merged @who's change${merged.dropped ? `, yours kept where they met (${merged.dropped} of theirs in the revision)` : ""} · saved ${hhmm()}`);
      else state = { kind: "saved", text: `saved ${hhmm()}` };
    } else {
      sent = null;
      state = { kind: "refused", text: said(r) };
    }
  }

  // ── the title, ticket-view's heading made editable ───────────────────────
  function titleEdited() {
    titleDraft = (titleEl?.textContent ?? "").replace(/\s+/g, " ");
    titleDirty = titleDraft !== (t?.title ?? "");
    if (titleTimer) clearTimeout(titleTimer);
    if (titleDirty) titleTimer = setTimeout(flushTitle, PAUSE);
  }
  async function flushTitle() {
    if (titleTimer) { clearTimeout(titleTimer); titleTimer = null; }
    if (!titleDirty) return;
    const title = titleDraft.trim();
    if (!title) return;
    const r = await b.call("tickets.amend", { id: doc_id, title });
    if (r.ok) { titleDirty = false; state = { kind: "saved", text: `saved ${hhmm()}` }; }
    else state = { kind: "refused", text: said(r) };
  }
  function titleMount(node) {
    titleEl = node;
    try { node.contentEditable = "plaintext-only"; } catch { node.contentEditable = "true"; }
    if (node.contentEditable !== "plaintext-only") node.contentEditable = "true";
    node.spellcheck = false;
    node.setAttribute("role", "textbox");
    node.setAttribute("aria-label", "the title");
    node.textContent = t?.title ?? "";
    const keys = (ev) => { if (ev.key === "Enter") { ev.preventDefault(); flushTitle(); view?.focus(); } };
    const pasted = (ev) => { ev.preventDefault(); document.execCommand("insertText", false, (ev.clipboardData?.getData("text/plain") ?? "").replace(/\s+/g, " ")); };
    node.addEventListener("input", titleEdited);
    node.addEventListener("blur", flushTitle);
    node.addEventListener("keydown", keys);
    node.addEventListener("paste", pasted);
    return { destroy() { titleEl = null; } };
  }

  // ── the surface: drawn blocks, the caret's block as markdown ─────────────
  const focusFx = StateEffect.define();
  const focused = StateField.define({ create: () => false, update(v, tr) { for (const e of tr.effects) if (e.is(focusFx)) v = e.value; return v; } });

  /** A block away from the caret: ticket-view's drawing of it. A click enters it at the clicked place. */
  class Drawn extends WidgetType {
    constructor(text, defs, hand) { super(); this.text = text; this.defs = defs; this.hand = hand; }
    eq(o) { return o.text === this.text && o.defs === this.defs && o.hand === this.hand; }
    toDOM(v) {
      const d = document.createElement("div");
      d.className = "blk";
      d.setAttribute("data-block", "");
      if (this.hand) { d.classList.add("eb-hand"); d.setAttribute("data-hand", this.hand); }
      // ticket-view itself, drawing this block alone: the drawing #/ticket/:id makes of it.
      d.view = mount(TicketView, { target: d, props: { body: this.text, defs: this.defs, block: true } });
      d.addEventListener("mousedown", (ev) => {
        if (ev.button !== 0) return;
        const a = ev.target?.closest?.("a[href]");
        if (a && (ev.metaKey || ev.ctrlKey)) { ev.preventDefault(); window.open(a.href, "_blank", "noopener"); return; }
        if (this.hand) { accept(); }
        ev.preventDefault();
        enter(v, d, this.text, ev);
      });
      d.addEventListener("click", (ev) => { if (ev.target?.closest?.("a[href]")) ev.preventDefault(); });
      return d;
    }
    destroy(dom) { if (dom.view) { unmount(dom.view); dom.view = null; } }
    ignoreEvent() { return true; }
  }
  /** The caret into the block drawn as `dom`, where the pointer was. */
  function enter(v, dom, text, ev) {
    let base;
    try { base = v.posAtDOM(dom); } catch { return; }
    let off = 0;
    const pt = document.caretPositionFromPoint ? document.caretPositionFromPoint(ev.clientX, ev.clientY)
      : document.caretRangeFromPoint ? ((r) => r && { offsetNode: r.startContainer, offset: r.startOffset })(document.caretRangeFromPoint(ev.clientX, ev.clientY)) : null;
    if (pt && dom.contains(pt.offsetNode)) {
      const r = document.createRange();
      r.setStart(dom, 0);
      try { r.setEnd(pt.offsetNode, pt.offset); off = sourceOffset(text, r.toString()); } catch { off = 0; }
    }
    v.focus();
    v.dispatch({ selection: { anchor: Math.min(base + off, v.state.doc.length) }, effects: focusFx.of(true), scrollIntoView: false });
  }

  class Upload extends WidgetType {
    constructor(key, name, said) { super(); this.key = key; this.name = name; this.said = said; }
    eq(o) { return o.key === this.key && o.said === this.said; }
    toDOM() {
      const d = document.createElement("span");
      d.className = "eb-upload";
      d.setAttribute("data-upload", this.name);
      d.setAttribute("data-state", this.said == null ? "uploading" : "refused");
      if (this.said != null) d.setAttribute("data-refused", "");
      d.textContent = this.said == null ? `uploading ${this.name}…` : `${this.name}: ${this.said}`;
      return d;
    }
  }
  class Bullet extends WidgetType {
    toDOM() { const s = document.createElement("span"); s.className = "eb-bullet"; s.textContent = "•"; return s; }
    eq() { return true; }
    ignoreEvent() { return false; }
  }

  /** The decorations of `state`: each block the caret is not in drawn; in the caret's, the markdown formed. */
  function surface(state) {
    const src = state.doc.toString();
    const all = blocks(src);
    const defs = refDefs(src);
    const focus = state.field(focused);
    const ranges = state.selection.ranges;
    const hands = state.field(handed);
    const out = [];
    const shown = [];
    for (let k = 0; k < all.length; k++) {
      const blk = all[k];
      const end = k + 1 < all.length ? all[k + 1].from - 1 : state.doc.length;
      if (focus && ranges.some((r) => r.from <= end && r.to >= blk.from)) { shown.push([blk.from, end]); continue; }
      let hand = null;
      hands.between(blk.from, end, (_f, _t, d) => { hand = d.spec.attributes?.["data-hand"] ?? hand; });
      out.push(Decoration.replace({ widget: new Drawn(blk.text, defs, hand), block: true }).range(blk.from, end));
    }
    if (shown.length) out.push(...formed(state, shown));
    return Decoration.set(out, true);
  }

  /** The markdown of the shown blocks formed in place: a class per formed thing, marks hidden off the caret's line. */
  function formed(state, shown) {
    const out = [];
    const sel = state.selection.main;
    const caretLine = state.doc.lineAt(sel.head).number;
    const inShown = (pos) => shown.some(([a, z]) => pos >= a && pos <= z);
    const touches = (from, to) => sel.from <= to && sel.to >= from;
    const lineOf = (pos) => state.doc.lineAt(pos);
    const hide = (from, to) => { if (to > from) out.push(Decoration.replace({}).range(from, to)); };
    const mark = (from, to, cls, attrs) => { if (to > from) out.push(Decoration.mark({ class: cls, ...(attrs ? { attributes: attrs } : {}) }).range(from, to)); };
    const lines = (from, to, cls) => { for (let l = lineOf(from); ; l = lineOf(l.to + 1)) { out.push(Decoration.line({ class: cls }).range(l.from)); if (l.to >= to || l.to + 1 > state.doc.length) break; } };
    for (const [a, z] of shown) syntaxTree(state).iterate({
      from: a, to: z,
      enter(ref) {
        if (!inShown(ref.from)) return;
        const name = ref.name;
        const h = /^(ATX|Setext)Heading([1-6])$/.exec(name);
        if (h) {
          const l = lineOf(ref.from);
          out.push(Decoration.line({ class: `eb-h eb-h${h[2]}` }).range(l.from));
          if (l.number !== caretLine) for (const m of ref.node.getChildren("HeaderMark")) hide(m.from, Math.min(ref.to, m.to + (state.doc.sliceString(m.to, m.to + 1) === " " ? 1 : 0)));
          return;
        }
        switch (name) {
          case "Emphasis": case "StrongEmphasis": case "Strikethrough": case "InlineCode": {
            const ms = ref.node.getChildren(name === "Strikethrough" ? "StrikethroughMark" : name === "InlineCode" ? "CodeMark" : "EmphasisMark");
            const cls = name === "Emphasis" ? "eb-em" : name === "StrongEmphasis" ? "eb-strong" : name === "Strikethrough" ? "eb-strike" : "eb-code";
            if (ms.length >= 2) mark(ms[0].to, ms[ms.length - 1].from, cls); else mark(ref.from, ref.to, cls);
            if (!touches(ref.from, ref.to)) for (const m of ms) hide(m.from, m.to);
            return;
          }
          case "Link": case "Image": {
            const node = ref.node;
            const url = node.getChild("URL");
            const href = url ? state.doc.sliceString(url.from, url.to) : "";
            const marks = node.getChildren("LinkMark");
            const text = marks.length >= 2 ? [marks[0].to, marks[1].from] : null;
            if (text) mark(text[0], text[1], name === "Link" ? "eb-link" : "eb-alt", href ? { title: href } : null);
            if (!touches(ref.from, ref.to)) {
              for (const m of marks) hide(m.from, m.to);
              if (url) hide(url.from, url.to);
              for (const m of node.getChildren("LinkTitle")) hide(m.from, m.to);
            }
            return false;
          }
          case "ListMark": {
            const l = lineOf(ref.from);
            if (l.number !== caretLine && /^[-*+]$/.test(state.doc.sliceString(ref.from, ref.to))) out.push(Decoration.replace({ widget: new Bullet() }).range(ref.from, ref.to));
            else mark(ref.from, ref.to, "eb-mk");
            return;
          }
          case "ListItem": lines(ref.from, ref.to, "eb-li"); return;
          case "Blockquote": {
            lines(ref.from, ref.to, "eb-quote");
            for (const m of ref.node.getChildren("QuoteMark")) mark(m.from, m.to, "eb-mk");
            return;
          }
          case "FencedCode": {
            const first = lineOf(ref.from), last = lineOf(ref.to);
            lines(ref.from, ref.to, "eb-fence");
            out.push(Decoration.line({ class: "eb-fence-first" }).range(first.from));
            out.push(Decoration.line({ class: "eb-fence-last" }).range(last.from));
            for (const m of ref.node.getChildren("CodeMark")) mark(m.from, m.to, "eb-mk");
            for (const m of ref.node.getChildren("CodeInfo")) mark(m.from, m.to, "eb-info");
            return false;
          }
          case "HorizontalRule": mark(ref.from, ref.to, "eb-mk"); return;
          case "HTMLBlock": lines(ref.from, ref.to, "eb-html"); return false;
          case "HTMLTag": mark(ref.from, ref.to, "eb-tag"); return;
          case "Table": lines(ref.from, ref.to, "eb-table"); return false;
          case "Escape": mark(ref.from, ref.from + 1, "eb-mk"); return;
          default: return;
        }
      },
    });
    return out;
  }

  const drawn = StateField.define({
    create: surface,
    update(deco, tr) { return tr.docChanged || tr.selection || tr.effects.some((e) => e.is(focusFx) || e.is(handAdd) || e.is(handClear)) ? surface(tr.state) : deco; },
    provide: (f) => EditorView.decorations.from(f),
  });
  const look = EditorView.theme({
    "&": { backgroundColor: "transparent", color: "var(--ink)" },
    ".cm-content": { padding: "0 0 24px", caretColor: "var(--ink)", fontFamily: "var(--sans)", fontSize: "14px", lineHeight: "1.55", whiteSpace: "pre-wrap" },
    ".cm-line": { padding: "0" },
    "&.cm-focused": { outline: "none" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--ink)" },
    ".cm-scroller": { fontFamily: "inherit", overflow: "visible", lineHeight: "inherit" },
    ".cm-placeholder": { color: "var(--muted)" },
  });

  // ── keys: bold, italic, a link ───────────────────────────────────────────
  /** Wrap the selection in `mk` (or take it off when it is already wrapped). */
  const wrapWith = (mk) => (v) => {
    v.dispatch(v.state.changeByRange((r) => {
      const n = mk.length;
      const before = v.state.sliceDoc(r.from - n, r.from), after = v.state.sliceDoc(r.to, r.to + n);
      if (before === mk && after === mk) return { changes: [{ from: r.from - n, to: r.from }, { from: r.to, to: r.to + n }], range: EditorSelection.range(r.from - n, r.to - n) };
      return { changes: [{ from: r.from, insert: mk }, { from: r.to, insert: mk }], range: EditorSelection.range(r.from + n, r.to + n) };
    }));
    return true;
  };
  function linkKey(v) {
    const r = v.state.selection.main;
    const label = v.state.sliceDoc(r.from, r.to) || "link";
    const url = "https://";
    v.dispatch({ changes: { from: r.from, to: r.to, insert: `[${label}](${url})` }, selection: { anchor: r.from + label.length + 3, head: r.from + label.length + 3 + url.length } });
    return true;
  }

  // ── EDITOR-5: links and images ───────────────────────────────────────────
  // An upload is a widget beside the text, never text in it: the source holds no placeholder, so no
  // amend can carry one. It rides the changes around it, and its image lands where it ended up.
  const IMAGES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
  const CAP = 7 * 1024 * 1024; // media's; past about 7.5 MiB the socket drops the message
  let uploads = 0;
  const upAdd = StateEffect.define({ map: (v, m) => ({ ...v, pos: m.mapPos(v.pos) }) });
  const upDrop = StateEffect.define();
  const upFade = StateEffect.define();
  const upRefuse = StateEffect.define();
  const upWidget = (key, name, said, pos) => Decoration.widget({ widget: new Upload(key, name, said), side: 1 }).range(pos);
  const pendingUploads = StateField.define({
    create: () => Decoration.none,
    update(deco, tr) {
      deco = deco.map(tr.changes);
      for (const e of tr.effects) {
        if (e.is(upAdd)) deco = deco.update({ add: [upWidget(e.value.key, e.value.name, null, e.value.pos)], sort: true });
        else if (e.is(upDrop)) deco = deco.update({ filter: (_f, _t, d) => d.spec.widget.key !== e.value });
        else if (e.is(upFade)) deco = deco.update({ filter: (_f, _t, d) => d.spec.widget.said == null });
        else if (e.is(upRefuse)) {
          let at = null;
          deco.between(0, tr.state.doc.length, (f, _t, d) => { if (d.spec.widget.key === e.value.key) at = f; });
          if (at != null) deco = deco.update({ filter: (_f, _t, d) => d.spec.widget.key !== e.value.key, add: [upWidget(e.value.key, e.value.name, e.value.said, at)] });
        }
      }
      return deco;
    },
    provide: (f) => EditorView.decorations.from(f),
  });
  const base64 = (file) => new Promise((done, fail) => {
    const r = new FileReader();
    r.onload = () => done(String(r.result).replace(/^data:[^,]*,/, ""));
    r.onerror = () => fail(r.error);
    r.readAsDataURL(file);
  });
  /** One image: its widget at `pos`, `media.put`, then `![name](media:<hash>)` on a line of its own. */
  async function upload(file, pos) {
    const key = ++uploads;
    const name = (file.name || "image").replace(/\.[a-z0-9]+$/i, "").replace(/[[\]\n]/g, " ") || "image";
    view.dispatch({ effects: upAdd.of({ key, name, pos }) });
    const refuse = (why) => view?.dispatch({ effects: upRefuse.of({ key, name, said: why }) });
    if (!IMAGES.includes(file.type)) return refuse("PNG, JPEG, GIF or WebP only");
    if (file.size > CAP) return refuse("over the 7 MiB an image may be");
    const data = await base64(file).catch(() => null);
    if (data == null) return refuse("the file could not be read");
    const r = await b.call("media.put", { ticket: doc_id, mime: file.type, data });
    if (!view) return;
    const stored = r.events?.find((e) => e.type === "stored");
    if (!stored) return refuse(String(r.message ?? r.events?.[0]?.message ?? r.events?.[0]?.type ?? "refused").replace(/^Invalid params: /, ""));
    let at = null;
    view.state.field(pendingUploads).between(0, view.state.doc.length, (f, _t, d) => { if (d.spec.widget.key === key) at = f; });
    if (at == null) return;
    const line = view.state.doc.lineAt(at);
    // A paragraph of its own: right after a line it would render inside that line.
    const above = at > line.from ? "\n\n" : line.number > 1 && view.state.doc.line(line.number - 1).text.trim() !== "" ? "\n" : "";
    const insert = `${above}![${name}](media:${stored.hash})${at < line.to ? "\n\n" : ""}`;
    view.dispatch({ changes: { from: at, insert }, effects: upDrop.of(key) });
  }
  const images = (list) => [...(list ?? [])].filter((f) => f.type.startsWith("image/"));
  function pasted(ev, v) {
    const files = images(ev.clipboardData?.files);
    if (files.length) {
      ev.preventDefault();
      for (const f of files) upload(f, v.state.selection.main.head);
      return true;
    }
    const text = (ev.clipboardData?.getData("text/plain") ?? "").trim();
    const sel = v.state.selection.main;
    if (sel.empty || !/^https?:\/\/\S+$/i.test(text)) return false;
    ev.preventDefault();
    const label = v.state.sliceDoc(sel.from, sel.to);
    v.dispatch({ changes: { from: sel.from, to: sel.to, insert: `[${label}](${text})` } });
    return true;
  }
  function dropped(ev, v) {
    const files = images(ev.dataTransfer?.files);
    if (!files.length) return false;
    ev.preventDefault();
    const pos = v.posAtCoords({ x: ev.clientX, y: ev.clientY }) ?? v.state.selection.main.head;
    for (const f of files) upload(f, pos);
    return true;
  }

  // ── EDITOR-6: write in at the owner's request ────────────────────────────
  // A selection and one line become a question on the ticket, queued now. An outside amend while a
  // request is open marks the lines it changed (a drawn block holding one, the block) with whose hand
  // it was, until a click accepts them (which closes the requests) or an edit takes the mark off.
  let requests = $state([]);
  let hand = $state(null);
  let hasSelection = $state(false);
  let asking = $state(null); // { a, b, text, top } while the line is open
  let instruction = $state("");
  let askRefused = $state("");
  let askBox = $state(null);
  const handAdd = StateEffect.define({ map: (v, m) => ({ ...v, at: v.at.map((p) => m.mapPos(p)) }) });
  const handClear = StateEffect.define();
  const handed = StateField.define({
    create: () => Decoration.none,
    update(deco, tr) {
      deco = deco.map(tr.changes);
      if (tr.docChanged && !tr.annotation(Outside)) tr.changes.iterChangedRanges((_a, _b, from, to) => {
        const a = tr.state.doc.lineAt(from).from, z = tr.state.doc.lineAt(to).from;
        deco = deco.update({ filter: (f) => f < a || f > z });
      });
      for (const e of tr.effects) {
        if (e.is(handClear)) deco = Decoration.none;
        else if (e.is(handAdd)) deco = deco.update({ add: e.value.at.map((p) => Decoration.line({ class: "eb-hand", attributes: { "data-hand": e.value.who } }).range(p)), sort: true });
      }
      return deco;
    },
    provide: (f) => EditorView.decorations.from(f),
  });
  function openAsk() {
    if (!view || !wrap) return false;
    const sel = view.state.selection.main;
    if (sel.empty) return false;
    const d = view.state.doc;
    const coords = view.coordsAtPos(sel.to);
    const box = wrap.getBoundingClientRect();
    asking = { a: d.lineAt(sel.from).number, b: d.lineAt(sel.to).number, text: view.state.sliceDoc(sel.from, sel.to), top: (coords?.bottom ?? box.top) - box.top + 4 };
    instruction = "";
    askRefused = "";
    tick().then(() => askBox?.focus());
    return true;
  }
  async function sendAsk() {
    const what = instruction.trim();
    if (!asking || !what) return;
    const { a, b: z, text } = asking;
    const quoted = text.replace(/\s+/g, " ").trim();
    const where = a === z ? `line ${a}` : `lines ${a}–${z}`;
    const question = `${what} — ${where}: «${quoted.length > 200 ? quoted.slice(0, 200) + "…" : quoted}»`;
    if (dirty) await flush(); // the question lands on the text it is about
    const asked = await b.call("tickets.inputs.ask", { ticket: doc_id, question });
    const input = asked.events?.find((e) => e.type === "input_changed")?.input?.id;
    if (!asked.ok || !input) { askRefused = said(asked); return; }
    const queued = await b.call("tickets.queue", { id: doc_id, urgency: "now", reason: "an agent is asked to write in", input: [input] });
    asking = null;
    requests.push({ input, where, state: queued.ok ? "asked" : "refused", said: queued.ok ? "" : String(queued.message ?? "refused") });
  }
  async function accept() {
    view?.dispatch({ effects: handClear.of(null) });
    hand = null;
    for (const r of requests.filter((x) => x.state === "answered" || x.state === "asked")) {
      const c = await b.call("tickets.inputs.close", { ticket: doc_id, input: r.input });
      r.state = c.ok ? "accepted" : "refused";
      r.said = c.ok ? "" : String(c.message ?? "refused");
    }
  }
  $effect(() => {
    if (!asking) return;
    return b.keys({
      escape: { to: () => { asking = null; view?.focus(); }, label: "close the request", in_inputs: true, only: true },
      "ctrl+enter": { to: sendAsk, label: "ask an agent", in_inputs: true, only: true },
    });
  });

  let picker = $state(null);
  function picked(ev) {
    for (const f of ev.currentTarget.files ?? []) if (view) upload(f, view.state.selection.main.head);
    ev.currentTarget.value = "";
  }

  function init(tk) {
    acked = tk.body;
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: tk.body,
        extensions: [
          history(),
          keymap.of([
            { key: "Mod-s", run: () => { flush(); return true; } },
            { key: "Mod-Enter", run: openAsk },
            { key: "Mod-b", run: wrapWith("**") },
            { key: "Mod-i", run: wrapWith("*") },
            { key: "Mod-k", run: linkKey },
            { key: "Escape", run: (v) => { v.contentDOM.blur(); return true; } },
            ...markdownKeymap, indentWithTab, ...defaultKeymap, ...historyKeymap,
          ]),
          markdownSupport({ base: markdownLanguage }),
          EditorView.lineWrapping,
          focused,
          handed,
          drawn,
          pendingUploads,
          look,
          placeholder("Write here — # a heading, - a list, **bold**; it saves as you type"),
          EditorView.updateListener.of((u) => {
            if (u.selectionSet) hasSelection = !u.state.selection.main.empty;
            if (u.focusChanged && u.view.hasFocus !== u.state.field(focused)) queueMicrotask(() => view?.dispatch({ effects: focusFx.of(view.hasFocus) }));
            if (u.docChanged && !u.transactions.some((tr) => tr.annotation(Outside))) edited();
          }),
          EditorView.domEventHandlers({
            blur: () => { if (dirty || pending !== null) flush(); return false; },
            paste: pasted,
            drop: dropped,
            click: (ev) => { if (ev.target?.closest?.(".eb-hand")) accept(); return false; },
          }),
          EditorView.contentAttributes.of({ "data-field": "body-source", "aria-label": "the body" }),
        ],
      }),
    });
    host.editorView = view; // the page tests read the document and the caret through it
  }
  function bodyMount(node) {
    host = node;
    if (t && !view) init(t);
    return { destroy() { host = null; } };
  }

  $effect(() => {
    if (!t || view || !host) return;
    untrack(() => init(t));
  });
  $effect(() => {
    const body = t?.body;
    if (body == null) return;
    untrack(() => incoming(body));
  });
  $effect(() => {
    const title = t?.title;
    if (title == null) return;
    untrack(() => { if (!titleDirty && titleEl && document.activeElement !== titleEl) titleEl.textContent = title; });
  });
  $effect(() => () => {
    if (timer) clearTimeout(timer);
    if (titleTimer) { clearTimeout(titleTimer); if (titleDirty && titleDraft.trim()) b.call("tickets.amend", { id: doc_id, title: titleDraft.trim() }); }
    if (view) { if (dirty && pending === null) { const text = doc(); if (text !== acked) b.call("tickets.amend", { id: doc_id, body: text }); } view.destroy(); view = null; }
  });
</script>

<section class="editor-body" data-surface="editor-body" data-ticket={doc_id} bind:this={wrap}>
  {#if sheet === "loading" || (!t && ticket.status === "pending")}
    <div class="skel" data-loading aria-busy="true"><i class="bone title"></i><i class="bone"></i><i class="bone half"></i></div>
  {:else if !t && ticket.status !== "pending"}
    <p class="empty" data-empty data-not-found>No ticket {doc_id} here{ticket.message ? ` — ${ticket.message}` : ""}.</p>
  {:else}
    <TicketView status={t.status} edit={bodyMount} titleEdit={titleMount}/>
  {/if}
  {#if asking}
    <form class="ask" data-ask data-state="open" style={`top: ${asking.top}px`} onsubmit={(ev) => { ev.preventDefault(); sendAsk(); }}>
      <span class="where">{asking.a === asking.b ? `line ${asking.a}` : `lines ${asking.a}–${asking.b}`}</span>
      <input class="say" data-field="instruction" aria-label="what an agent should do" placeholder="what should an agent do with it?" autocomplete="off" bind:this={askBox} bind:value={instruction}/>
      <button class="pick" data-action="ask" disabled={!instruction.trim()}>ask an agent</button>
      {#if askRefused}<span class="no" data-refused>{askRefused}</span>{/if}
    </form>
  {/if}
  {#each requests as r (r.input)}
    <p class="asked" data-ask={r.input} data-state={r.state}>
      {#if r.state === "asked"}asked <b>{r.input}</b> · queued <b>now</b> · waiting for an agent
      {:else if r.state === "answered"}<b>{hand ?? "an agent"}</b> wrote in · <button class="link" data-action="accept" onclick={accept}>accept</button> or click the marked text
      {:else if r.state === "accepted"}accepted · <b>{r.input}</b> closed
      {:else}<span class="no" data-refused>{r.said}</span>{/if}
    </p>
  {/each}
  {#if t}
    <p class="state" data-field="write-state" data-state={state.kind} data-refused={state.kind === "refused" ? "" : undefined} data-hand={state.who ?? undefined}>
      {#if state.kind === "idle"}<span class="muted">saves as you type</span>{:else}{state.text}{/if}
      <button type="button" class="pick" data-action="ask-open" disabled={!hasSelection} onmousedown={(ev) => ev.preventDefault()} onclick={openAsk}>ask an agent <kbd>ctrl</kbd><kbd>enter</kbd></button>
      <button type="button" class="pick image" data-action="image" onmousedown={(ev) => ev.preventDefault()} onclick={() => picker?.click()}>image</button>
      <input class="file" type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple aria-label="an image" bind:this={picker} onchange={picked}/>
      <a class="revs" href={"#/ticket/" + doc_id} data-action="revisions">revisions</a>
    </p>
  {/if}
</section>

<style>
  .editor-body { position: relative; display: block; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .state { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin: 14px 0 0; padding-top: 8px; font-size: 11px; color: var(--muted); border-top: var(--border) solid var(--line); }
  .state[data-state=saved]::before { content: "\2713"; color: var(--live); }
  .state[data-state=saving]::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--muted); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .state[data-state=refused] { color: var(--warn); }
  .state[data-state=refused]::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--warn); }
  .state[data-state=merged], .state[data-state=outside] { color: var(--ink); }
  .state[data-state=merged]::before, .state[data-state=outside]::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .revs { color: var(--accent); text-decoration: none; }
  .revs:hover, .revs:global([data-sheet~=hover]) { text-decoration: underline; }
  .revs:focus-visible, .revs:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; border-radius: var(--radius-sm); }
  .pick[data-action=ask-open] { margin-left: auto; }
  .pick { padding: 0 8px; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .pick:hover, .pick:global([data-sheet~=hover]) { background: var(--hover); }
  .pick:focus-visible, .pick:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .pick:disabled, .pick:global([data-sheet~=disabled]) { color: var(--disabled); cursor: not-allowed; }
  kbd { display: inline-block; min-width: 14px; padding: 0 3px; font-family: var(--font-text); font-size: 10px; line-height: 14px; text-align: center; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .ask { position: absolute; left: 0; right: 0; z-index: 3; display: flex; flex-wrap: wrap; gap: 6px; align-items: center; padding: 6px 8px; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-lg); box-shadow: 0 6px 18px var(--shadow); }
  .where { font-size: 11px; color: var(--muted); }
  .say { flex: 1; min-width: 150px; padding: 2px 6px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .say:focus-visible, .say:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .no { color: var(--warn); }
  .asked { margin: 6px 0 0; font-size: 11px; color: var(--muted); }
  .asked b { font-weight: var(--weight-medium); color: var(--accent); }
  .link { padding: 0; font: inherit; color: var(--accent); background: none; border: 0; cursor: pointer; text-decoration: underline; }
  .link:hover, .link:global([data-sheet~=hover]) { color: var(--ink); }
  .link:focus-visible, .link:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .file { display: none; }
  .muted { color: var(--muted); }
  .empty { margin: 0; padding: 8px 0; color: var(--muted); }
  .skel .bone { display: block; width: 100%; height: 10px; margin: 8px 0; background: var(--skeleton); border-radius: var(--radius-sm); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .skel .bone.title { width: 55%; height: 22px; }
  .skel .bone.half { width: 50%; }
  @keyframes breathe { from { opacity: 1; } to { opacity: .5; } }
  /* The surface: a drawn block is ticket-view's (its rules reach in); the caret's block is the
     markdown, formed on the same sizes. */
  .editor-body :global(.cm-editor) { font: inherit; max-width: 100%; }
  .editor-body :global(.cm-scroller) { min-width: 0; }
  .editor-body :global(.cm-content) { flex: 1 1 0; min-width: 0; max-width: 100%; }
  .editor-body :global(.blk) { min-width: 0; max-width: 100%; }
  .editor-body :global(.cm-content ::selection) { background: var(--row-sel); }
  .editor-body :global(.blk) { cursor: text; }
  .editor-body :global(.blk:hover), .editor-body :global(.blk[data-sheet~=hover]) { background: var(--hover); box-shadow: 0 0 0 4px var(--hover); border-radius: var(--radius-sm); }
  .editor-body :global(.eb-h) { font-family: var(--font-display); font-weight: var(--weight-strong); line-height: 1.3; }
  .editor-body :global(.eb-h1) { font-size: 20px; }
  .editor-body :global(.eb-h2) { font-size: 16px; }
  .editor-body :global(.eb-em) { font-style: italic; }
  .editor-body :global(.eb-strong) { font-weight: var(--weight-strong); }
  .editor-body :global(.eb-strike) { text-decoration: line-through; color: var(--muted); }
  .editor-body :global(.eb-code) { padding: 0 3px; font-family: var(--font-text); font-size: 12px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .editor-body :global(.eb-link) { color: var(--accent); text-decoration: underline; }
  .editor-body :global(.eb-alt) { color: var(--muted); font-size: 12px; }
  .editor-body :global(.eb-mk), .editor-body :global(.eb-info) { color: var(--muted); font-family: var(--font-text); font-size: 12px; }
  .editor-body :global(.eb-bullet) { display: inline-block; width: 1em; color: var(--muted); }
  .editor-body :global(.eb-li) { padding-left: 4px; }
  .editor-body :global(.eb-quote) { padding-left: 12px; color: var(--muted); border-left: 3px solid var(--line); }
  .editor-body :global(.eb-fence) { padding: 0 10px; font-family: var(--font-text); font-size: 12.5px; background: var(--panel); border-left: var(--border) solid var(--line); border-right: var(--border) solid var(--line); }
  .editor-body :global(.eb-fence-first) { padding-top: 6px; border-top: var(--border) solid var(--line); border-radius: var(--radius) var(--radius) 0 0; }
  .editor-body :global(.eb-fence-last) { padding-bottom: 6px; border-bottom: var(--border) solid var(--line); border-radius: 0 0 var(--radius) var(--radius); }
  .editor-body :global(.eb-html), .editor-body :global(.eb-table), .editor-body :global(.eb-tag) { font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .editor-body :global(.eb-upload) { display: block; margin: 4px 0; padding: 6px 10px; font-family: var(--font-text); font-size: 11px; color: var(--muted); background: var(--card); border: var(--border) dashed var(--line); border-radius: var(--radius-lg); }
  .editor-body :global(.eb-upload[data-state=refused]) { color: var(--warn); border-color: var(--warn); }
  .editor-body :global(.eb-hand) { cursor: pointer; background: var(--sel); box-shadow: inset 2px 0 0 var(--accent); }
  .editor-body :global(.eb-hand::after) { content: attr(data-hand); float: right; padding-left: 8px; font-family: var(--font-text); font-size: 10px; color: var(--muted); }
  @media (prefers-reduced-motion: reduce) {
    .state[data-state=saving]::before, .skel .bone { animation: none; }
  }
</style>
