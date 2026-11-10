<!-- FRAME-25: an open node of the nav in real Svelte (svelte/1) — v1's
     tree (AGS-20, SCALE-24) — one page of its children, asked once when it
     opens (`list { parent, limit: 200 }`; a shortcode with no root, `list
     { epic }`) — the one call an open costs (a watch per open node would be
     a stream of the whole store each; the roll-ups above stay live on the
     tree's one `group_changed`). Each child is a row (nav-row-sv)
     that opens in turn, at any depth, to its own children — this component
     again — and after a ticket's children come the shortcodes hosted under
     it (AGS-20: a sub-shortcode nests under its host), each a nav-group-sv —
     in its root's place when its root is one of the children (AGS-23).
     A node of more than a page says how many more and asks for the next
     when told. nav-tree-sv imports it; it is a component in its own right
     too. What is open is the nav's (nav-tree-sv's context), kept across
     reloads. -->
<script module>
  // PERF-15: a row is the fields drawn, not the whole summary row.
  const FIELDS = ["id", "uid", "title", "kind", "status", "child_count", "parent"];
  const PAGE = 200;
  export const uses = ["tickets.list"];
</script>

<script>
  import { getContext, setContext } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { live } from "plexus-ui/host";
  import { ticketHref } from "./ticket-links.svelte";
  import Row from "./nav-row-sv.svelte";
  import NavChildren from "./nav-children-sv.svelte";
  import NavGroup from "./nav-group-sv.svelte";

  let { parent = null, epic = null, count = null } = $props();
  // STUDIO-24: alone (no nav-tree-sv above it, as on the studio's canvas) it keeps
  // its own open set and hands it down, so its arrows open and close.
  const nav = getContext("menger.nav") ?? ownNav();
  function ownNav() {
    const open = new SvelteSet();
    const n = { isOpen: (id) => open.has(id), toggle: (id) => (open.has(id) ? open.delete(id) : open.add(id)), hosted: () => [] };
    setContext("menger.nav", n);
    return n;
  }
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  let pages = $state(1);
  const kids = live({ snapshot: "tickets.list",
                      params: () => ({ ...(parent != null ? { parent } : { epic }), summary: true, limit: PAGE * pages, fields: FIELDS }),
                      key: "id" });
  const list = $derived($kids.rows);
  const hosted = $derived(parent != null ? nav.hosted(parent) : []);
  // AGS-23: a child that roots a shortcode hosted here is that shortcode's row, in its place.
  const rooted = $derived(new Map(hosted.filter((g) => g.root != null).map((g) => [g.root, g])));
  const listed = $derived(new Set(list.map((c) => c.id)));
  const more = $derived(count != null ? Math.max(0, count - list.length) : 0);
  const expandable = (c) => (c.child_count ?? 0) > 0 || nav.hosted(c.id).length > 0;
</script>

<ul class="nav-children" role="group" data-nav-children={parent ?? epic}>
  {#each list as c (c.id)}
    {@const open = nav.isOpen(c.id)}
    {@const sub = rooted.get(c.id)}
    {#if sub}
      <NavGroup g={sub}/>
    {:else}
    <li class="nav-item" role="treeitem" aria-selected="false" aria-expanded={expandable(c) ? (open ? "true" : "false") : undefined}>
      <Row id={c.id} href={ticketHref(c, base)} title={c.title} kind={c.kind} status={c.status} frozen={c.frozen}
           expandable={expandable(c)} {open} children={c.child_count || null} ontoggle={() => nav.toggle(c.id)}/>
      {#if open && expandable(c)}
        <NavChildren parent={c.id} count={c.child_count}/>
      {/if}
    </li>
    {/if}
  {/each}
  {#each hosted.filter((g) => !listed.has(g.root)) as g (g.shortcode)}
    <NavGroup {g}/>
  {/each}
  {#if more > 0 && $kids.status === "live" && list.length >= PAGE * pages}
    <li class="nav-item more"><button class="more-btn" onclick={() => (pages += 1)}>{more} more</button></li>
  {/if}
  {#if $kids.status === "live" && list.length === 0 && hosted.length === 0}<li class="nav-item empty">nothing under it</li>{/if}
</ul>

<style>
  .nav-children { list-style: none; margin: 0 0 0 9px; padding: 0 0 0 12px; }
  .nav-item { position: relative; }
  /* v1's connectors: ├─ for a row with a sibling after it, └─ for the last. */
  .nav-item::before { content: ""; position: absolute; left: -12px; top: 0; bottom: 0; border-left: var(--border) solid var(--tree-line); }
  .nav-item:last-child::before { bottom: auto; height: calc(var(--pitch) / 2); }
  .nav-item::after { content: ""; position: absolute; left: -12px; top: calc(var(--pitch) / 2); width: 10px; border-top: var(--border) solid var(--tree-line); }
  .empty { padding: 4px 6px; color: var(--muted); font-size: 11px; }
  .more-btn { margin: 2px 0; padding: 1px 9px; font-size: 11px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .more-btn:hover, .more-btn:global([data-sheet~=hover]) { color: var(--ink); }
</style>
