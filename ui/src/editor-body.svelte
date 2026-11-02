<!-- EDITOR-3: the ticket's body in a CodeMirror 6 editor (svelte/1), the
     markdown exact — the editor's document is the body byte for byte — and
     formed in place: a Lezer parse of the markdown decorates headings,
     emphasis, lists, links, inline and fenced code, quotes, rules and images
     where they stand, and hides their syntax marks except on the caret's
     line or inside the caret's span (a live preview: what is typed is what
     is seen, the source one caret move away). Live on the page's `get` spec
     (the one ticket-body and ticket-head read: one get, one watch, shared).
     Writes: 600 ms after the last key, or when the focus leaves, one
     `tickets.amend { id, body }`, the body whole; the title above it the
     same way, `{ id, title }`. The bar's line says saving · saved HH:MM ·
     the service's refusal (`[data-refused]`) · whose hand an outside amend
     was. An outside amend (one that is not the body this editor last sent):
     with the draft clean it is applied as changes the editor maps the caret
     and the scroll through; with the draft dirty it waits for the pause,
     then is merged line by line with ours, ours winning where they overlap,
     and the merge is sent and said — the revision the service kept has
     theirs. CodeMirror comes from ../plexus-ui/package.json (the node_modules
     the svelte/1 build reads), bundled into this module by the tool. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.amend", "tickets.revisions.list"];

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
</script>

<script>
  import { untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";
  import { Annotation, EditorState, StateField } from "@codemirror/state";
  import { Decoration, EditorView, WidgetType, keymap } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { markdown as markdownSupport, markdownLanguage } from "@codemirror/lang-markdown";
  import { syntaxTree } from "@codemirror/language";

  let { id, sheet = null } = $props();
  const b = bus();
  const PAUSE = 600;

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  // ticket-head's spec word for word: the page holds one `get` and one `watch` of the ticket, shared.
  const t = $derived(ticket.rows.find((r) => r.title != null && !ticket.rows.some((x) => Array.isArray(x.tombstones))));

  // The bar's line: what the last write did, and whose the last outside one was.
  let state = $state({ kind: "idle", text: "" });
  let titleDraft = $state("");
  let titleDirty = false;
  let titleTimer = null;
  let host = null;
  let view = null;
  // The write model, outside Svelte's reactivity: the body the service holds as far as we know
  // (`acked`), the one in flight (`sent`), whether the document differs from `acked` (`dirty`),
  // an outside body waiting for the pause (`pending`), and the pause's timer.
  let acked = null, sent = null, dirty = false, pending = null, timer = null;
  const Outside = Annotation.define();
  const hhmm = () => { const d = new Date(); return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`; };
  const doc = () => view.state.doc.toString();

  // ── the write model ──────────────────────────────────────────────────────
  function edited() {
    dirty = doc() !== acked;
    if (timer) clearTimeout(timer);
    if (dirty || pending !== null) timer = setTimeout(flush, PAUSE);
  }

  /** Whose hand the newest revision was: its name, else its actor, else nobody known. */
  async function whose() {
    const r = await b.call("tickets.revisions.list", { id, limit: 1 });
    // Each item is its own `revision { revision }` event (a `rows { items }` page is read the same way).
    const row = (r.events ?? []).flatMap((e) => (e.type === "rows" ? e.items ?? [] : [e])).map((e) => e.revision ?? e).find((e) => e.rev != null) ?? null;
    return row ? row.name || row.actor || "someone" : "someone";
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
    whose().then((who) => { state = { kind: "outside", text: `${who} changed the body`, who }; });
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
      text = m.text;
      acked = theirs;
      merged = m;
    }
    if (text === acked) { dirty = false; if (merged) state = { kind: "saved", text: `saved ${hhmm()}` }; return; }
    sent = text;
    state = { kind: "saving", text: "saving…" };
    const r = await b.call("tickets.amend", { id, body: text });
    if (r.ok) {
      acked = text;
      if (sent === text) sent = null;
      dirty = doc() !== acked;
      if (merged) {
        const who = await whose();
        state = { kind: "merged", text: `merged ${who}'s change${merged.dropped ? `, yours kept where they met (${merged.dropped} of theirs in the revision)` : ""} · saved ${hhmm()}`, who };
      } else state = { kind: "saved", text: `saved ${hhmm()}` };
    } else {
      sent = null;
      state = { kind: "refused", text: String(r.message ?? "refused").replace(/^Invalid params: /, "") };
    }
  }

  function titleEdited() {
    titleDirty = titleDraft !== (t?.title ?? "");
    if (titleTimer) clearTimeout(titleTimer);
    if (titleDirty) titleTimer = setTimeout(flushTitle, PAUSE);
  }
  async function flushTitle() {
    if (titleTimer) { clearTimeout(titleTimer); titleTimer = null; }
    if (!titleDirty || !t) return;
    const title = titleDraft.trim();
    if (!title) return;
    const r = await b.call("tickets.amend", { id, title });
    if (r.ok) { titleDirty = false; state = { kind: "saved", text: `saved ${hhmm()}` }; }
    else state = { kind: "refused", text: String(r.message ?? "refused").replace(/^Invalid params: /, "") };
  }

  // ── the live preview ─────────────────────────────────────────────────────
  class Bullet extends WidgetType {
    toDOM() { const s = document.createElement("span"); s.className = "eb-bullet"; s.textContent = "•"; return s; }
    eq() { return true; }
    ignoreEvent() { return false; }
  }
  class Rule extends WidgetType {
    toDOM() { const s = document.createElement("span"); s.className = "eb-rule"; return s; }
    eq() { return true; }
  }
  class Img extends WidgetType {
    constructor(src, alt, name) { super(); this.src = src; this.alt = alt; this.name = name; }
    eq(o) { return o.src === this.src && o.alt === this.alt; }
    toDOM() {
      const d = document.createElement("div");
      d.className = "eb-img";
      const i = document.createElement("img");
      i.src = this.src; i.alt = this.alt; i.loading = "lazy";
      i.setAttribute("data-image", this.name);
      d.append(i);
      return d;
    }
    ignoreEvent() { return false; }
  }
  const srcOf = (url) => {
    const m = /^media:([0-9a-fA-F]{64})$/.exec(url.trim());
    if (m) return { src: `${location.origin}/media/${m[1].toLowerCase()}`, name: m[1].toLowerCase() };
    if (/^https?:\/\//i.test(url.trim())) return { src: url.trim(), name: url.trim() };
    return null;
  };

  /** The decorations of `state`: a class per formed thing, the marks hidden away from the caret. */
  function decorate(state) {
    const out = [];
    const sel = state.selection.main;
    const caretLine = state.doc.lineAt(sel.head).number;
    const touches = (from, to) => sel.from <= to && sel.to >= from;
    const lineOf = (pos) => state.doc.lineAt(pos);
    const hide = (from, to) => { if (to > from) out.push(Decoration.replace({}).range(from, to)); };
    const mark = (from, to, cls, attrs) => { if (to > from) out.push(Decoration.mark({ class: cls, ...(attrs ? { attributes: attrs } : {}) }).range(from, to)); };
    const lines = (from, to, cls) => { for (let l = lineOf(from); ; l = lineOf(l.to + 1)) { out.push(Decoration.line({ class: cls }).range(l.from)); if (l.to >= to || l.to + 1 > state.doc.length) break; } };
    const hideMarks = (node, names) => { for (const c of node.getChildren(names)) hide(c.from, c.to); for (const n of names) for (const c of node.getChildren(n)) hide(c.from, c.to); };
    syntaxTree(state).iterate({
      enter(ref) {
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
            // The class on the text between the marks (one span), the marks hidden away from the caret.
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
            if (name === "Image") {
              const at = srcOf(href);
              if (at) out.push(Decoration.widget({ widget: new Img(at.src, text ? state.doc.sliceString(text[0], text[1]) : "", at.name), block: true, side: 1 }).range(lineOf(ref.to).to));
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
            for (const m of ref.node.getChildren("QuoteMark")) { if (lineOf(m.from).number !== caretLine) hide(m.from, Math.min(ref.to, m.to + (state.doc.sliceString(m.to, m.to + 1) === " " ? 1 : 0))); else mark(m.from, m.to, "eb-mk"); }
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
          case "HorizontalRule": {
            if (lineOf(ref.from).number !== caretLine) out.push(Decoration.replace({ widget: new Rule() }).range(ref.from, ref.to));
            else mark(ref.from, ref.to, "eb-mk");
            return;
          }
          case "HTMLBlock": lines(ref.from, ref.to, "eb-html"); return false;
          case "Table": lines(ref.from, ref.to, "eb-table"); return false;
          case "Escape": mark(ref.from, ref.from + 1, "eb-mk"); return;
          default: return;
        }
      },
    });
    return Decoration.set(out, true);
  }
  const formed = StateField.define({
    create: decorate,
    update(deco, tr) { return tr.docChanged || tr.selection ? decorate(tr.state) : deco; },
    provide: (f) => EditorView.decorations.from(f),
  });
  const look = EditorView.theme({
    "&": { backgroundColor: "transparent", color: "var(--ink)" },
    ".cm-content": { padding: "6px 0 24px", caretColor: "var(--ink)", fontFamily: "var(--sans)", fontSize: "14px", lineHeight: "1.6" },
    ".cm-line": { padding: "1px 8px 1px 10px", borderLeft: "2px solid transparent" },
    "&.cm-focused": { outline: "none" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--ink)" },
    "&.cm-focused .cm-activeLine, .cm-activeLine": { backgroundColor: "var(--sel)", borderLeftColor: "var(--accent)", borderRadius: "0 var(--radius) var(--radius) 0" },
    ".cm-scroller": { fontFamily: "inherit", overflow: "visible" },
  });

  function init(ticket) {
    acked = ticket.body;
    titleDraft = ticket.title;
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: ticket.body,
        extensions: [
          history(),
          keymap.of([{ key: "Mod-s", run: () => { flush(); return true; } }, indentWithTab, ...defaultKeymap, ...historyKeymap]),
          markdownSupport({ base: markdownLanguage }),
          EditorView.lineWrapping,
          formed,
          look,
          EditorView.updateListener.of((u) => { if (u.docChanged && !u.transactions.some((tr) => tr.annotation(Outside))) edited(); }),
          EditorView.domEventHandlers({ blur: () => { if (dirty || pending !== null) flush(); return false; } }),
          EditorView.contentAttributes.of({ "data-field": "body", "aria-label": "the body" }),
        ],
      }),
    });
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
    untrack(() => { if (!titleDirty) titleDraft = title; });
  });
  $effect(() => () => {
    if (timer) clearTimeout(timer);
    if (titleTimer) clearTimeout(titleTimer);
    if (view) { if (dirty && pending === null) { const text = doc(); if (text !== acked) b.call("tickets.amend", { id, body: text }); } view.destroy(); view = null; }
  });
</script>

<section class="editor-body" data-surface="editor-body" data-ticket={id}>
  {#if sheet === "loading" || (!t && ticket.status === "pending")}
    <div class="skel" data-loading aria-busy="true"><i class="bone title"></i><i class="bone"></i><i class="bone half"></i></div>
  {:else if !t && ticket.status !== "pending"}
    <p class="empty" data-empty data-not-found>No ticket {id} here{ticket.message ? ` — ${ticket.message}` : ""}.</p>
  {:else}
    <input class="title" data-field="title" aria-label="the title" spellcheck="false" autocomplete="off" bind:value={titleDraft} oninput={titleEdited} onblur={flushTitle}/>
  {/if}
  <div class="cm" bind:this={host}></div>
  {#if t}
    <p class="state" data-field="write-state" data-state={state.kind} data-refused={state.kind === "refused" ? "" : undefined} data-hand={state.who ?? undefined}>
      {#if state.kind === "idle"}<span class="muted">every change is written 600 ms after the last key</span>{:else}{state.text}{/if}
      <a class="revs" href={"#/ticket/" + id} data-action="revisions">revisions</a>
    </p>
  {/if}
</section>

<style>
  .editor-body { display: block; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .title { display: block; width: 100%; margin: 0 0 6px; padding: 4px 10px; font-family: var(--font-display); font-size: 18px; line-height: 1.3; font-weight: var(--weight-strong); color: var(--ink); background: transparent; border: var(--border) solid transparent; border-radius: var(--radius); }
  .title:hover, .title:global([data-sheet~=hover]) { border-color: var(--line); }
  .title:focus, .title:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .state { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin: 8px 0 0; padding: 0 10px; font-size: 11px; color: var(--muted); }
  .state[data-state=saved]::before { content: "\2713"; color: var(--live); }
  .state[data-state=saving]::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--muted); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .state[data-state=refused] { color: var(--warn); }
  .state[data-state=refused]::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--warn); }
  .state[data-state=merged], .state[data-state=outside] { color: var(--ink); }
  .state[data-state=merged]::before, .state[data-state=outside]::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .revs { margin-left: auto; color: var(--accent); text-decoration: none; }
  .revs:hover, .revs:global([data-sheet~=hover]) { text-decoration: underline; }
  .revs:focus-visible, .revs:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; border-radius: var(--radius-sm); }
  .muted { color: var(--muted); }
  .empty { margin: 0; padding: 8px 10px; color: var(--muted); }
  .skel .bone { display: block; width: 100%; height: 10px; margin: 8px 10px; background: var(--skeleton); border-radius: var(--radius-sm); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .skel .bone.title { width: 55%; height: 22px; }
  .skel .bone.half { width: 50%; }
  @keyframes breathe { from { opacity: 1; } to { opacity: .5; } }
  /* The formed source: CodeMirror draws the lines, these classes form them (the sheet's tokens, no literal). */
  .cm :global(.cm-content ::selection) { background: var(--row-sel); }
  .cm :global(.eb-h) { font-family: var(--font-display); font-weight: var(--weight-strong); }
  .cm :global(.eb-h1) { font-size: 22px; line-height: 1.3; padding-top: 8px; }
  .cm :global(.eb-h2) { font-size: 17px; line-height: 1.35; padding-top: 10px; }
  .cm :global(.eb-h3), .cm :global(.eb-h4), .cm :global(.eb-h5), .cm :global(.eb-h6) { font-size: 14px; padding-top: 8px; }
  .cm :global(.eb-em) { font-style: italic; }
  .cm :global(.eb-strong) { font-weight: var(--weight-strong); }
  .cm :global(.eb-strike) { text-decoration: line-through; color: var(--muted); }
  .cm :global(.eb-code) { padding: 0 3px; font-family: var(--font-text); font-size: 12px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .cm :global(.eb-link) { color: var(--accent); text-decoration: underline; }
  .cm :global(.eb-alt) { color: var(--muted); font-size: 12px; }
  .cm :global(.eb-mk), .cm :global(.eb-info) { color: var(--muted); opacity: .7; font-family: var(--font-text); font-size: 12px; }
  .cm :global(.eb-bullet) { display: inline-block; width: 1em; color: var(--muted); }
  .cm :global(.eb-quote) { color: var(--muted); border-left-color: var(--line); }
  .cm :global(.eb-fence) { font-family: var(--font-text); font-size: 12.5px; line-height: 1.55; background: var(--panel); border-left-color: var(--line); }
  .cm :global(.eb-fence-first) { margin-top: 6px; border-radius: var(--radius) var(--radius) 0 0; }
  .cm :global(.eb-fence-last) { margin-bottom: 6px; border-radius: 0 0 var(--radius) var(--radius); }
  .cm :global(.eb-html), .cm :global(.eb-table) { font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .cm :global(.eb-rule) { display: inline-block; width: 100%; height: 0; vertical-align: middle; border-top: var(--border) solid var(--line); }
  .cm :global(.eb-img) { padding: 4px 10px 6px; }
  .cm :global(.eb-img img) { display: block; max-width: 100%; max-height: 60vh; border: var(--border) solid var(--line); border-radius: var(--radius); }
  @media (prefers-reduced-motion: reduce) {
    .state[data-state=saving]::before, .skel .bone { animation: none; }
  }
</style>
