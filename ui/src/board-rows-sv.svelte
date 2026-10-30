<!-- FRAME-14: board-rows in real Svelte (svelte/1), board-rows.rhx's twin in
     behaviour and marks. Unfiltered: `nav extents` once — each shortcode's
     last sequence and holes — and the row a scroll lands on is an id by
     arithmetic over them; the window is one `list { after, limit: 200,
     summary: true }` page, re-asked only when a scroll leaves it (after 20 ms
     of quiet: FRAME-25, the spec's `debounce_ms` in the host's registry, as
     rhx's; no timer of its own), drawn at its offset. Filtered (the route's `filter`): the count is
     `nav counts { filter }`'s sum and the page is `list { filter, at }`.
     `created` moves the count and puts the row in; `status_changed` patches it
     in place (a keyed each: the same element). -->
<script module>
  export const uses = ["tickets.nav.extents", "tickets.watch", "tickets.nav.counts", "tickets.list"];
</script>

<script>
  import { live } from "plexus-ui/host";

  let { filter = null } = $props();
  const PITCH = 28;
  const PAGE = 200;
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";

  // The extents first: the board's first call, before the window's list.
  const extents = live({ snapshot: "tickets.nav.extents", params: {}, changes: "tickets.watch", key: "shortcode",
                           on: { extent: "upsert:.", created: "upsert:ticket" } });
  const counts = live({ snapshot: "tickets.nav.counts", params: () => ({ filter: filter ?? null }), key: "shortcode", on: { count: "upsert:." } });

  let first = $state(0);
  let win = $state({ after: null, at: null });
  const rows = live({ snapshot: "tickets.list",
    params: () => ({ summary: true, limit: PAGE, filter: filter ?? null, after: win.after, at: win.at }),
    changes: "tickets.watch", changes_params: () => ({ filter: filter ?? null }), key: "id",
    on: { created: "upsert:ticket", status_changed: "upsert:ticket", amended: "upsert:ticket",
          kind_changed: "upsert:ticket", labels_changed: "upsert:ticket", deleted: "remove:ids" },
    debounce_ms: 20 });
  const list = $derived($rows.rows);

  const held = (e) => (e.last_seq ?? e.seq) - (e.holes ?? []).length;
  const total = $derived(filter == null ? $extents.rows.reduce((a, e) => a + held(e), 0) : $counts.rows.reduce((a, c) => a + c.held, 0));

  // The row `start` (0-based) by arithmetic over the extents: the id it follows.
  function afterOf(start) {
    const ext = [...$extents.rows].sort((a, z) => (a.shortcode < z.shortcode ? -1 : a.shortcode > z.shortcode ? 1 : 0));
    let k = start - 1;
    for (const e of ext) {
      const holes = e.holes ?? [];
      const n = held(e);
      if (k >= n) { k -= n; continue; }
      let seq = 0;
      for (;;) {
        seq += 1;
        if (!holes.includes(seq)) {
          if (k === 0) break;
          k -= 1;
        }
      }
      return e.epic + "-" + seq;
    }
    return win.after;
  }

  function scrolled(ev) {
    const top = Math.floor(ev.currentTarget.scrollTop / PITCH);
    const seen = Math.ceil(ev.currentTarget.clientHeight / PITCH) + 1;
    if (top >= first && top + seen <= first + PAGE) return;
    let start = top < first ? top + seen - PAGE : top;
    if (start < 0) start = 0;
    first = start;
    win = filter != null ? { after: null, at: start } : start === 0 ? { after: null, at: null } : { after: afterOf(start), at: null };
  }
</script>

<section class="board" data-board>
  <header class="board-head">
    <span class="board-count">{total}</span>
    <span class="board-count-label">tickets</span>
    {#if $rows.status !== "live"}<span class="board-status" data-board-status={$rows.status}>{$rows.status}</span>{/if}
  </header>
  <div class="board-scroll" data-row-height="28" onscroll={scrolled}>
    <div class="board-space" style={"height:" + PITCH * total + "px"}>
      <div class="board-window" style={"transform:translateY(" + PITCH * first + "px)"}>
      {#each list as r (r.id)}
        <a class="board-row" data-id={r.id} data-status={r.status} href={base + "ticket/" + r.id}>
          <span class="row-id" data-field="id">{r.id}</span>
          <span class="row-title" data-field="title">{r.title}</span>
          <span class="row-kind" data-field="kind">{r.kind}</span>
          <span class="row-status" data-field="status">{r.status}</span>
        </a>
      {/each}
      </div>
    </div>
  </div>
</section>
