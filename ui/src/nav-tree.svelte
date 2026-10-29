<script>
  // UI-8: the nav — `nav groups` for the shortcode rows, live on
  // `group_changed`; a shortcode opens to one page of `list { parent }`. A stub.
  state(#{ open: "", flat: false });
  let groups = live(#{ snapshot: "tickets.nav.groups", params: #{}, changes: "tickets.watch", key: "shortcode", on: #{ group_changed: "upsert:group" } });
</script>

<nav class="nav-tree">
<h2 class="nav-heading">Shortcodes</h2>
</nav>
