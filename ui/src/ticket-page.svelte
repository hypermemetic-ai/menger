<script>
  // UI-8: a ticket — `get`, its chips, its body drawn without scripts or
  // styles, its children by `list { parent }`, `set_status` from a menu. A stub.
  prop("id");
  state(#{ menu: false });
  let ticket = live(#{ snapshot: "tickets.get", params: #{ id: id }, changes: "tickets.watch", key: "id",
                       on: #{ amended: "upsert:ticket", status_changed: "upsert:ticket" } });
  let children = live(#{ snapshot: "tickets.list", params: #{ parent: id, summary: true, limit: 200 }, changes: "tickets.watch", key: "id",
                         on: #{ created: "upsert:ticket", amended: "upsert:ticket", deleted: "remove:ids" } });
</script>

<article class="ticket-page">
<h1 class="ticket-heading">{id}</h1>
</article>
