<!-- FRAME-14: nav-tree in real Svelte (svelte/1), nav-tree.rhx's twin: Tree
     is `nav groups` — a row per shortcode with its roll-up, live on
     `group_changed` — and a shortcode opens to one page of its root's
     children (nav-children-sv); Flat is one page of `list { filter, summary,
     limit: 200 }` in store order (nav-flat-sv). Under a filter each row says
     how many match. FRAME-25: the two are siblings imported from ui/src
     (FRAME-19), bundled into this module and on its bus. FRAME-20: Tree iterates the handle's getter,
     `groups.rows` (only a changed group renders). -->
<script module>
  export const uses = ["tickets.nav.groups", "tickets.watch", "tickets.nav.counts"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import NavFlat from "./nav-flat-sv.svelte";
  import NavChildren from "./nav-children-sv.svelte";

  let { filter = null } = $props();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";

  const groups = live({ snapshot: "tickets.nav.groups", params: {}, changes: "tickets.watch", key: "shortcode", on: { group_changed: "upsert:group" } });
  const counts = live({ snapshot: "tickets.nav.counts", params: () => ({ filter: filter ?? null }), key: "shortcode", on: { count: "upsert:." } });
  let flat = $state(false);
  let open = $state([]);

  const toggle = (sc) => (open = open.includes(sc) ? open.filter((x) => x !== sc) : [...open, sc]);
  const heldIn = (sc) => $counts.rows.filter((c) => c.shortcode === sc).reduce((a, c) => a + c.held, 0);
</script>

<nav class="nav-tree" data-nav aria-label="shortcodes">
  <div class="nav-modes" role="group" aria-label="view">
    <button class="nav-mode" data-nav-mode="tree" aria-pressed={flat ? "false" : "true"} onclick={() => (flat = false)}>Tree</button>
    <button class="nav-mode" data-nav-mode="flat" aria-pressed={flat ? "true" : "false"} onclick={() => (flat = true)}>Flat</button>
  </div>
  {#if flat}
    <NavFlat {filter}/>
  {:else}
    <ul class="nav-groups">
      {#each groups.rows as g (g.shortcode)}
        <li class="nav-group" data-nav-group={g.shortcode}>
          <div class="nav-group-row">
            <button class="nav-toggle" data-nav-toggle={g.shortcode} aria-expanded={open.includes(g.shortcode) ? "true" : "false"} onclick={() => toggle(g.shortcode)}>{open.includes(g.shortcode) ? "▾" : "▸"}</button>
            <a class="nav-code" href={g.root == null ? base : base + "ticket/" + g.root} title={g.root_row == null ? g.shortcode : g.root_row.title}>{g.shortcode}</a>
            {#if filter != null}
              <span class="nav-held" data-nav-held>{heldIn(g.shortcode)}</span>
            {/if}
            <span class="nav-rollup"><span data-rollup-done>{g.done}</span><span class="of">of</span><span data-rollup-total>{g.total}</span></span>
          </div>
          {#if open.includes(g.shortcode) && g.root != null}
            <NavChildren parent={g.root}/>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</nav>
