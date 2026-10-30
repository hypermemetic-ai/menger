<!-- FRAME-14: nav-tree in real Svelte (svelte/1), nav-tree.rhx's twin with
     its two children inside it (a svelte/1 module imports no other
     component): Tree is `nav groups` — a row per shortcode with its roll-up,
     live on `group_changed` — and a shortcode opens to one page of its
     root's children (nav-children.rhx's: one list, asked when it opens).
     Flat is one page of `list { filter, summary, limit: 200 }` in store order
     (nav-flat.rhx's). Under a filter each row says how many match. FRAME-20:
     Tree iterates the handle's getter, `groups.rows` (only a changed group
     renders). -->
<script module>
  export const uses = ["tickets.nav.groups", "tickets.watch", "tickets.nav.counts", "tickets.list"];
</script>

<script>
  import { bus } from "plexus-ui/host";
  import { onDestroy } from "svelte";

  let { filter = null } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const NONE = { subscribe: (fn) => (fn({ rows: [], status: "pending" }), () => {}) };

  const groups = b.live({ snapshot: "tickets.nav.groups", params: {}, changes: "tickets.watch", key: "shortcode", on: { group_changed: "upsert:group" } });
  const counts = $derived(b.live({ snapshot: "tickets.nav.counts", params: { filter: filter ?? null }, key: "shortcode", on: { count: "upsert:." } }));
  let flat = $state(false);
  const flatRows = $derived(flat ? b.live({ snapshot: "tickets.list", params: { filter: filter ?? null, summary: true, limit: 200 }, key: "id" }) : NONE);

  // The open shortcodes, each with its root's children while it is open.
  let open = $state([]);
  let kids = $state({});
  const offs = {};
  function toggle(g) {
    if (open.includes(g.shortcode)) {
      open = open.filter((x) => x !== g.shortcode);
      offs[g.shortcode]?.();
      delete offs[g.shortcode];
      delete kids[g.shortcode];
      return;
    }
    open = [...open, g.shortcode];
    if (g.root == null) return;
    const list = b.live({ snapshot: "tickets.list", params: { parent: g.root, summary: true, limit: 200 }, key: "id" });
    offs[g.shortcode] = list.subscribe((v) => (kids[g.shortcode] = v));
  }
  onDestroy(() => Object.values(offs).forEach((off) => off()));

  const heldIn = (sc) => $counts.rows.filter((c) => c.shortcode === sc).reduce((a, c) => a + c.held, 0);
</script>

<nav class="nav-tree" data-nav aria-label="shortcodes">
  <div class="nav-modes" role="group" aria-label="view">
    <button class="nav-mode" data-nav-mode="tree" aria-pressed={flat ? "false" : "true"} onclick={() => (flat = false)}>Tree</button>
    <button class="nav-mode" data-nav-mode="flat" aria-pressed={flat ? "true" : "false"} onclick={() => (flat = true)}>Flat</button>
  </div>
  {#if flat}
    <ul class="nav-flat" data-nav-flat>
      {#each $flatRows.rows as t (t.id)}
        <li><a class="nav-row" data-nav-flat-row={t.id} data-status={t.status} href={base + "ticket/" + t.id}><span class="row-id">{t.id}</span> <span class="row-title">{t.title}</span></a></li>
      {/each}
    </ul>
  {:else}
    <ul class="nav-groups">
      {#each groups.rows as g (g.shortcode)}
        <li class="nav-group" data-nav-group={g.shortcode}>
          <div class="nav-group-row">
            <button class="nav-toggle" data-nav-toggle={g.shortcode} aria-expanded={open.includes(g.shortcode) ? "true" : "false"} onclick={() => toggle(g)}>{open.includes(g.shortcode) ? "▾" : "▸"}</button>
            <a class="nav-code" href={g.root == null ? base : base + "ticket/" + g.root} title={g.root_row == null ? g.shortcode : g.root_row.title}>{g.shortcode}</a>
            {#if filter != null}
              <span class="nav-held" data-nav-held>{heldIn(g.shortcode)}</span>
            {/if}
            <span class="nav-rollup"><span data-rollup-done>{g.done}</span><span class="of">of</span><span data-rollup-total>{g.total}</span></span>
          </div>
          {#if open.includes(g.shortcode) && g.root != null}
            <ul class="nav-children" data-nav-children={g.root}>
              {#each kids[g.shortcode]?.rows ?? [] as c (c.id)}
                <li><a class="nav-row" data-nav-row={c.id} data-status={c.status} href={base + "ticket/" + c.id}><span class="row-id">{c.id}</span> <span class="row-title">{c.title}</span></a></li>
              {/each}
              {#if kids[g.shortcode]?.status === "live" && kids[g.shortcode].rows.length === 0}<li class="empty">nothing under it</li>{/if}
            </ul>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</nav>
