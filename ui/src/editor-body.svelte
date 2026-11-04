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
     has theirs. EDITOR-5, EDITOR-10: what comes from outside is media-ingest's
     (a file of any kind and size, a URL alone, pasted HTML, an image from
     another tab), and a drawn block's lone file or URL is drawn by
     media-draw (a player, a PDF viewer, a file card, a link card). EDITOR-6: a selection and one line become a
     queued request. Keys: mod+b / mod+i bold and italic, mod+k a link,
     mod+z / mod+shift+z undo and redo, mod+s saves now, mod+enter asks an
     agent. CodeMirror comes from ../plexus-ui/package.json (the node_modules
     the svelte/1 build reads), bundled into this module by the tool.
     V2-29: the same surface writes a document of the docs service (`kind:
     "doc"`, `#/doc/:id`) — one component, the store behind a small adapter
     (`isDoc` below), never a fork. A doc's body is a Yjs CRDT (the owner's
     `pairing`: live co-editing): the page holds a `Y.Doc` synced by
     `docs.state`, the editor bound to its text by y-codemirror.next's
     `yCollab` (its own undo), every local change sent as it happens (batched
     every 50 ms, `docs.update` with this page's session) and every other
     hand's arriving on `docs.watch { id, session }` (a gap in the `seq`s asks
     `docs.state` with this copy's state vector). Carets by Yjs awareness
     (`docs.here`), and who is here drawn above the canvas
     (`[data-field=present]`). The title saves by `docs.rename`, an image by
     `media.put { doc }`; a `history` scrubber replays the doc at any change
     (`docs.history`, `docs.replay`). The write-in (EDITOR-6) is a ticket's.
     V2-30: on a doc, a selection and `ask` (mod+enter, or the button) keep a
     question on that range (`docs.ask`, to anyone or to an agent); the
     margin beside the text (doc-margin, a sibling) holds each thread at its
     range's height, the range underlined in the caret's block, and follows
     the text as it changes (`marginTick`). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.amend", "tickets.revisions.list", "tickets.inputs.ask", "tickets.queue", "tickets.inputs.close",
                       "docs.get", "docs.watch", "docs.state", "docs.update", "docs.rename", "docs.present", "docs.here", "docs.history", "docs.replay", "docs.ask"];

  /** V2-29: bytes ↔ standard base64, as the docs service carries a Yjs update. */
  export const toB64 = (bytes) => { let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000)); return btoa(s); };
  export const fromB64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

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
  import TicketView, { blocks, pieceClass, refDefs } from "./ticket-view.svelte";
  import Margin, { threadMarks } from "./doc-margin.svelte";
  import { ingest } from "./media-ingest.svelte";
  import { drawAll } from "./media-draw.svelte";
  import { Annotation, EditorSelection, EditorState, StateEffect, StateField, Transaction } from "@codemirror/state";
  import { Decoration, EditorView, WidgetType, keymap, placeholder } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { markdown as markdownSupport, markdownKeymap, markdownLanguage } from "@codemirror/lang-markdown";
  import { syntaxTree } from "@codemirror/language";
  import * as Y from "yjs";
  import { yCollab, yUndoManagerKeymap } from "y-codemirror.next";
  import { Awareness, applyAwarenessUpdate, encodeAwarenessUpdate, removeAwarenessStates } from "y-protocols/awareness";
  import { langOf } from "./code-card.svelte";

  let { id, sheet = null, kind = "ticket" } = $props();
  // The document this instance edits: fixed at mount (editor-screen keys the editor by it), so a
  // write still in hand when the screen moves to another document lands on its own ticket.
  const doc_id = untrack(() => id);
  // V2-29: the adapter — a ticket (`tickets.*`, a whole body after a pause) or a doc (`docs.*`, a
  // Yjs CRDT, live). Fixed at mount, as the document is.
  const isDoc = untrack(() => kind === "doc");
  const b = bus();
  const PAUSE = 600;

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id: doc_id }), changes: "tickets.watch", key: "id", when: () => !isDoc,
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  // ticket-head's spec word for word: the page holds one `get` and one `watch` of the ticket, shared.
  const tk = $derived(ticket.rows.find((r) => r.title != null && !ticket.rows.some((x) => Array.isArray(x.tombstones))));

  // ── V2-29: a doc — its title and who wrote it (`docs.get`, renames and the delete on its watch),
  // its CRDT's sync (`docs.state`, then each other hand's update), and who is here ──────────────
  const SESSION = "s-" + Math.random().toString(36).slice(2, 10);
  const meta = live({ snapshot: "docs.get", params: () => ({ id: doc_id }), changes: "docs.watch", changes_params: () => ({ id: doc_id }), key: "id", when: () => isDoc,
                      on: { renamed: "upsert:doc", deleted: "upsert:." } });
  const sync = live({ snapshot: "docs.state", params: () => ({ id: doc_id }), changes: "docs.watch", changes_params: () => ({ id: doc_id, session: SESSION }), key: "id", when: () => isDoc,
                      on: { updated: "upsert:.", squashed: "upsert:." } });
  const here = live({ snapshot: "docs.present", params: () => ({ id: doc_id }), changes: "docs.watch", changes_params: () => ({ id: doc_id }), key: "session", when: () => isDoc,
                      on: { present: "upsert:present", left: "remove:session" } });
  const dk = $derived(meta.rows.find((r) => r.id === doc_id && r.title != null));
  // The document as the surface reads it, whichever store: its title, status and body (a doc's body is its CRDT's).
  const t = $derived(isDoc ? (dk ? { id: dk.id, title: dk.title, status: null, body: null, kind: "doc", by: dk.by } : undefined) : tk);
  const present = $derived(isDoc ? here.rows.filter((p) => p.session && p.name) : []);

  // The CRDT, the awareness of carets on it, and what this page has sent and heard.
  const ydoc = isDoc ? new Y.Doc() : null;
  const ytext = ydoc?.getText("body") ?? null;
  const aware = ydoc ? new Awareness(ydoc) : null;
  const REMOTE = "remote";
  let synced = $state(false);
  let known = new Set(); // the seqs this copy holds: answered to its own updates, or heard
  let outbox = [], sendTimer = null, sending = false;
  function took(r) {
    if (!ydoc || r?.update == null) return;
    if (r.vector != null) {
      // A state: what this copy lacked; then what the service lacks of this copy, sent back.
      if (r.update) Y.applyUpdate(ydoc, fromB64(r.update), REMOTE);
      for (let n = 1; n <= (r.revision ?? 0); n++) known.add(n);
      const theirs = Y.encodeStateAsUpdate(ydoc, fromB64(r.vector));
      if (synced && theirs.length > 2) { outbox.push(theirs); sendSoon(); }
      synced = true;
      return;
    }
    if (r.update) Y.applyUpdate(ydoc, fromB64(r.update), REMOTE);
    if (r.seq != null) {
      // A seq this copy never held before this one: something went by unheard — ask for what is missing.
      if (r.seq > 1 && !known.has(r.seq - 1)) resync();
      known.add(r.seq);
    }
  }
  async function resync() {
    const r = await b.call("docs.state", { id: doc_id, vector: toB64(Y.encodeStateVector(ydoc)) });
    const st = r.events?.find((e) => e.type === "state");
    if (st) took(st);
  }
  function sendSoon() { if (!sendTimer) sendTimer = setTimeout(send, 50); }
  async function send() {
    sendTimer = null;
    if (sending || !outbox.length) return;
    const update = outbox.length === 1 ? outbox[0] : Y.mergeUpdates(outbox);
    outbox = [];
    sending = true;
    state = { kind: "saving", text: "saving…" };
    const r = await b.call("docs.update", { id: doc_id, update: toB64(update), session: SESSION });
    sending = false;
    const ok = r.events?.find((e) => e.type === "applied");
    if (r.ok && ok) { known.add(ok.revision); state = { kind: "saved", text: `saved ${hhmm()}` }; }
    else { outbox.unshift(update); state = { kind: "refused", text: said(r.ok ? { message: r.events?.[0]?.message ?? r.events?.[0]?.type } : r) }; }
    if (outbox.length) sendSoon();
  }
  if (ydoc) ydoc.on("update", (u, origin) => { if (origin !== REMOTE && synced) { outbox.push(u); sendSoon(); } });
  // Carets: this page's awareness said by `docs.here`; everyone else's applied from their presence.
  let hereTimer = null;
  const sayHere = () => { hereTimer = null; if (aware) b.call("docs.here", { id: doc_id, session: SESSION, client: ydoc.clientID, awareness: toB64(encodeAwarenessUpdate(aware, [ydoc.clientID])) }); };
  if (aware) aware.on("update", (_c, origin) => { if (origin !== REMOTE && !hereTimer) hereTimer = setTimeout(sayHere, 100); });
  let clients = new Map(); // session → its Yjs client id, to take its caret off when it leaves
  $effect(() => {
    const rows = present;
    untrack(() => {
      if (!aware) return;
      const now = new Map();
      for (const p of rows) {
        if (p.session === SESSION) { if (!aware.getLocalState()?.user) aware.setLocalStateField("user", { name: p.name, color: p.colour, colorLight: p.colour + "33" }); continue; }
        if (p.client != null) now.set(p.session, p.client);
        if (p.awareness) try { applyAwarenessUpdate(aware, fromB64(p.awareness), REMOTE); } catch { /* an old caret */ }
      }
      for (const [session, client] of clients) if (!now.has(session)) removeAwarenessStates(aware, [client], REMOTE);
      clients = now;
    });
  });
  $effect(() => {
    if (!isDoc) return;
    const r = sync.rows.find((x) => x.id === doc_id);
    if (r) untrack(() => took(r));
  });

  // The scrubber: the doc as it stood at any change kept.
  let replaying = $state(null); // { seqs, at, doc, said } while open
  async function openHistory() {
    if (replaying) { replaying = null; return; }
    const r = await b.call("docs.history", { id: doc_id, limit: 1000 });
    const seqs = (r.events ?? []).filter((e) => e.type === "change" && e.change.kind !== "delete").map((e) => e.change).reverse();
    replaying = { seqs, at: seqs.length - 1, doc: null, said: "" };
    await scrub(seqs.length - 1);
  }
  async function scrub(i) {
    if (!replaying) return;
    const c = replaying.seqs[i];
    replaying.at = i;
    if (!c) return;
    const r = await b.call("docs.replay", { id: doc_id, seq: c.seq });
    const d = r.events?.find((e) => e.type === "doc")?.doc;
    if (replaying && replaying.at === i) { replaying.doc = d ?? null; replaying.said = d ? "" : said(r); }
  }

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
    view.dispatch({ effects: media.fade() }); // a refused upload goes with the next key
    if (isDoc) return; // a doc's changes go as the CRDT makes them
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
    const r = isDoc ? await b.call("docs.rename", { id: doc_id, title }) : await b.call("tickets.amend", { id: doc_id, title });
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
    constructor(piece, defs, hand) { super(); this.piece = piece; this.text = piece.text; this.defs = defs; this.hand = hand; }
    eq(o) { return o.text === this.text && o.defs === this.defs && o.hand === this.hand && pieceClass(o.piece) === pieceClass(this.piece) && o.piece.wrap?.open === this.piece.wrap?.open && o.piece.wrap?.close === this.piece.wrap?.close; }
    toDOM(v) {
      const d = document.createElement("div");
      d.className = pieceClass(this.piece);
      d.setAttribute("data-block", "");
      if (this.piece.wrap) d.setAttribute("data-tag", this.piece.wrap.tag);
      if (this.hand) { d.classList.add("eb-hand"); d.setAttribute("data-hand", this.hand); }
      // ticket-view itself, drawing this piece alone: the drawing #/ticket/:id makes of it.
      d.view = mount(TicketView, { target: d, props: { body: this.text, piece: this.piece, defs: this.defs, block: true } });
      // EDITOR-10: a lone file or URL drawn as its kind — a player, a viewer, a card.
      drawAll(d, b);
      d.addEventListener("mousedown", (ev) => {
        if (ev.button !== 0) return;
        // A player, a viewer, a card's download: used where it is, the caret stays out.
        if (ev.target?.closest?.("[data-media-live] :is(video, audio, iframe, a[download]), [data-media-live] a.mx-src")) return;
        if (ev.target?.closest?.("button, input, select, textarea, summary")) return; // a control drawn in the block is its own (a code card's copy)
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
    /** A guess at the drawn height before it is measured (a wrapped line of about 90 characters is 22 px),
     *  so blocks CodeMirror has not drawn yet sit close to where they will. */
    get estimatedHeight() { return 10 + 22 * Math.max(1, Math.ceil(this.text.length / 90)); }
  }

  // ── EDITOR-9: the page stays put ─────────────────────────────────────────
  // Revealing a block (or hiding one) changes heights; a pin holds one position of the document at
  // the place on screen it had before the change: the clicked character for a click, else the edge
  // of the unit the caret lands in (its top moving down, its bottom moving up). The transaction is
  // sent without CodeMirror's own scroll into view; once the new layout is measured, the scroller is
  // moved by the difference, then just enough to keep the caret in sight.
  const Pin = Annotation.define();
  /** The element that scrolls the editor: the nearest ancestor that scrolls, else the page. */
  function scrollerOf(el) {
    for (let e = el?.parentElement; e; e = e.parentElement) {
      const o = getComputedStyle(e).overflowY;
      if ((o === "auto" || o === "scroll") && e.scrollHeight > e.clientHeight) return e;
    }
    return document.scrollingElement || document.documentElement;
  }
  /** The text of `dom` before the point (`node`, `offset`), what a reader sees: a diagram's labels aside. */
  function seenBefore(dom, node, offset) {
    const walk = document.createTreeWalker(dom, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => (n.parentElement?.closest?.(".diagram, [hidden]") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
    });
    let out = "";
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      if (n === node) return out + n.data.slice(0, offset);
      if (node.nodeType !== 3 && node.contains?.(n) && node.childNodes[offset] && (node.childNodes[offset] === n || node.childNodes[offset].contains(n))) return out;
      out += n.data;
    }
    return out;
  }
  /** The caret into the block drawn as `dom`, at the clicked character, that character held under the pointer. */
  function enter(v, dom, text, ev) {
    let base;
    try { base = v.posAtDOM(dom); } catch { return; }
    let off = 0, y = ev.clientY;
    const pt = document.caretPositionFromPoint ? document.caretPositionFromPoint(ev.clientX, ev.clientY)
      : document.caretRangeFromPoint ? ((r) => r && { offsetNode: r.startContainer, offset: r.startOffset })(document.caretRangeFromPoint(ev.clientX, ev.clientY)) : null;
    if (pt && dom.contains(pt.offsetNode) && !pt.offsetNode.parentElement?.closest?.(".diagram")) {
      off = sourceOffset(text, seenBefore(dom, pt.offsetNode, pt.offset));
      try {
        const r = document.createRange();
        r.setStart(pt.offsetNode, pt.offset);
        r.collapse(true);
        const box = r.getClientRects()[0] ?? r.getBoundingClientRect();
        if (box && box.height) y = box.top;
      } catch { /* the pointer's own height */ }
    }
    const pos = Math.min(base + off, v.state.doc.length);
    v.focus();
    v.dispatch({ selection: { anchor: pos }, effects: focusFx.of(true), scrollIntoView: false, annotations: Pin.of({ pos, side: 1, edge: "top", y }) });
  }
  /** What is shown as markdown in `state`: the units the caret is in (a key, to tell a change). */
  const shownKey = (state) => {
    if (!state.field(focused)) return "";
    const all = blocksOf(state.doc);
    const ranges = state.selection.ranges;
    return all.map((b, k) => { const end = k + 1 < all.length ? all[k + 1].from - 1 : state.doc.length; return ranges.some((r) => r.from <= end && r.to >= b.from) ? b.from : -1; }).filter((x) => x >= 0).join(",");
  };
  /** The unit holding `pos` in `state`: its `[from, to]`. */
  function unitAt(state, pos) {
    const all = blocksOf(state.doc);
    for (let k = 0; k < all.length; k++) {
      const end = k + 1 < all.length ? all[k + 1].from - 1 : state.doc.length;
      if (pos >= all[k].from && pos <= end) return [all[k].from, Math.max(all[k].from, all[k].to)];
    }
    return [pos, pos];
  }
  /** A pin for a move of the caret from `was` to `now` (no text changed): the edge of the unit it lands in. */
  function autoPin(v, state, was, now) {
    const [a, z] = unitAt(state, now);
    const down = now >= was;
    const top = v.documentTop;
    try {
      const block = v.lineBlockAt(down ? a : z);
      return down ? { pos: a, side: 1, edge: "top", y: top + block.top } : { pos: z, side: -1, edge: "bottom", y: top + block.bottom };
    } catch { return null; }
  }
  /** After the update: the pinned position back where it was, then the caret in sight. */
  function hold(v, pin) {
    v.requestMeasure({
      key: "editor-pin",
      read: (view) => {
        const c = view.coordsAtPos(pin.pos, pin.side);
        const caret = view.coordsAtPos(view.state.selection.main.head, 1);
        return { at: c ? (pin.edge === "bottom" ? c.bottom : c.top) : null, caret };
      },
      write: ({ at, caret }, view) => {
        const sc = scrollerOf(view.dom);
        const page = sc === document.scrollingElement || sc === document.documentElement;
        const by = (dy) => { if (Math.abs(dy) < 0.5) return; if (page) window.scrollBy(0, dy); else sc.scrollTop += dy; };
        if (at != null) by(at - pin.y);
        if (!caret) return;
        const shift = at != null ? at - pin.y : 0;
        const box = page ? { top: 0, bottom: window.innerHeight } : sc.getBoundingClientRect();
        const ct = caret.top - shift, cb = caret.bottom - shift;
        if (ct < box.top + 4) by(ct - box.top - 24);
        else if (cb > box.bottom - 4) by(cb - box.bottom + 24);
      },
    });
  }
  /** Every transaction: when it changes what is shown as markdown, sent pinned and without scrolling. */
  function dispatched(trs, v) {
    const last = trs[trs.length - 1];
    let pin = trs.map((t) => t.annotation(Pin)).find(Boolean) ?? null;
    if (!pin && !trs.some((t) => t.docChanged) && shownKey(v.state) !== shownKey(last.state)) {
      const was = v.state.selection.main.head, now = last.state.selection.main.head;
      pin = autoPin(v, last.state, was, last.state.field(focused) ? now : was);
    }
    if (pin && trs.some((t) => t.scrollIntoView) && !trs.some((t) => t.docChanged)) {
      // The same move, without CodeMirror's scroll into view: the pin decides where the page goes.
      const t = v.state.update({ selection: last.selection ?? v.state.selection, effects: trs.flatMap((x) => x.effects), annotations: [Pin.of(pin), ...(last.annotation(Transaction.userEvent) ? [Transaction.userEvent.of(last.annotation(Transaction.userEvent))] : [])], scrollIntoView: false });
      v.update([t]);
    } else v.update(trs);
    if (pin) hold(v, pin);
  }
  /** ↑ / ↓ across units: CodeMirror's vertical motion jumps a drawn block whole (to the next text it
   *  finds, the document's end when every block below is drawn), so a move that would leave the caret's
   *  unit goes into the adjacent one — its start going down, its last line going up. Inside a unit, and
   *  with shift held, CodeMirror's own. */
  function vertical(v, down) {
    const sel = v.state.selection.main;
    const [a, z] = unitAt(v.state, sel.head);
    const all = blocksOf(v.state.doc);
    const end = (() => { const k = all.findIndex((b) => b.from === a); return k >= 0 && k + 1 < all.length ? all[k + 1].from - 1 : v.state.doc.length; })();
    const moved = v.moveVertically(sel, down);
    if (moved.head >= a && moved.head <= end && moved.head !== sel.head) return false;
    const next = down ? all.find((b) => b.from > end) : [...all].reverse().find((b) => b.to < a);
    if (!next) return false;
    const to = down ? next.from : v.state.doc.lineAt(next.to).from;
    v.dispatch({ selection: { anchor: to }, scrollIntoView: true, userEvent: "select" });
    return true;
  }
  const BLOCKS = new WeakMap();
  /** `blocks()` of a document, kept per document (a document is immutable). */
  const blocksOf = (d) => { let b = BLOCKS.get(d); if (!b) { b = blocks(d.toString()); BLOCKS.set(d, b); } return b; };

  class Bullet extends WidgetType {
    toDOM() { const s = document.createElement("span"); s.className = "eb-bullet"; s.textContent = "•"; return s; }
    eq() { return true; }
    ignoreEvent() { return false; }
  }

  /** The decorations of `state`: each block the caret is not in drawn; in the caret's, the markdown formed. */
  function surface(state) {
    const all = blocksOf(state.doc);
    const defs = refDefs(state.doc.toString());
    const focus = state.field(focused);
    const ranges = state.selection.ranges;
    const hands = state.field(handed);
    const out = [];
    const shown = [];
    for (let k = 0; k < all.length; k++) {
      const blk = all[k];
      const end = k + 1 < all.length ? all[k + 1].from - 1 : state.doc.length;
      if (focus && ranges.some((r) => r.from <= end && r.to >= blk.from)) {
        shown.push([blk.from, end]);
        continue;
      }
      let hand = null;
      hands.between(blk.from, end, (_f, _t, d) => { hand = d.spec.attributes?.["data-hand"] ?? hand; });
      out.push(Decoration.replace({ widget: new Drawn(blk, defs, hand), block: true }).range(blk.from, end));
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
            // EDITOR-12: each line's leading whitespace, the hanging indent its wrapped rows keep on a phone.
            for (let l = first; ; l = lineOf(l.to + 1)) {
              let ind = 0;
              for (const ch of l.text) { if (ch === " ") ind++; else if (ch === "\t") ind += 4; else break; }
              out.push(Decoration.line({ class: "eb-fence", attributes: { style: `--ind: ${Math.min(ind, 8)}` } }).range(l.from));
              if (l.number >= last.number || l.to + 1 > state.doc.length) break;
            }
            out.push(Decoration.line({ class: "eb-fence-first" }).range(first.from));
            out.push(Decoration.line({ class: "eb-fence-last" }).range(last.from));
            for (const m of ref.node.getChildren("CodeMark")) mark(m.from, m.to, "eb-mk");
            for (const m of ref.node.getChildren("CodeInfo")) mark(m.from, m.to, "eb-info");
            const info = ref.node.getChild("CodeInfo"), body = ref.node.getChild("CodeText");
            if (info && body) for (const r of paintOf(state.doc.sliceString(info.from, info.to), state.doc.sliceString(body.from, body.to))) mark(body.from + r.from, body.from + r.to, r.cls);
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
    update(deco, tr) { return tr.docChanged || tr.selection || tr.effects.some((e) => e.is(focusFx) || e.is(handAdd) || e.is(handClear) || e.is(repaint)) ? surface(tr.state) : deco; },
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

  // ── EDITOR-5, EDITOR-10: anything from outside ───────────────────────────
  // A file of any kind and size, a URL alone, pasted HTML, an image from another tab: media-ingest's,
  // kept for the doc (V2-29) or the ticket this surface writes.
  const media = ingest({ bus: b, owner: isDoc ? { doc: doc_id } : { ticket: doc_id } });

  // ── EDITOR-6: write in at the owner's request ────────────────────────────
  // A selection and one line become a question on the ticket, queued now. An outside amend while a
  // request is open marks the lines it changed (a drawn block holding one, the block) with whose hand
  // it was, until a click accepts them (which closes the requests) or an edit takes the mark off.
  let requests = $state([]);
  let hand = $state(null);
  let hasSelection = $state(false);
  let asking = $state(null); // { a, b, text, top } while the line is open
  let askTo = $state(""); // V2-30: a doc's question to anyone (""), or to an agent ("agent")
  let marginTick = $state(0); // V2-30: bumped when the text or its layout moved, for the margin
  let marginFrame = null;
  const moved = () => { if (!marginFrame) marginFrame = requestAnimationFrame(() => { marginFrame = null; marginTick++; }); };
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
    asking = { a: d.lineAt(sel.from).number, b: d.lineAt(sel.to).number, from: sel.from, to: sel.to, text: view.state.sliceDoc(sel.from, sel.to), top: (coords?.bottom ?? box.top) - box.top + 4 };
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
    if (isDoc) {
      // V2-30: kept on the doc's range, in characters of its text.
      const chars = (pos) => [...view.state.sliceDoc(0, pos)].length;
      const r = await b.call("docs.ask", { id: doc_id, from: chars(asking.from), to: chars(asking.to), text: what, to_whom: askTo || undefined });
      if (r.ok && r.events?.some((e) => e.type === "asked")) { asking = null; view?.focus(); }
      else askRefused = said(r.ok ? { message: r.events?.[0]?.message ?? r.events?.[0]?.type } : r);
      return;
    }
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
    acked = isDoc ? null : tk.body;
    view = new EditorView({
      parent: host,
      dispatchTransactions: dispatched,
      state: EditorState.create({
        doc: isDoc ? ytext.toString() : tk.body,
        extensions: [
          isDoc ? yCollab(ytext, aware, { undoManager: new Y.UndoManager(ytext) }) : history(),
          isDoc ? keymap.of(yUndoManagerKeymap) : [],
          isDoc ? threadMarks : [],
          keymap.of([
            { key: "Mod-s", run: () => { flush(); return true; } },
            { key: "Mod-Enter", run: openAsk },
            { key: "Mod-b", run: wrapWith("**") },
            { key: "Mod-i", run: wrapWith("*") },
            { key: "Mod-k", run: linkKey },
            { key: "ArrowDown", run: (v) => vertical(v, true) },
            { key: "ArrowUp", run: (v) => vertical(v, false) },
            { key: "Escape", run: (v) => { v.contentDOM.blur(); return true; } },
            ...markdownKeymap, indentWithTab, ...defaultKeymap, ...(isDoc ? [] : historyKeymap),
          ]),
          markdownSupport({ base: markdownLanguage }),
          EditorView.lineWrapping,
          focused,
          handed,
          drawn,
          media.extension,
          look,
          placeholder("Write here — # a heading, - a list, **bold**; it saves as you type"),
          EditorView.updateListener.of((u) => {
            if (u.selectionSet) hasSelection = !u.state.selection.main.empty;
            if (u.focusChanged && u.view.hasFocus !== u.state.field(focused)) queueMicrotask(() => view?.dispatch({ effects: focusFx.of(view.hasFocus) }));
            if (u.docChanged && !u.transactions.some((tr) => tr.annotation(Outside))) edited();
            if (isDoc && (u.docChanged || u.geometryChanged || u.viewportChanged)) moved();
          }),
          EditorView.domEventHandlers({
            blur: () => { if (dirty || pending !== null) flush(); return false; },
            click: (ev) => { if (ev.target?.closest?.(".eb-hand")) accept(); return false; },
          }),
          EditorView.contentAttributes.of({ "data-field": "body-source", "aria-label": "the body" }),
        ],
      }),
    });
    host.editorView = view; // the page tests read the document and the caret through it
    if (isDoc) moved();
  }
  /** Whether the surface can be drawn: the document read — and, for a doc, its CRDT synced. */
  const ready = () => t && (!isDoc || synced);
  // EDITOR-9: a fence's source painted as its drawn card paints it: the language's module (code-paint)
  // keeps its painting on globalThis.mengerCodeLanguages; when one arrives, the surface is drawn again.
  const repaint = StateEffect.define();
  const painted = new Map(); // `${lang}\n${text}` → ranges, the last few
  function paintOf(info, text) {
    const lang = langOf(info);
    const kept = lang && globalThis.mengerCodeLanguages?.get(lang);
    if (!kept || text.length > 20000) return [];
    const key = `${lang}\n${text}`;
    let r = painted.get(key);
    if (!r) { try { r = kept.paint(text); } catch { r = []; } painted.set(key, r); if (painted.size > 64) painted.delete(painted.keys().next().value); }
    return r;
  }
  $effect(() => {
    const arrived = () => view?.dispatch({ effects: repaint.of(null) });
    addEventListener("menger:code-language", arrived);
    return () => removeEventListener("menger:code-language", arrived);
  });

  function bodyMount(node) {
    host = node;
    if (ready() && !view) init(t);
    return { destroy() { host = null; } };
  }

  $effect(() => {
    if (!t || (isDoc && !synced) || view || !host) return;
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
    if (titleTimer) { clearTimeout(titleTimer); if (titleDirty && titleDraft.trim()) { if (isDoc) b.call("docs.rename", { id: doc_id, title: titleDraft.trim() }); else b.call("tickets.amend", { id: doc_id, title: titleDraft.trim() }); } }
    if (isDoc) {
      // What this page typed and has not sent goes now, to its own doc; its caret goes with it.
      if (sendTimer) clearTimeout(sendTimer);
      if (outbox.length) b.call("docs.update", { id: doc_id, update: toB64(Y.mergeUpdates(outbox)), session: SESSION });
      if (hereTimer) clearTimeout(hereTimer);
      aware?.destroy();
      if (marginFrame) cancelAnimationFrame(marginFrame);
    }
    if (view) { if (!isDoc && dirty && pending === null) { const text = doc(); if (text !== acked) b.call("tickets.amend", { id: doc_id, body: text }); } view.destroy(); view = null; }
    ydoc?.destroy();
  });
</script>

<section class="editor-body" data-surface="editor-body" data-ticket={doc_id} data-kind={isDoc ? "doc" : "ticket"} bind:this={wrap}>
  {#if isDoc && present.length}
    <p class="here" data-field="present" aria-label="who is here">
      {#each present as p (p.session)}<span class="who" data-present={p.session} data-me={p.session === SESSION ? "" : undefined} data-agent={p.agent ? "" : undefined}><i class="dot" style={`background-color: ${p.colour}`} aria-hidden="true"></i>{p.name}{p.session === SESSION ? " (you)" : ""}</span>{/each}
    </p>
  {/if}
  {#if replaying}
    <div class="replay" data-field="replay">
      <input class="scrub" type="range" min="0" max={Math.max(0, replaying.seqs.length - 1)} value={replaying.at} aria-label="the document's changes" data-field="scrubber" oninput={(ev) => scrub(Number(ev.currentTarget.value))}/>
      <span class="when" data-field="replay-at">{#if replaying.seqs[replaying.at]}change {replaying.seqs[replaying.at].seq} of {replaying.seqs[replaying.seqs.length - 1].seq} · {replaying.seqs[replaying.at].kind} · {replaying.seqs[replaying.at].by} · {new Date(replaying.seqs[replaying.at].at).toLocaleString()}{/if}</span>
      <button type="button" class="pick" data-action="live" onclick={() => (replaying = null)}>back to live</button>
      {#if replaying.said}<span class="no" data-refused>{replaying.said}</span>{/if}
      {#if replaying.doc}<div class="then" data-field="replayed"><TicketView body={replaying.doc.body} title={replaying.doc.title}/></div>{/if}
    </div>
  {/if}
  {#if sheet === "loading" || (!t && (isDoc ? meta.status === "pending" : ticket.status === "pending")) || (isDoc && t && !synced)}
    <div class="skel" data-loading aria-busy="true"><i class="bone title"></i><i class="bone"></i><i class="bone half"></i></div>
  {:else if !t}
    <p class="empty" data-empty data-not-found>No {isDoc ? "document" : "ticket"} {doc_id} here{(isDoc ? meta.message : ticket.message) ? ` — ${isDoc ? meta.message : ticket.message}` : ""}.</p>
  {:else}
    <TicketView status={t.status} edit={bodyMount} titleEdit={titleMount}/>
  {/if}
  {#if isDoc && t && synced}<Margin id={doc_id} {ydoc} view={() => view} tick={marginTick}/>{/if}
  {#if asking}
    <form class="ask" data-ask data-state="open" style={`top: ${asking.top}px`} onsubmit={(ev) => { ev.preventDefault(); sendAsk(); }}>
      <span class="where">{asking.a === asking.b ? `line ${asking.a}` : `lines ${asking.a}–${asking.b}`}</span>
      <input class="say" data-field="instruction" aria-label={isDoc ? "your question" : "what an agent should do"} placeholder={isDoc ? "ask a question about it…" : "what should an agent do with it?"} autocomplete="off" bind:this={askBox} bind:value={instruction}/>
      {#if isDoc}<select class="pick" data-field="to-whom" aria-label="asked of" bind:value={askTo}><option value="">anyone</option><option value="agent">an agent</option></select>{/if}
      <button class="pick" data-action="ask" disabled={!instruction.trim()}>{isDoc ? "ask" : "ask an agent"}</button>
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
      <button type="button" class="pick" data-action="ask-open" disabled={!hasSelection} onmousedown={(ev) => ev.preventDefault()} onclick={openAsk}>{isDoc ? "ask" : "ask an agent"} <kbd>ctrl</kbd><kbd>enter</kbd></button>
      <button type="button" class="pick image" data-action="image" onmousedown={(ev) => ev.preventDefault()} onclick={() => picker?.click()}>image</button>
      <input class="file" type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple aria-label="an image" bind:this={picker} onchange={picked}/>
      {#if isDoc}<button type="button" class="pick" data-action="history" aria-pressed={!!replaying} onmousedown={(ev) => ev.preventDefault()} onclick={openHistory}>history</button>
      {:else}<a class="revs" href={"#/ticket/" + doc_id} data-action="revisions">revisions</a>{/if}
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
  .editor-body :global(.dm-range) { background: var(--hover); text-decoration: underline dotted var(--accent); text-underline-offset: 3px; }
  .here { display: flex; flex-wrap: wrap; gap: 10px; margin: 0 0 10px; font-size: 11px; color: var(--muted); }
  .who { display: inline-flex; gap: 5px; align-items: center; }
  .who .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--muted); }
  .who[data-agent] .dot { border-radius: var(--radius-xs); }
  .replay { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin: 0 0 12px; padding: 8px 10px; font-size: 11px; color: var(--muted); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .scrub { flex: 1 1 200px; min-width: 0; accent-color: var(--accent); }
  .when { flex-basis: 100%; order: 3; }
  .then { flex-basis: 100%; order: 4; padding-top: 8px; border-top: var(--border) solid var(--line); }
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
  .editor-body :global(.cm-editor) { font: inherit; max-width: 100%; overflow-anchor: none; }
  .editor-body :global(.cm-scroller) { min-width: 0; }
  .editor-body :global(.cm-content) { flex: 1 1 0; min-width: 0; max-width: 100%; }
  .editor-body :global(.blk) { min-width: 0; max-width: 100%; }
  .editor-body :global(.cm-content ::selection) { background: var(--row-sel); }
  .editor-body :global(.blk) { cursor: text; }
  /* EDITOR-9: the caret's line shows its markdown and nothing more: no frame, tint or highlight. */
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
  .editor-body :global(.eb-quote) { padding-left: 14px; color: var(--muted); border-left: 2px solid var(--line); }
  .editor-body :global(.eb-fence) { padding: 0 12px; font-family: var(--font-text); font-size: 12.5px; white-space: pre; background: var(--panel); }
  .editor-body :global(.eb-fence .tok-keyword), .editor-body :global(.eb-fence .tok-atom), .editor-body :global(.eb-fence .tok-bool) { color: var(--code-keyword); }
  .editor-body :global(.eb-fence .tok-string), .editor-body :global(.eb-fence .tok-string2), .editor-body :global(.eb-fence .tok-literal) { color: var(--code-string); }
  .editor-body :global(.eb-fence .tok-number) { color: var(--code-number); }
  .editor-body :global(.eb-fence .tok-comment) { color: var(--code-comment); font-style: italic; }
  .editor-body :global(.eb-fence .tok-typeName), .editor-body :global(.eb-fence .tok-className), .editor-body :global(.eb-fence .tok-namespace) { color: var(--code-type); }
  .editor-body :global(.eb-fence .tok-definition) { color: var(--code-def); }
  .editor-body :global(.eb-fence .tok-propertyName) { color: var(--code-property); }
  .editor-body :global(.eb-fence .tok-meta), .editor-body :global(.eb-fence .tok-macroName) { color: var(--code-meta); }
  .editor-body :global(.eb-fence .tok-operator), .editor-body :global(.eb-fence .tok-punctuation) { color: var(--muted); }
  .editor-body :global(.eb-fence-first) { padding-top: 10px; border-radius: var(--radius) var(--radius) 0 0; }
  .editor-body :global(.eb-fence-last) { padding-bottom: 10px; border-radius: 0 0 var(--radius) var(--radius); }
  .editor-body :global(.eb-html), .editor-body :global(.eb-table), .editor-body :global(.eb-tag) { font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .editor-body :global(.eb-upload) { display: block; margin: 4px 0; padding: 6px 10px; font-family: var(--font-text); font-size: 11px; color: var(--muted); background: var(--card); border: var(--border) dashed var(--line); border-radius: var(--radius-lg); }
  .editor-body :global(.eb-upload[data-state=refused]) { color: var(--warn); border-color: var(--warn); }
  .editor-body :global(.eb-hand) { cursor: pointer; background: var(--sel); box-shadow: inset 2px 0 0 var(--accent); }
  .editor-body :global(.eb-hand::after) { content: attr(data-hand); float: right; padding-left: 8px; font-family: var(--font-text); font-size: 10px; color: var(--muted); }
  /* EDITOR-12: on a phone (under 641 px) the surface never scrolls sideways: the caret's fence wraps, its rows
     hung past each line's own indent (code-card's rule); every line breaks a long word; who is here wraps. */
  @media (max-width: 640px) {
    .editor-body :global(.cm-content) { overflow-wrap: anywhere; word-break: normal; }
    .editor-body :global(.eb-fence) { white-space: pre-wrap; overflow-wrap: anywhere; tab-size: 4; padding-left: calc(12px + (var(--ind, 0) + 2) * 1ch); text-indent: calc((var(--ind, 0) + 2) * -1ch); }
    .who, .when, .asked, .state { overflow-wrap: anywhere; min-width: 0; }
    .say { min-width: 0; flex-basis: 100%; }
    .pick[data-action=ask-open] { margin-left: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .state[data-state=saving]::before, .skel .bone { animation: none; }
  }
</style>
