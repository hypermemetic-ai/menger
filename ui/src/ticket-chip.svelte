<script>
  // UI-8: a ticket named on a page — its id at once, its title when `get`
  // answers.
  prop("id"); prop("kind");
  let t = live(#{ snapshot: "tickets.get", params: #{ id: id }, key: "id" });
</script>

<a class="chip" data-chip={kind} data-chip-id={id} href={"#/ticket/" + id}>
  <span class="chip-kind">{kind}</span>
  <span class="chip-code">{id}</span>
  <span class="chip-title" data-chip-title>{if t.rows.len() > 0 { t.rows[0].title } else { "" }}</span>
</a>
