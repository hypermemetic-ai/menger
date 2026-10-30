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
