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
     theirs. EDITOR-5: a URL pasted over a selection links it; an image pasted,
     dropped or picked goes to `media.put` and lands as `![name](media:<hash>)`,
     an upload card beside the text until it does. CodeMirror comes from ../plexus-ui/package.json (the node_modules
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
</script>

<script>
  import { tick, untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";
  import { Annotation, EditorState, StateEffect, StateField } from "@codemirror/state";
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
    view.dispatch({ effects: upFade.of(null) }); // a refused upload goes with the next key
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

  /** `inputs ask` writes the body too: a change that only appends a question is nobody's hand on the text. */
  const onlyAsks = (was, now) => now.startsWith(was.trimEnd()) && /^\s*<m-input id="ask-\d+" kind="text">[\s\S]*<\/m-input>\s*$/.test(now.slice(was.trimEnd().length));

  /** The bar says whose an outside change was; with a request open, its lines carry the hand. */
  async function heardHand(before, after, text) {
    const who = await whose();
    state = { kind: text ? "merged" : "outside", text: text ?? `${who} changed the body`, who: text ? (state.who ?? who) : who };
    if (text) state = { ...state, text: text.replace("@who", who), who };
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
    const r = await b.call("tickets.amend", { id, body: text });
    if (r.ok) {
      acked = text;
      if (sent === text) sent = null;
      dirty = doc() !== acked;
      if (merged && !merged.only) await heardHand(merged.before, text, `merged @who's change${merged.dropped ? `, yours kept where they met (${merged.dropped} of theirs in the revision)` : ""} · saved ${hhmm()}`);
      else state = { kind: "saved", text: `saved ${hhmm()}` };
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

  // ── EDITOR-5: links and images ───────────────────────────────────────────
  // An upload is a widget beside the text, never text in it: the source holds no placeholder, so no
  // amend can carry one. It rides the changes around it, and its image lands where it ended up.
  const IMAGES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
  const CAP = 7 * 1024 * 1024; // media's; past about 7.5 MiB the socket drops the message
  let uploads = 0;
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
    const refuse = (said) => view?.dispatch({ effects: upRefuse.of({ key, name, said }) });
    if (!IMAGES.includes(file.type)) return refuse("PNG, JPEG, GIF or WebP only");
    if (file.size > CAP) return refuse("over the 7 MiB an image may be");
    const data = await base64(file).catch(() => null);
    if (data == null) return refuse("the file could not be read");
    const r = await b.call("media.put", { ticket: id, mime: file.type, data });
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
  // request is open marks the lines it changed with whose hand it was, until a click accepts them
  // (which closes the requests) or an edit takes the mark off that line alone.
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
    if (!view) return false;
    const sel = view.state.selection.main;
    if (sel.empty) return false;
    const doc = view.state.doc;
    const coords = view.coordsAtPos(sel.to);
    const box = host.getBoundingClientRect();
    asking = { a: doc.lineAt(sel.from).number, b: doc.lineAt(sel.to).number, text: view.state.sliceDoc(sel.from, sel.to), top: (coords?.bottom ?? box.top) - box.top + 4 };
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
    const asked = await b.call("tickets.inputs.ask", { ticket: id, question });
    const input = asked.events?.find((e) => e.type === "input_changed")?.input?.id;
    if (!asked.ok || !input) { askRefused = String(asked.message ?? "refused").replace(/^Invalid params: /, ""); return; }
    const queued = await b.call("tickets.queue", { id, urgency: "now", reason: "an agent is asked to write in", input: [input] });
    asking = null;
    requests.push({ input, where, state: queued.ok ? "asked" : "refused", said: queued.ok ? "" : String(queued.message ?? "refused") });
  }
  async function accept() {
    view?.dispatch({ effects: handClear.of(null) });
    hand = null;
    for (const r of requests.filter((x) => x.state === "answered" || x.state === "asked")) {
      const c = await b.call("tickets.inputs.close", { ticket: id, input: r.input });
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

  function init(ticket) {
    acked = ticket.body;
    titleDraft = ticket.title;
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: ticket.body,
        extensions: [
          history(),
          keymap.of([{ key: "Mod-s", run: () => { flush(); return true; } }, { key: "Mod-Enter", run: openAsk }, indentWithTab, ...defaultKeymap, ...historyKeymap]),
          markdownSupport({ base: markdownLanguage }),
          EditorView.lineWrapping,
          formed,
          pendingUploads,
          handed,
          look,
          EditorView.updateListener.of((u) => { if (u.selectionSet) hasSelection = !u.state.selection.main.empty; if (u.docChanged && !u.transactions.some((tr) => tr.annotation(Outside))) edited(); }),
          EditorView.domEventHandlers({ blur: () => { if (dirty || pending !== null) flush(); return false; }, paste: pasted, drop: dropped, click: (ev) => { if (ev.target?.closest?.(".eb-hand")) accept(); return false; } }),
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
  <div class="cmwrap">
    <div class="cm" bind:this={host}></div>
    {#if asking}
      <form class="ask" data-ask data-state="open" style={`top: ${asking.top}px`} onsubmit={(ev) => { ev.preventDefault(); sendAsk(); }}>
        <span class="where">{asking.a === asking.b ? `line ${asking.a}` : `lines ${asking.a}–${asking.b}`}</span>
        <input class="say" data-field="instruction" aria-label="what an agent should do" placeholder="what should an agent do with it?" autocomplete="off" bind:this={askBox} bind:value={instruction}/>
        <button class="pick" data-action="ask" disabled={!instruction.trim()}>ask an agent</button>
        {#if askRefused}<span class="no" data-refused>{askRefused}</span>{/if}
      </form>
    {/if}
  </div>
  {#each requests as r (r.input)}
    <p class="asked" data-ask={r.input} data-state={r.state}>
      {#if r.state === "asked"}asked <b>{r.input}</b> · queued <b>now</b> · waiting for an agent
      {:else if r.state === "answered"}<b>{hand ?? "an agent"}</b> wrote in · <button class="link" data-action="accept" onclick={accept}>accept</button> or click the marked lines
      {:else if r.state === "accepted"}accepted · <b>{r.input}</b> closed
      {:else}<span class="no" data-refused>{r.said}</span>{/if}
    </p>
  {/each}
  {#if t}
    <p class="state" data-field="write-state" data-state={state.kind} data-refused={state.kind === "refused" ? "" : undefined} data-hand={state.who ?? undefined}>
      {#if state.kind === "idle"}<span class="muted">every change is written 600 ms after the last key</span>{:else}{state.text}{/if}
      <button type="button" class="pick" data-action="ask-open" disabled={!hasSelection} onclick={openAsk}>ask an agent <kbd>ctrl</kbd><kbd>enter</kbd></button>
      <button type="button" class="pick image" data-action="image" onclick={() => picker?.click()}>image</button>
      <input class="file" type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple aria-label="an image" bind:this={picker} onchange={picked}/>
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
  .pick[data-action=ask-open] { margin-left: auto; }
  .pick { padding: 0 8px; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .pick:hover, .pick:global([data-sheet~=hover]) { background: var(--hover); }
  .pick:focus-visible, .pick:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .pick:disabled, .pick:global([data-sheet~=disabled]) { color: var(--disabled); cursor: not-allowed; }
  kbd { display: inline-block; min-width: 14px; padding: 0 3px; font-family: var(--font-text); font-size: 10px; line-height: 14px; text-align: center; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .cmwrap { position: relative; }
  .ask { position: absolute; left: 10px; right: 10px; z-index: 3; display: flex; flex-wrap: wrap; gap: 6px; align-items: center; padding: 6px 8px; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-lg); box-shadow: 0 6px 18px var(--shadow); }
  .where { font-size: 11px; color: var(--muted); }
  .say { flex: 1; min-width: 150px; padding: 2px 6px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .say:focus-visible, .say:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .no { color: var(--warn); }
  .asked { margin: 6px 0 0; padding: 0 10px; font-size: 11px; color: var(--muted); }
  .asked b { font-weight: var(--weight-medium); color: var(--accent); }
  .link { padding: 0; font: inherit; color: var(--accent); background: none; border: 0; cursor: pointer; text-decoration: underline; }
  .link:hover, .link:global([data-sheet~=hover]) { color: var(--ink); }
  .link:focus-visible, .link:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .cm :global(.eb-hand) { cursor: pointer; background: var(--sel); border-left-color: var(--accent); }
  .cm :global(.eb-hand::after) { content: attr(data-hand); float: right; padding-left: 8px; font-family: var(--font-text); font-size: 10px; color: var(--muted); }
  .file { display: none; }
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
  .cm :global(.eb-upload) { display: block; margin: 4px 0; padding: 6px 10px; font-family: var(--font-text); font-size: 11px; color: var(--muted); background: var(--card); border: var(--border) dashed var(--line); border-radius: var(--radius-lg); }
  .cm :global(.eb-upload[data-state=refused]) { color: var(--warn); border-color: var(--warn); }
  .cm :global(.eb-img) { padding: 4px 10px 6px; }
  .cm :global(.eb-img img) { display: block; max-width: 100%; max-height: 60vh; border: var(--border) solid var(--line); border-radius: var(--radius); }
  @media (prefers-reduced-motion: reduce) {
    .state[data-state=saving]::before, .skel .bone { animation: none; }
  }
</style>
