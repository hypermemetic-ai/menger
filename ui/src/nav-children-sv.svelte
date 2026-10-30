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
