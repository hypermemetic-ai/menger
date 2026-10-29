<script>
  let rows = live(#{ snapshot: "tickets.list", params: #{ summary: true, limit: 200 },
                     changes: "tickets.watch", key: "id",
                     on: #{ created: "upsert:ticket", amended: "upsert:ticket", deleted: "remove:ids" } });
</script>

<h1 class="hello">Hello from v1</h1>
<p class="status">{rows.status}</p>
<ul class="rows">
{#each rows.rows as r (r.id)}<li data-id={r.id}>{r.id + " " + r.title}</li>{/each}
</ul>
