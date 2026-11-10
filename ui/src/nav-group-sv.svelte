<!-- A shortcode in the nav, v1's (AGS-20, AGS-23) in real Svelte
     (svelte/1): drawn as its root ticket's row — the root's id, title, kind
     and status, from the group (`root_row`), no `get` — with the shortcode's
     roll-up boxed (`done/total`, live on `group_changed`) and, under a
     filter, how many of it match. Opened, it shows the root's children
     (nav-children-sv, `list { parent: root }`); a shortcode with no root
     lists its tickets (`list { epic }`). nav-tree-sv draws the top level
     with it, and nav-children-sv a sub-shortcode under its host. -->
<script module>
  export const uses = [];
</script>

<script>
  import { getContext } from "svelte";
  import { ticketHref } from "./ticket-links.svelte";
  import Row from "./nav-row-sv.svelte";
  import NavChildren from "./nav-children-sv.svelte";

  let { g } = $props();
  const nav = getContext("menger.nav") ?? { isOpen: () => false, toggle: () => {}, held: () => null, board: "#/board", base: "#/" };
  const open = $derived(nav.isOpen(g.shortcode));
  const r = $derived(g.root_row);
</script>

<li class="nav-item nav-group" role="treeitem" aria-selected="false" aria-expanded={open ? "true" : "false"} data-nav-group={g.shortcode}>
  <Row id={g.root ?? g.epic} mark="group" href={g.root == null ? nav.board : ticketHref(r ? { id: g.root, title: r.title } : { id: g.root }, nav.base)}
       title={r?.title ?? ""} kind={r?.kind ?? null} status={r?.status ?? null} frozen={r?.frozen ?? false}
       expandable={true} {open} rollup={{ done: g.done, total: g.total }} held={nav.held(g.shortcode)} ontoggle={() => nav.toggle(g.shortcode)}/>
  {#if open}
    {#if g.root != null}
      <NavChildren parent={g.root}/>
    {:else}
      <NavChildren epic={g.epic}/>
    {/if}
  {/if}
</li>

<style>
  .nav-item { position: relative; }
  /* v1's connectors at every level: ├─ before a sibling, └─ for the last. */
  .nav-item::before { content: ""; position: absolute; left: -12px; top: 0; bottom: 0; border-left: var(--border) solid var(--tree-line); }
  .nav-item:last-child::before { bottom: auto; height: calc(var(--pitch) / 2); }
  .nav-item::after { content: ""; position: absolute; left: -12px; top: calc(var(--pitch) / 2); width: 10px; border-top: var(--border) solid var(--tree-line); }
</style>
