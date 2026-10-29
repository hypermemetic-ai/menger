<script>
  // UI-8: the board — `extents` once, then one `list { after, limit: 200,
  // summary: true }` page for the window; row k an id by arithmetic. A stub.
  state(#{ top: 0, filter: "" });
  let extents = live(#{ snapshot: "tickets.nav.extents", params: #{}, changes: "tickets.watch", key: "shortcode", on: #{} });
</script>

<section class="board">
<h2 class="board-heading">Board</h2>
</section>
