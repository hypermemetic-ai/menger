<!-- FRAME-25: an open shortcode in the nav in real Svelte (svelte/1),
     nav-children.rhx's twin — one page of its root's children, asked once
     when it opens (its roll-up stays live on the row above). nav-tree-sv
     imports it; it is a component in its own right too. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  const FIELDS = ["id", "title", "kind", "status"];
  export const uses = ["tickets.list"];
</script>

<script>
  import { live } from "plexus-ui/host";

  let { parent } = $props();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const kids = live({ snapshot: "tickets.list", params: () => ({ parent, summary: true, limit: 200, fields: FIELDS }), key: "id" });
  const list = $derived($kids.rows);
</script>

<ul class="nav-children" data-nav-children={parent}>
  {#each list as c (c.id)}
    <li><a class="nav-row" data-nav-row={c.id} data-status={c.status} href={base + "ticket/" + c.id}><span class="row-id">{c.id}</span> <span class="row-title">{c.title}</span></a></li>
  {/each}
  {#if $kids.status === "live" && list.length === 0}<li class="empty">nothing under it</li>{/if}
</ul>

<style>
  .nav-children { list-style: none; margin: 0 0 4px 16px; padding: 0 0 0 10px; border-left: var(--border) solid var(--tree-line); }
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
  li::after { content: ""; position: absolute; left: -10px; top: 50%; width: 8px; border-top: var(--border) solid var(--tree-line); }
</style>
