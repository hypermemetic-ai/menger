<script>
  // UI-8: the nav. Tree: `nav groups` — a row per shortcode with its
  // roll-up, live on `group_changed` — and a shortcode opens to one page of
  // its root's children (nav-children). Flat: one page of
  // `list { filter, summary, limit: 200 }` in store order (nav-flat). Under
  // a filter each row says how many match (`nav counts { filter }`).
  prop("filter");
  state(#{ open: [], flat: false });
  let groups = live(#{ snapshot: "tickets.nav.groups", params: #{}, changes: "tickets.watch", key: "shortcode", on: #{ group_changed: "upsert:group" } });
  let counts = live(#{ snapshot: "tickets.nav.counts", params: #{ filter: filter }, key: "shortcode", on: #{ count: "upsert:." } });
  fn toggle(state, ev) {
    let s = ev.target.data["nav-toggle"];
    if s in state.open { state.open = state.open.filter(|x| x != s); } else { state.open.push(s); }
    state
  }
  fn tree(state, ev) { state.flat = false; state }
  fn flat(state, ev) { state.flat = true; state }
</script>

<nav class="nav-tree" data-nav aria-label="shortcodes">
  <div class="nav-modes" role="group" aria-label="view">
    <button class="nav-mode" data-nav-mode="tree" aria-pressed={if state.flat { "false" } else { "true" }} on:click={tree}>Tree</button>
    <button class="nav-mode" data-nav-mode="flat" aria-pressed={if state.flat { "true" } else { "false" }} on:click={flat}>Flat</button>
  </div>
  {#if state.flat}
    <nav-flat filter={filter}/>
  {:else}
    <ul class="nav-groups">
      {#each groups as g (g.shortcode)}
        <li class="nav-group" data-nav-group={g.shortcode}>
          <div class="nav-group-row">
            <button class="nav-toggle" data-nav-toggle={g.shortcode} aria-expanded={if g.shortcode in state.open { "true" } else { "false" }} on:click={toggle}>{if g.shortcode in state.open { "▾" } else { "▸" }}</button>
            <a class="nav-code" href={if g.root == () { "#/" } else { "#/ticket/" + g.root }} title={if g.root_row == () { g.shortcode } else { g.root_row.title }}>{g.shortcode}</a>
            {#if filter != ()}
              <span class="nav-held" data-nav-held>{counts.rows.filter(|c| c.shortcode == g.shortcode).reduce(|a, c| a + c.held, 0)}</span>
            {/if}
            <span class="nav-rollup"><span data-rollup-done>{g.done}</span><span class="of">of</span><span data-rollup-total>{g.total}</span></span>
          </div>
          {#if g.shortcode in state.open && g.root != ()}
            <nav-children parent={g.root}/>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</nav>
