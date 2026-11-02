<!-- EDITOR-8: the writing screen (svelte/1) — one document on one surface,
     and the documents beside it. A document is a ticket. The surface is
     editor-body (the ticket drawn as ticket-view draws it, edited in place),
     ticket-questions (V2-20) under it; nothing is drawn twice. The list, a
     narrow column (a drawer under 1024 px, `docs` in the bar): a search
     (`tickets.find`, id or title), the documents opened here lately
     (`menger.editor.recent`, this browser's), and this shortcode's docs
     (`tickets.list { filter: "in S and kind doc" }`, live by the watch).
     `+ new` makes a ticket of kind `doc` in the current shortcode
     (`tickets.create { epic, title: "Untitled", kind: "doc" }` — a doc is
     born `Doc`, SDLC-11) and opens it at `#/edit/<id>`. Switching documents
     keeps autosave: the editor is keyed by the document, and the one left
     sends what it holds as it goes. `#/` opens the document last opened in
     this browser (`menger.editor.last`, written here and by the ticket
     page's body); `#/edit/:id` a given one. Keys: `escape` (outside the
     text) and the bar's *board* go to `#/board`; `n` is *new*; `/` the
     search. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.create", "tickets.nav.groups", "tickets.list", "tickets.find"];
  export const LAST = "menger.editor.last";
  export const RECENT = "menger.editor.recent";
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import Editor from "plexus-ui:editor-body";
  import Questions from "plexus-ui:ticket-questions";

  let { id = null, sheet = null } = $props();
  const b = bus();
  const FIELDS = ["id", "title", "kind", "status"];
  const remembered = () => { try { return localStorage.getItem(LAST) || null; } catch { return null; } };
  const remember = (v) => { try { localStorage.setItem(LAST, v); } catch { /* a private window */ } };
  const readRecent = () => { try { const r = JSON.parse(localStorage.getItem(RECENT) || "[]"); return Array.isArray(r) ? r.filter((x) => x && typeof x.id === "string") : []; } catch { return []; } };
  // The document on screen: the route's, else the last opened here.
  let last = $state(remembered());
  const current = $derived(id ?? last);
  $effect(() => { if (id) { remember(id); last = id; } });

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id: current }), changes: "tickets.watch", key: "id", when: () => current != null,
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  // The page's spec word for word: one get and one watch of the ticket, shared with every part.
  const t = $derived(ticket.rows.find((r) => r.id === current && r.title != null));
  // Where *new* creates: the current ticket's shortcode, else the first group (asked only when there is no ticket).
  const groups = live({ snapshot: "tickets.nav.groups", params: {}, key: "shortcode", when: () => current == null, on: {} });
  const shortcode = $derived(t?.epic ?? groups.rows.find((g) => g.shortcode)?.shortcode ?? null);

  // ── the documents ────────────────────────────────────────────────────────
  let recent = $state(readRecent());
  $effect(() => {
    if (!t) return;
    const row = { id: t.id, title: t.title };
    const next = [row, ...recent.filter((r) => r.id !== row.id)].slice(0, 12);
    if (JSON.stringify(next) === JSON.stringify(recent)) return;
    recent = next;
    try { localStorage.setItem(RECENT, JSON.stringify(next)); } catch { /* a private window */ }
  });
  const docs = live({ snapshot: "tickets.list", params: () => ({ filter: `in ${shortcode} and kind doc`, summary: true, limit: 200, fields: FIELDS }),
                      changes: "tickets.watch", changes_params: () => ({ filter: `in ${shortcode} and kind doc` }), key: "id", when: () => shortcode != null,
                      on: { created: "upsert:ticket", amended: "upsert:ticket", updated: "upsert:ticket", kind_changed: "upsert:ticket", deleted: "remove:ids" } });
  const theirs = $derived(docs.rows.filter((r) => r.id && r.title != null && r.kind === "doc").slice().reverse());
  let q = $state("");
  const found = live({ snapshot: "tickets.find", params: () => ({ query: q.trim(), limit: 20, fields: FIELDS }), key: "id", when: () => q.trim().length > 0, debounce_ms: 150 });
  const hits = $derived(q.trim() ? found.rows.filter((r) => r.id && r.title != null) : []);
  let drawer = $state(false);
  let searchBox = $state(null);
  const open = (to) => { drawer = false; q = ""; if (to !== current) b.navigate("#/edit/" + to); };

  let making = $state(false);
  let refused = $state("");
  async function create() {
    if (making || !shortcode) return;
    making = true;
    refused = "";
    const r = await b.call("tickets.create", { epic: shortcode, title: "Untitled", kind: "doc" });
    making = false;
    const made = r.events?.find((e) => e.type === "created")?.ticket;
    if (r.ok && made) { remember(made.id); drawer = false; b.navigate("#/edit/" + made.id); }
    else refused = String(r.message ?? "refused").replace(/^Invalid params: /, "");
  }
  b.keys({
    escape: { to: () => (drawer ? (drawer = false) : b.navigate("#/board")), label: "the board" },
    n: { to: create, label: "a new document" },
    "/": { to: () => { drawer = true; searchBox?.focus(); }, label: "find a document" },
  });
</script>

<div class="editor-screen" data-surface="editor-screen" data-ticket={current ?? undefined} data-drawer={drawer ? "open" : undefined}>
  <div class="bar">
    <button type="button" class="btn docs" data-action="docs" aria-expanded={drawer} onclick={() => (drawer = !drawer)}>docs</button>
    {#if current}
      <a class="id" href={"#/ticket/" + current} data-field="current">{current}</a>
      {#if t}<span class="kind" data-field="kind"><i class="kmark" data-kind={t.kind} aria-hidden="true"></i>{t.kind}</span>{/if}
    {:else}
      <span class="none" data-empty>no document open yet</span>
    {/if}
    <span class="right">
      <a class="board" href="#/board" data-action="board"><kbd>esc</kbd> board</a>
    </span>
    {#if refused}<p class="refused" data-refused role="alert">{refused}</p>{/if}
  </div>
  <div class="main">
    <nav class="list" data-field="documents" aria-label="documents">
      <button type="button" class="btn primary new" data-action="new" disabled={making || !shortcode || sheet === "loading"} onclick={create}>+ new{shortcode ? ` in ${shortcode}` : ""} <kbd>n</kbd></button>
      <input class="find" data-field="find" type="search" placeholder="find a ticket…" aria-label="find a ticket" autocomplete="off" spellcheck="false" bind:value={q} bind:this={searchBox}/>
      {#if q.trim()}
        <p class="head">found</p>
        {#each hits as r (r.id)}
          <button type="button" class="doc" data-doc={r.id} data-current={r.id === current ? "" : undefined} onclick={() => open(r.id)}><span class="did">{r.id}</span>{r.title}</button>
        {:else}
          <p class="none">{found.status === "pending" ? "finding…" : "nothing found"}</p>
        {/each}
      {:else}
        {#if recent.length}
          <p class="head">recent</p>
          {#each recent as r (r.id)}
            <button type="button" class="doc" data-doc={r.id} data-recent data-current={r.id === current ? "" : undefined} onclick={() => open(r.id)}><span class="did">{r.id}</span>{r.title}</button>
          {/each}
        {/if}
        {#if shortcode}
          <p class="head">docs in {shortcode}</p>
          {#each theirs as r (r.id)}
            <button type="button" class="doc" data-doc={r.id} data-current={r.id === current ? "" : undefined} onclick={() => open(r.id)}><span class="did">{r.id}</span>{r.title}</button>
          {:else}
            <p class="none">{docs.status === "pending" ? "…" : "none yet"}</p>
          {/each}
        {/if}
      {/if}
    </nav>
    <button type="button" class="scrim" aria-label="close the documents" tabindex="-1" onclick={() => (drawer = false)}></button>
    <div class="page">
      {#if current}
        <div class="sheet">
          {#key current}<Editor id={current} {sheet}/>{/key}
          {#key current}<Questions id={current}/>{/key}
        </div>
      {:else}
        <div class="welcome" data-welcome>
          <p class="lead">Write a document here. It saves as you type.</p>
          <p class="muted">Open one from the list or the <a href="#/board">board</a>, or make a new one{#if shortcode} in {shortcode}{/if}.</p>
          <button type="button" class="btn primary" data-action="new" disabled={making || !shortcode} onclick={create}>+ new document <kbd>n</kbd></button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .editor-screen { display: flex; flex-direction: column; flex: 1; min-height: 0; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .bar { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; flex: none; min-height: 36px; padding: 4px 14px; font-size: 12px; color: var(--muted); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .id { color: var(--accent); text-decoration: none; }
  .id:hover, .id:global([data-sheet~=hover]) { text-decoration: underline; }
  .id:focus-visible, .id:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; border-radius: var(--radius-sm); }
  .kind { display: inline-flex; gap: 5px; align-items: center; }
  .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind="scope"] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind="gap"] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind="spike"] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind="execution"] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .kmark[data-kind="doc"] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .right { margin-left: auto; display: flex; gap: 10px; align-items: center; }
  .board { display: inline-flex; gap: 5px; align-items: center; color: var(--muted); text-decoration: none; }
  .board:hover, .board:global([data-sheet~=hover]) { color: var(--ink); }
  .board:focus-visible, .board:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; border-radius: var(--radius-sm); }
  .btn { display: inline-flex; gap: 5px; align-items: center; padding: 1px 10px; font-size: 12px; line-height: 20px; white-space: nowrap; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .btn:hover, .btn:global([data-sheet~=hover]) { background: var(--hover); border-color: var(--muted); }
  .btn:focus-visible, .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn:disabled, .btn:global([data-sheet~=disabled]) { color: var(--disabled); background: var(--card); border-color: var(--line); cursor: not-allowed; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .btn.primary:disabled, .btn.primary:global([data-sheet~=disabled]) { color: var(--disabled); background: var(--card); border-color: var(--line); }
  .docs { display: none; }
  .refused { flex-basis: 100%; margin: 0; color: var(--warn); }
  .main { position: relative; flex: 1; min-height: 0; display: grid; grid-template-columns: 232px minmax(0, 1fr); }
  .list { display: flex; flex-direction: column; gap: 2px; min-height: 0; overflow: auto; padding: 12px 10px 24px; background: var(--panel); border-right: var(--border) solid var(--line); }
  .new { justify-content: center; margin-bottom: 8px; }
  .find { margin-bottom: 6px; padding: 4px 8px; font: inherit; font-size: 12px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .find:focus-visible, .find:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 0; }
  .head { margin: 10px 4px 2px; font-size: 10px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  .doc { display: block; width: 100%; padding: 4px 6px; overflow: hidden; font: inherit; font-size: 12px; line-height: 1.35; text-align: left; text-overflow: ellipsis; white-space: nowrap; color: var(--ink); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .doc:hover, .doc:global([data-sheet~=hover]) { background: var(--hover); }
  .doc:focus-visible, .doc:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .doc[data-current], .doc:global([data-sheet~=selected]) { background: var(--row-sel); }
  .did { margin-right: 6px; font-size: 11px; color: var(--muted); }
  .scrim { display: none; }
  .page { min-width: 0; min-height: 0; overflow: auto; }
  .sheet { max-width: 760px; margin: 0 auto; padding: 24px 28px 64px; }
  .welcome { padding: 40px 20px; text-align: center; }
  .lead { margin: 0 0 6px; font: var(--weight-strong) 18px var(--font-display); }
  .muted { margin: 0 0 14px; color: var(--muted); }
  .muted a { color: var(--accent); }
  .none { margin: 2px 6px; font-size: 12px; color: var(--muted); }
  kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  @media (max-width: 1023px) {
    .docs { display: inline-flex; }
    .main { grid-template-columns: minmax(0, 1fr); }
    .list { position: absolute; z-index: 4; top: 0; bottom: 0; left: 0; width: min(280px, 82vw); transform: translateX(-100%); transition: transform var(--motion-fast) var(--ease); box-shadow: 0 6px 18px var(--shadow); padding-bottom: calc(24px + env(safe-area-inset-bottom)); }
    .editor-screen[data-drawer=open] .list { transform: none; }
    .editor-screen[data-drawer=open] .scrim { display: block; position: absolute; inset: 0; z-index: 3; padding: 0; background: var(--shadow); border: 0; }
    .sheet { padding: 16px 16px calc(48px + env(safe-area-inset-bottom)); }
    .board kbd { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .btn, .list { transition: none; }
  }
</style>
