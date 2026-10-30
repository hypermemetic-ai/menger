<!-- FRAME-14: board-rows in real Svelte (svelte/1), board-rows.rhx's twin in
     behaviour and marks. Unfiltered: `nav extents` once — each shortcode's
     last sequence and holes — and the row a scroll lands on is an id by
     arithmetic over them; the window is one `list { after, limit: 200,
     summary: true }` page, re-asked only when a scroll leaves it (after 20 ms
     of quiet: FRAME-25, the spec's `debounce_ms` in the host's registry, as
     rhx's; no timer of its own), drawn at its offset. Filtered (the route's `filter`): the count is
     `nav counts { filter }`'s sum and the page is `list { filter, at }`.
     `created` moves the count and puts the row in; `status_changed` patches it
     in place (a keyed each: the same element). FRAME-20: the window iterates
     the handle's getter, `rows.rows`, not the store's `$rows.rows`: a row a
     change leaves alone is the same object, so only the changed row renders. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  const FIELDS = ["id", "title", "kind", "status"];
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
    params: () => ({ summary: true, limit: PAGE, fields: FIELDS, filter: filter ?? null, after: win.after, at: win.at }),
    changes: "tickets.watch", changes_params: () => ({ filter: filter ?? null }), key: "id",
    on: { created: "upsert:ticket", status_changed: "upsert:ticket", amended: "upsert:ticket",
          kind_changed: "upsert:ticket", labels_changed: "upsert:ticket", deleted: "remove:ids" },
    debounce_ms: 20 });
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
    {#if rows.status !== "live"}<span class="board-status" data-board-status={rows.status}>{rows.status}</span>{/if}
  </header>
  <div class="board-scroll" data-row-height="28" onscroll={scrolled}>
    <div class="board-space" style={"height:" + PITCH * total + "px"}>
      <div class="board-window" style={"transform:translateY(" + PITCH * first + "px)"}>
      {#each rows.rows as r (r.id)}
        <a class="board-row" data-id={r.id} data-status={r.status} href={base + "ticket/" + r.id}>
          <i class="kmark" data-kind={r.kind} aria-hidden="true"></i>
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

<style>
  .board { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; background: var(--bg); }
  .board-head { display: flex; gap: 6px; align-items: baseline; padding: 7px 14px; color: var(--muted); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .board-count { color: var(--ink); font-weight: var(--weight-bold); font-variant-numeric: tabular-nums; }
  .board-status { margin-left: auto; font-size: 11px; color: var(--warn); }
  .board-status::before { content: ""; display: inline-block; width: 6px; height: 6px; margin-right: 5px; vertical-align: 1px; border-radius: 50%; background: var(--warn); }
  .board-scroll { flex: 1; min-height: 0; overflow: auto; position: relative; outline: none; }
  .board-scroll:focus-visible { box-shadow: inset 0 0 0 2px var(--focus); }
  .board-space { position: relative; }
  .board-window { position: absolute; left: 0; right: 0; top: 0; will-change: transform; }
  .board-row { display: grid; grid-template-columns: 10px 104px minmax(0, 1fr) auto auto; gap: 10px; align-items: center; height: var(--pitch); padding: 0 14px; color: var(--ink); text-decoration: none; white-space: nowrap; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .board-row:hover { background: var(--hover); }
  .board-row:focus-visible { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .row-id { color: var(--accent); font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; }
  .row-title { overflow: hidden; text-overflow: ellipsis; }
  .row-kind { color: var(--muted); font-size: 11px; }
  .row-status { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .board-row[data-status=Draft] .row-status { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .board-row[data-status=Pending] .row-status { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .board-row[data-status=Ready] .row-status { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .board-row[data-status=Coding] .row-status { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .board-row[data-status="In Review"] .row-status { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .board-row[data-status=Complete] .row-status { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .board-row[data-status=Blocked] .row-status { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .board-row[data-status=Superseded] .row-status { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .board-row[data-status=Canceled] .row-status { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  @media (max-width: 720px) {
    .board-row { grid-template-columns: 10px 88px minmax(0, 1fr) auto; }
    .row-kind { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .board-row { transition: none; }
  }
</style>
