<script>
  state(#{ seen: "" });
  let rows = live(#{ snapshot: "tickets.list", params: #{ summary: true, limit: 200 },
                     changes: "tickets.watch", key: "id",
                     on: #{ created: "upsert:ticket", amended: "upsert:ticket", deleted: "remove:ids" } });
  fn scrolled(state, ev) { state.seen = "scroll " + ev.scroll_top + "/" + ev.scroll_height + "/" + ev.client_height; state }
  fn keyed(state, ev) { state.seen = "key " + ev.key; state }
  fn ticked(state, ev) { state.seen = "checked " + ev.checked; state }
</script>

<h1 class="hello">Hello from v1</h1>
<p class="status">{rows.status}</p>
<p class="seen">{state.seen}</p>
<input class="keys" on:keydown={keyed}/>
<input class="tick" type="checkbox" on:change={ticked}/>
<ul class="rows" style="max-height: 240px; overflow: auto; margin: 0" on:scroll={scrolled}>
{#each rows as r (r.id)}<li data-id={r.id}>{r.id + " " + r.title}</li>{/each}
</ul>
