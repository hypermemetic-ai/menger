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

<style>
  .nav-tree { display: flex; flex-direction: column; min-height: 100%; padding: 8px 0; background: var(--panel); font-size: var(--text-size); }
  .nav-modes { display: flex; gap: 4px; padding: 0 10px 8px; }
  .nav-mode { padding: 1px 9px; font-size: 11px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .nav-mode:hover { color: var(--ink); }
  .nav-mode:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .nav-mode[aria-pressed=true] { color: var(--accent); border-color: var(--accent); background: var(--sel); }
  .nav-groups { list-style: none; margin: 0; padding: 0 4px; }
  .nav-group-row { display: grid; grid-template-columns: 18px minmax(0, 1fr) auto auto; gap: 4px; align-items: center; height: var(--pitch); padding: 0 6px; border-radius: var(--radius); transition: background-color var(--motion-fast) var(--ease); }
  .nav-group-row:hover { background: var(--hover); }
  .nav-toggle { padding: 0; color: var(--muted); background: none; border: 0; cursor: pointer; }
  .nav-toggle:focus-visible { outline: 2px solid var(--focus); border-radius: var(--radius-sm); }
  .nav-code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink); text-decoration: none; font-weight: var(--weight-medium); }
  .nav-code:hover { color: var(--accent); }
  .nav-code:focus-visible { outline: 2px solid var(--focus); border-radius: var(--radius-sm); }
  .nav-held { font-size: 11px; color: var(--accent); font-variant-numeric: tabular-nums; }
  .nav-rollup { display: flex; gap: 3px; font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }
  @media (prefers-reduced-motion: reduce) {
    .nav-mode, .nav-group-row { transition: none; }
  }
</style>
