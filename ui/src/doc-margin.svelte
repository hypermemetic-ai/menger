<!-- V2-30: the margin of a doc on the writing surface (svelte/1, a sibling
     editor-body imports for a doc alone). Each question asked on a range of
     the doc (`docs.ask`) is a card beside its text, at the height the range
     stands, the range underlined faintly in the text (`threadMarks`, a
     CodeMirror field editor-body adds); its answers below it, a box to
     answer (`docs.reply`: words, links, an image pasted goes through
     `media.put { doc }`), and resolve (`docs.resolve`), which folds it — the
     margin then counts the resolved, a click shows them. The ranges are the
     doc's CRDT's own: each thread's anchor is two Yjs relative positions,
     resolved here in the page's copy (`Y.createAbsolutePositionFromRelativePosition`),
     so the cards and the underlines follow the text through every hand's
     edit as it happens. A thread whose text was deleted whole is detached
     and listed at the end. Live by `docs.threads` and `docs.watch` (`asked`,
     `replied`, `resolved`). Under 1400 px the margin is a list under the
     text. -->
<script module>
  import { StateEffect, StateField } from "@codemirror/state";
  import { Decoration, EditorView } from "@codemirror/view";

  export const uses = ["docs.threads", "docs.watch", "docs.reply", "docs.resolve", "media.put"];

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

  let { id, ydoc = null, view = () => null, tick = 0 } = $props();
  // Beside a surface (editor-body hands its view and the doc's CRDT) the cards stand at their ranges;
  // alone (the sheet) they are a list, each range as the service resolved it.
  const placedBeside = untrack(() => ydoc != null);
  const b = bus();
  const doc_id = untrack(() => id);
  const threads = live({ snapshot: "docs.threads", params: () => ({ id: doc_id }), changes: "docs.watch", changes_params: () => ({ id: doc_id }), key: "thread",
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
    const r = await b.call("docs.reply", { id: doc_id, thread: t.thread, body });
    if (r.ok && r.events?.some((e) => e.type === "replied")) { drafts[t.thread] = ""; refused[t.thread] = ""; }
    else refused[t.thread] = String(r.message ?? r.events?.[0]?.message ?? r.events?.[0]?.type ?? "refused");
  }
  async function resolve(t, again) {
    const r = await b.call("docs.resolve", { id: doc_id, thread: t.thread, open: again || undefined });
    if (!r.ok) refused[t.thread] = String(r.message ?? "refused");
  }
  /** An image pasted into an answer goes to the doc's media, and its link into the answer. */
  async function pasted(ev, t) {
    const file = [...(ev.clipboardData?.files ?? [])][0];
    if (!file) return;
    ev.preventDefault();
    const data = await new Promise((done) => { const r = new FileReader(); r.onload = () => done(String(r.result).replace(/^data:[^,]*,/, "")); r.readAsDataURL(file); });
    const r = await b.call("media.put", { doc: doc_id, data, name: file.name || "image", mime: file.type || undefined });
    const stored = r.events?.find((e) => e.type === "stored");
    if (!stored) { refused[t.thread] = String(r.message ?? r.events?.[0]?.message ?? "refused"); return; }
    const name = (file.name || "image").replace(/\.[a-z0-9]+$/i, "").replace(/[[\]\n]/g, " ");
    drafts[t.thread] = `${drafts[t.thread] ?? ""}${file.type.startsWith("image/") ? "!" : ""}[${name}](media:${stored.hash})`;
  }
  const when = (ms) => new Date(ms).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
</script>

<aside class="doc-margin" data-field="margin" data-beside={placedBeside ? "" : undefined} aria-label="questions on the text" bind:this={wrap}>
  {#each shown as t (t.thread)}
    <article class="card" data-thread={t.thread} data-state={t.resolved ? "resolved" : t.gone ? "detached" : "open"}>
      <p class="quote" title={t.quote}>{#if t.gone}<span class="tag" data-detached>detached</span>{/if}«{t.quote.length > 80 ? t.quote.slice(0, 80) + "…" : t.quote}»</p>
      <div class="q"><b>{t.by}</b>{#if t.to_whom} → <b data-field="to-whom">{t.to_whom}</b>{/if} <span class="muted">{when(t.at)}</span><div class="md">{@html markdown(mediaLinks(t.question))}</div></div>
      {#each t.replies ?? [] as r (r.n)}
        <div class="r" data-reply={r.n}><b>{r.by}</b> <span class="muted">{when(r.at)}</span><div class="md">{@html markdown(mediaLinks(r.body))}</div></div>
      {/each}
      {#if t.resolved}
        <p class="muted done">resolved by {t.resolved.by} · <button type="button" class="link" data-action="reopen" onclick={() => resolve(t, true)}>open again</button></p>
      {:else}
        <textarea class="say" data-field="reply" rows="2" placeholder="answer… (an image pastes)" aria-label="an answer" bind:value={drafts[t.thread]}
                  onkeydown={(ev) => { if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) { ev.preventDefault(); reply(t); } }} onpaste={(ev) => pasted(ev, t)}></textarea>
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
  .doc-margin { font-size: 12px; color: var(--ink); }
  .doc-margin[data-beside] { position: absolute; top: 0; left: calc(100% + 16px); width: 220px; }
  .doc-margin[data-beside] .card { position: absolute; margin: 0; left: 0; right: 0; }
  .card { margin-bottom: 8px; padding: 8px 10px; background: var(--card); border: var(--border) solid var(--line); border-left: 3px solid var(--accent); border-radius: var(--radius); box-shadow: 0 2px 8px var(--shadow); }
  .card[data-state=resolved] { border-left-color: var(--line); opacity: .75; }
  .card[data-state=detached] { border-left-color: var(--warn); }
  .quote { margin: 0 0 4px; overflow: hidden; color: var(--muted); font-style: italic; white-space: nowrap; text-overflow: ellipsis; }
  .tag { margin-right: 4px; padding: 0 4px; font-style: normal; color: var(--warn); border: var(--border) solid var(--warn); border-radius: var(--radius-sm); }
  .q, .r { margin: 4px 0; }
  .r { padding-left: 8px; border-left: 2px solid var(--line); }
  .md :global(p) { margin: 2px 0; }
  .md :global(img) { max-width: 100%; border-radius: var(--radius-sm); }
  .md :global(a) { color: var(--accent); }
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
  .doc-margin[data-beside] .count { position: absolute; bottom: -28px; }
  @media (max-width: 1399px) {
    .doc-margin[data-beside] { position: static; width: auto; margin-top: 16px; }
    .doc-margin[data-beside] .card { position: static; margin-bottom: 8px; }
    .doc-margin[data-beside] .count { position: static; }
  }
  /* EDITOR-12: on a phone a card is the column's width and its words break: the quote wraps to two lines. */
  @media (max-width: 640px) {
    .card { overflow-wrap: anywhere; min-width: 0; }
    .quote { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; white-space: normal; }
    .acts { flex-wrap: wrap; }
  }
</style>
