<!-- FRAME-25: the nav's Flat in real Svelte (svelte/1), nav-flat.rhx's twin —
     one page of `list { filter, summary, limit: 200 }` in the store's order.
     nav-tree-sv imports it (bundled into that module, on its bus); it is a
     component in its own right too. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  const FIELDS = ["id", "title", "kind", "status"];
  export const uses = ["tickets.list"];
</script>

<script>
  import { live } from "plexus-ui/host";

  let { filter = null } = $props();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const flat = live({ snapshot: "tickets.list", params: () => ({ filter: filter ?? null, summary: true, limit: 200, fields: FIELDS }), key: "id" });
  const list = $derived($flat.rows);
</script>

<ul class="nav-flat" data-nav-flat>
  {#each list as t (t.id)}
    <li><a class="nav-row" data-nav-flat-row={t.id} data-status={t.status} href={base + "ticket/" + t.id}><span class="row-id">{t.id}</span> <span class="row-title">{t.title}</span></a></li>
  {/each}
</ul>

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
  .nav-row:hover { background: var(--hover); }
  .nav-row:focus-visible { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .row-id { flex: none; color: var(--accent); font-variant-numeric: tabular-nums; }
  .row-title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .empty { padding: 4px 6px; color: var(--muted); font-size: 11px; }
</style>
