<script>
  prop("id");                    // declared props; the engine passes them at mount
  state(#{ expanded: false });   // declared state, its initial value
  let rows = live(#{ snapshot: "tickets.list", params: #{ parent: id, summary: true, limit: 200 },
                     changes: "tickets.watch", key: "id",
                     on: #{ created: "upsert:ticket", amended: "upsert:ticket", deleted: "remove:id" } });
  fn toggle(state, ev) { state.expanded = !state.expanded; state }
  fn rename(state, ev) { call("tickets.update", #{ id: id, title: ev.value }); state }
</script>

<div class="row" on:click={toggle}>{title}</div>
{#if state.expanded}<p>{body}</p>{/if}
{#each rows as r (r.id)}<ticket-row id={r.id}/>{/each}
