<!-- V2-30: the margin of a note on the writing surface (svelte/1, a sibling
     editor-body imports for a note alone). Each question asked on a range of
     the note (`notes.ask`) is a card beside its text, at the height the range
     stands, the range underlined faintly in the text (`threadMarks`, a
     CodeMirror field editor-body adds); its answers below it, a box to
     answer (`notes.reply`: words, links, an image pasted goes through
     `media.put { note }`), and resolve (`notes.resolve`), which folds it — the
     margin then counts the resolved, a click shows them. The ranges are the
     note's CRDT's own: each thread's anchor is two Yjs relative positions,
     resolved here in the page's copy (`Y.createAbsolutePositionFromRelativePosition`),
     so the cards and the underlines follow the text through every hand's
     edit as it happens. A thread whose text was deleted whole is detached
     and listed at the end. Live by `notes.threads` and `notes.watch` (`asked`,
     `replied`, `resolved`). Under 1400 px the margin is a list under the
     text. NOTES-4: in a note's column beside its slot (narrower than 1100
     px, the container `note-col`) the margin is a list under the text at
     every width, each card quoting the words it is about, in the text's
     order; flat there, as the page is.
     NOTES-5: and the marks an agent leaves on the note (`notes.mark`): each
     range underlined where it stands (in `--accent` when it carries a link,
     which a click opens), its line of state in the mono face under the block
     that holds it with who left it, and a dot in that block's margin. The
     ranges are anchored as a question's are, resolved here the same way and
     set whole on the surface (`noteMarks`, a field editor-body adds beside
     `threadMarks`): in the caret's block the field's own decorations
     underline them; a drawn block is handed the marks it holds and draws
     them in its drawing (`drawMarks`). A mark whose words were deleted is
     detached and drawn nowhere. Live by `notes.marks` and `notes.watch`
     (`mark_changed`, `mark_removed`). -->
<script module>
  import { StateEffect, StateField } from "@codemirror/state";
  import { Decoration, EditorView } from "@codemirror/view";
  import { ticketHref } from "./ticket-links.svelte";

  export const uses = ["notes.threads", "notes.watch", "notes.reply", "notes.resolve", "media.put", "notes.marks"];

  /** NOTES-5: the agents' marks on the surface — an underline each (`spec.m`: { mark, link, label, by }),
   *  set whole by the margin, mapped through every change between. */
  export const setNoteMarks = StateEffect.define();
  export const noteMarks = StateField.define({
    create: () => Decoration.none,
    update(deco, tr) {
      deco = deco.map(tr.changes);
      for (const e of tr.effects) if (e.is(setNoteMarks)) deco = e.value;
      return deco;
    },
    provide: (f) => EditorView.decorations.from(f),
  });
  /** The marks of `state` touching `from..to`, each `{ mark, link, label, by, a, z }` (`a`, `z` from `from`). */
  export function marksIn(state, from, to) {
    const set = state.field(noteMarks, false);
    const out = [];
    set?.between(from, to, (f, t, d) => { if (t > from && f < to) out.push({ ...d.spec.m, a: Math.max(f, from) - from, z: Math.min(t, to) - from }); });
    return out;
  }
  /** Where a mark's link goes: a ticket's page, an address of the app, or an https address (`external`). */
  export function markHref(link) {
    if (/^https:\/\//i.test(link)) return { href: link, external: true };
    if (link.startsWith("#/")) return { href: link, external: false };
    return { href: ticketHref({ id: link.replace(/^[^:]*:/, "") }), external: false };
  }
  /** Follows a mark's link: the app's own in place, an https address in a new tab. */
  export function followMark(b, link) {
    const to = markHref(link);
    if (to.external) window.open(to.href, "_blank", "noopener");
    else b.navigate(to.href);
  }
  /** Markdown as a reader sees it, near enough to find a range's words in a drawing: links and images by their
   *  words, emphasis and code marks gone, a heading's, a list's and a quote's marks gone, white space one space. */
  const seen = (md) => md.replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1").replace(/[*_~`]+/g, "").replace(/^\s{0,3}#{1,6}\s+/gm, "").replace(/^\s*(?:[-*+]|\d+[.)])\s+/gm, "").replace(/^\s*>\s?/gm, "").replace(/\s+/g, " ");
  /** The text nodes of `el` a reader sees, as one string with white space runs one space, and where each of its characters is. */
  function readOf(el) {
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, { acceptNode: (n) => (n.parentElement?.closest?.("[data-mark-state], .diagram, [hidden], style, script") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT) });
    let text = "";
    const at = [];
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      for (let i = 0; i < n.data.length; i++) {
        const space = /\s/.test(n.data[i]);
        if (space && text.endsWith(" ")) continue;
        text += space ? " " : n.data[i];
        at.push([n, i]);
      }
    }
    return { text, at };
  }
  /** Each piece of `s..e` of a reading, wrapped in a span `make` makes, text node by text node. */
  function wrap(read, s, e, make) {
    const runs = [];
    for (let k = s; k < e; k++) {
      const [n, i] = read.at[k];
      const last = runs[runs.length - 1];
      if (last && last.n === n) last.j = i + 1; else runs.push({ n, i, j: i + 1 });
    }
    const out = [];
    for (const r of runs.reverse()) {
      const range = document.createRange();
      range.setStart(r.n, r.i);
      range.setEnd(r.n, r.j);
      const span = make();
      range.surroundContents(span);
      out.push(span);
    }
    return out;
  }
  /** NOTES-5: the marks a drawn block holds (`marks`: `marksIn` of it), drawn in its drawing `el` of `text`
   *  (the block's markdown): each range's words underlined — found as a reader sees them, at the same
   *  occurrence the markdown holds them — linked ones in the accent and opened by `follow(link)` on a
   *  click or enter; the block's dot; under it each mark's line of state, `¶n` its place among the blocks.
   *  Drawn again whole on every call. */
  export function drawMarks(el, text, marks, n, follow) {
    for (const old of el.querySelectorAll(":scope > [data-mark-state]")) old.remove();
    for (const u of el.querySelectorAll("[data-mark]")) u.replaceWith(...u.childNodes);
    el.normalize();
    if (!marks.length) { el.removeAttribute("data-marked"); return; }
    el.setAttribute("data-marked", "");
    for (const m of marks) {
      const words = seen(text.slice(m.a, m.z)).trim();
      if (!words) continue;
      const read = readOf(el);
      const before = seen(text.slice(0, m.a));
      let nth = 0;
      for (let k = before.indexOf(words); k >= 0; k = before.indexOf(words, k + words.length)) nth++;
      let at = -1;
      for (let k = 0; k <= nth; k++) { at = read.text.indexOf(words, at + 1); if (at < 0) break; }
      if (at < 0) at = read.text.indexOf(words);
      if (at < 0) continue;
      wrap(read, at, at + words.length, () => {
        const u = document.createElement("span");
        u.className = m.link ? "nm-u nm-linked" : "nm-u";
        u.dataset.mark = m.mark;
        if (m.link) {
          u.dataset.markLink = m.link;
          u.setAttribute("role", "link");
          u.tabIndex = 0;
          u.title = m.link;
          u.addEventListener("click", (ev) => { ev.preventDefault(); ev.stopPropagation(); follow(m.link); });
          u.addEventListener("keydown", (ev) => { if (ev.key === "Enter") { ev.preventDefault(); ev.stopPropagation(); follow(m.link); } });
        }
        return u;
      });
    }
    for (const m of marks.filter((x) => x.label)) el.append(stateLine(m, n));
  }
  /** A mark's line of state, `¶n · who · label`. */
  export function stateLine(m, n) {
    const p = document.createElement("div");
    p.className = "nm-state";
    p.dataset.markState = m.mark;
    p.textContent = `¶${n} · ${m.by} · ${m.label}`;
    return p;
  }

  /** The underlines of the open threads' ranges: set whole by the margin, mapped through every change between. */
  export const setThreadMarks = StateEffect.define();
  export const threadMarks = StateField.define({
    create: () => Decoration.none,
    update(deco, tr) {
      deco = deco.map(tr.changes);
      for (const e of tr.effects) if (e.is(setThreadMarks)) deco = e.value;
      return deco;
    },
    provide: (f) => EditorView.decorations.from(f),
  });
</script>

<script>
  import { tick as settled, untrack } from "svelte";
  import { bus, live, markdown } from "plexus-ui/host";
  import * as Y from "yjs";
  import { mediaLinks } from "./ticket-view.svelte";
  // EDITOR-20: a ticket id in a question or an answer is the same pill a body draws (ticket-pills).
  import PillTable, { drawPills } from "./ticket-pills.svelte";

  let { id, ydoc = null, view = () => null, tick = 0 } = $props();
  // Beside a surface (editor-body hands its view and the note's CRDT) the cards stand at their ranges;
  // alone (the sheet) they are a list, each range as the service resolved it.
  const placedBeside = untrack(() => ydoc != null);
  const b = bus();
  const doc_id = untrack(() => id);
  const threads = live({ snapshot: "notes.threads", params: () => ({ id: doc_id }), changes: "notes.watch", changes_params: () => ({ id: doc_id }), key: "thread",
                         on: { asked: "upsert:thread", replied: "upsert:thread", resolved: "upsert:thread" } });
  const fromB64 = (t) => Uint8Array.from(atob(t), (c) => c.charCodeAt(0));
  /** Where a relative position stands in the page's copy now (a UTF-16 index), or null. */
  function at(rel) {
    if (!ydoc) return null;
    try { return Y.createAbsolutePositionFromRelativePosition(Y.decodeRelativePosition(fromB64(rel)), ydoc)?.index ?? null; } catch { return null; }
  }
  let showResolved = $state(false);
  let wrap = $state(null);
  /** Every thread with its range in this copy — re-read on every change of the text (`tick`). */
  const placed = $derived.by(() => {
    void tick;
    return threads.rows.filter((t) => t.thread && t.anchor).map((t) => {
      const from = ydoc ? at(t.anchor.from) : t.from ?? null, to = ydoc ? at(t.anchor.to) : t.to ?? null;
      const detached = from == null || to == null || (to <= from && t.quote !== "");
      return { ...t, a: detached ? null : from, z: detached ? null : to, gone: detached };
    }).sort((x, y) => (x.gone - y.gone) || ((x.a ?? 0) - (y.a ?? 0)) || (x.at - y.at));
  });
  // NOTES-5: the marks, each where its anchor stands in this copy now; a detached one is drawn nowhere.
  const marks = live({ snapshot: "notes.marks", params: () => ({ id: doc_id }), changes: "notes.watch", changes_params: () => ({ id: doc_id }), key: "mark",
                       on: { mark_changed: "upsert:mark", mark_removed: "remove:mark" } });
  const marked = $derived.by(() => {
    void tick;
    return marks.rows.filter((m) => m.mark && m.anchor).map((m) => {
      const a = ydoc ? at(m.anchor.from) : m.from ?? null, z = ydoc ? at(m.anchor.to) : m.to ?? null;
      return { mark: m.mark, link: m.link ?? null, label: m.label ?? null, by: m.by, a, z };
    }).filter((m) => m.a != null && m.z != null && m.z > m.a);
  });
  $effect(() => {
    const ranges = marked.slice().sort((x, y) => (x.a - y.a) || (x.z - y.z));
    untrack(() => {
      const v = view();
      if (!v) return;
      const len = v.state.doc.length;
      const keyOf = (xs) => JSON.stringify(xs.map((m) => [m.a, m.z, m.mark, m.link, m.label, m.by]));
      const held = [];
      v.state.field(noteMarks, false)?.between(0, len, (a, z, d) => { held.push({ a, z, ...d.spec.m }); });
      if (keyOf(held) === keyOf(ranges.filter((m) => m.z <= len))) return; // the surface holds them already
      const deco = ranges.filter((m) => m.z <= len).map((m) => Decoration.mark({ class: m.link ? "nm-u nm-linked" : "nm-u", attributes: { "data-mark": m.mark, ...(m.link ? { "data-mark-link": m.link, title: m.link } : {}) },
                                                                      m: { mark: m.mark, link: m.link, label: m.label, by: m.by } }).range(m.a, m.z));
      v.dispatch({ effects: setNoteMarks.of(Decoration.set(deco, true)) });
    });
  });
  const open = $derived(placed.filter((t) => !t.resolved));
  const resolved = $derived(placed.filter((t) => t.resolved));
  const shown = $derived(showResolved ? placed : open);

  // The underlines, set whole whenever the threads or the text move.
  $effect(() => {
    const ranges = open.filter((t) => !t.gone && t.z > t.a);
    untrack(() => {
      const v = view();
      if (!v) return;
      const len = v.state.doc.length;
      const marks = ranges.filter((t) => t.z <= len).map((t) => Decoration.mark({ class: "dm-range", attributes: { "data-thread": t.thread } }).range(t.a, t.z));
      v.dispatch({ effects: setThreadMarks.of(Decoration.set(marks, true)) });
    });
  });

  // Each card at the height of its range, pushed down past the one above it.
  $effect(() => {
    void shown; void tick;
    untrack(() => settled().then(() => {
      const v = view();
      if (!wrap || !v) return;
      const wide = getComputedStyle(wrap).position === "absolute";
      const base = wrap.getBoundingClientRect().top;
      let floor = 0;
      for (const card of wrap.querySelectorAll("[data-thread]")) {
        const t = shown.find((x) => x.thread === card.dataset.thread);
        if (!wide || !t) { card.style.top = ""; continue; }
        const c = t.gone ? null : v.coordsAtPos(Math.min(t.a, v.state.doc.length));
        const want = c ? c.top - base : floor;
        const top = Math.max(want, floor);
        card.style.top = `${top}px`;
        floor = top + card.offsetHeight + 8;
      }
      wrap.style.minHeight = wide ? `${floor}px` : "";
    }));
  });

  let drafts = $state({});
  let refused = $state({});
  async function reply(t) {
    const body = (drafts[t.thread] ?? "").trim();
    if (!body) return;
    const r = await b.call("notes.reply", { id: doc_id, thread: t.thread, body });
    if (r.ok && r.events?.some((e) => e.type === "replied")) { drafts[t.thread] = ""; refused[t.thread] = ""; }
    else refused[t.thread] = String(r.message ?? r.events?.[0]?.message ?? r.events?.[0]?.type ?? "refused");
  }
  async function resolve(t, again) {
    const r = await b.call("notes.resolve", { id: doc_id, thread: t.thread, open: again || undefined });
    if (!r.ok) refused[t.thread] = String(r.message ?? "refused");
  }
  /** An image pasted into an answer goes to the note's media, and its link into the answer. */
  async function pasted(ev, t) {
    const file = [...(ev.clipboardData?.files ?? [])][0];
    if (!file) return;
    ev.preventDefault();
    const data = await new Promise((done) => { const r = new FileReader(); r.onload = () => done(String(r.result).replace(/^data:[^,]*,/, "")); r.readAsDataURL(file); });
    const r = await b.call("media.put", { note: doc_id, data, name: file.name || "image", mime: file.type || undefined });
    const stored = r.events?.find((e) => e.type === "stored");
    if (!stored) { refused[t.thread] = String(r.message ?? r.events?.[0]?.message ?? "refused"); return; }
    const name = (file.name || "image").replace(/\.[a-z0-9]+$/i, "").replace(/[[\]\n]/g, " ");
    drafts[t.thread] = `${drafts[t.thread] ?? ""}${file.type.startsWith("image/") ? "!" : ""}[${name}](media:${stored.hash})`;
  }
  // EDITOR-13: ctrl/⌘+enter in an answer sends it — the engine's chord, heard in the reply box it was pressed in.
  const replyKey = (ev) => { const t = shown.find((x) => String(x.thread) === ev?.target?.data?.["reply-to"]); if (t) reply(t); };
  b.keys({ "ctrl+enter": { to: replyKey, label: "send the answer" }, "meta+enter": { to: replyKey, label: "send the answer" } });
  const when = (ms) => new Date(ms).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
</script>

<PillTable/><aside class="note-margin" data-field="margin" data-beside={placedBeside ? "" : undefined} aria-label="questions on the text" bind:this={wrap}>
  {#each shown as t (t.thread)}
    <article class="card" data-thread={t.thread} data-state={t.resolved ? "resolved" : t.gone ? "detached" : "open"}>
      <p class="quote" title={t.quote}>{#if t.gone}<span class="tag" data-detached>detached</span>{/if}«{t.quote.length > 80 ? t.quote.slice(0, 80) + "…" : t.quote}»</p>
      <div class="q"><b>{t.by}</b>{#if t.to_whom} → <b data-field="to-whom">{t.to_whom}</b>{/if} <span class="muted">{when(t.at)}</span><div class="md" use:drawPills={t.question}>{@html markdown(mediaLinks(t.question))}</div></div>
      {#each t.replies ?? [] as r (r.n)}
        <div class="r" data-reply={r.n}><b>{r.by}</b> <span class="muted">{when(r.at)}</span><div class="md" use:drawPills={r.body}>{@html markdown(mediaLinks(r.body))}</div></div>
      {/each}
      {#if t.resolved}
        <p class="muted done">resolved by {t.resolved.by} · <button type="button" class="link" data-action="reopen" onclick={() => resolve(t, true)}>open again</button></p>
      {:else}
        <textarea class="say" data-field="reply" rows="2" placeholder="answer… (an image pastes)" aria-label="an answer" bind:value={drafts[t.thread]}
                  data-reply-to={t.thread} onpaste={(ev) => pasted(ev, t)}></textarea>
        <p class="acts">
          <button type="button" class="pick" data-action="reply" disabled={!(drafts[t.thread] ?? "").trim()} onclick={() => reply(t)}>answer</button>
          <button type="button" class="pick" data-action="resolve" onclick={() => resolve(t, false)}>resolve</button>
        </p>
      {/if}
      {#if refused[t.thread]}<p class="no" data-refused>{refused[t.thread]}</p>{/if}
    </article>
  {/each}
  {#if resolved.length}
    <button type="button" class="count" data-field="resolved-count" aria-pressed={showResolved} onclick={() => (showResolved = !showResolved)}>{showResolved ? "hide" : "show"} {resolved.length} resolved</button>
  {/if}
</aside>

<style>
  .note-margin { font-size: 12px; color: var(--ink); }
  .note-margin[data-beside] { position: absolute; top: 0; left: calc(100% + 16px); width: 220px; }
  .note-margin[data-beside] .card { position: absolute; margin: 0; left: 0; right: 0; }
  .card { margin-bottom: 8px; padding: 8px 10px; background: var(--card); border: var(--border) solid var(--line); border-left: 3px solid var(--accent); border-radius: var(--radius); box-shadow: 0 2px 8px var(--shadow); }
  .card[data-state=resolved] { border-left-color: var(--line); opacity: .75; }
  .card[data-state=detached] { border-left-color: var(--warn); }
  .quote { margin: 0 0 4px; overflow: hidden; color: var(--muted); font-style: italic; white-space: nowrap; text-overflow: ellipsis; }
  .tag { margin-right: 4px; padding: 0 4px; font-style: normal; color: var(--warn); border: var(--border) solid var(--warn); border-radius: var(--radius-sm); }
  .q, .r { margin: 4px 0; }
  .r { padding-left: 8px; border-left: 2px solid var(--line); }
  .md :global(p) { margin: 2px 0; }
  .md :global(img) { max-width: 100%; border-radius: var(--radius-sm); }
  .md :global(a:not(.tpill)) { color: var(--accent); }
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
  .muted { color: var(--muted); font-size: 11px; }
  .done { margin: 6px 0 0; }
  .say { box-sizing: border-box; width: 100%; margin-top: 6px; padding: 4px 6px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius-sm); resize: vertical; }
  .say:focus-visible, .say:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .acts { display: flex; gap: 6px; margin: 4px 0 0; }
  .pick { padding: 0 8px; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .pick:hover, .pick:global([data-sheet~=hover]) { background: var(--hover); }
  .pick:focus-visible, .pick:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .pick:disabled, .pick:global([data-sheet~=disabled]) { color: var(--disabled); cursor: not-allowed; }
  .link { padding: 0; font: inherit; color: var(--accent); background: none; border: 0; cursor: pointer; text-decoration: underline; }
  .link:focus-visible, .link:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .count { position: static; left: 0; padding: 2px 8px; font: inherit; font-size: 11px; color: var(--muted); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .count:focus-visible, .count:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .no { margin: 4px 0 0; color: var(--warn); }
  .note-margin[data-beside] .count { position: absolute; bottom: -28px; }
  @media (max-width: 1399px) {
    .note-margin[data-beside] { position: static; width: auto; margin-top: 16px; }
    .note-margin[data-beside] .card { position: static; margin-bottom: 8px; }
    .note-margin[data-beside] .count { position: static; }
  }
  /* NOTES-4: in a note's column beside its slot, a list under the text, flat. */
  @container note-col (max-width: 1099px) {
    .note-margin[data-beside] { position: static; width: auto; margin-top: 16px; }
    .note-margin[data-beside] .card { position: static; margin-bottom: 8px; box-shadow: none; }
    .note-margin[data-beside] .count { position: static; }
  }
  /* EDITOR-12: on a phone a card is the column's width and its words break: the quote wraps to two lines. */
  @media (max-width: 640px) {
    .card { overflow-wrap: anywhere; min-width: 0; }
    .quote { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; white-space: normal; }
    .acts { flex-wrap: wrap; }
  }
</style>
