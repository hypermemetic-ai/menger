<!-- FRAME-25: the nav's Flat in real Svelte (svelte/1), nav-flat.rhx's twin —
     one page of `list { filter, summary, limit: 200 }` in the store's order.
     nav-tree-sv imports it (bundled into that module, on its bus); it is a
     component in its own right too. SURFACE-10: a filter no ticket matches
     says so ([data-empty]), one the language refuses says its words
     ([data-refused]). SHELL.176-4: `current`, the ticket open beside the
     nav — its row marked (`aria-current="page"`, `[data-nav-current]`); once
     the page is read it tells the nav whether it holds it (FRAME-67,
     `nav.flatRead`), which pins it when it does not. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  const FIELDS = ["id", "uid", "title", "kind", "status"];
  export const uses = ["tickets.list"];
</script>

<script>
  import { getContext } from "svelte";
  import { live } from "plexus-ui/host";
  import { ticketHref } from "./ticket-links.svelte";

  let { filter = null, current = null } = $props();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const flat = live({ snapshot: "tickets.list", params: () => ({ filter: filter ?? null, summary: true, limit: 200, fields: FIELDS }), key: "id" });
  const list = $derived($flat.rows);
  // FRAME-67: the nav (nav-tree-sv) pins the open ticket when this page, read, does not hold it.
  const nav = getContext("menger.nav");
  $effect(() => {
    if (current != null && $flat.status === "live" && nav?.flatRead) nav.flatRead(current, filter ?? null, list.some((t) => t.id === current));
  });
</script>

<ul class="nav-flat" data-nav-flat>
  {#each list as t (t.id)}
    <li><a class="nav-row" data-nav-flat-row={t.id} data-status={t.status} data-nav-current={t.id === current ? t.id : undefined} aria-current={t.id === current ? "page" : undefined} href={ticketHref(t, base)}><span class="row-id">{t.id}</span> <span class="row-title">{t.title}</span></a></li>
  {/each}
</ul>
{#if $flat.status === "live" && list.length === 0}<p class="empty" data-empty>No ticket matches this filter.</p>{/if}
{#if $flat.status === "error" || $flat.status === "refused"}<p class="refused" data-refused>{$flat.message}</p>{/if}

<style>
  .nav-flat { list-style: none; margin: 0; padding: 0 4px; }
  li { position: relative; }
  .nav-row { display: flex; gap: 6px; align-items: center; height: calc(var(--pitch) - 4px); padding: 0 6px; overflow: hidden; white-space: nowrap; color: var(--ink); text-decoration: none; border-radius: var(--radius); transition: background-color var(--motion-fast) var(--ease); }
  .nav-row::before { content: ""; flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--st-draft-fg); }
  .nav-row[data-status=Pending]::before { background: var(--st-pending-fg); }
  .nav-row[data-status=Ready]::before { background: var(--st-ready-fg); }
  .nav-row[data-status=Coding]::before { background: var(--st-coding-fg); }
  .nav-row[data-status="In Review"]::before { background: var(--st-in-review-fg); }
  .nav-row[data-status=Complete]::before { background: var(--st-complete-fg); }
  .nav-row[data-status=Blocked]::before { background: var(--st-blocked-fg); }
  .nav-row[data-status=Superseded]::before { background: var(--st-superseded-fg); }
  .nav-row[data-status=Canceled]::before { background: var(--st-canceled-fg); }
  .nav-row[data-status="Epic"]::before { background: var(--st-epic-fg); }
  .nav-row[data-status="Doc"]::before { background: var(--st-doc-fg); }
  .nav-row:hover, .nav-row:global([data-sheet~=hover]) { background: var(--hover); }
  .nav-row[data-nav-current] { background: var(--row-sel); box-shadow: inset 3px 0 0 var(--accent); }
  .nav-row[data-nav-current] .row-title { font-weight: var(--weight-bold); }
  .nav-row:focus-visible, .nav-row:global([data-sheet~=focus]) { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .row-id { flex: none; color: var(--accent); font-variant-numeric: tabular-nums; }
  .row-title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .empty, .refused { margin: 0; padding: 4px 10px; color: var(--muted); font-size: 11px; }
  .refused { color: var(--warn); overflow-wrap: anywhere; }
</style>
