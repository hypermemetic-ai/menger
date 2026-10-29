<script>
  prop("id"); prop("title"); prop("body");   // declared props; the engine passes them at mount
  state(#{ expanded: false });   // declared state, its initial value
  let rows = live(#{ snapshot: "tickets.list", params: #{ parent: id, summary: true, limit: 200 },
                     changes: "tickets.watch", key: "id",
                     on: #{ created: "upsert:ticket", amended: "upsert:ticket", deleted: "remove:ids" } });
  fn toggle(state, ev) { state.expanded = !state.expanded; state }
  fn rename(state, ev) { call("tickets.update", #{ id: ev.props.id, title: ev.value }); state }
</script>

<div class="row" on:click={toggle}>{title}</div>
{#if state.expanded}<p>{body}</p>{/if}
{#each rows as r (r.id)}<ticket-row id={r.id}/>{/each}
