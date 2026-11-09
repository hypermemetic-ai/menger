<!-- UI-8: the first component — a page of tickets, live (created, amended,
     deleted), and what the page last heard of a scroll, a key and a
     checkbox. page-shell's `main` (ui/slots.json), at `#/hello`. SV-4: real
     Svelte (svelte/1), hello-list.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = ["tickets.list", "tickets.watch"];
</script>

<script>
  import { live } from "plexus-ui/host";

  let seen = $state("");
  const rows = live({ snapshot: "tickets.list", params: { summary: true, limit: 200, fields: ["id", "title", "kind", "status"] },
                      changes: "tickets.watch", key: "id",
                      on: { created: "upsert:ticket", amended: "upsert:ticket", deleted: "remove:ids" } });
  const scrolled = (e) => { const t = e.currentTarget; seen = `scroll ${t.scrollTop}/${t.scrollHeight}/${t.clientHeight}`; };
</script>

<h1 class="hello">Hello from v1</h1>
<p class="status">{rows.status}</p>
<p class="seen">{seen}</p>
<input class="keys" onkeydown={(e) => (seen = "key " + e.key)}/>
<input class="tick" type="checkbox" onchange={(e) => (seen = "checked " + e.currentTarget.checked)}/>
<ul class="rows" onscroll={scrolled}>
{#each rows.rows as r (r.id)}<li data-id={r.id}>{r.id + " " + r.title}</li>{/each}
</ul>

<style>
  .rows { max-height: 240px; overflow: auto; margin: 0; }
</style>
